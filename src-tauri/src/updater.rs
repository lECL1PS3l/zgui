use crate::config::{Roots, Settings, UpdEntry};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct CatEntry {
    pub id: String,
    pub group: String,
    pub label: String,
    pub url: String,
    pub dest: PathBuf,
    pub catalog_only: bool,
}

pub const UA: &str = "zgui/0.1 (zapret-gui updater)";

fn raw(repo: &str, branch: &str, path: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/refs/heads/{}/{}",
        repo, branch, path
    )
}

pub fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())
}

fn api(repo: &str, path: &str) -> String {
    format!("https://api.github.com/repos/{}/{}", repo, path)
}

/// Строит каталог обновляемых конфигов (whitelist). Бинарники и user-файлы не трогаются.
pub fn collect_entries(data: &Path, roots: &Roots, _settings: &Settings) -> Result<Vec<CatEntry>, String> {
    let cli = client()?;
    let mut out: Vec<CatEntry> = Vec::new();

    let mut push = |group: &str, label: &str, url: String, dest: PathBuf, catalog_only: bool| {
        let id = format!("{}:{}", group, label.replace(['/', '\\'], "__"));
        out.push(CatEntry { id, group: group.to_string(), label: label.to_string(), url, dest, catalog_only })
    };

    // flowseal: списки в live-корень движка
    if let Some(r) = roots.flowseal.as_ref() {
        let root = PathBuf::from(r);
        for f in [
            "list-general.txt",
            "list-google.txt",
            "list-exclude.txt",
            "ipset-exclude.txt",
            "ipset-all.txt",
        ] {
            let dest = root.join("lists").join(f);
            push("flowseal lists", f, raw("Flowseal/zapret-discord-youtube", "main", &format!("lists/{}", f)), dest, false);
        }
    }

    // flowseal: служебные данные для движка. Системный hosts GUI не применяет.
    for f in ["version.txt", "ipset-service.txt"] {
        let dest = data.join("catalog/flowseal/.service").join(f);
        push(
            "flowseal service",
            f,
            raw("Flowseal/zapret-discord-youtube", "main", &format!(".service/{}", f)),
            dest,
            true,
        );
    }

    // flowseal: стратегии (*.bat) — динамический список через GitHub API
    let bats = fetch_bat_names(&cli)?;
    for name in bats {
        let dest = data.join("catalog/flowseal/raw").join(&name);
        push(
            "flowseal strategies",
            &name,
            raw("Flowseal/zapret-discord-youtube", "main", &name),
            dest,
            true,
        );
    }

    // geoblock: списки заблокированных в РФ доменов (itdoginfo/allow-domains)
    let geo = data.join("catalog/geoblock");
    let geolists: [(&str, &str); 9] = [
        ("allow-domains-russia-inside.lst", "Russia/inside-raw.lst"),
        ("allow-domains-geoblock.lst", "Categories/geoblock.lst"),
        ("allow-domains-block.lst", "Categories/block.lst"),
        ("allow-domains-news.lst", "Categories/news.lst"),
        ("allow-domains-youtube.lst", "Services/youtube.lst"),
        ("allow-domains-discord.lst", "Services/discord.lst"),
        ("allow-domains-telegram.lst", "Services/telegram.lst"),
        ("allow-domains-twitter.lst", "Services/twitter.lst"),
        ("allow-domains-meta.lst", "Services/meta.lst"),
    ];
    for (label, path) in geolists {
        push(
            "geoblock domains",
            label,
            raw("itdoginfo/allow-domains", "main", path),
            geo.join(label),
            true,
        );
    }

    // geoblock IP: заблокированные Роскомнадзором подсети (runetfreedom/russia-blocked-geoip)
    let ips: [(&str, &str); 2] = [
        ("russia-blocked-text.lst", "text/ru-blocked.txt"),
        ("russia-blocked-community-text.lst", "text/ru-blocked-community.txt"),
    ];
    for (label, path) in ips {
        push(
            "geoblock ip",
            label,
            raw("runetfreedom/russia-blocked-geoip", "release", path),
            geo.join(label),
            true,
        );
    }

    Ok(out)
}

