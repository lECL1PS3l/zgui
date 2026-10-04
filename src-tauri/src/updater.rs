use crate::config::{Roots, Settings, UpdEntry, SELF_REPO};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const UA: &str = "zgui/0.1 (zapret-gui updater)";

/// Верхний предел скачиваемого тела: архивы автора и presets.json — единицы
/// МиБ, 64 МиБ с запасом; защита от «бесконечного» ответа (память/диск).
pub const MAX_FETCH: u64 = 64 * 1024 * 1024;

/// Потолок на одну запись архива и их число — защита от zip-бомбы.
const MAX_ZIP_ENTRY: u64 = 16 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 5000;

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

/// Скачивает один файл с жёстким лимитом размера.
fn fetch_bytes(cli: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = cli
        .get(url)
        .header("Cache-Control", "no-cache")
        .send()
        .map_err(|e| format!("{}: {}", url, e))?;
    if !resp.status().is_success() {
        return Err(format!("{}: HTTP {}", url, resp.status()));
    }
    read_limited(resp, url)
}

/// Читает тело с лимитом: даже чанковый ответ без `Content-Length` не раздует память.
fn read_limited(resp: reqwest::blocking::Response, url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if resp.content_length().unwrap_or(0) > MAX_FETCH {
        return Err(format!("{url}: файл слишком большой"));
    }
    let mut buf = Vec::new();
    resp.take(MAX_FETCH + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{url}: {e}"))?;
    if buf.len() as u64 > MAX_FETCH {
        return Err(format!("{url}: файл слишком большой"));
    }
    Ok(buf)
}

/// Имя *.bat стратегии: только простой файл в корне репозитория — без
/// разделителей пути, `..`, скрытых имён и без префикса `service`.
fn bat_name_ok(name: &str) -> bool {
    let l = name.to_lowercase();
    !name.is_empty()
        && name.len() <= 120
        && !name.contains(['/', '\\'])
        && !name.contains("..")
        && !name.starts_with('.')
        && !name.chars().any(|c| c.is_control())
        && l.ends_with(".bat")
        && !l.starts_with("service")
}

// ------------------------------------------------- обновление из zip автора

/// Запись каталога обновлений: файл архива + куда он ложится и что там лежит.
pub struct CatEntry {
    pub id: String,
    pub group: String,
    pub label: String,
    pub dest: PathBuf,
    /// Копия файла живёт только в каталоге (не в движке) — её бэкап кладём в catalog.
    pub catalog_only: bool,
    pub bytes: Vec<u8>,
}

/// Разбирает zip репозитория: `(путь-без-обёртки, содержимое)`.
/// Обёртка GitHub (`repo-main/`) срезается; Dirs пропускаются.
fn read_repo_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::Read;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(format!("слишком много файлов в архиве: {}", archive.len()));
    }
    let mut out = Vec::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let raw = entry.name().to_string();
        // Срезаем каталог-обёртку GitHub («repo-main/...»).
        let rel = raw
            .split_once('/')
            .map(|(_, r)| r.to_string())
            .unwrap_or(raw);
        if rel.is_empty() || rel.contains('\\') || rel.split('/').any(|p| p == ".." || p.is_empty())
        {
            continue;
        }
        let size = entry.size();
        if size > MAX_ZIP_ENTRY || total.saturating_add(size) > MAX_FETCH {
            crate::logger::log_code(
                "warn",
                "updater",
                "W-UPD-004",
                &format!("пропускаю крупную запись архива: {rel}"),
            );
            continue;
        }
        let mut buf = Vec::with_capacity(size as usize);
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        total += buf.len() as u64;
        if total > MAX_FETCH {
            return Err("архив превышает допустимый размер".into());
        }
        out.push((rel, buf));
    }
    if out.is_empty() {
        return Err("архив пуст или не содержит файлов".into());
    }
    Ok(out)
}

