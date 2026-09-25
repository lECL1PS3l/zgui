use std::sync::{Arc, Mutex};

use tg_ws_proxy_rs::config::Config;
use tg_ws_proxy_rs::server::{self, ListenInfo, RunError};

#[derive(Default, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TgStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub link: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Inner {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    listen: Option<ListenInfo>,
    error: Option<String>,
    /// Номер запуска: задача прошлого запуска не затирает состояние нового
    /// (раньше завершившаяся задача безусловно писала «остановлен», даже если
    /// уже был поднят новый сервер).
    epoch: u64,
    /// Сигнал «задача сервера завершилась»: `start` ждёт его перед повторным
    /// биндом порта — иначе быстрый stop→start даёт «порт занят».
    done_rx: Option<tokio::sync::oneshot::Receiver<()>>,
}

/// Состояние Telegram-прокси. Клонируется через `Arc` внутрь async-задачи.
#[derive(Clone, Default)]
pub struct TgState {
    inner: Arc<Mutex<Inner>>,
}

impl TgState {
    pub fn status(&self) -> TgStatus {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        TgStatus {
            running: g.shutdown.is_some(),
            port: g.listen.as_ref().map(|l| l.addr.port()),
            link: g.listen.as_ref().map(|l| l.tg_link.clone()),
            error: g.error.clone(),
        }
    }

    pub fn stop(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = g.shutdown.take() {
            let _ = tx.send(());
        }
        g.listen = None;
        // Старая ошибка (например, «порт занят») не должна висеть после остановки.
        g.error = None;
    }

    /// Запускает прокси с минимумом параметров (user friendly): только порт.
    /// Остальное — дефолты крейта (встроенный пул CF-доменов, DC-IP и т.д.).
    /// `secret` — постоянный MTProto-секрет (32 hex): Telegram переиспользует одну
    /// запись прокси вместо накопления мёртвых. None — крейт сгенерит случайный.
    /// Возвращает статус только после фактического бинда сокета (или ошибку).
    pub async fn start(&self, port: u16, faketls_domain: Option<String>, secret: Option<String>) -> Result<TgStatus, String> {
        // Быстрый stop→start: ждём фактического завершения прошлой задачи, иначе
        // новый бинд ловит «порт занят» (listener старой задачи ещё не закрыт).
        let prev_done = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.shutdown.is_some() {
                return Err(crate::texts::TG_ALREADY.into());
            }
            g.done_rx.take()
        };
        if let Some(rx) = prev_done {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await;
        }

