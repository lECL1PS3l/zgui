use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

pub const ENGINE_FLOWSEAL: &str = "flowseal";
pub const ENGINE_ZAPRET2: &str = "zapret2";
pub const ENGINE_GOODBYEDPI: &str = "goodbyedpi";
pub const ENGINE_DPIBREAK: &str = "dpibreak";
pub const SERVICE_NAME: &str = "zapret";

/// Репозиторий самого Z GUI: отсюда качаются движки нашей сборки (self_asset)
/// и OTA-набор пресетов (presets.json в ассетах последнего релиза).
pub const SELF_REPO: &str = "lECL1PS3l/zgui";

/// Описание встроенного движка. Единая точка правды: exe, репозиторий релизов,
/// человекочитаемое имя. Все проверки «какой движок» идут через реестр.
pub struct EngineDef {
    pub id: &'static str,
    pub label: &'static str,
    pub repo: &'static str,
    pub exe: &'static str,
}

/// Реестр поддерживаемых движков. Все системные (WinDivert), plug-and-play.
pub fn engines() -> &'static [EngineDef] {
    &[
        EngineDef {
            id: ENGINE_FLOWSEAL,
            label: "Flowseal (zapret winws)",
            repo: "Flowseal/zapret-discord-youtube",
            exe: "winws.exe",
        },
        EngineDef {
            id: ENGINE_ZAPRET2,
            label: "zapret2 (winws2)",
            repo: "bol-van/zapret2",
            exe: "winws2.exe",
        },
        EngineDef {
            id: ENGINE_GOODBYEDPI,
            label: "GoodbyeDPI",
            repo: "ValdikSS/GoodbyeDPI",
            exe: "goodbyedpi.exe",
        },
        EngineDef {
            id: ENGINE_DPIBREAK,
            label: "DPIBreak",
            repo: "dilluti0n/dpibreak",
            exe: "dpibreak.exe",
        },
    ]
}

pub fn engine_def(id: &str) -> Option<&'static EngineDef> {
    engines().iter().find(|d| d.id == id)
}

pub fn engine_ids() -> Vec<&'static str> {
    engines().iter().map(|d| d.id).collect()
}

#[derive(Clone, Default, Debug)]
pub struct Roots {
    /// Сериализуется как ПЛОСКИЙ объект `{"flowseal": "..."}` (см. ручной Serialize).
    /// Старый state.json (поле flowseal) читается без миграции.
    map: BTreeMap<String, String>,
}

/// Плоская сериализация: `{"flowseal":"...","zapret2":"..."}`, а НЕ `{"map":{...}}`.
/// Раньше derive писал обёртку `map`, из-за чего свой же файл не читался обратно
/// (кастомный Deserialize ждал плоский объект) — настройки терялись каждый запуск.
impl Serialize for Roots {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.map.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Roots {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let map: BTreeMap<String, String> = match &raw {
            // Формат 1.3.0 (баг): {"map": {"flowseal": "..."}} — читаем внутренний объект.
            serde_json::Value::Object(o) => match o.get("map") {
                Some(serde_json::Value::Object(inner)) => {
                    serde_json::from_value(serde_json::Value::Object(inner.clone()))
                        .map_err(serde::de::Error::custom)?
                }
                // Плоский (текущий и легаси {"flowseal": "..."}).
                _ => serde_json::from_value(raw.clone()).map_err(serde::de::Error::custom)?,
            },
            _ => serde_json::from_value(raw.clone()).map_err(serde::de::Error::custom)?,
        };
        Ok(Roots { map })
    }
}

