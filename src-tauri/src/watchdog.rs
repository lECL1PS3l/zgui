//! Watchdog: периодически проверяет доступность YouTube и Discord при запущенной
//! стратегии и предупреждает, если она перестала отвечать. **Авто-восстановления нет**
//! (решение владельца) — только уведомление.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Домены, которые проверяет watchdog (как в FreeConnect).
const WATCH_DOMAINS: &[(&str, &str)] = &[
    ("YouTube", "www.youtube.com"),
    ("Discord", "discord.com"),
];

const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Сколько неудач подряд считаем сбоем обхода.
const FAIL_THRESHOLD: u32 = 3;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomStatus {
    pub label: String,
    pub host: String,
    pub ok: bool,
    pub ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogStatus {
    /// Идёт ли наблюдение (запущен профиль).
    pub active: bool,
    /// Подряд идущих неудач.
    pub failures: u32,
    /// Есть ли сейчас тревога (стратегия не отвечает).
    pub alarm: bool,
    /// Последняя проверка (Unix-время, сек).
    pub checked_at: u64,
    /// Результат по доменам.
    pub domains: Vec<DomStatus>,
}

#[derive(Default)]
pub struct WatchdogState {
    status: Mutex<WatchdogStatus>,
    running: AtomicBool,
}

impl WatchdogState {
    pub fn status(&self) -> WatchdogStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn set(&self, s: WatchdogStatus) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }
}

/// HTTPS-проба (TLS + HTTP-заголовки) с измерением времени.
/// TCP-connect давал ложное «работает»: DPI пропускает рукопожатие TCP
/// и режет TLS по SNI, поэтому браузер не открывает сайт, а проба «успешна».
async fn probe(client: &reqwest::Client, host: &str) -> (bool, u64) {
    let start = std::time::Instant::now();
    let url = format!("https://{}/", host);
    let ok = client.get(url).send().await.is_ok();
    (ok, start.elapsed().as_millis() as u64)
}

/// Запускает фоновый watchdog один раз за процесс.
pub fn spawn(app: AppHandle, state: Arc<WatchdogState>) {
    if state.running.swap(true, Ordering::SeqCst) {
        return; // уже запущен
    }
    let client = match reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            crate::logger::log("warn", "watchdog", &format!("не удалось создать клиент пробы: {e}"));
            state.running.store(false, Ordering::SeqCst);
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        let mut failures: u32 = 0;
        let mut alarm = false;
        loop {
            tokio::time::sleep(CHECK_INTERVAL).await;

            // Наблюдаем только когда стратегия реально запущена (программа или
            // служба) и не идёт тест — владелец обхода уже учитывает и то, и другое.
            let owner = {
                let g = app.state::<crate::Global>();
                crate::current_owner(&g)
            };
            let active = matches!(
                owner,
                crate::WinwsOwner::App(_) | crate::WinwsOwner::Service(_)
            );
            if !active {
                // Стратегия не запущена — сбрасываем тревогу и не шумим.
                failures = 0;
                alarm = false;
                state.set(WatchdogStatus {
                    active: false,
                    ..Default::default()
                });
                let _ = app.emit("zgui:watchdog", state.status());
                continue;
            }

            let mut domains: Vec<DomStatus> = Vec::with_capacity(WATCH_DOMAINS.len());
            for (label, host) in WATCH_DOMAINS {
                let (ok, ms) = probe(&client, host).await;
                domains.push(DomStatus {
                    label: label.to_string(),
                    host: host.to_string(),
                    ok,
                    ms,
                });
            }
            let all_ok = domains.iter().all(|d| d.ok);
            if all_ok {
                failures = 0;
                if alarm {
                    alarm = false;
                    crate::logger::log("ok", "watchdog", "стратегия снова отвечает");
                    let _ = app.emit(
                        "zgui:toast",
                        serde_json::json!({"kind":"ok","text": crate::texts::WATCHDOG_OK}),
                    );
                }
            } else {
                failures = failures.saturating_add(1);
                if failures >= FAIL_THRESHOLD && !alarm {
                    alarm = true;
                    let failed: Vec<&str> =
                        domains.iter().filter(|d| !d.ok).map(|d| d.label.as_str()).collect();
                    crate::logger::log(
                        "warn",
                        "watchdog",
                        &format!("стратегия не отвечает: не отвечают {}", failed.join(", ")),
                    );
                    let _ = app.emit(
                        "zgui:toast",
                        serde_json::json!({
                            "kind":"warn",
                            "text": crate::texts::watchdog_failed(&failed.join(", "))
                        }),
                    );
                }
            }

            state.set(WatchdogStatus {
                active: true,
                failures,
                alarm,
                checked_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                domains,
            });
            let _ = app.emit("zgui:watchdog", state.status());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_status_is_inactive() {
        let s = WatchdogState::default();
        let st = s.status();
        assert!(!st.active);
        assert_eq!(st.failures, 0);
        assert!(!st.alarm);
        assert!(!s.is_running());
    }

    #[tokio::test]
    async fn probe_localhost_fails() {
        // localhost без HTTPS-слушателя — ожидаем неудачу; важен факт отсутствия паники.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(800))
            .no_proxy()
            .build()
            .unwrap();
        let (ok, _ms) = probe(&client, "localhost").await;
        assert!(!ok, "на localhost нет HTTPS-сервера");
    }
}
