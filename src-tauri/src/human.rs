//! Перевод технических ошибок (Rust/OS/HTTP) на понятный русский.
//!
//! Тестер не должен видеть «os error 5» или «error sending request for url»:
//! такие строки пугают и не подсказывают, что делать. Здесь — единая таблица
//! соответствий; всё непонятное остаётся, но с пометкой «подробности в журнале».

/// Максимальная длина сырой строки, которую показываем пользователю.
const RAW_LIMIT: usize = 220;

fn cut(s: &str) -> String {
    if s.chars().count() <= RAW_LIMIT {
        return s.to_string();
    }
    let head: String = s.chars().take(RAW_LIMIT).collect();
    format!("{head}…")
}

/// Есть ли в строке признаки «сырой» технической ошибки.
fn looks_technical(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    [
        "os error",
        "error",
        "failed",
        "denied",
        "not found",
        "timed out",
        "timeout",
        "http ",
        "panic",
        "invalid",
        "unexpected",
        "cannot",
        "unable",
        "connection",
        "refused",
        "reset by peer",
        "0x",
        "exception",
    ]
    .iter()
    .any(|m| l.contains(m))
}

/// Понятный текст для пользователя по сырой ошибке.
pub fn humanize(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return crate::texts::HUMAN_EMPTY.into();
    }
    let l = s.to_ascii_lowercase();

    // Уже человеческое сообщение (наше, на русском) — не трогаем.
    let has_cyr = s.chars().any(|c| ('а'..='я').contains(&c.to_ascii_lowercase()));
    if has_cyr && !looks_technical(s) {
        return s.to_string();
    }

    let hit: Option<&str> = if l.contains("os error 5")
        || l.contains("access is denied")
        || l.contains("отказано в доступе")
        || l.contains("administrator")
        || l.contains("admin_required")
    {
        Some(crate::texts::HUMAN_ADMIN)
    } else if l.contains("os error 32") || l.contains("being used by another process") {
        Some(crate::texts::HUMAN_FILE_BUSY)
    } else if l.contains("os error 112") || l.contains("not enough space") || l.contains("no space left") {
        Some(crate::texts::HUMAN_NO_SPACE)
    } else if l.contains("os error 2") || l.contains("os error 3") || l.contains("cannot find")
        || l.contains("не удается найти")
    {
        Some(crate::texts::HUMAN_NOT_FOUND)
    } else if l.contains("error sending request")
        || l.contains("error trying to connect")
        || l.contains("dns error")
        || l.contains("timed out")
        || l.contains("connection refused")
        || l.contains("connection reset")
        || l.contains("network is unreachable")
    {
        Some(crate::texts::HUMAN_NETWORK)
    } else if l.contains("certificate") || l.contains("tls handshake") || l.contains("ssl") {
        Some(crate::texts::HUMAN_CERT)
    } else if l.contains("http 403") || l.contains(" 403") {
        Some(crate::texts::HUMAN_HTTP_403)
    } else if l.contains("http 404") || l.contains(" 404") {
        Some(crate::texts::HUMAN_HTTP_404)
    } else if l.contains("http 5") {
        Some(crate::texts::HUMAN_HTTP_5XX)
    } else if l.contains("invalid args") || l.contains("expected u16") || l.contains("invalid type")
        || l.contains("invalid value")
    {
        Some(crate::texts::HUMAN_BAD_VALUE)
    } else if l.contains("process exited immediately") || l.contains("сразу завершился") {
        Some(crate::texts::HUMAN_ENGINE_DIED)
    } else if l.contains("launch_error") || l.contains("не удалось запустить процесс") {
        Some(crate::texts::HUMAN_LAUNCH)
    } else if l.contains("panic") || l.contains("panicked") {
        Some(crate::texts::HUMAN_PANIC)
    } else if l.contains("no such file") || l.contains("not found") {
        Some(crate::texts::HUMAN_FILE_MISSING)
    } else {
        None
    };

    match hit {
        Some(msg) => msg.to_string(),
        None => {
            if has_cyr {
                s.to_string()
            } else {
                crate::texts::human_unexpected(&cut(s))
            }
        }
    }
}

/// Ошибка с контекстом действия: «<что делали>: <понятный текст>».
pub fn with_context(context: &str, raw: &str) -> String {
    format!("{context}: {}", humanize(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_common_os_and_http_errors() {
        assert!(humanize("failed to remove file: Отказано в доступе (os error 5)")
            .contains("права администратора"));
        assert!(humanize("error sending request for url (https://api.github.com/…): error trying to connect")
            .contains("Нет связи с сервером"));
        assert!(humanize("HTTP status client error (403 Forbidden)").contains("403"));
        assert!(humanize("invalid args `port` for command `tg_start`: invalid type: integer 70000, expected u16")
            .contains("Недопустимое значение поля"));
        assert!(humanize("ADMIN_REQUIRED: winws needs administrator rights").contains("права администратора"));
    }

    #[test]
    fn keeps_friendly_russian_messages_untouched() {
        let msg = "в папке не найден winws.exe — укажите корень движка";
        assert_eq!(humanize(msg), msg);
    }

    #[test]
    fn unknown_technical_text_is_marked() {
        let out = humanize("some weird failure 0xDEADBEEF");
        assert!(out.contains("Журнале"));
    }

    #[test]
    fn context_prefix_is_added() {
        assert!(with_context("скачивание движка", "os error 112: not enough space")
            .starts_with("скачивание движка:"));
    }
}
