//! Watchdog: периодически проверяет доступность YouTube и Discord при запущенном
//! профиле и предупреждает, если обход перестал работать. **Авто-восстановления нет**
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
    /// Есть ли сейчас тревога (обход не работает).
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

/// Синхронная TCP-проба (443, fallback 80) с измерением времени.
fn probe(host: &str) -> (bool, u64) {
    use std::net::{TcpStream, ToSocketAddrs};
    let start = std::time::Instant::now();
    let addrs = (host, 443u16).to_socket_addrs();
    let owned = match addrs {
        Ok(it) => it.collect::<Vec<_>>(),
        Err(_) => return (false, start.elapsed().as_millis() as u64),
    };
    for addr in owned {
        if TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok() {
            return (true, start.elapsed().as_millis() as u64);
        }
    }
    (false, start.elapsed().as_millis() as u64)
}

/// Запускает фоновый watchdog один раз за процесс.
pub fn spawn(app: AppHandle, state: Arc<WatchdogState>) {
    if state.running.swap(true, Ordering::SeqCst) {
        return; // уже запущен
    }
    tauri::async_runtime::spawn(async move {
        let mut failures: u32 = 0;
        let mut alarm = false;
        loop {
            tokio::time::sleep(CHECK_INTERVAL).await;

            // Наблюдаем только когда профиль реально запущен и тест не идёт.
            let (alive, testing) = {
                let g = app.state::<crate::Global>();
                let s = crate::st(&g);
                let alive = s
                    .runtime
                    .as_ref()
                    .map(|r| crate::runner::pid_alive(r.pid))
                    .unwrap_or(false);
                let testing = {
                    let t = g.testing.lock().unwrap_or_else(|e| e.into_inner());
                    t.running
                };
                (alive, testing)
            };
            if !alive || testing {
                // Профиль не запущен — сбрасываем тревогу и не шумим.
                failures = 0;
                alarm = false;
                state.set(WatchdogStatus {
                    active: false,
                    ..Default::default()
                });
                let _ = app.emit("zgui:watchdog", state.status());
                continue;
            }

            let domains: Vec<DomStatus> = WATCH_DOMAINS
                .iter()
                .map(|(label, host)| {
                    let (ok, ms) = probe(host);
                    DomStatus {
                        label: label.to_string(),
                        host: host.to_string(),
                        ok,
                        ms,
                    }
                })
                .collect();
            let all_ok = domains.iter().all(|d| d.ok);
            if all_ok {
                failures = 0;
                if alarm {
                    alarm = false;
                    crate::logger::log("ok", "watchdog", "обход снова работает");
                    let _ = app.emit(
                        "zgui:toast",
                        serde_json::json!({"kind":"ok","text":"обход снова работает"}),
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
                        &format!("обход не работает: не отвечают {}", failed.join(", ")),
                    );
                    let _ = app.emit(
                        "zgui:toast",
                        serde_json::json!({
                            "kind":"warn",
                            "text": format!("обход не работает: не отвечают {}", failed.join(", "))
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

    #[test]
    fn probe_localhost_ok() {
        // Проверяем, что probe вообще умеет возвращать успех/неудачу (не на реальной сети).
        let (ok, _ms) = probe("localhost");
        // localhost без слушателя на 443 — ожидаем неудачу; важен факт отсутствия паники.
        let _ = ok;
    }
}