/// Раскладывает файлы архива по каталогу обновлений:
/// - `*.bat` (корень репо) → `data/catalog/flowseal/raw` (стратегии-профили);
/// - `lists/*` → `lists/` живого движка flowseal (кроме заглушки ipset-all.txt);
/// - `.service/ipset-service.txt` → реальный `ipset-all.txt` движка + копия в каталог.
fn collect_entries(files: Vec<(String, Vec<u8>)>, data: &Path, roots: &Roots) -> Vec<CatEntry> {
    let mut out: Vec<CatEntry> = Vec::new();
    let root = roots.path("flowseal");
    let mut push = |group: &str, label: &str, dest: PathBuf, catalog_only: bool, bytes: Vec<u8>| {
        let id = format!("{}:{}", group, label.replace(['/', '\\'], "__"));
        out.push(CatEntry {
            id,
            group: group.to_string(),
            label: label.to_string(),
            dest,
            catalog_only,
            bytes,
        });
    };

    for (rel, bytes) in files {
        if !rel.contains('/') {
            // Стратегии автора — только простые *.bat имена.
            if bat_name_ok(&rel) {
                push(
                    "flowseal strategies",
                    &rel,
                    data.join("catalog/flowseal/raw").join(&rel),
                    true,
                    bytes,
                );
            }
            continue;
        }
        let (dir, name) = rel.split_once('/').unwrap_or(("", rel.as_str()));
        if name.contains('/') {
            continue;
        }
        match dir {
            "lists" if name != "ipset-all.txt" => {
                // Списки — в живой движок (если он есть). Заглушку ipset-all.txt
                // из репозитория не берём: реальный список — в .service.
                if let Some(root) = &root {
                    push(
                        "flowseal lists",
                        name,
                        root.join("lists").join(name),
                        false,
                        bytes,
                    );
                }
            }
            ".service" if name == "ipset-service.txt" => {
                if let Some(root) = &root {
                    push(
                        "flowseal lists",
                        "ipset-all.txt",
                        root.join("lists/ipset-all.txt"),
                        false,
                        bytes.clone(),
                    );
                }
                let dest = data.join("catalog/flowseal/.service").join(name);
                push("flowseal service", name, dest, true, bytes);
            }
            _ => {}
        }
    }
    out
}

fn entry_base(e: &CatEntry) -> UpdEntry {
    UpdEntry {
        id: e.id.clone(),
        group: e.group.clone(),
        label: e.label.clone(),
        dest: e.dest.to_string_lossy().into_owned(),
        exists: false,
        status: "unknown".into(),
        remote_hash: None,
        applied_hash: None,
        local_hash: None,
        size: e.bytes.len() as u64,
        error: None,
        added: 0,
        removed: 0,
    }
}

