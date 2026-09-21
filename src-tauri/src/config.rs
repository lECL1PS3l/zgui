use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const ENGINE_FLOWSEAL: &str = "flowseal";
pub const SERVICE_NAME: &str = "zapret";

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Roots {
    pub flowseal: Option<String>,
}

impl Roots {
    pub fn path(&self, engine: &str) -> Option<PathBuf> {
        if engine == ENGINE_FLOWSEAL {
            self.flowseal.as_ref().map(PathBuf::from)
        } else {
            None
        }
    }
    pub fn set(&mut self, engine: &str, p: Option<String>) {
        if engine == ENGINE_FLOWSEAL {
            self.flowseal = p;
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub update_interval_hours: u32,
    pub game_filter: String,
    pub ipset_mode: String,
    pub autostart_mode: String,
    pub autostart_profile: Option<String>,
    #[serde(default)]
    pub always_admin: bool,
    #[serde(default)]
    pub tg_autostart: bool,
    #[serde(default = "default_tg_port")]
    pub tg_port: u16,
    /// Одноразовая миграция: старый дефолт интервала 6 ч → 72 ч.
    #[serde(default)]
    pub interval_migrated: bool,
    /// Тема оформления: "grey" (графит, по умолчанию), "dark" (космос), "light" (белая).
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Автозапуск GUI при входе: задача планировщика «ZapretGUI» (уровень «наивысшие»).
    #[serde(default)]
    pub boot_app: bool,
    /// Первый запуск: предложение «Всегда запускать от администратора» уже показано.
    #[serde(default)]
    pub admin_onboarded: bool,
}

fn default_tg_port() -> u16 {
    1443
}

fn default_theme() -> String {
    "grey".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            update_interval_hours: 72,
            game_filter: "off".into(),
            ipset_mode: "loaded".into(),
            autostart_mode: "none".into(),
            autostart_profile: None,
            always_admin: false,
            tg_autostart: false,
            tg_port: default_tg_port(),
            interval_migrated: false,
            theme: default_theme(),
            boot_app: false,
            admin_onboarded: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub args: Vec<String>,
    pub builtin: bool,
    pub source: Option<String>,
    pub updated_at: Option<String>,
}

impl Profile {
    pub fn exe_name(&self) -> &'static str {
        "winws.exe"
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub profile_id: String,
    pub pid: u32,
    pub started_at: u64,
    pub via: String,
    pub alive: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdEntry {
    pub id: String,
    pub group: String,
    pub label: String,
    pub dest: String,
    pub exists: bool,
    pub status: String,
    pub remote_hash: Option<String>,
    pub applied_hash: Option<String>,
    pub local_hash: Option<String>,
    pub size: u64,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterCache {
    pub last_check: Option<String>,
    pub entries: Vec<UpdEntry>,
    pub last_auto: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    #[serde(skip)]
    pub data: PathBuf,
    #[serde(default)]
    pub roots: Roots,
    pub settings: Settings,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub runtime: Option<Runtime>,
    #[serde(default)]
    pub updater: UpdaterCache,
    #[serde(default)]
    pub service_checked_at: u64,
    #[serde(default)]
    pub service_running: Option<bool>,
    #[serde(default)]
    pub service_strategy: Option<String>,
    #[serde(default)]
    pub boot_pending: bool,
    /// Версия установленного движка Flowseal (из тега релиза при обновлении
    /// или версия вшитого движка при первом запуске).
    #[serde(default)]
    pub engine_version: Option<String>,
}

impl AppState {
    pub fn load(data: PathBuf) -> Self {
        let path = data.join("state.json");
        let raw = fs::read_to_string(&path).ok();
        let parsed = raw.as_ref().and_then(|s| serde_json::from_str::<AppState>(s).ok());
        // Битый state.json не должен молча превращаться в «программу без настроек»:
        // сохраняем копию рядом, чтобы пользователь мог показать её разработчику.
        if parsed.is_none() {
            if let Some(text) = raw.as_ref() {
                if !text.trim().is_empty() {
                    let stamp = crate::profiles::now_str();
                    let bad = data.join(format!("state.json.bad-{stamp}"));
                    let _ = fs::write(&bad, text);
                    crate::logger::log(
                        "warn",
                        "config",
                        &format!("state.json повреждён — копия сохранена как {}", bad.display()),
                    );
                }
            }
        }
        let mut st: AppState = parsed.unwrap_or_else(|| AppState {
                data: data.clone(),
                roots: Roots::default(),
                settings: Settings::default(),
                profiles: Vec::new(),
                runtime: None,
                updater: UpdaterCache::default(),
                service_checked_at: 0,
                service_running: None,
                service_strategy: None,
                boot_pending: false,
                engine_version: None,
            });
        st.data = data;
        // Одноразовая миграция: старый дефолт автопроверки 6 ч → новый 72 ч.
        // Пользовательские значения, отличные от 6, не трогаются.
        if !st.settings.interval_migrated {
            if st.settings.update_interval_hours == 6 {
                st.settings.update_interval_hours = 72;
            }
            st.settings.interval_migrated = true;
            st.save();
        }
        st.ensure_dirs();
        st
    }

    pub fn save(&self) {
        let path = self.data.join("state.json");
        let tmp = self.data.join("state.json.tmp");
        if let Ok(s) = serde_json::to_string_pretty(self) {
            if fs::write(&tmp, s).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }

    pub fn ensure_dirs(&self) {
        let dirs = [
            self.data.clone(),
            self.data.join("catalog"),
            self.data.join("catalog/flowseal/raw"),
            self.data.join("catalog/flowseal/lists"),
            self.data.join("catalog/flowseal/.service"),
            self.data.join("catalog/sources/flowseal"),
            self.data.join("catalog/geoblock"),
            self.data.join("logs"),
            self.data.join("tmp"),
            self.data.join("engines"),
            self.data.join("webview"),
        ];
        for d in dirs {
            let _ = fs::create_dir_all(d);
        }
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn catalog_dir(&self, engine: &str) -> PathBuf {
        self.data.join("catalog").join(engine.to_lowercase())
    }

    pub fn raw_strategies_dir(&self) -> PathBuf {
        self.data.join("catalog/flowseal/raw")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data.join("logs")
    }

    pub fn root_for(&self, engine: &str) -> Option<PathBuf> {
        self.roots.path(engine)
    }
}

pub fn portable_data_dir() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = executable
        .parent()
        .ok_or_else(|| "не удалось определить папку zgui.exe".to_string())?;
    let data = directory.join("data");
    fs::create_dir_all(&data).map_err(|e| {
        format!(
            "не удаётся создать portable-папку {}: {}. Поместите zgui.exe в доступную для записи папку.",
            data.display(),
            e
        )
    })?;
    Ok(data)
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

pub fn file_sha256(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|b| sha256_hex(&b))
}

/// Декодирует текст, автоматически определяя кодировку. Логи и вывод PowerShell/winws
/// приходят в UTF-16 (Out-File по умолчанию), UTF-8 (BOM/без) или OEM-кодировке (CP866).
/// Раньше всё читалось как UTF-8 → кириллица превращалась в «иероглифы».
pub fn decode_text(bytes: &[u8]) -> String {
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
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    // UTF-16 без BOM: каждый второй байт — ноль (латиница/цифры/кириллица в BMP).
    if bytes.len() >= 4 && bytes.len().is_multiple_of(2) {
        let zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
        if zeros * 2 >= bytes.len() / 2 {
            let u16s: Vec<u16> = bytes
                .as_chunks::<2>().0.iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16_lossy(&u16s);
        }
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    #[cfg(windows)]
    {
        return decode_oem(bytes);
    }
    #[allow(unreachable_code)]
    String::from_utf8_lossy(bytes).into_owned()
}

/// Декодирует OEM-кодировку (CP866) — так печатают консольные winws/winws2 и cmd.
#[cfg(windows)]
fn decode_oem(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::MultiByteToWideChar;
    const CP_OEMCP: u32 = 1;
    unsafe {
        let len = MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0);
        if len <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut buf = vec![0u16; len as usize];
        let n = MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), bytes.len() as i32, buf.as_mut_ptr(), len);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        buf.truncate(n as usize);
        String::from_utf16_lossy(&buf)
    }
}

pub fn read_text_auto(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|b| decode_text(&b))
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("zgui_tmp");
    let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(data).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn tail_file(path: &Path, max_chars: usize) -> String {
    match fs::read(path) {
        Ok(bytes) => {
            let s = decode_text(&bytes);
            let lines: Vec<&str> = s.lines().collect();
            let mut out = String::new();
            let mut count = 0usize;
            for l in lines.iter().rev() {
                if out.chars().count() + l.chars().count() + 1 > max_chars {
                    break;
                }
                if count < 400 {
                    out.insert_str(0, l);
                    out.insert(0, '\n');
                    count += 1;
                }
            }
            out.trim_start().to_string()
        }
        Err(_) => String::new(),
    }
}

pub fn find_exe(root: &Path, name: &str) -> Option<String> {
    fn walk(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
        if depth > 5 {
            return None;
        }
        let entries = fs::read_dir(dir).ok()?;
        for e in entries.flatten() {
            let p = e.path();
            let ftype = e.file_type().ok();
            if ftype.map(|t| t.is_file()).unwrap_or(false) {
                if p.file_name().and_then(|n| n.to_str()).map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false) {
                    return Some(p);
                }
            } else if ftype.map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(f) = walk(&p, name, depth + 1) {
                    return Some(f);
                }
            }
        }
        None
    }
    walk(root, name, 0).and_then(|p| {
        p.strip_prefix(root)
            .ok()
            .map(|r| r.to_string_lossy().replace('\\', "/"))
    })
}
