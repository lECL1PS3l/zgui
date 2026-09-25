//! Диагностика окружения — паритет с `service.bat` автора flowseal.
//! Только предупреждения (в UI-баннер «Важно перед запуском»); ничего не
//! убиваем и не меняем. Исключение — TCP timestamps: автор включает их при
//! каждом запуске .bat, мы делаем то же при старте GUI с правами админа.

use std::path::Path;

use crate::runner::hidden_command;

/// Включает TCP timestamps (`netsh interface tcp set global timestamps=enabled`).
/// Как у автора (`service.bat: :tcp_enable`); без прав команда молча не сработает.
pub fn ensure_tcp_timestamps() {
    let _ = hidden_command("cmd.exe")
        .args(["/c", "chcp 437 >nul & netsh interface tcp set global timestamps=enabled"])
        .output();
}

/// Состояние TCP timestamps из вывода `netsh interface tcp show global`,
/// снятого под `chcp 437` (иначе локаль ломает разбор). None — не смогли понять.
pub(crate) fn parse_timestamps_line(netsh_out: &str) -> Option<bool> {
    let line = netsh_out
        .lines()
        .find(|l| l.to_ascii_lowercase().contains("timestamps"))?;
    let low = line.to_ascii_lowercase();
    Some(low.contains("enabled") && !low.contains("disabled"))
}

pub fn tcp_timestamps_enabled() -> Option<bool> {
    let out = hidden_command("cmd.exe")
        .args(["/c", "chcp 437 >nul & netsh interface tcp show global"])
        .output()
        .ok()?;
    parse_timestamps_line(&String::from_utf8_lossy(&out.stdout))
}

/// Состояние службы по `sc query`: `Some(running)` — спросили и поняли,
/// `None` — не смогли (не пугаем пользователя ложным предупреждением).
/// Раньше ошибка запроса трактовалась как «запущена» (fail-open).
fn service_state(name: &str) -> Option<bool> {
    match hidden_command("sc.exe").args(["query", name]).output() {
        Ok(o) => {
            if o.status.code() == Some(1060) {
                // «Служба не установлена» — точно не запущена.
                return Some(false);
            }
            if o.status.success() {
                return Some(String::from_utf8_lossy(&o.stdout).to_uppercase().contains("RUNNING"));
            }
            None
        }
        Err(_) => None,
    }
}

/// Известные конфликтующие службы из диагностики автора. `sc query` без имени
/// печатает DisplayName — поэтому часть проверок по подстрокам сразу трёх слов.
pub(crate) fn conflicts_from_sc(txt_upper: &str) -> Vec<&'static str> {
    let mut v = Vec::new();
    if txt_upper.contains("KILLER") {
        v.push("Killer");
    }
    if txt_upper.contains("INTEL") && txt_upper.contains("CONNECTIVITY") && txt_upper.contains("NETWORK") {
        v.push("Intel Connectivity Network Service");
    }
    if txt_upper.contains("TRACSRVWRAPPER") || txt_upper.contains("EPWD") {
        v.push("Check Point");
    }
    if txt_upper.contains("SMARTBYTE") {
        v.push("SmartByte");
    }
    if txt_upper.contains("GOODBYEDPI") {
        v.push("GoodbyeDPI");
    }
    if txt_upper.contains("DISCORDFIX_ZAPRET") {
        v.push("discordfix_zapret");
    }
    if txt_upper.contains("WINWS1") {
        v.push("winws1");
    }
    if txt_upper.contains("WINWS2") {
        v.push("winws2");
    }
    v
}

pub fn conflicting_services() -> Vec<&'static str> {
    let Ok(o) = hidden_command("sc.exe").args(["query"]).output() else {
        return Vec::new();
    };
    conflicts_from_sc(&String::from_utf8_lossy(&o.stdout).to_uppercase())
}