        let mut args: Vec<String> = vec![
            "tg-ws-proxy".into(),
            "--port".into(),
            port.to_string(),
            // Только этот компьютер: без `--host` крейт при наличии LAN-адреса
            // слушает 0.0.0.0 (мост видел бы весь локальный сегмент). Для
            // Telegram Desktop на этой же машине локального хоста достаточно.
            "--host".into(),
            "127.0.0.1".into(),
            "--default-domains".into(),
        ];
        if let Some(d) = faketls_domain.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            args.push("--listen-faketls-domain".into());
            args.push(d.to_string());
        }
        if let Some(s) = secret.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            args.push("--secret".into());
            args.push(s.to_string());
        }

        let config = Config::try_from_args(args)?;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let (listen_tx, listen_rx) = tokio::sync::oneshot::channel::<Result<ListenInfo, String>>();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

        let epoch = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.epoch = g.epoch.wrapping_add(1);
            g.shutdown = Some(tx);
            g.error = None;
            g.listen = None;
            g.done_rx = Some(done_rx);
            g.epoch
        };

        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut listen_tx = Some(listen_tx);
            let res = server::run_with_listen(
                config,
                async {
                    let _ = rx.await;
                },
                |info| {
                    let mut g = state.inner.lock().unwrap_or_else(|e| e.into_inner());
                    g.listen = Some(info.clone());
                    if let Some(tx) = listen_tx.take() {
                        let _ = tx.send(Ok(info));
                    }
                },
            )
            .await;

            let mut g = state.inner.lock().unwrap_or_else(|e| e.into_inner());
            // Состояние затирает только задача АКТУАЛЬНОГО запуска: иначе
            // завершение старой задачи гасило статус уже нового сервера.
            if g.epoch == epoch {
                g.shutdown = None;
                g.listen = None;
                let msg = match res {
                    Ok(()) => crate::texts::TG_STOPPED.to_string(),
                    Err(RunError::Bind { addr, source }) => crate::texts::tg_port_busy(
                        &addr.to_string(),
                        &crate::human::humanize(&source.to_string()),
                    ),
                    Err(other) => other.to_string(),
                };
                g.error = Some(msg.clone());
                // Если бинд не удался — разбудить ожидающего с ошибкой.
                if let Some(tx) = listen_tx.take() {
                    let _ = tx.send(Err(msg));
                }
            }
            drop(g);
            // Всегда: ожидающий start() не должен ждать впустую свои 5 секунд.
            let _ = done_tx.send(());
        });

        // Ждём фактического бинда (или ошибки) — до 20 секунд.
        match tokio::time::timeout(std::time::Duration::from_secs(20), listen_rx).await {
            Ok(Ok(Ok(_))) => {}
            Ok(Ok(Err(e))) => return Err(e),
            Ok(Err(_)) => return Err(crate::texts::TG_CRASHED.into()),
            Err(_) => return Err(crate::texts::TG_TIMEOUT.into()),
        }
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_status_is_stopped() {
        let s = TgState::default();
        let st = s.status();
        assert!(!st.running);
        assert!(st.port.is_none());
        assert!(st.error.is_none());
    }

    #[test]
    fn stop_on_idle_is_noop() {
        let s = TgState::default();
        s.stop();
        assert!(!s.status().running);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn start_binds_and_reports_port() {
        let s = TgState::default();
        // Порт 0 — ОС выдаст свободный, тест не конфликтует с реальным 1443.
        let st = s.start(0, None, None).await.expect("прокси должен запуститься");
        assert!(st.running, "статус должен быть running после бинда");
        assert!(st.port.is_some(), "порт должен быть известен после бинда");
        assert!(
            st.link.as_deref().unwrap_or("").starts_with("tg://proxy?"),
            "должна быть сформирована tg-ссылка: {:?}",
            st.link
        );
        // Мост слушает только 127.0.0.1 — и ссылка обязана указывать туда же
        // (LAN-адрес в ней не подключился бы к локальному listener).
        assert!(
            st.link.as_deref().unwrap_or("").contains("server=127.0.0.1"),
            "ссылка должна указывать на 127.0.0.1: {:?}",
            st.link
        );
        s.stop();
        assert!(!s.status().running, "после stop статус не running");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn busy_port_reports_bind_error() {
        // Занятый порт: пользователь должен увидеть причину («занят»), а не
        // «не запустился за 20 секунд» — ошибка бинда идёт через listen-канал.
        // Мост биндится на 127.0.0.1 (см. `--host` в start) — и держателя
        // берём на том же адресе, иначе Windows разрешает бинд мимо wildcard.
        let holder = std::net::TcpListener::bind("127.0.0.1:0").expect("тестовый сокет");
        let port = holder.local_addr().expect("адрес").port();
        let s = TgState::default();
        let err = s.start(port, None, None).await.expect_err("порт занят — ожидалась ошибка");
        assert!(err.contains("занят"), "ожидалось сообщение о занятом порте, получено: {err}");
        assert!(!s.status().running, "упавший старт не должен оставлять статус running");
        drop(holder);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn start_with_fixed_secret_is_stable() {
        let s = TgState::default();
        let secret = "0123456789abcdef0123456789abcdef";
        let st = s.start(0, None, Some(secret.to_string())).await.expect("прокси должен запуститься");
        assert!(
            st.link.as_deref().unwrap_or("").contains(secret),
            "ссылка должна содержать заданный (постоянный) секрет: {:?}",
            st.link
        );
        s.stop();
    }
}