fn fetch_bat_names(cli: &reqwest::blocking::Client) -> Result<Vec<String>, String> {
    let resp = cli
        .get(api("Flowseal/zapret-discord-youtube", "contents/?ref=main"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API: HTTP {}", resp.status()));
    }
    let arr: Vec<serde_json::Value> = resp.json().map_err(|e| e.to_string())?;
    let mut names: Vec<String> = arr
        .into_iter()
        .filter_map(|v| {
            let name = v["name"].as_str()?.to_string();
            let is_bat = name.to_lowercase().ends_with(".bat");
            let is_service = name.to_lowercase().starts_with("service");
            if is_bat && !is_service {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    Ok(names)
}

/// Тег последнего релиза движка Flowseal (без ведущей «v»).
pub fn check_engine_latest() -> Result<String, String> {
    let cli = client()?;
    let resp = cli
        .get(api("Flowseal/zapret-discord-youtube", "releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API: HTTP {}", resp.status()));
    }
    let rel: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let tag = rel["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim()
        .trim_start_matches('v')
        .to_string();
    if tag.is_empty() {
        return Err("в релизе нет tag_name".into());
    }
    Ok(tag)
}

fn fetch_bytes(cli: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = cli
        .get(url)
        .header("Cache-Control", "no-cache")
        .send()
        .map_err(|e| format!("{}: {}", url, e))?;
    if !resp.status().is_success() {
        return Err(format!("{}: HTTP {}", url, resp.status()));
    }
    resp.bytes()
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
}

fn check_entry(cli: &reqwest::blocking::Client, e: &CatEntry, settings: &Settings, archive: &UpdArchive) -> UpdEntry {
    let mut u = UpdEntry {
        id: e.id.clone(),
        group: e.group.clone(),
        label: e.label.clone(),
        dest: e.dest.to_string_lossy().into_owned(),
        exists: false,
        status: "unknown".into(),
        remote_hash: None,
        applied_hash: None,
        local_hash: None,
        size: 0,
        error: None,
    };

    // ipset-all: уважаем ручной режим none/any
    if e.label == "ipset-all.txt" && settings.ipset_mode != "loaded" {
        u.status = "skip-user".into();
        return u;
    }

    let bytes = match fetch_bytes(cli, &e.url) {
        Ok(b) => b,
        Err(err) => {
            u.status = "err".into();
            u.error = Some(err);
            return u;
        }
    };
    u.size = bytes.len() as u64;
    let remote = crate::config::sha256_hex(&bytes);
    u.remote_hash = Some(remote.clone());
    let applied = archive.applied(e.id.as_str());
    u.applied_hash = applied.clone();

    if e.dest.exists() {
        u.exists = true;
        let local = crate::config::file_sha256(&e.dest);
        u.local_hash = local.clone();
        if local.as_deref() == Some(remote.as_str()) {
            u.status = "ok".into();
        } else if applied == local || applied.is_none() {
            // Файл отличается от удалённого и мы его не применяли (или применяли
            // ровно то, что лежит сейчас) — можно спокойно обновить.
            u.status = "avail".into();
        } else {
            u.status = "modified".into();
        }
    } else {
        u.status = "new".into();
    }
    u
}

/// Проверяет все конфиги из каталога. Записи проверяются параллельно
/// (45+ сетевых запросов), порядок в результате сохраняется исходный.
pub fn check_all(data: &Path, roots: &Roots, settings: &Settings) -> Result<Vec<UpdEntry>, String> {
    let cli = Arc::new(client()?);
    let archive = Arc::new(UpdArchive::load(data));
    let entries = collect_entries(data, roots, settings)?;
    let settings = Arc::new(settings.clone());
    let mut out: Vec<(usize, UpdEntry)> = std::thread::scope(|scope| {
        let handles: Vec<_> = entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| {
                let (cli, settings, archive) = (cli.clone(), settings.clone(), archive.clone());
                scope.spawn(move || (i, check_entry(&cli, &e, &settings, &archive)))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or((0, UpdEntry::default())))
            .collect()
    });
    out.sort_by_key(|(i, _)| *i);
    Ok(out.into_iter().map(|(_, u)| u).collect())
}

/// Применяет выбранные обновления (ид-ы или все доступные).
pub fn apply_updates(data: &Path, roots: &Roots, settings: &Settings, ids: Vec<String>) -> Result<Vec<UpdEntry>, String> {
    let cli = client()?;
    let mut archive = UpdArchive::load(data);
    let entries = collect_entries(data, roots, settings)?;
    let ts = crate::profiles::now_str();
    let mut out: Vec<UpdEntry> = Vec::new();

    for e in entries {
        let u0 = check_entry(&cli, &e, settings, &archive);
        let selected = if ids.is_empty() {
            matches!(u0.status.as_str(), "avail" | "new")
        } else {
            ids.contains(&e.id)
        };
        if !selected {
            out.push(u0);
            continue;
        }
        if matches!(u0.status.as_str(), "err" | "skip-user") {
            out.push(u0);
            continue;
        }
        let bytes = match fetch_bytes(&cli, &e.url) {
            Ok(b) => b,
            Err(err) => {
                let mut u = u0.clone();
                u.status = "err".into();
                u.error = Some(err);
                out.push(u);
                continue;
            }
        };
        let remote = crate::config::sha256_hex(&bytes);
        if e.dest.exists() {
            backup(&e.dest, &ts, data, e.catalog_only);
        }
        if let Some(parent) = e.dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if crate::config::atomic_write(&e.dest, &bytes).is_err() {
            let mut u = u0.clone();
            u.status = "err".into();
            u.error = Some("не удалось записать файл".into());
            archive.record(&e.id, &remote, ts.clone());
            out.push(u);
            continue;
        }
        archive.record(&e.id, &remote, ts.clone());
        let mut u = u0;
        u.status = "ok".into();
        u.applied_hash = Some(remote.clone());
        u.local_hash = crate::config::file_sha256(&e.dest);
        u.exists = true;
        u.error = None;
        out.push(u);
    }

    archive.save(data);
    Ok(out)
}

/// Реестр применённых хэшей (что конкретно мы записали).
#[derive(Clone, Default)]
pub struct UpdArchive {
    map: std::collections::HashMap<String, String>,
}

impl UpdArchive {
    fn file(data: &Path) -> PathBuf {
        data.join("catalog/applied.json")
    }
    pub fn load(data: &Path) -> Self {
        let m = fs::read_to_string(Self::file(data))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { map: m }
    }
    pub fn applied(&self, id: &str) -> Option<String> {
        self.map.get(id).cloned()
    }
    /// Убирает записи групп с указанным префиксом (например, вырезанного движка).
    pub fn purge_prefix(&mut self, prefix: &str) -> usize {
        let before = self.map.len();
        self.map.retain(|k, _| !k.starts_with(prefix));
        before - self.map.len()
    }
    pub fn record(&mut self, id: &str, hash: &str, _ts: String) {
        self.map.insert(id.to_string(), hash.to_string());
    }
    pub fn save(&self, data: &Path) {
        let _ = fs::write(Self::file(data), serde_json::to_string(&self.map).unwrap_or_default());
    }
}

fn backup(src: &Path, ts: &str, data: &Path, catalog_only: bool) {
    let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let dir = if catalog_only {
        data.join("catalog/.backups").join(ts)
    } else {
        let Some(parent) = src.parent() else { return };
        parent.join(".backups").join(ts)
    };
    let _ = fs::create_dir_all(&dir);
    let _ = fs::copy(src, dir.join(&name));
}

// ------------------------------------------------- telegram bridge update

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TgBridgeInfo {
    /// Версия встроенного крейта (compile-time).
    pub local_version: String,
    /// Последняя версия крейта в апстриме ZUI (если удалось узнать).
    pub upstream_version: Option<String>,
    /// Последний коммит Flowseal/tg-ws-proxy (дата + сообщение).
    pub upstream_commit: Option<String>,
    /// Есть ли обновление моста.
    pub update_available: bool,
    /// Человеко-читаемое пояснение/ошибка.
    pub note: Option<String>,
}

/// Извлекает `version = "x.y.z"` из содержимого Cargo.toml.
fn parse_toml_version(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("version") {
            let rest = rest.trim_start().trim_start_matches('=').trim();
            let v = rest.trim_matches('"').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Сравнивает вкомпилированную версию моста с апстримом.
/// Два сигнала: точная версия крейта в ZUI и свежесть коммитов Flowseal.
pub fn check_tg_bridge() -> TgBridgeInfo {
    let local_version = tg_ws_proxy_rs::VERSION.to_string();
    let mut info = TgBridgeInfo {
        local_version: local_version.clone(),
        upstream_version: None,
        upstream_commit: None,
        update_available: false,
        note: None,
    };

    let Ok(cli) = client() else {
        info.note = Some("не удалось создать HTTP-клиент".into());
        return info;
    };

    // Сигнал 1: версия крейта в апстриме ZUI.
    let zui_url = "https://raw.githubusercontent.com/AmantesNihilo/zapret-universal-interface/main/crates/tg-ws-proxy-rs/Cargo.toml";
    match fetch_bytes(&cli, zui_url) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            if let Some(v) = parse_toml_version(&text) {
                info.update_available = version_is_newer(&v, &local_version);
                info.upstream_version = Some(v);
            }
        }
        Err(e) => {
            info.note = Some(format!("не удалось узнать версию ZUI: {}", e));
        }
    }

    // Сигнал 2: последний коммит Flowseal/tg-ws-proxy (информационно).
    let commits_url = api("Flowseal/tg-ws-proxy", "commits?per_page=1");
    if let Ok(bytes) = fetch_bytes(&cli, &commits_url) {
        let text = String::from_utf8_lossy(&bytes);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(arr) = v.as_array() {
                if let Some(first) = arr.first() {
                    let date = first["commit"]["committer"]["date"].as_str().unwrap_or("").to_string();
                    let msg = first["commit"]["message"].as_str().unwrap_or("").lines().next().unwrap_or("").to_string();
                    if !date.is_empty() {
                        info.upstream_commit = Some(format!("{} — {}", date, msg));
                    }
                }
            }
        }
    }

    info
}

/// Сравнивает версии вида `2.3.4-zui.2`: числовая основа покомпонентно,
/// затем номер суффикса (последнее число после `-`). Отсутствие суффикса = 0.
fn version_is_newer(candidate: &str, current: &str) -> bool {
    fn split(s: &str) -> (Vec<u64>, u64) {
        let mut it = s.splitn(2, '-');
        let base = it.next().unwrap_or("");
        let suffix = it.next().unwrap_or("");
        let nums = base.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect();
        let sn = suffix.rsplit('.').next().unwrap_or("").parse::<u64>().unwrap_or(0);
        (nums, sn)
    }
    let (a, asuf) = split(candidate);
    let (b, bsuf) = split(current);
    if a != b {
        return a > b;
    }
    asuf > bsuf
}

#[cfg(test)]
mod tg_tests {
    use super::*;

    #[test]
    fn parses_version_from_cargo_toml() {
        let t = "[package]\nname = \"x\"\nversion = \"2.3.4-zui.2\"\nedition = \"2024\"\n";
        assert_eq!(parse_toml_version(t).as_deref(), Some("2.3.4-zui.2"));
    }

    #[test]
    fn version_compare() {
        assert!(version_is_newer("2.4.0", "2.3.4-zui.2"));
        assert!(version_is_newer("2.3.5", "2.3.4"));
        assert!(!version_is_newer("2.3.4", "2.3.4-zui.2"));
        assert!(version_is_newer("2.3.4-zui.3", "2.3.4-zui.2"));
        assert!(!version_is_newer("2.3.4-zui.2", "2.3.4-zui.2"));
    }
}
