use crate::config::{Profile, ENGINE_FLOWSEAL};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_str() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

pub fn make_id(prefix: &str) -> String {
    // Одной секунды мало: два профиля, созданных подряд, получали одинаковый id,
    // и второй «перетирал» первый при поиске по id. Добавляем миллисекунды и счётчик.
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!("{}-{}-{}", prefix, ms, n)
}

/// Flowseal-стиль: пресеты-шаблоны (будут дополнены каталогом из репозитория).
pub fn builtin_flowseal_presets() -> Vec<Profile> {
    vec![]
}

/// Читает текст .bat независимо от кодировки: UTF-8, UTF-8 с BOM, UTF-16 LE с BOM.
/// Авторские стратегии из репозитория бывают сохранены в UTF-16 — из-за этого
/// токенизатор раньше получал пустой набор аргументов и стратегия молча пропадала.
pub fn decode_strategy_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = bytes[2..]
            .as_chunks::<2>().0.iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Разбирает .bat стратегию flowseal, извлекая argv для winws.exe.
/// Возвращает токены с оставшимися плейсхолдерами %GameFilterTCP%/UDP%.
pub fn parse_flowseal_bat(content: &str, root: &Path) -> Vec<String> {
    let joined = join_continuations(content);
    let tokens = tokenize_cmd(&joined);
    let Some(pos) = tokens
        .iter()
        .position(|t| t.to_lowercase().contains("winws.exe"))
    else {
        return Vec::new();
    };
    let rest = &tokens[pos + 1..];
    let bin = root.join("bin");
    let lists = root.join("lists");
    rest.iter()
        .map(|t| expand_vars(t, root, &bin, &lists))
        .collect()
}

fn join_continuations(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let lines = src.lines();
    let mut pending = false;
    for raw in lines {
        let line = raw.trim_end();
        if pending {
            out.push(' ');
            pending = false;
        }
        if line.ends_with('^') {
            out.push_str(line.trim_end_matches('^'));
            pending = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Разбивает командную строку на токены (как их увидит argv), снимая кавычки.
fn tokenize_cmd(src: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut started = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' if !in_q => {
                if started {
                    tokens.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            '"' => {
                if in_q {
                    in_q = false;
                } else {
                    in_q = true;
                    started = true;
                }
            }
            '^' => {
                if let Some(&n) = chars.peek() {
                    if "!\"^%&<>()".contains(n) {
                        cur.push(n);
                        chars.next();
                        started = true;
                    }
                }
            }
            '\\' => {
                if let Some(&'"') = chars.peek() {
                    cur.push('"');
                    chars.next();
                    started = true;
                } else {
                    cur.push(c);
                    started = true;
                }
            }
            _ => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(cur);
    }
    tokens
}

fn expand_vars(t: &str, root: &Path, bin: &Path, lists: &Path) -> String {
    let s = t
        .replace("%BIN%", &format!("{}/", bin.to_string_lossy()))
        .replace("%LISTS%", &format!("{}/", lists.to_string_lossy()));
    let s = s.replace("%~dp0", &format!("{}/", root.to_string_lossy()));
    s
}

/// Заменяет плейсхолдеры игрового фильтра на реальные диапазоны портов.
pub fn apply_game_filter(args: &[String], tcp: &str, udp: &str) -> Vec<String> {
    args.iter()
        .map(|a| {
            a.replace("%GameFilterTCP%", tcp)
                .replace("%GameFilterUDP%", udp)
        })
        .collect()
}

pub fn game_filter_ports(mode: &str) -> (String, String) {
    match mode {
        "all" => ("1024-65535".into(), "1024-65535".into()),
        "tcp" => ("1024-65535".into(), "12".into()),
        "udp" => ("12".into(), "1024-65535".into()),
        _ => ("12".into(), "12".into()),
    }
}

/// Импортирует .bat из каталога raw-стратегий в профили (id = имя файла без .bat = engine flowseal).
pub fn import_bat_profiles(
    root: &Path,
    bats: &[(String, String)],
    existing: &[Profile],
) -> Vec<Profile> {
    let mut out: Vec<Profile> = Vec::new();
    for (fname, content) in bats {
        let id = fname.trim_end_matches(".bat").to_string();
        let args = parse_flowseal_bat(content, root);
        if args.is_empty() {
            continue;
        }
        let prev = existing.iter().find(|p| p.id == id);
        let custom = prev.map(|p| !p.builtin).unwrap_or(false);
        out.push(Profile {
            id: id.clone(),
            name: pretty_name(id.trim_end_matches(".bat")),
            engine: ENGINE_FLOWSEAL.into(),
            args,
            builtin: false,
            source: Some(fname.clone()),
            updated_at: if custom {
                prev.and_then(|p| p.updated_at.clone())
            } else {
                Some(now_str())
            },
        });
    }
    out
}

fn pretty_name(raw: &str) -> String {
    let base = raw.trim_end_matches(".bat");
    let tagged = base
        .replace(" (ALT", " · ALT")
        .replace(" (FAKE", " · FAKE")
        .replace(" (EXP", " · EXP")
        .replace(" (SIMPLE", " · SIMPLE");
    let mut s = tagged.to_string();
    s = s.replace(['(', ')'], "");
    if s == "general" {
        s = "General".into();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bat_into_argv() {
        let bat = "@echo off\r\n%SystemRoot%\\System32\\... \r\n\"%~dp0bin\\winws.exe\" --wf-tcp-out=80,443 ^\r\n --filter-tcp=80 --new\r\n";
        let args = parse_flowseal_bat(bat, Path::new("C:\\zapret"));
        assert!(args.iter().any(|a| a.contains("--wf-tcp-out=80,443")));
        assert!(args.len() >= 3, "ожидалось ≥3 аргумента, получено {:?}", args);
    }

    #[test]
    fn game_filter_ports_off() {
        assert_eq!(game_filter_ports("off"), ("12".into(), "12".into()));
        assert_eq!(game_filter_ports("all"), ("1024-65535".into(), "1024-65535".into()));
    }

    #[test]
    fn decodes_utf16_and_bom() {
        let text = "set \"BIN=%~dp0bin\"\r\nwinws.exe --new --filter-tcp=80";
        let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let decoded = decode_strategy_bytes(&bytes);
        assert!(decoded.contains("--filter-tcp=80"));
        assert!(decoded.contains("%~dp0"));
        assert!(decode_strategy_bytes(b"\xEF\xBB\xBF\"tok\"").contains("tok"));
        assert!(decode_strategy_bytes(b"plain ascii").contains("plain"));
    }
}