use crate::config::{Roots, Settings, UpdEntry, SELF_REPO};
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
    if let Some(root) = roots.path("flowseal") {
        for f in [
            "list-general.txt",
            "list-google.txt",
            "list-exclude.txt",
            "ipset-exclude.txt",
        ] {
            let dest = root.join("lists").join(f);
            push("flowseal lists", f, raw("Flowseal/zapret-discord-youtube", "main", &format!("lists/{}", f)), dest, false);
        }
        // ipset-all.txt: в GitHub лежит заглушка «none» (203.0.113.113/32), реальный
        // список — в .service/ipset-service.txt (так его же качает и service.bat).
        // Раньше GUI перетирал живой ipset заглушкой: все `--ipset=` правила не
        // совпадали ни с чем (QUIC/UDP YouTube и IP-правила переставали работать).
        push(
            "flowseal lists",
            "ipset-all.txt",
            raw("Flowseal/zapret-discord-youtube", "main", ".service/ipset-service.txt"),
            root.join("lists").join("ipset-all.txt"),
            false,
        );
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
    let mut result: Vec<UpdEntry> = out.into_iter().map(|(_, u)| u).collect();
    // OTA-набор пресетов — не файловая запись (живёт в state.json), добавляем
    // сводной строкой в конец каталога.
    result.push(check_preset_entry(&archive));
    Ok(result)
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

// ------------------------------------------------- OTA: набор пресетов

/// Один пресет из HTTP-набора (presets.json в ассетах релиза SELF_REPO).
#[derive(Clone, Debug)]
pub struct RemotePreset {
    pub id: String,
    pub engine: String,
    pub name: String,
    pub args: Vec<String>,
}

/// Разобранный набор: версия + сами пресеты.
#[derive(Clone, Debug, Default)]
pub struct RemotePresetSet {
    pub version: String,
    pub presets: Vec<RemotePreset>,
}

/// id сводной записи набора пресетов в каталоге обновлений и в applied.json.
pub const PRESETS_ENTRY_ID: &str = "presets:set";
/// Группа набора пресетов в UI.
pub const PRESETS_GROUP: &str = "Набор пресетов";

/// Разбирает presets.json: объект `{version, presets:[...]}` либо голый массив
/// пресетов. Пресеты с неизвестным движком пропускаются (набор мог быть новее
/// программы), у которых пустой id или args — тоже.
pub fn parse_preset_set(bytes: &[u8]) -> Result<RemotePresetSet, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("presets.json: {e}"))?;
    let (version, arr) = match &v {
        serde_json::Value::Array(a) => (String::new(), a.clone()),
        serde_json::Value::Object(_) => (
            v["version"].as_str().unwrap_or("").trim().to_string(),
            v["presets"].as_array().cloned().unwrap_or_default(),
        ),
        _ => return Err("presets.json: ожидался объект или массив".into()),
    };

    let mut presets = Vec::new();
    for item in arr {
        let id = item["id"].as_str().unwrap_or("").trim().to_string();
        let engine = item["engine"].as_str().unwrap_or("").trim().to_string();
        let name = item["name"].as_str().unwrap_or("").trim().to_string();
        let args: Vec<String> = item["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if id.is_empty() || args.is_empty() {
            continue;
        }
        if crate::config::engine_def(&engine).is_none() {
            continue;
        }
        let name = if name.is_empty() { id.clone() } else { name };
        presets.push(RemotePreset { id, engine, name, args });
    }
    if presets.is_empty() {
        return Err("presets.json: нет валидных пресетов".into());
    }
    Ok(RemotePresetSet { version, presets })
}

/// Качает presets.json из ассетов последнего релиза SELF_REPO.
pub fn fetch_preset_set() -> Result<RemotePresetSet, String> {
    let cli = client()?;
    let resp = cli
        .get(format!("https://api.github.com/repos/{}/releases/latest", SELF_REPO))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API: HTTP {}", resp.status()));
    }
    let rel: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let url = rel["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some("presets.json")))
        .and_then(|x| x["browser_download_url"].as_str())
        .map(str::to_string);
    let Some(url) = url else {
        return Err("в релизе нет ассета presets.json".into());
    };
    parse_preset_set(&fetch_bytes(&cli, &url)?)
}

