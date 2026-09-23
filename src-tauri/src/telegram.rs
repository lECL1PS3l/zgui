use std::sync::{Arc, Mutex};

use tg_ws_proxy_rs::config::Config;
use tg_ws_proxy_rs::server::{self, ListenInfo, RunError};
use tg_ws_proxy_rs::stats::STATS;

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

    pub fn stats(&self) -> String {
        STATS.summary()
    }

    pub fn stop(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = g.shutdown.take() {
            let _ = tx.send(());
        }
        g.listen = None;
    }

    /// Запускает прокси с минимумом параметров (user friendly): только порт.
    /// Остальное — дефолты крейта (встроенный пул CF-доменов, DC-IP и т.д.).
    /// `secret` — постоянный MTProto-секрет (32 hex): Telegram переиспользует одну
    /// запись прокси вместо накопления мёртвых. None — крейт сгенерит случайный.
    /// Возвращает статус только после фактического бинда сокета (или ошибку).
    pub async fn start(&self, port: u16, faketls_domain: Option<String>, secret: Option<String>) -> Result<TgStatus, String> {
        {
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.shutdown.is_some() {
                return Err("Telegram-прокси уже запущен".into());
            }
        }

        let mut args: Vec<String> = vec![
            "tg-ws-proxy".into(),
            "--port".into(),
            port.to_string(),
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

        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.shutdown = Some(tx);
            g.error = None;
            g.listen = None;
        }

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
            g.shutdown = None;
            g.listen = None;
            let msg = match res {
                Ok(()) => "Telegram-прокси остановлен".to_string(),
                Err(RunError::Bind { addr, source }) => format!("порт {} занят: {}", addr, source),
                Err(other) => other.to_string(),
            };
            g.error = Some(msg.clone());
            // Если бинд не удался — разбудить ожидающего с ошибкой.
            if let Some(tx) = listen_tx.take() {
                let _ = tx.send(Err(msg));
            }
        });

        // Ждём фактического бинда (или ошибки) — до 20 секунд.
        match tokio::time::timeout(std::time::Duration::from_secs(20), listen_rx).await {
            Ok(Ok(Ok(_))) => {}
            Ok(Ok(Err(e))) => return Err(e),
            Ok(Err(_)) => return Err("Telegram-прокси неожиданно завершился".into()),
            Err(_) => return Err("Telegram-прокси не запустился за 20 секунд".into()),
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
        s.stop();
        assert!(!s.status().running, "после stop статус не running");
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
