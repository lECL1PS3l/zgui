//! Приведение окружения «как у автора flowseal»: включение TCP timestamps.
//! Ничего не убиваем и не меняем кроме этого (автор включает timestamps при
//! каждом запуске service.bat — делаем то же при старте GUI с правами админа).

use crate::runner::hidden_command;

/// Включает TCP timestamps (`netsh interface tcp set global timestamps=enabled`).
/// Без прав команда молча не сработает.
pub fn ensure_tcp_timestamps() {
    let _ = hidden_command("cmd.exe")
        .args(["/c", "chcp 437 >nul & netsh interface tcp set global timestamps=enabled"])
        .output();
}
