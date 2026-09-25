use crate::config::{Profile, ENGINE_FLOWSEAL};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Безопасный компонент имени лог-файла по id профиля: id приходит из внешнего
/// источника (имя .bat или OTA-пресет), а подставляется в `stdout-{id}.txt` —
/// `..`/разделители пускать нельзя, иначе файл ушёл бы за пределы `data/logs`.
pub fn log_file_component(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect();
    if cleaned.is_empty() || cleaned.contains("..") {
        "profile".into()
    } else {
        cleaned
    }
}

pub fn now_str() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

/// Похоже ли на UTF-16 без BOM: в типичном тексте (команды/пути .bat) каждый
/// второй байт — нулевой. Возвращает порядок байтов (LE) / big-endian (BE).
fn looks_like_utf16(bytes: &[u8]) -> Option<bool> {
    let even = bytes.len() - (bytes.len() % 2);
    if even < 8 {
        return None;
    }
    let pairs = even / 2;
    let zero_second = (0..pairs).filter(|i| bytes[i * 2 + 1] == 0).count();
    let zero_first = (0..pairs).filter(|i| bytes[i * 2] == 0).count();
    // Порог 9/10: обычный UTF-8/ASCII даёт почти ноль совпадений.
    if zero_second * 10 >= pairs * 9 {
        Some(false)
    } else if zero_first * 10 >= pairs * 9 {
        Some(true)
    } else {
        None
    }
}

/// Читает текст .bat независимо от кодировки: UTF-8 (±BOM), UTF-16 LE/BE с BOM
/// и UTF-16 без BOM (эвристика). Авторские стратегии из репозитория бывают
/// сохранены в UTF-16 — из-за этого токенизатор раньше получал пустой набор
/// аргументов и стратегия молча пропадала.
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
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = bytes[2..]
            .as_chunks::<2>().0.iter()
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    if let Some(big_endian) = looks_like_utf16(bytes) {
        let u16s: Vec<u16> = bytes
            .as_chunks::<2>().0.iter()
            .map(|c| if big_endian { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) })
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
                    if in_q {
                        // Внутри кавычек cmd не экранирует кареткой: обычный символ.
                        cur.push('^');
                    } else {
                        // Вне кавычек `^` делает ЛЮБОЙ следующий символ
                        // литеральным (в т.ч. пробел: `^ ` — часть аргумента,
                        // а не разделитель).
                        cur.push(n);
                        chars.next();
                    }
                    started = true;
                } else {
                    cur.push('^');
                    started = true;
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

/// Проверка и нормализация диапазонов портов в формате автора
/// (`service.bat: :gf_validate_item`): список `порт` или `start-end` через
/// запятую, каждый 1..65535, начало ≤ конца. None — формат неверен.
pub fn validate_port_range(raw: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for item in raw.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (a, b) = item
            .split_once('-')
            .map(|(a, b)| (a.trim(), b.trim()))
            .unwrap_or((item, item));
        let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else { return None };
        if a == 0 || b == 0 || a > 65535 || b > 65535 || a > b {
            return None;
        }
        parts.push(if a == b { a.to_string() } else { format!("{a}-{b}") });
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Диапазоны портов игрового фильтра: режимы как у автора (all/tcp/udp/off),
/// невалидный пользовательский диапазон не ломает запуск — берём дефолт.
pub fn game_filter_ports(mode: &str, tcp: &str, udp: &str) -> (String, String) {
    let tcp = validate_port_range(tcp).unwrap_or_else(|| "1024-65535".into());
    let udp = validate_port_range(udp).unwrap_or_else(|| "1024-65535".into());
    let (t, u) = match mode {
        "all" => (tcp.as_str(), udp.as_str()),
        "tcp" => (tcp.as_str(), "12"),
        "udp" => ("12", udp.as_str()),
        _ => ("12", "12"),
    };
    (t.into(), u.into())
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
    fn decodes_utf16_variants_including_big_endian() {
        let text = "\"%~dp0bin\\winws.exe\" --arg";
        let le_bom: Vec<u8> = [0xFF, 0xFE]
            .iter()
            .copied()
            .chain(text.encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        let be_bom: Vec<u8> = [0xFE, 0xFF]
            .iter()
            .copied()
            .chain(text.encode_utf16().flat_map(|u| u.to_be_bytes()))
            .collect();
        let le: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let be: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        for (label, bytes) in [("le+bom", le_bom), ("be+bom", be_bom), ("le", le), ("be", be)] {
            assert_eq!(decode_strategy_bytes(&bytes), text, "кодировка {label}");
        }
        assert_eq!(decode_strategy_bytes(text.as_bytes()), text);
    }

    #[test]
    fn caret_escapes_any_char_outside_quotes() {
        // `^ ` — литеральный пробел (не разделитель); в кавычках `^` обычный.
        assert_eq!(tokenize_cmd("a^ b \"c^d\""), vec!["a b".to_string(), "c^d".to_string()]);
    }

    #[test]
    fn log_component_neutralizes_path_traversal() {
        assert_eq!(log_file_component("general (ALT)"), "general__ALT_");
        assert_eq!(log_file_component("preset:zapret2-youtube"), "preset_zapret2-youtube");
        assert_eq!(log_file_component(r"..\..\evil"), "profile");
        assert_eq!(log_file_component(""), "profile");
    }

    #[test]
    fn game_filter_ports_off() {
        assert_eq!(game_filter_ports("off", "", ""), ("12".into(), "12".into()));
        assert_eq!(game_filter_ports("all", "", ""), ("1024-65535".into(), "1024-65535".into()));
        assert_eq!(game_filter_ports("tcp", "1000-2000", ""), ("1000-2000".into(), "12".into()));
        assert_eq!(game_filter_ports("udp", "мусор", "500-600"), ("12".into(), "500-600".into()));
    }

    #[test]
    fn port_range_validation_matches_author_rules() {
        // Пример самого автора (исключение RTMP).
        assert_eq!(validate_port_range("1024-1934,1936-65535").as_deref(), Some("1024-1934,1936-65535"));
        assert_eq!(validate_port_range("443").as_deref(), Some("443"));
        assert_eq!(validate_port_range(" 12 , 14-20 ").as_deref(), Some("12,14-20"));
        assert_eq!(validate_port_range("0"), None);
        assert_eq!(validate_port_range("65536"), None);
        assert_eq!(validate_port_range("100-50"), None);
        assert_eq!(validate_port_range("abc"), None);
        assert_eq!(validate_port_range(""), None);
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