/// Уникальные непустые строки (для сравнения списков).
fn norm_lines(s: &str) -> std::collections::HashSet<&str> {
    s.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Дельта строк текстового списка: (добавлено, удалено) по уникальным
/// непустым строкам. Для бинарных/не-UTF8 файлов вернёт (0, 0).
fn line_delta(old: &[u8], new: &[u8]) -> (u64, u64) {
    let (Ok(o), Ok(n)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return (0, 0);
    };
    let so = norm_lines(o);
    let sn = norm_lines(n);
    (
        sn.difference(&so).count() as u64,
        so.difference(&sn).count() as u64,
    )
}

/// ipset-all: уважаем ручной режим none/any — файл ведёт пользователь.
fn ipset_skipped(e: &CatEntry, settings: &Settings) -> bool {
    e.label == "ipset-all.txt" && settings.ipset_mode != "loaded"
}

/// Статус записи: файла нет → new; байты совпадают с архивом → ok;
/// отличается (в т.ч. руками юзера) → modified.
fn classify_entry(e: &CatEntry, settings: &Settings) -> UpdEntry {
    let mut u = entry_base(e);
    u.remote_hash = Some(crate::config::sha256_hex(&e.bytes));
    if ipset_skipped(e, settings) {
        u.status = "skip-user".into();
        return u;
    }
    match fs::read(&e.dest) {
        Ok(local) => {
            u.exists = true;
            u.local_hash = Some(crate::config::sha256_hex(&local));
            u.status = if local == e.bytes {
                "ok".into()
            } else {
                "modified".into()
            };
        }
        Err(_) => u.status = "new".into(),
    }
    u
}

/// Скачивает один zip-архив репозитория автора.
fn fetch_repo_zip(cli: &reqwest::blocking::Client) -> Result<Vec<(String, Vec<u8>)>, String> {
    let repo = crate::config::engine_def("flowseal")
        .ok_or_else(|| "flowseal: движок не найден в реестре".to_string())?
        .repo;
    let url = format!("https://codeload.github.com/{}/zip/refs/heads/main", repo);
    let bytes = fetch_bytes(cli, &url)?;
    read_repo_zip(&bytes)
}

/// Проверяет каталог: скачивает архив один раз и классифицирует все записи.
pub fn check_all(data: &Path, roots: &Roots, settings: &Settings) -> Result<Vec<UpdEntry>, String> {
    let cli = client()?;
    let files = fetch_repo_zip(&cli)?;
    let mut result: Vec<UpdEntry> = collect_entries(files, data, roots)
        .iter()
        .map(|e| classify_entry(e, settings))
        .collect();
    // OTA-набор пресетов — не файловая запись (живёт в state.json), добавляем
    // сводной строкой в конец каталога.
    result.push(check_preset_entry(data));
    Ok(result)
}

/// Применяет выбранные обновления (ид-ы или все доступные: new/modified).
/// Перед перезаписью существующего файла создаётся копия в `.backups/<ts>`;
/// если копию сделать не удалось — файл не трогаем.
pub fn apply_updates(
    data: &Path,
    roots: &Roots,
    settings: &Settings,
    ids: Vec<String>,
) -> Result<Vec<UpdEntry>, String> {
    let cli = client()?;
    let files = fetch_repo_zip(&cli)?;
    let entries = collect_entries(files, data, roots);
    Ok(apply_entries(&entries, data, roots, settings, ids))
}

/// Применение без сети (отделено для тестов).
fn apply_entries(
    entries: &[CatEntry],
    data: &Path,
    roots: &Roots,
    settings: &Settings,
    ids: Vec<String>,
) -> Vec<UpdEntry> {
    let ts = crate::profiles::now_str();
    let mut out: Vec<UpdEntry> = Vec::new();
    let mut applied_lists = false;

    for e in entries {
        let u0 = classify_entry(e, settings);
        let selected = if ids.is_empty() {
            matches!(u0.status.as_str(), "new" | "modified")
        } else {
            ids.contains(&e.id)
        };
        if !selected || matches!(u0.status.as_str(), "err" | "skip-user") {
            out.push(u0);
            continue;
        }
        // Дельта строк (для текстовых списков) до подмены файла.
        let (d_added, d_removed) = match fs::read(&e.dest) {
            Ok(old) => line_delta(&old, &e.bytes),
            Err(_) => (0, 0),
        };
        if e.dest.exists() && !backup(&e.dest, &ts, data, e.catalog_only) {
            let mut u = u0;
            u.status = "err".into();
            u.error = Some(crate::texts::BACKUP_FAILED.into());
            out.push(u);
            continue;
        }
        if let Some(parent) = e.dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if crate::config::atomic_write(&e.dest, &e.bytes).is_err() {
            let mut u = u0;
            u.status = "err".into();
            u.error = Some("не удалось записать файл".into());
            out.push(u);
            continue;
        }
        if e.label != "ipset-all.txt" && e.group == "flowseal lists" {
            applied_lists = true;
        }
        let mut u = u0;
        u.status = "ok".into();
        u.exists = true;
        u.local_hash = Some(crate::config::sha256_hex(&e.bytes));
        u.error = None;
        u.added = d_added;
        u.removed = d_removed;
        out.push(u);
    }

    // После обновления списков приводим ipset-all.txt к режиму пользователя.
    if applied_lists {
        if let Some(root) = roots.path("flowseal") {
            sync_ipset(&root, data, settings);
        }
    }
    out
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

/// id сводной записи набора пресетов в каталоге обновлений.
pub const PRESETS_ENTRY_ID: &str = "presets:set";
/// Группа набора пресетов в UI.
pub const PRESETS_GROUP: &str = "Набор пресетов";

/// Разбирает presets.json: объект `{version, presets:[...]}` либо голый массив.
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
        // args должен быть массивом СТРОК целиком: нестроковый элемент — битый
        // пресет (усечённая команда опаснее), пропускаем его полностью.
        let args: Option<Vec<String>> = item["args"].as_array().and_then(|a| {
            a.iter()
                .map(|x| x.as_str().map(str::to_string))
                .collect::<Option<Vec<String>>>()
        });
        let Some(args) = args else { continue };
        if args.is_empty() {
            continue;
        }
        if !crate::presets::valid_preset_id(&id) {
            if !id.is_empty() {
                crate::logger::log(
                    "warn",
                    "updater",
                    &format!("пресет с недопустимым id пропущен: {id:?}"),
                );
            }
            continue;
        }
        if crate::config::engine_def(&engine).is_none() {
            continue;
        }
        let name = if name.is_empty() { id.clone() } else { name };
        presets.push(RemotePreset {
            id,
            engine,
            name,
            args,
        });
    }
    if presets.is_empty() {
        return Err("presets.json: нет валидных пресетов".into());
    }
    Ok(RemotePresetSet { version, presets })
}

/// Качает presets.json из ассетов последнего релиза SELF_REPO.
/// `Ok(None)` — ассет ещё не опубликован: это НЕ ошибка (встроенные пресеты
/// актуальны), иначе «Применить доступные» падало бы на пустом месте.
pub fn fetch_preset_set() -> Result<Option<RemotePresetSet>, String> {
    let cli = client()?;
    let resp = cli
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            SELF_REPO
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API: HTTP {}", resp.status()));
    }
    let rel: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let url = rel["assets"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|x| x["name"].as_str() == Some("presets.json"))
        })
        .and_then(|x| x["browser_download_url"].as_str())
        .unwrap_or("")
        .to_string();
    if url.is_empty() {
        return Ok(None);
    }
    let bytes = fetch_bytes(&cli, &url)?;
    Ok(Some(parse_preset_set(&bytes)?))
}