/// Сводная запись набора пресетов для каталога обновлений.
pub fn check_preset_entry(archive: &UpdArchive) -> UpdEntry {
    let mut u = UpdEntry {
        id: PRESETS_ENTRY_ID.into(),
        group: PRESETS_GROUP.into(),
        label: "presets.json".into(),
        dest: String::new(),
        exists: false,
        status: "unknown".into(),
        remote_hash: None,
        applied_hash: None,
        local_hash: None,
        size: 0,
        error: None,
    };
    match fetch_preset_set() {
        Ok(set) => {
            u.exists = true;
            u.remote_hash = Some(set.version.clone());
            let applied = archive.applied(PRESETS_ENTRY_ID);
            u.applied_hash = applied.clone();
            // Версия набора пустая — не с чем сравнивать; считаем актуальным,
            // если набор уже применялся, иначе предлагаем применить.
            u.status = if (!set.version.is_empty() && applied.as_deref() == Some(set.version.as_str()))
                || (set.version.is_empty() && applied.is_some())
            {
                "ok".into()
            } else {
                "avail".into()
            };
        }
        Err(e) => {
            u.status = "err".into();
            u.error = Some(e);
        }
    }
    u
}

/// Применяет набор пресетов к профилям: обновляет builtin-пресеты
/// (id `preset:<id>`) и добавляет недостающие. Кастомные профили не трогаются.
/// Возвращает (обновлено, добавлено).
pub fn apply_preset_set(profiles: &mut Vec<crate::config::Profile>, set: &RemotePresetSet) -> (usize, usize) {
    let (mut updated, mut added) = (0usize, 0usize);
    for rp in &set.presets {
        let p = crate::presets::preset_profile(&rp.id, &rp.engine, &rp.name, rp.args.clone());
        match profiles.iter_mut().find(|x| x.id == p.id) {
            Some(ex) => {
                if ex.builtin {
                    if ex.args != p.args || ex.name != p.name || ex.engine != p.engine {
                        ex.args = p.args;
                        ex.name = p.name;
                        ex.engine = p.engine;
                        updated += 1;
                    }
                }
                // кастомный профиль с тем же id — не трогаем.
            }
            None => {
                profiles.push(p);
                added += 1;
            }
        }
    }
    (updated, added)
}

/// Применяет уже скачанный набор пресетов: обновляет builtin-пресеты в
/// `profiles`, фиксирует версию в applied.json. Кастомные профили не трогаются.
/// Возвращает (обновлено, добавлено). Сеть здесь НЕ используется — вызывающая
/// сторона качает набор заранее (вне блокировки state).
pub fn apply_presets(data: &Path, profiles: &mut Vec<crate::config::Profile>, set: &RemotePresetSet) -> (usize, usize) {
    let (updated, added) = apply_preset_set(profiles, set);
    let mut archive = UpdArchive::load(data);
    archive.record(PRESETS_ENTRY_ID, &set.version, crate::profiles::now_str());
    archive.save(data);
    (updated, added)
}

/// Заглушка Flowseal для режима ipset «none» (по ней service.bat определяет режим).
pub const IPSET_PLACEHOLDER: &str = "203.0.113.113/32\n";