impl Roots {
    pub fn path(&self, engine: &str) -> Option<PathBuf> {
        engine_def(engine)
            .and_then(|_| self.map.get(engine).map(PathBuf::from))
    }
    pub fn set(&mut self, engine: &str, p: Option<String>) {
        if engine_def(engine).is_none() {
            return;
        }
        match p {
            Some(v) => {
                self.map.insert(engine.into(), v);
            }
            None => {
                self.map.remove(engine);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub update_interval_hours: u32,
    pub game_filter: String,
    /// Кастомные диапазоны Game Filter (формат автора: `порт` или `start-end`
    /// через запятую; пример исключения RTMP: 1024-1934,1936-65535).
    #[serde(default = "default_port_range")]
    pub game_filter_tcp: String,
    #[serde(default = "default_port_range")]
    pub game_filter_udp: String,
    pub ipset_mode: String,
    pub autostart_mode: String,
    pub autostart_profile: Option<String>,
    #[serde(default)]
    pub always_admin: bool,
    #[serde(default)]
    pub tg_autostart: bool,
    #[serde(default = "default_tg_port")]
    pub tg_port: u16,
    /// Предлагать автоматически подключить Telegram-прокси, когда запущен Telegram
    /// и нет VPN/туннеля. По умолчанию выключено (включается галочкой в «Telegram»).
    #[serde(default)]
    pub tg_offer: bool,
    /// Постоянный секрет MTProto-бриджа (32 hex). Один и тот же между запусками:
    /// Telegram переиспользует одну запись прокси вместо накопления мёртвых.
    #[serde(default)]
    pub tg_secret: Option<String>,
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

fn default_port_range() -> String {
    "1024-65535".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            update_interval_hours: 72,
            game_filter: "off".into(),
            game_filter_tcp: default_port_range(),
            game_filter_udp: default_port_range(),
            ipset_mode: "loaded".into(),
            autostart_mode: "none".into(),
            autostart_profile: None,
            always_admin: false,
            tg_autostart: false,
            tg_port: default_tg_port(),
            tg_offer: false,
            tg_secret: None,
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
        engine_def(&self.engine).map(|d| d.exe).unwrap_or("winws.exe")
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
    /// Кулдаун следующей автопроверки (epoch-сек): ставится перед запуском
    /// фоновой проверки, чтобы неудачная попытка повторялась не каждые 2 с.
    #[serde(default)]
    pub next_auto: Option<u64>,
}

/// Последняя запись `state.json` не удалась (файл занят/нет прав): UI покажет
/// предупреждение, а не будет молча работать со старым состоянием.
static SAVE_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn save_failed() -> bool {
    SAVE_FAILED.load(std::sync::atomic::Ordering::SeqCst)
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
    /// Обнаружен winws нашего движка, запущенный вне программы (ручной .bat,
    /// старая служба) — показываем предупреждение и даём остановить.
    #[serde(default)]
    pub external_winws: bool,
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
                external_winws: false,
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

    /// Сохраняет состояние (tmp + rename). Возвращает `false` при ошибке и
    /// поднимает флаг `save_failed` — UI показывает проблему, вместо того чтобы
    /// молча работать со старым `state.json`.
    pub fn save(&self) -> bool {
        let path = self.data.join("state.json");
        let tmp = self.data.join("state.json.tmp");
        let json = match serde_json::to_string_pretty(self) {
            Ok(s) => s,
            Err(e) => {
                crate::logger::log("err", "state", &format!("сериализация настроек: {e}"));
                SAVE_FAILED.store(true, Ordering::SeqCst);
                return false;
            }
        };
        // Антивирус/OneDrive иногда держат файл доли секунды — одна повторная
        // попытка закрывает такие случаи.
        for attempt in 0..2 {
            if fs::write(&tmp, &json).is_ok() && fs::rename(&tmp, &path).is_ok() {
                SAVE_FAILED.store(false, Ordering::SeqCst);
                return true;
            }
            if attempt == 0 {
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
        crate::logger::log("err", "state", "state.json не сохранён (файл занят или нет прав)");
        SAVE_FAILED.store(true, Ordering::SeqCst);
        false
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

/// Закреплённые SHA-256 наших собственных релизных ассетов (наши сборки движков).
/// Эталон лежит в коде и не зависит от канала загрузки: при замене ассета в
/// релизе хеш обновляется здесь и выпускается GUI (осознанный компромисс —
/// защита от подмены на CDN/зеркалах, которой не даёт хеш «из тех же байтов»).
pub const PINNED_ASSETS: &[(&str, &str)] = &[(
    "engine-zapret2.zip",
    "cb93d635338562408b557d9f8341b35f38f713f8cbb22d7f335cd278e82490f6",
)];

pub fn pinned_asset_sha256(name: &str) -> Option<&'static str> {
    PINNED_ASSETS.iter().find(|(n, _)| *n == name).map(|(_, h)| *h)
}

/// Сверяет SHA-256 данных с эталоном формата GitHub API («sha256:<hex>»).
/// `None` — эталона нет: вызывающий сам решает, блокировать или предупредить.
pub fn digest_matches(data: &[u8], digest: Option<&str>) -> Option<bool> {
    let d = digest?;
    let want = d.strip_prefix("sha256:").unwrap_or(d);
    Some(sha256_hex(data).eq_ignore_ascii_case(want))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_and_roots_migration() {
        // Реестр: все четыре движка, exe-имена корректны.
        let all = engines();
        assert_eq!(all.len(), 4);
        assert!(engine_def("zapret2").map(|d| d.exe == "winws2.exe").unwrap_or(false));
        assert!(engine_def("goodbyedpi").map(|d| d.exe == "goodbyedpi.exe").unwrap_or(false));
        assert!(engine_def("dpibreak").map(|d| d.exe == "dpibreak.exe").unwrap_or(false));
        assert!(engine_def("unknown").is_none());

        // Старый state.json: roots {"flowseal": "C:\\z"} → новая map.
        let legacy = serde_json::json!({"flowseal": "C:\\z"});
        let roots: Roots = serde_json::from_value(legacy).unwrap();
        assert_eq!(roots.path("flowseal").unwrap(), PathBuf::from("C:\\z"));
        assert!(roots.path("zapret2").is_none());

        // set работает для зарегистрированных движков, чужой id игнорируется.
        let mut r = Roots::default();
        r.set("zapret2", Some("D:\\e2".into()));
        r.set("bogus", Some("X".into()));
        assert_eq!(r.path("zapret2").unwrap(), PathBuf::from("D:\\e2"));
        assert!(r.path("bogus").is_none());
        r.set("zapret2", None);
        assert!(r.path("zapret2").is_none());

        // exe_name через реестр; неизвестный движок — безопасный фоллбек winws.
        let p = Profile {
            id: "x".into(),
            name: "x".into(),
            engine: "goodbyedpi".into(),
            args: vec![],
            builtin: true,
            source: None,
            updated_at: None,
        };
        assert_eq!(p.exe_name(), "goodbyedpi.exe");
        let mut q = p.clone();
        q.engine = "unknown".into();
        assert_eq!(q.exe_name(), "winws.exe");
    }

    #[test]
    fn roots_roundtrips_flat_map_and_legacy() {
        // Плоская сериализация (не {"map":{...}}) — корень бага с потерей настроек.
        let mut r = Roots::default();
        r.set(ENGINE_FLOWSEAL, Some("C:\\z".into()));
        r.set(ENGINE_ZAPRET2, Some("D:\\e2".into()));
        let js = serde_json::to_string(&r).unwrap();
        assert!(js.starts_with('{') && js.contains("\"flowseal\"") && !js.contains("\"map\""));
        let back: Roots = serde_json::from_str(&js).unwrap();
        assert_eq!(back.path(ENGINE_FLOWSEAL).unwrap(), PathBuf::from("C:\\z"));
        assert_eq!(back.path(ENGINE_ZAPRET2).unwrap(), PathBuf::from("D:\\e2"));

        // Формат 1.3.0 с обёрткой map — читаем (обратная совместимость).
        let wrapped = serde_json::json!({"map": {"zapret2": "D:\\e2"}});
        let r2: Roots = serde_json::from_value(wrapped).unwrap();
        assert_eq!(r2.path(ENGINE_ZAPRET2).unwrap(), PathBuf::from("D:\\e2"));

        // Легаси плоский {"flowseal": ...}.
        let leg = serde_json::json!({"flowseal": "C:\\z"});
        let r3: Roots = serde_json::from_value(leg).unwrap();
        assert_eq!(r3.path(ENGINE_FLOWSEAL).unwrap(), PathBuf::from("C:\\z"));
    }

    #[test]
    fn appstate_roundtrips_settings_and_roots() {
        // Регресс: раньше AppState писался без возможности прочитать обратно
        // (roots: {"map":...}) — каждый запуск «state.json повреждён», настройки
        // сбрасывались. Проверяем полный round-trip.
        let mut roots = Roots::default();
        roots.set(ENGINE_FLOWSEAL, Some("C:\\z".into()));
        let settings = Settings {
            admin_onboarded: true,
            always_admin: true,
            update_interval_hours: 5,
            ..Settings::default()
        };
        let state = AppState {
            data: PathBuf::new(), // #[serde(skip)]
            roots,
            settings,
            profiles: vec![],
            runtime: None,
            updater: UpdaterCache::default(),
            service_checked_at: 0,
            service_running: None,
            service_strategy: None,
            boot_pending: false,
            external_winws: false,
            engine_version: None,
        };
        let js = serde_json::to_string(&state).unwrap();
        let back: AppState = serde_json::from_str(&js).expect("state.json должен читаться обратно");
        assert_eq!(back.roots.path(ENGINE_FLOWSEAL).unwrap(), PathBuf::from("C:\\z"));
        assert!(back.settings.admin_onboarded, "admin_onboarded должен сохраниться");
        assert!(back.settings.always_admin);
        assert_eq!(back.settings.update_interval_hours, 5);
    }

    #[test]
    fn tg_offer_defaults_to_off() {
        // Просьба владельца 25.09: предложение TG-моста не выскакивает само,
        // пока пользователь не включит галочку (и не раньше админ-вопроса).
        assert!(!Settings::default().tg_offer);
    }
}
