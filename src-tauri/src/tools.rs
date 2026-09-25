//! Инструменты обслуживания — паритет с меню автора flowseal (`service.bat`):
//! чистка кэша Discord, замена активных фейков, скачивание свежего hosts.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runner::hidden_command;

/// Суммарный размер каталога (для отчёта «освобождено ~N МБ»).
fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

/// Чистка кэша Discord: гасит клиенты (кнопка вызывается вручную — как у автора
/// с подтверждением) и удаляет Cache/Code Cache/GPUCache у всех четырёх сборок.
pub fn clear_discord_cache() -> Result<String, String> {
    let exes = ["Discord.exe", "DiscordPTB.exe", "DiscordCanary.exe", "DiscordDevelopment.exe"];
    for exe in exes {
        let _ = hidden_command("taskkill.exe").args(["/F", "/IM", exe]).output();
    }
    let appdata = std::env::var_os("APPDATA").ok_or(crate::texts::APPDATA_MISSING)?;
    let appdata = PathBuf::from(appdata);
    let mut removed = 0usize;
    let mut freed = 0u64;
    for v in ["discord", "discordptb", "discordcanary", "discorddevelopment"] {
        for sub in ["Cache", "Code Cache", "GPUCache"] {
            let dir = appdata.join(v).join(sub);
            if !dir.exists() {
                continue;
            }
            freed += dir_size(&dir);
            if std::fs::remove_dir_all(&dir).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(crate::texts::discord_cache_cleared(removed, freed / (1024 * 1024)))
}

#[derive(serde::Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FakesView {
    pub files: Vec<String>,
    pub active_discord: Option<String>,
    pub active_game: Option<String>,
}

fn active_file(kind: &str) -> &'static str {
    if kind == "game" {
        "ACTIVE_GAME_UDP.bin"
    } else {
        "ACTIVE_DISCORD_UDP.bin"
    }
}

fn name_of_hash(bin: &Path, hash: &Option<String>) -> Option<String> {
    let hash = hash.as_deref()?;
    let rd = std::fs::read_dir(bin).ok()?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let low = name.to_lowercase();
        if !low.ends_with(".bin") || low.starts_with("active_") {
            continue;
        }
        if crate::config::file_sha256(&e.path()).as_deref() == Some(hash) {
            return Some(name);
        }
    }
    None
}

/// Фейки (`bin\*.bin`, без ACTIVE_*) и активные файлы (совпадение по SHA-256 —
/// так же сравнивает автор в `:replace_active_fakes`).
pub fn fakes_view(root: &Path) -> FakesView {
    let bin = root.join("bin");
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&bin) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let low = name.to_lowercase();
            if low.ends_with(".bin") && !low.starts_with("active_") {
                files.push(name);
            }
        }
    }
    files.sort();
    FakesView {
        active_discord: name_of_hash(&bin, &crate::config::file_sha256(&bin.join(active_file("discord")))),
        active_game: name_of_hash(&bin, &crate::config::file_sha256(&bin.join(active_file("game")))),
        files,
    }
}

/// Заменяет ACTIVE_*_UDP.bin выбранным фейком (имя — только файл из bin, без путей).
pub fn replace_active_fake(root: &Path, kind: &str, name: &str) -> Result<String, String> {
    let bin = root.join("bin");
    let name = name.trim();
    if name.is_empty() || name.contains('\\') || name.contains('/') || name.contains("..") {
        return Err(crate::texts::FAKE_BAD_NAME.into());
    }
    let src = bin.join(name);
    if !src.is_file() {
        return Err(crate::texts::fake_missing(&src.display().to_string()));
    }
    let dst = bin.join(active_file(kind));
    std::fs::copy(&src, &dst).map_err(|e| crate::texts::fake_replace_failed(&e.to_string()))?;
    Ok(crate::texts::fake_replaced(name))
}

const HOSTS_URL: &str =
    "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/main/.service/hosts";

/// Значимые строки hosts (без комментариев/пустых) — для сравнения версий.
pub(crate) fn hosts_key_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Скачивает свежий hosts автора в `data\catalog\hosts-from-author.txt` и
/// сообщает, отличается ли он от системного (замена — вручную, под админом,
/// как у автора: открываем оба файла).
pub fn hosts_update(data: &Path) -> Result<String, String> {
    let cli = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| crate::texts::hosts_download_failed(&e.to_string()))?;
    let resp = cli
        .get(HOSTS_URL)
        .header("Cache-Control", "no-cache")
        .send()
        .map_err(|e| crate::human::with_context(crate::texts::HOSTS_DOWNLOAD_CONTEXT, &e.to_string()))?;
    if !resp.status().is_success() {
        return Err(crate::texts::hosts_http_error(resp.status().as_u16()));
    }
    let text = resp.text().map_err(|e| crate::texts::hosts_download_failed(&e.to_string()))?;
    if text.trim().is_empty() {
        return Err(crate::texts::HOSTS_EMPTY.into());
    }
    let out = data.join("catalog").join("hosts-from-author.txt");
    crate::config::atomic_write(&out, text.as_bytes())?;

    let system = std::env::var_os("SystemRoot").map(|r| {
        PathBuf::from(r)
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
    });
    let same = system
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|cur| hosts_key_lines(&cur) == hosts_key_lines(&text))
        .unwrap_or(false);
    if same {
        Ok(crate::texts::hosts_up_to_date(&out.display().to_string()))
    } else {
        Ok(crate::texts::hosts_downloaded(&out.display().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_key_lines_ignore_comments_and_case() {
        let a = "# comment\n127.0.0.1 Example.COM\n\n127.0.0.1 example.com\n";
        assert_eq!(hosts_key_lines(a), vec!["127.0.0.1 example.com", "127.0.0.1 example.com"]);
        assert!(hosts_key_lines("# only\n\n").is_empty());
    }

    #[test]
    fn fakes_view_lists_and_detects_active() {
        let tmp = std::env::temp_dir().join(format!("zgui-fakes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let bin = tmp.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("fake_a.bin"), b"AAAA").unwrap();
        std::fs::write(bin.join("fake_b.bin"), b"BBBB").unwrap();
        std::fs::write(bin.join(active_file("discord")), b"BBBB").unwrap();
        let v = fakes_view(&tmp);
        assert_eq!(v.files, vec!["fake_a.bin", "fake_b.bin"]);
        assert_eq!(v.active_discord.as_deref(), Some("fake_b.bin"));
        assert_eq!(v.active_game, None);
        // Замена: game → fake_a.
        replace_active_fake(&tmp, "game", "fake_a.bin").unwrap();
        let v2 = fakes_view(&tmp);
        assert_eq!(v2.active_game.as_deref(), Some("fake_a.bin"));
        assert!(replace_active_fake(&tmp, "game", "..\\evil.bin").is_err());
        assert!(replace_active_fake(&tmp, "game", "нет.bin").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
