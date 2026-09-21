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
        return "неизвестная ошибка (подробности в журнале)".into();
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
        Some("Windows запросит права администратора для запуска обхода — включите «Всегда запускать программу от администратора» в «Настройках»")
    } else if l.contains("os error 32") || l.contains("being used by another process") {
        Some("файл занят другой программой — закройте её и повторите")
    } else if l.contains("os error 112") || l.contains("not enough space") {
        Some("на диске не хватает места")
    } else if l.contains("os error 2") || l.contains("os error 3") || l.contains("cannot find")
        || l.contains("не удается найти")
    {
        Some("файл или папка не найдены — возможно, движок ещё не установлен")
    } else if l.contains("error sending request")
        || l.contains("error trying to connect")
        || l.contains("dns error")
        || l.contains("timed out")
        || l.contains("connection refused")
        || l.contains("connection reset")
        || l.contains("network is unreachable")
    {
        Some("нет связи с сервером — проверьте интернет (или выключите VPN) и повторите")
    } else if l.contains("http 403") || l.contains(" 403") {
        Some("сервер отклонил запрос (403) — возможно, исчерпан лимит обращений к GitHub, попробуйте позже")
    } else if l.contains("http 404") || l.contains(" 404") {
        Some("на сервере нет такого файла (404) — обновите программу")
    } else if l.contains("http 5") {
        Some("сервер временно недоступен (ошибка 5xx) — попробуйте позже")
    } else if l.contains("invalid args") || l.contains("expected u16") || l.contains("invalid type")
        || l.contains("invalid value")
    {
        Some("недопустимое значение поля — проверьте введённые числа")
    } else if l.contains("process exited immediately") || l.contains("сразу завершился") {
        Some("движок сразу завершился — подробности в «Журнале»")
    } else if l.contains("launch_error") || l.contains("не удалось запустить процесс") {
        Some("не удалось запустить процесс — возможно, запрос прав администратора отклонён")
    } else if l.contains("panic") || l.contains("panicked") {
        Some("внутренняя ошибка программы — подробности в «Журнале»")
    } else if l.contains("no such file") || l.contains("not found") {
        Some("файл не найден — проверьте, что движок установлен")
    } else {
        None
    };

    match hit {
        Some(msg) => msg.to_string(),
        None => {
            if has_cyr {
                s.to_string()
            } else {
                format!("непредвиденная ошибка: {} (подробности в «Журнале»)", cut(s))
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
            .contains("нет связи с сервером"));
        assert!(humanize("HTTP status client error (403 Forbidden)").contains("403"));
        assert!(humanize("invalid args `port` for command `tg_start`: invalid type: integer 70000, expected u16")
            .contains("недопустимое значение поля"));
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