/// Версия последнего применённого набора пресетов (файл в каталоге данных).
pub fn preset_stamp(data: &Path) -> Option<String> {
    fs::read_to_string(data.join("catalog/presets.version"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn save_preset_stamp(data: &Path, version: &str) {
    let p = data.join("catalog/presets.version");
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = crate::config::atomic_write(&p, version.as_bytes());
}

// ------------------------------------------------- обновление самой программы

/// Сведения о свежем релизе самой программы (SELF_REPO).
#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct AppUpdate {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub asset: Option<String>,
    pub url: Option<String>,
    pub size: Option<u64>,
    pub notes: Option<String>,
    pub error: Option<String>,
}

fn latest_release(cli: &reqwest::blocking::Client) -> Result<serde_json::Value, String> {
    let resp = cli
        .get(api(SELF_REPO, "releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API: HTTP {}", resp.status()));
    }
    resp.json().map_err(|e| e.to_string())
}

/// Выбирает из ассетов релиза портативный архив программы.
/// Приоритет: zip с именем программы (`z-gui`/`stable`), затем любой zip кроме `engine-*.zip`.
fn pick_zip_asset(rel: &serde_json::Value) -> Option<(String, String, u64)> {
    let arr = rel["assets"].as_array()?;
    let lname = |a: &&serde_json::Value| a["name"].as_str().unwrap_or("").to_lowercase();
    let a = arr
        .iter()
        .find(|a| {
            let n = lname(a);
            n.ends_with(".zip") && (n.contains("z-gui") || n.contains("stable"))
        })
        .or_else(|| {
            arr.iter()
                .find(|a| {
                    let n = lname(a);
                    n.ends_with(".zip") && !n.contains("engine-")
                })
        })
        .or_else(|| arr.iter().find(|a| lname(a).ends_with(".zip")))?;
    Some((
        a["name"].as_str()?.to_string(),
        a["browser_download_url"].as_str()?.to_string(),
        a["size"].as_u64().unwrap_or(0),
    ))
}

/// Сравнение версий вида `1.2.3` (покомпонентно, числа). true, если `a` новее `b`.
fn version_gt(a: &str, b: &str) -> bool {
    let pa: Vec<u32> = a.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let pb: Vec<u32> = b.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// Проверяет свежий релиз программы. Ошибки не пробрасываем — кладём в поле `error`.
pub fn app_update_info() -> AppUpdate {
    let mut info = AppUpdate {
        current: env!("CARGO_PKG_VERSION").to_string(),
        ..Default::default()
    };
    let cli = match client() {
        Ok(c) => c,
        Err(e) => {
            info.error = Some(e);
            return info;
        }
    };
    let rel = match latest_release(&cli) {
        Ok(r) => r,
        Err(e) => {
            info.error = Some(e);
            return info;
        }
    };
    let tag = rel["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    if let Some((name, url, size)) = pick_zip_asset(&rel) {
        info.asset = Some(name);
        info.url = Some(url);
        info.size = Some(size);
    }
    info.notes = rel["body"].as_str().map(|s| s.to_string());
    info.available = version_gt(&tag, &info.current) && info.url.is_some();
    info.latest = if tag.is_empty() { None } else { Some(tag) };
    info
}

/// Скачивает архив последнего релиза в `<data>/updates/`, возвращает путь к файлу.
pub fn app_update_download(data: &Path) -> Result<String, String> {
    let info = app_update_info();
    if let Some(e) = info.error {
        return Err(e);
    }
    let (name, url) = match (info.asset, info.url) {
        (Some(n), Some(u)) => (n, u),
        _ => return Err("в последнем релизе не найден архив программы".into()),
    };
    // Имя ассета — внешние данные: берём только базовое имя без разделителей пути.
    let safe = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if safe.is_empty() || safe.contains("..") || safe.len() > 120 {
        return Err("в релизе некорректное имя архива".into());
    }
    let cli = client()?;
    let bytes = fetch_bytes(&cli, &url)?;
    if let Some(want) = info.size {
        if want > 0 && bytes.len() as u64 != want {
            return Err(format!(
                "размер архива не совпал (ожидалось {want} б, получено {} б)",
                bytes.len()
            ));
        }
    }
    let dir = data.join("updates");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(safe);
    crate::config::atomic_write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(dest.display().to_string())
}

/// Сводная запись набора пресетов для каталога обновлений.
pub fn check_preset_entry(data: &Path) -> UpdEntry {
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
        added: 0,
        removed: 0,
    };
    match fetch_preset_set() {
        // Ассета в релизе ещё нет — не ошибка: работают встроенные пресеты.
        Ok(None) => {
            u.label = "presets.json — не опубликован (встроенные пресеты)".into();
            u.status = "skip-user".into();
        }
        Ok(Some(set)) => {
            u.exists = true;
            u.remote_hash = Some(set.version.clone());
            u.applied_hash = preset_stamp(data);
            u.status = if u.applied_hash.as_deref() == Some(set.version.as_str()) {
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
pub fn apply_preset_set(
    profiles: &mut Vec<crate::config::Profile>,
    set: &RemotePresetSet,
) -> (usize, usize) {
    let (mut updated, mut added) = (0usize, 0usize);
    for rp in &set.presets {
        let p = crate::presets::preset_profile(&rp.id, &rp.engine, &rp.name, rp.args.clone());
        match profiles.iter_mut().find(|x| x.id == p.id) {
            Some(ex) => {
                if ex.builtin && (ex.args != p.args || ex.name != p.name || ex.engine != p.engine) {
                    ex.args = p.args;
                    ex.name = p.name;
                    ex.engine = p.engine;
                    updated += 1;
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

/// Применяет уже скачанный набор пресетов и фиксирует его версию.
/// Сеть здесь НЕ используется — вызывающая сторона качает набор заранее.
pub fn apply_presets(
    data: &Path,
    profiles: &mut Vec<crate::config::Profile>,
    set: &RemotePresetSet,
) -> (usize, usize) {
    let (updated, added) = apply_preset_set(profiles, set);
    save_preset_stamp(data, &set.version);
    (updated, added)
}

/// Заглушка Flowseal для режима ipset «none» (по ней service.bat определяет режим).
pub const IPSET_PLACEHOLDER: &str = "203.0.113.113/32\n";

/// Приводит `lists/ipset-all.txt` движка к выбранному режиму ipset:
/// - «loaded» — реальный список из `catalog/.service/ipset-service.txt`
///   (фолбэк: `lists/ipset-all.txt.backup` из архива движка);
/// - «none» — заглушка (ipset-правила отключены);
/// - «any» — пустой файл (правила по любому IP).
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
    let Some(mut bytes) = body else { return };
    // Пользовательские подсети доклеиваем только в «loaded»: режимы none/any
    // намеренно отключают ipset-правила, include там не нужен.
    if settings.ipset_mode == "loaded" {
        let user =
            crate::scanner::read_user_include(&root.join("lists").join("ipset-all-user.txt"));
        if !user.is_empty() {
            bytes.push(b'\n');
            for line in &user {
                bytes.extend_from_slice(line.as_bytes());
                bytes.push(b'\n');
            }
        }
    }
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(err) = crate::config::atomic_write(&dest, &bytes) {
        crate::logger::log(
            "err",
            "updater",
            &format!(
                "ipset-all.txt: запись не удалась ({}) — {}",
                dest.display(),
                err
            ),
        );
    }
}

/// Возвращает `false`, если копия не создана (нет прав/места/каталога):
/// вызывающий тогда НЕ перезаписывает файл. Имя копии не затирает прежние.
fn backup(src: &Path, ts: &str, data: &Path, catalog_only: bool) -> bool {
    let name = src
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let dir = if catalog_only {
        data.join("catalog/.backups").join(ts)
    } else {
        let Some(parent) = src.parent() else {
            return false;
        };
        parent.join(".backups").join(ts)
    };
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let mut target = dir.join(&name);
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("{name}.{n}"));
        n += 1;
    }
    fs::copy(src, &target).is_ok()
}

// ------------------------------------------------- telegram bridge update

#[derive(serde::Serialize, Clone, Debug, Default)]
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
            // Только настоящая строка `version = "..."`; `version.workspace = true`
            // (наследование из workspace) раньше возвращала мусорную «версию».
            let rest = rest.trim_start();
            if !rest.starts_with('=') {
                continue;
            }
            let v = rest.trim_start_matches('=').trim().trim_matches('"').trim();
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
        info.note = Some(crate::texts::TG_CHECK_CLIENT.into());
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
            info.note = Some(crate::texts::tg_check_failed(&e));
        }
    }

    // Сигнал 2: последний коммит Flowseal/tg-ws-proxy (информационно).
    let commits_url = api("Flowseal/tg-ws-proxy", "commits?per_page=1");
    if let Ok(bytes) = fetch_bytes(&cli, &commits_url) {
        let text = String::from_utf8_lossy(&bytes);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(arr) = v.as_array() {
                if let Some(first) = arr.first() {
                    let date = first["commit"]["committer"]["date"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let msg = first["commit"]["message"]
                        .as_str()
                        .unwrap_or("")
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
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
        let nums = base
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect();
        let sn = suffix
            .rsplit('.')
            .next()
            .unwrap_or("")
            .parse::<u64>()
            .unwrap_or(0);
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
mod tests {
    use super::*;

    /// Roots с одним flowseal-корнем (иначе приватную карту не собрать).
    fn roots_with(root: &Path) -> Roots {
        serde_json::from_value(serde_json::json!({"flowseal": root.to_string_lossy()}))
            .expect("roots из json")
    }

    #[test]
    fn version_gt_compares_numerically() {
        assert!(version_gt("1.6.0", "1.5.0"));
        assert!(version_gt("1.10.0", "1.9.9"));
        assert!(!version_gt("1.5.0", "1.5.0"));
        assert!(!version_gt("1.5.0", "1.6.0"));
        assert!(version_gt("2.0", "1.9.9"));
    }

    #[test]
    fn pick_zip_asset_skips_engine_archives() {
        let rel = serde_json::json!({"assets": [
            {"name": "checksums.txt", "browser_download_url": "u0", "size": 1},
            {"name": "engine-zapret2.zip", "browser_download_url": "u1", "size": 2},
            {"name": "presets.json", "browser_download_url": "u2", "size": 3},
            {"name": "Z-GUI.Stable.1.2.zip", "browser_download_url": "u3", "size": 4}
        ]});
        let (name, url, size) = pick_zip_asset(&rel).expect("архив найден");
        assert_eq!(name, "Z-GUI.Stable.1.2.zip");
        assert_eq!(url, "u3");
        assert_eq!(size, 4);
        // Без «программного» имени берём любой zip, кроме engine-*.
        let rel2 = serde_json::json!({"assets": [
            {"name": "engine-zapret2.zip", "browser_download_url": "u1", "size": 2},
            {"name": "other.zip", "browser_download_url": "u9", "size": 5}
        ]});
        assert_eq!(pick_zip_asset(&rel2).expect("архив найден").0, "other.zip");
    }

    #[test]
    fn line_delta_counts_added_and_removed() {
        assert_eq!(line_delta(b"a\nb\n", b"a\nb\nc\n"), (1, 0));
        assert_eq!(line_delta(b"a\nb\nc\n", b"a\n"), (0, 2));
        assert_eq!(line_delta(b"# x\na\n", b"\na\nb\n"), (1, 1));
        // Не-UTF8 → без дельты.
        assert_eq!(line_delta(b"\xff\xfe", b"a\n"), (0, 0));
    }

    #[test]
    fn rejects_unsafe_bat_names() {
        // Реальные имена автора проходят.
        assert!(bat_name_ok("general (ALT).bat"));
        assert!(bat_name_ok("general.bat"));
        // Разделители пути и `..` — запись вне raw-каталога — отсекаются.
        assert!(!bat_name_ok("..\\evil.bat"));
        assert!(!bat_name_ok("sub/evil.bat"));
        assert!(!bat_name_ok(".hidden.bat"));
        assert!(!bat_name_ok("evil..bat"));
        assert!(!bat_name_ok("service.bat"));
        assert!(!bat_name_ok("Service2.bat"));
        assert!(!bat_name_ok("readme.txt"));
        assert!(!bat_name_ok(""));
    }

    /// Собирает zip «как GitHub»: с каталогом-обёрткой.
    fn repo_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write as _;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            w.start_file(format!("repo-main/{}", name), opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_maps_files_and_classifies_statuses() {
        let base = std::env::temp_dir().join(format!("zgui-upd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("data");
        let root = base.join("engine");
        fs::create_dir_all(root.join("lists")).unwrap();

        let zip = repo_zip(&[
            ("general.bat", b"@echo off\nwinws --wf-tcp=80"),
            ("service.bat", b"skip me"),
            ("readme.txt", b"skip me"),
            ("lists/list-general.txt", b"domain.example\n"),
            ("lists/ipset-all.txt", b"stub"),
            (".service/ipset-service.txt", b"1.2.3.0/24\n"),
        ]);
        let files = read_repo_zip(&zip).unwrap();
        let entries = collect_entries(files, &data, &roots_with(&root));

        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"general.bat"), "bat стратегия");
        assert!(labels.contains(&"list-general.txt"), "список в движок");
        assert!(labels.contains(&"ipset-all.txt"), "ipset из .service");
        assert!(labels.contains(&"ipset-service.txt"), "копия .service");
        assert!(!labels.contains(&"service.bat"), "service.bat не берём");
        assert!(!labels.contains(&"readme.txt"), "не .bat не берём");

        let settings = Settings {
            ipset_mode: "loaded".into(),
            ..Default::default()
        };
        let by_label = |l: &str| entries.iter().find(|e| e.label == l).unwrap();
        assert_eq!(
            classify_entry(by_label("general.bat"), &settings).status,
            "new"
        );

        // Совпадающий файл → ok; изменённый → modified.
        fs::create_dir_all(data.join("catalog/flowseal/raw")).unwrap();
        fs::write(
            data.join("catalog/flowseal/raw/general.bat"),
            b"@echo off\nwinws --wf-tcp=80",
        )
        .unwrap();
        assert_eq!(
            classify_entry(by_label("general.bat"), &settings).status,
            "ok"
        );
        fs::write(
            data.join("catalog/flowseal/raw/general.bat"),
            b"@echo off\nwinws --wf-tcp=443",
        )
        .unwrap();
        assert_eq!(
            classify_entry(by_label("general.bat"), &settings).status,
            "modified"
        );

        // Режим none — ipset не трогаем.
        let mut s2 = Settings {
            ipset_mode: "none".into(),
            ..Default::default()
        };
        assert_eq!(
            classify_entry(by_label("ipset-all.txt"), &s2).status,
            "skip-user"
        );
        s2.ipset_mode = "loaded".into();
        assert_eq!(classify_entry(by_label("ipset-all.txt"), &s2).status, "new");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn apply_writes_files_and_backs_up() {
        let base = std::env::temp_dir().join(format!("zgui-upd-apply-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("data");
        let root = base.join("engine");
        fs::create_dir_all(root.join("lists")).unwrap();
        let roots = roots_with(&root);
        let zip = repo_zip(&[
            ("general.bat", b"new-bat"),
            ("lists/list-general.txt", b"new-list\n"),
        ]);
        let files = read_repo_zip(&zip).unwrap();
        let entries = collect_entries(files, &data, &roots);
        let settings = Settings {
            ipset_mode: "loaded".into(),
            ..Default::default()
        };

        // Старое содержимое — должно попасть в бэкап.
        fs::create_dir_all(data.join("catalog/flowseal/raw")).unwrap();
        fs::write(data.join("catalog/flowseal/raw/general.bat"), b"old-bat").unwrap();
        fs::write(root.join("lists/list-general.txt"), b"old-list\n").unwrap();

        let out = apply_entries(&entries, &data, &roots, &settings, Vec::new());
        assert_eq!(out.len(), 2);
        assert!(
            out.iter().all(|u| u.status == "ok"),
            "оба файла должны записаться"
        );
        assert_eq!(
            fs::read(data.join("catalog/flowseal/raw/general.bat")).unwrap(),
            b"new-bat"
        );
        assert_eq!(
            fs::read(root.join("lists/list-general.txt")).unwrap(),
            b"new-list\n"
        );

        // Бэкапы старых версий на месте.
        let catalog_baks: Vec<_> = fs::read_dir(data.join("catalog/.backups"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(catalog_baks.len(), 1, "один ts-каталог бэкапов каталога");
        let ts_dir = catalog_baks[0].path();
        assert_eq!(fs::read(ts_dir.join("general.bat")).unwrap(), b"old-bat");
        let list_baks: Vec<_> = fs::read_dir(root.join("lists/.backups"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(list_baks.len(), 1, "один ts-каталог бэкапов движка");
        assert_eq!(
            fs::read(list_baks[0].path().join("list-general.txt")).unwrap(),
            b"old-list\n"
        );

        // Повторное применение — уже нечего обновлять (new/modified не осталось).
        let again = apply_entries(&entries, &data, &roots, &settings, Vec::new());
        assert!(
            again.iter().all(|u| u.status == "ok"),
            "статусы ok после записи"
        );

        let _ = fs::remove_dir_all(&base);
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
        assert_eq!(
            set.presets.len(),
            1,
            "пропуск неизвестного движка/пустых args"
        );
        assert_eq!(set.presets[0].id, "z2-x");

        // Голый массив без версии.
        let arr = r#"[{"id":"gd","engine":"goodbyedpi","args":["-9"]}]"#;
        let set2 = parse_preset_set(arr.as_bytes()).unwrap();
        assert!(set2.version.is_empty());
        assert_eq!(set2.presets.len(), 1);

        // Мусор и пустой набор — ошибки.
        assert!(parse_preset_set(b"not json").is_err());
        assert!(parse_preset_set(br#"{"version":"1","presets":[]}"#).is_err());

        // Нестроковый элемент в args — битый пресет целиком (не усекаем команду).
        let mixed = r#"[{"id":"z","engine":"goodbyedpi","args":["-9",5]}]"#;
        assert!(
            parse_preset_set(mixed.as_bytes()).is_err(),
            "args с числом должен отвергнуться"
        );
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
        assert_eq!(upd2, 0);
        assert_eq!(add2, 1);
        assert_eq!(
            profiles2
                .iter()
                .find(|p| p.id == "preset:z2-x")
                .unwrap()
                .args,
            vec!["keep".to_string()]
        );
    }

    #[test]
    fn preset_stamp_roundtrip() {
        let base = std::env::temp_dir().join(format!("zgui-stamp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("data");
        fs::create_dir_all(data.join("catalog")).unwrap();
        assert_eq!(preset_stamp(&data), None);
        save_preset_stamp(&data, "2026.09.26");
        assert_eq!(preset_stamp(&data).as_deref(), Some("2026.09.26"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn parses_version_from_cargo_toml() {
        let t = "[package]\nname = \"x\"\nversion = \"2.3.4-zui.2\"\nedition = \"2024\"\n";
        assert_eq!(parse_toml_version(t).as_deref(), Some("2.3.4-zui.2"));
        // Наследование из workspace — не версия пакета.
        assert_eq!(
            parse_toml_version("[package]\nversion.workspace = true\n"),
            None
        );
        assert_eq!(
            parse_toml_version("dependencies\nversion = \"1\"\n"),
            Some("1".into())
        );
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
    fn sync_ipset_materializes_loaded_list() {
        // Имя уникально для этого теста: tester.rs использует свой «zgui-ipset-»
        // каталог, и совпадение приводило к гонке при параллельных тестах.
        let base = std::env::temp_dir().join(format!("zgui-ipset-sync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("data");
        let root = base.join("engine");
        fs::create_dir_all(data.join("catalog/flowseal/.service")).unwrap();
        fs::create_dir_all(root.join("lists")).unwrap();
        let real = vec![b'1'; 4096];
        fs::write(
            data.join("catalog/flowseal/.service/ipset-service.txt"),
            &real,
        )
        .unwrap();
        fs::write(root.join("lists/ipset-all.txt"), IPSET_PLACEHOLDER).unwrap();

        let mut settings = Settings {
            ipset_mode: "loaded".into(),
            ..Default::default()
        };
        sync_ipset(&root, &data, &settings);
        assert_eq!(
            fs::read(root.join("lists/ipset-all.txt")).unwrap(),
            real,
            "loaded: должен быть реальный список"
        );

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
        assert!(
            fs::read(root.join("lists/ipset-all.txt"))
                .unwrap()
                .is_empty(),
            "any: пустой файл"
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn append_user_include_skips_comments_and_blanks() {
        let base = std::env::temp_dir().join(format!("zgui-user-inc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("flowseal");
        fs::create_dir_all(root.join("lists")).unwrap();
        let data = base.join("data");
        fs::create_dir_all(data.join("catalog/flowseal/.service")).unwrap();
        fs::write(
            root.join("lists/ipset-all-user.txt"),
            "# мои подсети\n\n80.93.214.0/24\n  193.169.239.0/24  \n",
        )
        .unwrap();

        let listed = crate::scanner::read_user_include(&root.join("lists/ipset-all-user.txt"));
        assert_eq!(
            listed,
            vec!["80.93.214.0/24".to_string(), "193.169.239.0/24".to_string()]
        );

        let mut settings = Settings {
            ipset_mode: "loaded".into(),
            ..Default::default()
        };
        fs::write(
            data.join("catalog/flowseal/.service/ipset-service.txt"),
            b"1.2.3.0/24\n",
        )
        .unwrap();
        sync_ipset(&root, &data, &settings);
        let got = fs::read_to_string(root.join("lists/ipset-all.txt")).unwrap();
        assert!(
            got.contains("1.2.3.0/24")
                && got.contains("80.93.214.0/24")
                && got.contains("193.169.239.0/24")
        );

        // none/any include НЕ подмешивают.
        settings.ipset_mode = "none".into();
        sync_ipset(&root, &data, &settings);
        assert_eq!(
            String::from_utf8(fs::read(root.join("lists/ipset-all.txt")).unwrap()).unwrap(),
            IPSET_PLACEHOLDER
        );

        let _ = fs::remove_dir_all(&base);
    }
}