/// Приводит `lists/ipset-all.txt` движка к выбранному режиму ipset:
/// - «loaded» — реальный список из `catalog/.service/ipset-service.txt`
///   (фолбэк: `lists/ipset-all.txt.backup` из архива движка);
/// - «none» — заглушка (ipset-правила отключены);
/// - «any» — пустой файл (правила по любому IP).
///
/// Без этого GUI оставлял заглушку из GitHub и `--ipset=ipset-all.txt`
/// не совпадал ни с одним адресом.
pub fn sync_ipset(root: &Path, data: &Path, settings: &Settings) {
    let dest = root.join("lists").join("ipset-all.txt");
    let body: Option<Vec<u8>> = match settings.ipset_mode.as_str() {
        "any" => Some(Vec::new()),
        "none" => Some(IPSET_PLACEHOLDER.as_bytes().to_vec()),
        _ => {
            let service = data.join("catalog/flowseal/.service/ipset-service.txt");
            let backup = root.join("lists/ipset-all.txt.backup");
            fs::read(&service).ok().or_else(|| fs::read(&backup).ok())
        }
    };
    // В «loaded» источника может не быть (нет ни catalog, ни backup) — файл не трогаем.
    let Some(bytes) = body else { return };
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = crate::config::atomic_write(&dest, &bytes);
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
    /// Убирает записи групп с указанным префиксом.
    #[allow(dead_code)]
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

    #[test]
    fn preset_set_parses_object_and_array_and_skips_unknown() {
        // Объект с version + массивом. Неизвестный движок и пустые args — пропуск.
        let body = r#"{"version":"2026.09.24","presets":[
            {"id":"z2-x","engine":"zapret2","name":"Z2 X","args":["--wf-tcp-out=80,443"]},
            {"id":"bad-engine","engine":"nope","name":"X","args":["-9"]},
            {"id":"no-args","engine":"goodbyedpi","name":"Y","args":[]}
        ]}"#;
        let set = parse_preset_set(body.as_bytes()).unwrap();
        assert_eq!(set.version, "2026.09.24");
        assert_eq!(set.presets.len(), 1, "пропуск неизвестного движка/пустых args");
        assert_eq!(set.presets[0].id, "z2-x");

        // Голый массив без версии.
        let arr = r#"[{"id":"gd","engine":"goodbyedpi","args":["-9"]}]"#;
        let set2 = parse_preset_set(arr.as_bytes()).unwrap();
        assert!(set2.version.is_empty());
        assert_eq!(set2.presets.len(), 1);

        // Мусор и пустой набор — ошибки.
        assert!(parse_preset_set(b"not json").is_err());
        assert!(parse_preset_set(br#"{"version":"1","presets":[]}"#).is_err());
    }

    #[test]
    fn apply_preset_set_updates_builtin_adds_new_keeps_custom() {
        let set = RemotePresetSet {
            version: "2026.09.24".into(),
            presets: vec![
                RemotePreset {
                    id: "z2-x".into(),
                    engine: "zapret2".into(),
                    name: "Z2 X".into(),
                    args: vec!["--wf-tcp-out=80,443".into()],
                },
                RemotePreset {
                    id: "new-one".into(),
                    engine: "goodbyedpi".into(),
                    name: "New".into(),
                    args: vec!["-9".into()],
                },
            ],
        };
        let mut profiles = vec![
            // builtin с тем же id, старыми аргументами — обновляется.
            crate::presets::preset_profile("z2-x", "zapret2", "старое", vec!["old".into()]),
            // кастомный профиль с СВОИМ id (не пресет) — не трогаем.
            crate::config::Profile {
                id: "мой".into(),
                name: "мой".into(),
                engine: "flowseal".into(),
                args: vec!["keep".into()],
                builtin: false,
                source: Some("manual".into()),
                updated_at: None,
            },
        ];
        let (updated, added) = apply_preset_set(&mut profiles, &set);
        assert_eq!(updated, 1, "builtin z2-x должен обновиться");
        assert_eq!(added, 1, "new-one должен добавиться");
        let z2 = profiles.iter().find(|p| p.id == "preset:z2-x").unwrap();
        assert_eq!(z2.args, vec!["--wf-tcp-out=80,443".to_string()]);
        assert!(z2.builtin);
        let custom = profiles.iter().find(|p| p.id == "мой").unwrap();
        assert_eq!(custom.args, vec!["keep".to_string()]);

        // Кастомный профиль, занявший id пресета, не перезаписывается.
        let mut profiles2 = vec![crate::config::Profile {
            id: "preset:z2-x".into(),
            name: "мой".into(),
            engine: "flowseal".into(),
            args: vec!["keep".into()],
            builtin: false,
            source: None,
            updated_at: None,
        }];
        let (upd2, add2) = apply_preset_set(&mut profiles2, &set);
        // z2-x занят кастомным (не трогаем), new-one добавляется.
        assert_eq!(upd2, 0);
        assert_eq!(add2, 1);
        assert_eq!(profiles2.iter().find(|p| p.id == "preset:z2-x").unwrap().args, vec!["keep".to_string()]);
    }

    #[test]
    fn sync_ipset_materializes_loaded_list() {
        let base = std::env::temp_dir().join(format!("zgui-ipset-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("data");
        let root = base.join("engine");
        fs::create_dir_all(data.join("catalog/flowseal/.service")).unwrap();
        fs::create_dir_all(root.join("lists")).unwrap();
        let real = vec![b'1'; 4096];
        fs::write(data.join("catalog/flowseal/.service/ipset-service.txt"), &real).unwrap();
        fs::write(root.join("lists/ipset-all.txt"), IPSET_PLACEHOLDER).unwrap();

        let mut settings = Settings { ipset_mode: "loaded".into(), ..Default::default() };
        sync_ipset(&root, &data, &settings);
        assert_eq!(fs::read(root.join("lists/ipset-all.txt")).unwrap(), real, "loaded: должен быть реальный список");

        // loaded без источника — файл не трогаем.
        fs::remove_file(data.join("catalog/flowseal/.service/ipset-service.txt")).unwrap();
        sync_ipset(&root, &data, &settings);
        assert_eq!(fs::read(root.join("lists/ipset-all.txt")).unwrap(), real);

        settings.ipset_mode = "none".into();
        sync_ipset(&root, &data, &settings);
        assert_eq!(
            String::from_utf8(fs::read(root.join("lists/ipset-all.txt")).unwrap()).unwrap(),
            IPSET_PLACEHOLDER
        );

        settings.ipset_mode = "any".into();
        sync_ipset(&root, &data, &settings);
        assert!(fs::read(root.join("lists/ipset-all.txt")).unwrap().is_empty(), "any: пустой файл");

        let _ = fs::remove_dir_all(&base);
    }
}