/// Запущен ли процесс по exe-имени (без пути) — через tasklist.
fn process_running(exe: &str) -> bool {
    let out = hidden_command("tasklist.exe")
        .args(["/FI", &format!("IMAGENAME eq {exe}"), "/FO", "CSV", "/NH"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .any(|l| l.trim_start().starts_with('"')),
        Err(_) => false,
    }
}

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

pub(crate) fn path_in_onedrive(path: &str, onedrive: Option<&str>) -> bool {
    let Some(od) = onedrive else { return false };
    let od = od.trim().replace('/', "\\").to_lowercase();
    if od.is_empty() {
        return false;
    }
    path.replace('/', "\\").to_lowercase().starts_with(&od)
}

/// Сколько значимых (не комментарии) строк hosts содержат youtube.
pub(crate) fn hosts_youtube_count(content: &str) -> usize {
    content
        .lines()
        .filter(|l| {
            let l = l.trim().to_lowercase();
            !l.is_empty() && !l.starts_with('#') && (l.contains("youtube.com") || l.contains("youtu.be"))
        })
        .count()
}

fn hosts_youtube_entries() -> usize {
    let Some(root) = std::env::var_os("SystemRoot") else { return 0 };
    let p = std::path::Path::new(&root)
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts");
    let Ok(content) = std::fs::read_to_string(&p) else { return 0 };
    hosts_youtube_count(&content)
}

/// Снимок диагностики с TTL (bootstrap зовётся каждые 4 с — не дёргаем
/// sc/tasklist/netsh каждый раз; но и «навсегда» кэшировать нельзя: конфликты,
/// hosts и timestamps меняются в течение сессии).
pub fn warnings(data_dir: &Path) -> Vec<String> {
    const TTL: std::time::Duration = std::time::Duration::from_secs(60);
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<String>)>> =
        std::sync::Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = guard.as_ref() {
        if at.elapsed() < TTL {
            return v.clone();
        }
    }
    let fresh = compute(data_dir);
    *guard = Some((std::time::Instant::now(), fresh.clone()));
    fresh
}

fn compute(data: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if service_state("BFE") == Some(false) {
        out.push(crate::texts::BFE_OFF.into());
    }
    if tcp_timestamps_enabled() == Some(false) {
        out.push(crate::texts::TIMESTAMPS_OFF.into());
    }
    let conf = conflicting_services();
    if !conf.is_empty() {
        out.push(crate::texts::conflicting_services(&conf.join(", ")));
    }
    if process_running("AdguardSvc.exe") {
        out.push(crate::texts::ADGUARD_RUNNING.into());
    }
    let d = data.to_string_lossy();
    if has_cyrillic(&d) {
        out.push(crate::texts::CYRILLIC_PATH.into());
    }
    if path_in_onedrive(&d, std::env::var("OneDrive").ok().as_deref()) {
        out.push(crate::texts::ONEDRIVE_PATH.into());
    }
    let yt = hosts_youtube_entries();
    if yt > 0 {
        out.push(crate::texts::hosts_youtube(
            yt,
            &format!(
                "{}\\System32\\drivers\\etc\\hosts",
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_line_parses_enabled_disabled() {
        let on = "TCP Global Parameters\n----------------------------------------------\nTCP timestamps                 : enabled\n";
        let off = "TCP Global Parameters\nTCP timestamps                 : disabled\n";
        assert_eq!(parse_timestamps_line(on), Some(true));
        assert_eq!(parse_timestamps_line(off), Some(false));
        assert_eq!(parse_timestamps_line("нет строки"), None);
    }

    #[test]
    fn conflicts_from_sc_detects_author_known_conflicts() {
        let txt = "SERVICE_NAME: Killer\nDISPLAY_NAME: Intel Connectivity Network Service\nTracSrvWrapper\nSMARTBYTE\n";
        let v = conflicts_from_sc(&txt.to_uppercase());
        assert!(v.contains(&"Killer"));
        assert!(v.contains(&"Intel Connectivity Network Service"));
        assert!(v.contains(&"Check Point"));
        assert!(v.contains(&"SmartByte"));
        assert!(!v.contains(&"GoodbyeDPI"));
    }

    #[test]
    fn cyrillic_and_onedrive_paths_are_detected() {
        assert!(has_cyrillic("E:\\Запрет\\data"));
        assert!(!has_cyrillic("E:\\zapret\\data"));
        assert!(path_in_onedrive("C:\\Users\\u\\OneDrive\\ZGUI", Some("C:\\Users\\u\\OneDrive")));
        assert!(!path_in_onedrive("E:\\ZGUI", Some("C:\\Users\\u\\OneDrive")));
        assert!(!path_in_onedrive("C:\\Users\\u\\OneDrive\\ZGUI", None));
    }

    #[test]
    fn hosts_entries_count_ignores_comments() {
        let hosts = "0.0.0.0 youtube.com\n127.0.0.1 youtu.be\n127.0.0.1 example.com\n# 0.0.0.0 youtube.com\n";
        assert_eq!(hosts_youtube_count(hosts), 2);
        assert_eq!(hosts_youtube_count("# only comment\n"), 0);
    }
}
