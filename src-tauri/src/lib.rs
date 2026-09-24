use crate::config::{
    AppState, Profile, Roots, Runtime, Settings, UpdaterCache, ENGINE_FLOWSEAL, ENGINE_ZAPRET2,
    SERVICE_NAME, SELF_REPO,
};
use crate::profiles as pf;
use crate::runner as rn;
use crate::service as svc;
use crate::updater as up;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Взаимная блокировка долгих операций: запуск/остановка стратегий, тест,
/// служба, DNS, обновления, сброс сети, восстановление интернета. Захват через
/// `lock()` — безвозвратно до отпускания; `try_lock()` — None, если уже занято.
/// Правило порядка: всегда берётся ДО `Global::state` (иначе дедлок).
static OPS: Mutex<()> = Mutex::new(());

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

pub mod config;
mod autotune;
mod dns;
mod embedded;
mod human;
mod logger;
mod netreset;
mod presets;
mod profiles;
mod runner;
mod service;
mod telegram;
mod tester;
mod updater;
mod watchdog;

pub struct Global {
    pub state: Mutex<AppState>,
    pub busy: AtomicBool,
    /// Идёт тест стратегий — взаимная блокировка с запуском/остановкой профилей.
    pub testing: Mutex<tester::TestProgress>,
    /// Идёт долгая операция (обновления, fetch_engine, DNS, net_reset, service).
    pub op_running: AtomicBool,
    pub telegram: telegram::TgState,
    pub watchdog: std::sync::Arc<watchdog::WatchdogState>,
    /// Предложение Telegram-моста уже сделано в этой сессии (спрашиваем один раз).
    pub tg_offer_shown: AtomicBool,
    /// Время последней проверки Telegram-предложения (throttle, epoch-сек).
    pub tg_offer_checked: std::sync::atomic::AtomicU64,
    /// Время последней проверки VPN-стража Telegram (throttle, epoch-сек).
    pub tg_guard_checked: std::sync::atomic::AtomicU64,
}

impl Global {
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }
    pub fn set_busy(&self, b: bool) {
        self.busy.store(b, Ordering::SeqCst);
    }
    /// Долгая операция активна (обновления, DNS, служба, сброс сети).
    /// Фронт блокирует кнопки профилей/теста на это время — взаимная блокировка.
    pub fn op_running(&self) -> bool {
        self.op_running.load(Ordering::SeqCst)
    }
    pub fn set_op_running(&self, b: bool) {
        self.op_running.store(b, Ordering::SeqCst);
    }
}

fn st(g: &Global) -> MutexGuard<'_, AppState> {
    g.state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Гард взаимной блокировки операций. `None` — уже занято (блокирующая
/// альтернатива `lock()` не используется: UI не должен ждать десятки секунд).
fn ops_try() -> Option<std::sync::MutexGuard<'static, ()>> {
    OPS.try_lock().ok()
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    let _ = app.emit(event, payload);
}

fn log_updates(data: &std::path::Path, msg: &str) {
    use std::io::Write;
    let p = data.join("logs").join("updates.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "[{}] {}", crate::profiles::now_str(), msg);
    }
}

// ---------------------------------------------------------------- DTO

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RootInfo {
    path: Option<String>,
    exe: Option<String>,
    ready: bool,
}

fn root_info(roots: &Roots, engine: &str, exe_name: &str) -> RootInfo {
    match roots.path(engine) {
        Some(p) => {
            let root = PathBuf::from(&p);
            let exe = crate::config::find_exe(&root, exe_name);
            let ready = exe.is_some();
            RootInfo {
                path: Some(p.to_string_lossy().into_owned()),
                exe,
                ready,
            }
        }
        None => RootInfo { path: None, exe: None, ready: false },
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ServiceInfo {
    installed: bool,
    running: Option<bool>,
    strategy: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdaterView {
    last_check: Option<String>,
    entries: Vec<crate::config::UpdEntry>,
}

/// Формирует предупреждения (не блокирующие) для UI.
fn collect_warnings(s: &AppState) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("Не рекомендуется запускать Zapret вместе с VPN".into());
    if s.external_winws {
        out.push(
            "Обход запущен вне программы (winws.exe не через наш GUI). Два winws конфликтуют: \
             нажмите «Остановить» и запустите стратегию здесь."
                .into(),
        );
    }

    for def in config::engines() {
        let Some(root) = s.roots.path(def.id) else { continue };
        let original_bats = std::fs::read_dir(&root)
            .map(|rd| {
                rd.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                    n.ends_with(".bat") && !n.starts_with("service")
                })
            })
            .unwrap_or(false);
        if original_bats {
            out.push(format!(
                "В корне {} найдены оригинальные .bat/.lua автора — отключите их автозапуск (службу/планировщик), иначе они будут конфликтовать с нашей программой.",
                def.id
            ));
        }
    }
    out
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    flowseal: RootInfo,
    /// Все движки реестра со статусом готовности (селектор UI).
    engines: Vec<EngineInfo>,
    settings: Settings,
    profiles: Vec<Profile>,
    runtime: Option<Runtime>,
    service: ServiceInfo,
    updates: UpdaterView,
    game_filter_ports: (String, String),
    busy: bool,
    elevated: bool,
    /// Долгая операция (обновления, DNS, служба, сброс сети): фронт блокирует
    /// запуск/остановку профилей и тест до её завершения (взаимная блокировка).
    op_running: bool,
    /// Кто держит обход: none|app|service|test|external (единый источник правды).
    owner: String,
    data_dir: String,
    warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EngineInfo {
    id: &'static str,
    label: &'static str,
    repo: &'static str,
    path: Option<String>,
    exe: Option<String>,
    ready: bool,
}

fn engines_info(s: &AppState) -> Vec<EngineInfo> {
    config::engine_ids()
        .iter()
        .map(|id| {
            let def = config::engine_def(id).unwrap();
            let info = root_info(&s.roots, def.id, def.exe);
            EngineInfo {
                id: def.id,
                label: def.label,
                repo: def.repo,
                path: info.path,
                exe: info.exe,
                ready: info.ready,
            }
        })
        .collect()
}

fn updater_view(s: &AppState) -> UpdaterView {
    UpdaterView {
        last_check: s.updater.last_check.clone(),
        entries: s.updater.entries.clone(),
    }
}

// -------------------------------------------------------- владелец обхода

/// Кто прямо сейчас держит обход (winws). Единственный источник правды для
/// индикаторов запуска и детекта «внешнего» процесса.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WinwsOwner {
    None,
    /// Профиль, запущенный самой программой (winws — дочерний процесс GUI).
    App(String),
    /// Служба zapret (профиль необязателен: мог не сохраниться).
    Service(Option<String>),
    /// Идёт прогон теста — winws управляется тестом, а не пользователем.
    Test,
    /// winws нашего движка, поднятый вне программы (ручной .bat).
    External,
}

/// Чистое решение без обращений к системе — чтобы покрывать логику тестами.
fn winws_owner_of(
    testing: bool,
    app_profile: Option<&str>,
    service_running: Option<bool>,
    service_strategy: Option<&str>,
    any_winws: bool,
    own_winws: bool,
) -> WinwsOwner {
    if testing {
        return WinwsOwner::Test;
    }
    if let Some(id) = app_profile {
        return WinwsOwner::App(id.to_string());
    }
    if service_running == Some(true) {
        return WinwsOwner::Service(service_strategy.map(str::to_string));
    }
    if any_winws && own_winws {
        return WinwsOwner::External;
    }
    WinwsOwner::None
}

fn owner_name(o: &WinwsOwner) -> &'static str {
    match o {
        WinwsOwner::None => "none",
        WinwsOwner::App(_) => "app",
        WinwsOwner::Service(_) => "service",
        WinwsOwner::Test => "test",
        WinwsOwner::External => "external",
    }
}

/// Текущий владелец обхода. Локи берутся по порядку `testing` → `state`;
/// не вызывать, уже удерживая `state`, иначе дедлок.
fn current_owner(g: &Global) -> WinwsOwner {
    let testing_flag = g.testing.lock().unwrap_or_else(|e| e.into_inner()).running;
    let s = st(g);
    let testing = testing_flag || tester::runner_alive(&s.data);
    let app = s
        .runtime
        .as_ref()
        .filter(|r| rn::pid_alive(r.pid))
        .map(|r| r.profile_id.as_str());
    let any = svc::any_winws_running();
    let own = any && !svc::own_engine_pids(&s.data, None).is_empty();
    winws_owner_of(
        testing,
        app,
        s.service_running,
        s.service_strategy.as_deref(),
        any,
        own,
    )
}

// ---------------------------------------------------------------- корневой

#[tauri::command(async)]
fn bootstrap(ga: State<'_, Global>) -> Bootstrap {
    let g = ga.inner();
    // Владельца считаем до захвата state: current_owner сам берёт этот лок.
    let owner = owner_name(&current_owner(g)).to_string();
    let s = st(g);
    let engines = engines_info(&s);
    let (tcp, udp) = pf::game_filter_ports(&s.settings.game_filter);
    let runtime = s.runtime.clone().map(|mut r| {
        r.alive = rn::pid_alive(r.pid);
        r
    });
    Bootstrap {
        flowseal: root_info(
            &s.roots,
            ENGINE_FLOWSEAL,
            crate::config::engine_def(ENGINE_FLOWSEAL).map(|d| d.exe).unwrap_or("winws.exe"),
        ),
        engines,
        settings: s.settings.clone(),
        profiles: s.profiles.clone(),
        runtime,
        service: ServiceInfo {
            installed: s.service_running.is_some(),
            running: s.service_running,
            strategy: s.service_strategy.clone(),
        },
        updates: updater_view(&s),
        game_filter_ports: (tcp, udp),
        busy: g.is_busy(),
        elevated: rn::is_elevated(),
        op_running: g.op_running(),
        owner,
        data_dir: s.data.to_string_lossy().into_owned(),
        warnings: collect_warnings(&s),
    }
}

// ------------------------------------------------------------- orphan proxy

/// Проверяет системный прокси: если он включён, указывает на локальный адрес
/// (127.0.0.1/localhost), но на этом порту никто не слушает — значит, остался
/// «осиротевшим» от выгруженного VPN/обходчика, и браузер шлёт трафик в никуда
/// (`ERR_PROXY_CONNECTION_FAILED`). Тогда сбрасываем прокси автоматически.
/// Корпоративные/внешние прокси НЕ трогаем.
/// Возвращает описание сброшенного прокси, если что-то починили.
/// Извлекает локальный (127.*/localhost) порт из значения `ProxyServer`.
/// Возвращает `None` для внешних/корпоративных прокси — их нельзя трогать.
fn local_proxy_port(server: &str) -> Option<u16> {
    let addr = server
        .split(';')
        .find_map(|p| p.split('=').nth(1).or(Some(p)))
        .unwrap_or(server)
        .trim();
    match addr.rsplit_once(':') {
        Some((host, p)) if host.starts_with("127.") || host.eq_ignore_ascii_case("localhost") => {
            p.trim().parse::<u16>().ok()
        }
        _ => None,
    }
}

pub fn heal_orphan_proxy() -> Option<String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let read = |name: &str| -> Option<String> {
        let out = rn::hidden_command("reg")
            .args(["query", key, "/v", name])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find(|l| l.contains(name))
            .and_then(|l| l.split_whitespace().nth(2))
            .map(|v| v.to_string())
    };
    let enabled = read("ProxyEnable").map(|v| v.trim() == "0x1" || v.trim() == "1").unwrap_or(false);
    if !enabled {
        return None;
    }
    let server = read("ProxyServer")?;
    let server = server.trim();
    // Берём только локальные прокси (127.0.0.1 / localhost), возможно с префиксом «http=».
    let port = local_proxy_port(server)?;
    // Кто-то реально слушает порт (живой VPN/прокси) — не вмешиваемся.
    let listening = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(250),
    )
    .is_ok();
    if listening {
        return None;
    }
    // Порт мёртв — сбрасываем системный прокси на «прямое подключение».
    let _ = rn::hidden_command("reg")
        .args([
            "add",
            key,
            "/v",
            "ProxyEnable",
            "/t",
            "REG_DWORD",
            "/d",
            "0",
            "/f",
        ])
        .output();
    let _ = rn::hidden_command("reg")
        .args(["delete", key, "/v", "ProxyServer", "/f"])
        .output();
    Some(server.to_string())
}

// ---------------------------------------------------------------- roots

fn seed_flowseal_configs(root: &std::path::Path, data: &std::path::Path, settings: &Settings) {
    let lists = root.join("lists");
    let _ = std::fs::create_dir_all(&lists);
    embedded::copy_tree_missing(&data.join("catalog/flowseal/lists"), &lists);
    let seeds: [(&str, &str); 3] = [
        ("ipset-exclude-user.txt", "203.0.113.113/32\n"),
        ("list-general-user.txt", "# Never leave this file empty\ndomain.example.abc\n"),
        ("list-exclude-user.txt", "domain.example.abc\n"),
    ];
    for (f, content) in seeds {
        let p = lists.join(f);
        if !p.exists() {
            let _ = std::fs::write(p, content);
        }
    }
    // ipset-all.txt: в движке/каталоге лежит заглушка Flowseal — материализуем
    // реальный список для режима «loaded» (иначе `--ipset=` правила мертвы).
    up::sync_ipset(root, data, settings);
}

#[tauri::command(async)]
fn set_root(app: AppHandle, ga: State<'_, Global>, engine: String, path: String) -> Result<RootInfo, String> {
    let g = ga.inner();
    let meta = engine_meta(&engine)?;
    let selected = PathBuf::from(&path);
    if !selected.is_dir() {
        return Err("указанная папка не существует".into());
    }
    let exe_name = meta.exe;
    if crate::config::find_exe(&selected, exe_name).is_none() {
        return Err(format!("в папке не найден {} — укажите корень распакованного движка", exe_name));
    }
    let root = embedded::engine_root_for_public(&selected).unwrap_or(selected);
    let (data, settings) = {
        let s = st(g);
        (s.data.clone(), s.settings.clone())
    };
    st(g).roots.set(&engine, Some(root.to_string_lossy().into_owned()));
    if engine == ENGINE_FLOWSEAL {
        embedded::neutralize_author_autoupdate(&root);
        seed_flowseal_configs(&root, &data, &settings);
    }
    let mut s = st(g);
    reload_bats_from_disk(&mut s);
    s.save();
    let info = root_info(&s.roots, &engine, exe_name);
    logger::log("ok", "engine", &format!("корень {engine} задан: {}", root.display()));
    emit(&app, "zgui:toast", serde_json::json!({"kind":"ok","text": format!("{}: корень задан ({})", engine, root.display())}));
    Ok(info)
}

struct EngineMeta {
    repo: &'static str,
    exe: &'static str,
    /// Имя zip-ассета в релизе НАШЕГО репо (собранная нами версия движка,
    /// живёт под Defender). None — движок качается из репо автора.
    self_asset: Option<&'static str>,
}

fn engine_meta(engine: &str) -> Result<EngineMeta, String> {
    let d = crate::config::engine_def(engine)
        .ok_or_else(|| format!("неизвестный движок «{engine}»"))?;
    // zapret2: релизный zip bol-van палятся Defender'ом (детект по содержимому),
    // поэтому качаем собственную сборку из релиза нашего репо.
    let self_asset = (engine == ENGINE_ZAPRET2).then_some("engine-zapret2.zip");
    Ok(EngineMeta { repo: d.repo, exe: d.exe, self_asset })
}

#[tauri::command]
fn fetch_engine(app: AppHandle, ga: State<'_, Global>, engine: String, dest: Option<String>) -> Result<String, String> {
    let meta = engine_meta(&engine)?;
    let g = ga.inner();
    // Второй запуск поверх первого писал бы в тот же tmp-zip (File::create обрезает
    // файл) и мог испортить распаковку. Взаимная блокировка: тест/стоп/обновления
    // не могут начаться, пока идёт загрузка.
    let _op = ops_try().ok_or("уже идёт операция — дождитесь завершения")?;
    if g.busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("уже идёт загрузка или проверка обновлений — дождитесь завершения".into());
    }
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "fetch"}));
    let dest = dest.unwrap_or_else(|| st(g).data.join("engines").join(&engine).to_string_lossy().into_owned());
    let data_dir = st(g).data.clone();
    let app2 = app.clone();
    let engine2 = engine.clone();
    std::thread::spawn(move || {
        let tag = format!("fetch:{}", engine2);
        emit(&app2, "zgui:prog", serde_json::json!({"id": tag, "phase": "meta", "msg": "получаю информацию о последнем релизе", "pct": 0}));
        match fetch_engine_impl(&app2, &meta, &dest, &data_dir, &engine2) {
            Ok((path, version)) => {
                let ga2 = app2.state::<Global>();
                let mut s = ga2.state.lock().unwrap();
                s.roots.set(&engine2, Some(path.clone()));
                if engine2 == ENGINE_FLOWSEAL {
                    s.engine_version = Some(version.trim_start_matches('v').to_string());
                }
                cleanup_stale_engine_dirs(std::path::Path::new(&path));
                if engine2 == ENGINE_FLOWSEAL {
                    seed_flowseal_configs(std::path::Path::new(&path), &s.data, &s.settings);
                }
                reload_bats_from_disk(&mut s);
                s.save();
                drop(s);
                logger::log("ok", "engine", &format!("движок {engine2} установлен: {path}"));
                emit(&app2, "zgui:prog", serde_json::json!({"id": tag, "phase": "done", "msg": format!("{} установлен", engine2), "pct": 100}));
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": format!("Движок {} установлен в {}", engine2, path)}));
            }
            Err(e) => {
                let friendly = human::with_context(&format!("не удалось установить движок {engine2}"), &e);
                logger::log("err", "engine", &format!("установка {engine2} не удалась: {e}"));
                emit(&app2, "zgui:prog", serde_json::json!({"id": tag, "phase": "error", "msg": friendly.clone(), "pct": -1}));
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": friendly}));
            }
        }
        app2.state::<Global>().set_busy(false);
        app2.state::<Global>().set_op_running(false);
        emit(&app2, "zgui:op", serde_json::json!({"running": false, "kind": "fetch"}));
    });
    Ok(format!("загрузка {} началась", engine))
}

fn fetch_engine_impl(app: &AppHandle, meta: &EngineMeta, dest: &str, data: &std::path::Path, engine: &str) -> Result<(String, String), String> {
    let cli = up::client()?;
    let tag = format!("fetch:{}", engine);

    // Движки с self_asset качаем из релиза нашего репо (наша сборка);
    // остальные — из репо автора.
    let repo = meta.self_asset.map(|_| SELF_REPO).unwrap_or(meta.repo);
    let api_url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let resp = cli.get(&api_url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API: HTTP {}", resp.status()));
    }
    let rel: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let tag_name = rel["tag_name"].as_str().unwrap_or("unknown").to_string();
    // Asset ищем по маске: точное имя self_asset, затем zip с exe внутри
    // (по exe-имени движка), затем любой zip; у dpibreak win-артефакт может
    // зваться по архитектуре.
    let zip_with_exe = |name: &str| -> Option<String> {
        rel["assets"].as_array().and_then(|a| {
            a.iter()
                .find(|x| {
                    x["name"].as_str().map(|n| n.to_lowercase().contains(name)).unwrap_or(false)
                        && x["name"].as_str().map(|n| n.ends_with(".zip")).unwrap_or(false)
                })
                .map(|x| x["browser_download_url"].as_str().unwrap_or("").to_string())
        })
    };
    let asset_url = meta
        .self_asset
        .and_then(|name| zip_with_exe(name.trim_end_matches(".zip")))
        .or_else(|| zip_with_exe(meta.exe.trim_end_matches(".exe")))
        .or_else(|| zip_with_exe("win"))
        .or_else(|| {
            rel["assets"]
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|x| x["name"].as_str().map(|n| n.ends_with(".zip")).unwrap_or(false))
                })
                .map(|x| x["browser_download_url"].as_str().unwrap_or("").to_string())
        })
        .unwrap_or_default();
    if asset_url.is_empty() {
        return Err("в релизе не найден zip-архив".into());
    }

    let tmp_zip = data.join("tmp").join(format!("{}-{}.zip", engine, tag_name));
    // Удаляется при выходе из функции (в т.ч. при ошибке скачивания/распаковки).
    let _zip_guard = rn::TempFile::new(tmp_zip.clone());
    let mut resp = cli.get(&asset_url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("скачивание: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut f = std::fs::File::create(&tmp_zip).map_err(|e| e.to_string())?;
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];
    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        downloaded += n as u64;
        if total > 0 {
            let pct = ((downloaded as f64 / total as f64) * 100.0) as i32;
            emit(app, "zgui:prog", serde_json::json!({"id": tag, "phase": "download", "msg": format!("скачиваю {} ({}%)", tag_name, pct), "pct": pct}));
        }
    }
    drop(f);

    let _ = std::fs::create_dir_all(dest);
    emit(app, "zgui:prog", serde_json::json!({"id": tag, "phase": "unzip", "msg": "распаковываю…", "pct": 90}));
    unzip(&tmp_zip, std::path::Path::new(dest)).map_err(|e| format!("распаковка: {}", e))?;
    let _ = std::fs::remove_file(&tmp_zip);

    let rootdir = PathBuf::from(dest);
    // Корень ищем рекурсивно по exe движка; нейтрализация автоапдейта —
    // только для Flowseal (файл utils/check_updates.enabled есть только там).
    let actual_root = crate::config::find_exe(&rootdir, meta.exe)
        .map(|rel| {
            let p = rootdir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            p.parent().map(|d| d.to_path_buf()).unwrap_or(rootdir)
        })
        .ok_or_else(|| format!("не найден {} в распакованном архиве", meta.exe))?;
    if engine == ENGINE_FLOWSEAL {
        embedded::neutralize_author_autoupdate(&actual_root);
    }
    Ok((actual_root.to_string_lossy().into_owned(), tag_name))
}

/// Удаляет оставшиеся вложенные каталоги релиза (например,
/// `zapret-discord-youtube-1.10.2` внутри корня после обновления на 1.10.3).
fn cleanup_stale_engine_dirs(root: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && name.starts_with("zapret-discord-youtube-") {
            let _ = std::fs::remove_dir_all(e.path());
            logger::log("info", "engine", &format!("удалён устаревший каталог движка: {name}"));
        }
    }
}

fn unzip(zip_path: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    // Общий каталог-обёртка срезается, если он есть у ВСЕХ записей. Раньше
    // разнородный/плоский архив (например наш engine-zapret2.zip) считался
    // ошибкой «неоднородная структура» и не распаковывался вообще.
    let top: Option<String> = embedded::common_root(&mut archive);

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let is_dir = entry.is_dir();
        let mut rel = name.clone();
        if let Some(t) = &top {
            if rel == *t || rel.starts_with(&format!("{}/", t)) {
                rel = rel.trim_start_matches(t).trim_start_matches('/').to_string();
            }
        }
        if rel.is_empty() {
            continue;
        }
        if rel.split(['/', '\\']).any(|p| p == "..") {
            continue;
        }
        let out = dest.join(&rel);
        if !out.starts_with(dest) {
            continue;
        }
        if is_dir {
            let _ = std::fs::create_dir_all(&out);
            continue;
        }
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------- профили

/// Первый запуск без движка: распаковывает встроенный релиз Flowseal,
/// чтобы GUI работал полностью автономно (движок уже внутри exe).
fn provision_engines(s: &mut AppState) {
    let engine = ENGINE_FLOWSEAL;
    let data = s.data.clone();
    // Нормализуем уже заданный корень (мог быть записан как engines/flowseal — каталог-обёртка).
    if let Some(p) = s.roots.path(engine) {
        let cur = PathBuf::from(&p);
        if let Some(norm) = embedded::engine_root_for_public(&cur) {
            embedded::neutralize_author_autoupdate(&norm);
            if norm != cur {
                s.roots.set(engine, Some(norm.to_string_lossy().into_owned()));
            }
            cleanup_stale_engine_dirs(&norm);
            // Версию могли не сохранить в старых сборках — восстанавливаем.
            if s.engine_version.is_none() {
                s.engine_version = engine_version_from_root(&norm)
                    .or_else(|| Some(embedded::ENGINE_VERSION.to_string()));
            }
            seed_flowseal_configs(&norm, &data, &s.settings);
            reload_bats_from_disk(s);
            return;
        }
    }
    // Корня нет или он не содержит движок — распаковываем встроенный.
    match embedded::ensure_embedded_engine(&data) {
        Ok(Some(path)) => {
            s.roots.set(engine, Some(path.to_string_lossy().into_owned()));
            if s.engine_version.is_none() {
                s.engine_version = Some(embedded::ENGINE_VERSION.to_string());
            }
            seed_flowseal_configs(&path, &data, &s.settings);
            reload_bats_from_disk(s);
        }
        Ok(None) => {}
        Err(e) => {
            log_updates(&data, &format!("embedded engine {}: {}", engine, e));
        }
    }
    // Новые движки (zapret2/goodbyedpi/dpibreak): подхватываем развёрнутый
    // data/engines/<id> (дистрибутив или загрузка fetch_engine), если корень ещё не задан.
    for id in config::engine_ids() {
        if id == ENGINE_FLOWSEAL {
            continue;
        }
        if s.roots.path(id).is_some() {
            continue;
        }
        if let Ok(Some(root)) = embedded::ensure_engine(&data, id) {
            s.roots.set(id, Some(root.to_string_lossy().into_owned()));
            logger::log("ok", "engine", &format!("движок {id} подхвачен: {}", root.display()));
        }
    }
}

fn ensure_presets(s: &mut AppState) {
    // Чистим ТОЛЬКО явно вырезанные вшитые пресеты (id из REMOVED_PRESET_IDS):
    // иначе остаются мёртвые стратегии вроде «GoodbyeDPI - 9» → `unknown option`.
    // Не трогаем пресеты, доставленные по воздуху (OTA): их id могут не входить
    // во вшитую таблицу, но они актуальны.
    let removed: std::collections::HashSet<String> = presets::REMOVED_PRESET_IDS
        .iter()
        .map(|id| format!("preset:{}", id))
        .collect();
    if !removed.is_empty() {
        let before = s.profiles.len();
        s.profiles.retain(|p| !removed.contains(&p.id));
        if s.profiles.len() != before {
            logger::log(
                "info",
                "profiles",
                &format!("удалено устаревших пресетов: {}", before - s.profiles.len()),
            );
        }
    }
    // Сеем вшитые пресеты всех движков: недостающие добавляем, существующие
    // (по id «preset:<id>») не трогаем — пользователь мог их скрыть/переименовать.
    // Вшитые пресеты: недостающие добавляем, существующие ПРИВОДИМ к таблице.
    // Без обновления аргументов исправленный в коде пресет оставался у
    // пользователя в старой (битой) версии из state.json — так GoodbyeDPI
    // «RU + DNS» держал несуществующий `-9` и падал с «unknown option».
    // Приоритет у OTA-набора, но только если он НОВЕЕ вшитой таблицы: иначе
    // после правки пресета в коде уже применённый старый набор не давал бы
    // починить профили пользователя.
    let ota_version = up::UpdArchive::load(&s.data).applied(up::PRESETS_ENTRY_ID);
    let ota_applied = ota_version
        .as_deref()
        .is_some_and(|v| v > presets::BUILTIN_PRESETS_VERSION);
    let mut refreshed = 0usize;
    for def in presets::builtin_presets() {
        let pid = presets::preset_profile_id(def.id);
        match s.profiles.iter_mut().find(|p| p.id == pid) {
            Some(existing) => {
                if ota_applied {
                    continue;
                }
                let args = def.args_vec();
                if existing.args != args || existing.name != def.name || existing.engine != def.engine {
                    existing.args = args;
                    existing.name = def.name.into();
                    existing.engine = def.engine.into();
                    existing.builtin = true;
                    existing.source = Some(pid);
                    refreshed += 1;
                }
            }
            None => s.profiles.push(def.to_profile()),
        }
    }
    if refreshed > 0 {
        logger::log(
            "info",
            "profiles",
            &format!("обновлены вшитые пресеты: {refreshed}"),
        );
    }
    // Кэш тестов: результаты по профилям, которых больше нет (вырезанные пресеты,
    // удалённые auto:-кандидаты), иначе висят в списке как «битые» стратегии.
    let mut cache = tester::TestCache::load(&s.data);
    let before = cache.results.len();
    cache.results.retain(|r| s.profiles.iter().any(|p| p.id == r.id));
    if cache.results.len() != before {
        logger::log(
            "info",
            "test",
            &format!("удалены устаревшие результаты тестов: {}", before - cache.results.len()),
        );
        cache.save(&s.data);
    }
    // Автозапуск мог указывать на удалённый пресет.
    if let Some(pid) = s.settings.autostart_profile.clone() {
        if !s.profiles.iter().any(|p| p.id == pid) {
            s.settings.autostart_mode = "none".into();
            s.settings.autostart_profile = None;
        }
    }
}


fn is_author_profile(p: &Profile) -> bool {
    p.builtin
        || p.source.as_deref().is_some_and(|source| {
            source.starts_with("preset:") || source.to_ascii_lowercase().ends_with(".bat")
        })
}

#[tauri::command(async)]
fn refresh_catalog(app: AppHandle, ga: State<'_, Global>) -> Result<Vec<Profile>, String> {
    let g = ga.inner();
    let mut s = st(g);
    let raw = s.raw_strategies_dir();
    let root = s.roots.path(ENGINE_FLOWSEAL);
    let existing = s.profiles.clone();
    if let Some(r) = root {
        let mut bats = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&raw) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.to_lowercase().ends_with(".bat") {
                    if let Ok(bytes) = std::fs::read(e.path()) {
                        bats.push((name, pf::decode_strategy_bytes(&bytes)));
                    }
                }
            }
        }
        let imported = pf::import_bat_profiles(&r, &bats, &existing);
        let imported_ids: std::collections::HashSet<String> = imported.iter().map(|p| p.id.clone()).collect();
        // держим ручные (custom) профили, убираем потерянные импортированные
        s.profiles.retain(|p| {
            // Кандидаты автоподбора (source «auto:») не из .bat-каталога —
            // синхронизация не должна их удалять.
            let is_auto = p.source.as_deref().is_some_and(|s| s.starts_with("auto:"));
            !(p.engine == ENGINE_FLOWSEAL
                && p.source.is_some()
                && !p.builtin
                && !is_auto
                && !imported_ids.contains(&p.id))
        });
        for p in imported {
            if p.builtin {
                continue;
            }
            if let Some(ex) = s.profiles.iter_mut().find(|x| x.id == p.id) {
                if !ex.builtin {
                    ex.args = p.args.clone();
                    ex.updated_at = p.updated_at.clone();
                }
            } else {
                s.profiles.push(p);
            }
        }
    }
    ensure_presets(&mut s);
    s.save();
    let out = s.profiles.clone();
    emit(&app, "zgui:toast", serde_json::json!({"kind":"ok","text":"каталог стратегий синхронизирован"}));
    Ok(out)
}

#[tauri::command(async)]
fn save_profile(ga: State<'_, Global>, id: Option<String>, name: String, engine: String, args: Vec<String>) -> Result<Vec<Profile>, String> {
    let g = ga.inner();
    let mut s = st(g);
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("укажите название стратегии".into());
    }
    if args.is_empty() {
        return Err("список аргументов пуст — нечего сохранять".into());
    }
    // Дубли имён путают: в списке и в автозапуске две записи выглядят одинаково.
    if s.profiles
        .iter()
        .any(|p| p.id != id.clone().unwrap_or_default() && p.name.eq_ignore_ascii_case(&name))
    {
        return Err(format!("стратегия с названием «{name}» уже есть — выберите другое имя"));
    }
    match id {
        None => s.profiles.push(Profile {
            id: pf::make_id("custom"),
            name,
            engine,
            args,
            builtin: false,
            source: None,
            updated_at: Some(pf::now_str()),
        }),
        Some(pid) => {
            if let Some(p) = s.profiles.iter_mut().find(|p| p.id == pid) {
                p.name = name;
                p.builtin = false;
                p.args = args;
                p.updated_at = Some(pf::now_str());
            } else {
                return Err("профиль не найден".into());
            }
        }
    }
    s.save();
    Ok(s.profiles.clone())
}

/// Готовит кандидатов автоподбора как профили (id `auto:<движок>:<slug>`,
/// тег source `auto:<движок>`). Существующие обновляются, новых добавляем.
#[tauri::command(async)]
fn autotune_prepare(ga: State<'_, Global>, engine: String) -> Result<Vec<Profile>, String> {
    if crate::config::engine_def(&engine).is_none() {
        return Err("неизвестный движок".into());
    }
    let cands = autotune::candidates(&engine);
    if cands.is_empty() {
        return Err(format!("для движка «{engine}» нет кандидатов для подбора"));
    }
    let g = ga.inner();
    let mut s = st(g);
    let tag = format!("auto:{engine}");
    for c in cands {
        let id = format!("auto:{engine}:{}", c.id);
        if let Some(p) = s.profiles.iter_mut().find(|p| p.id == id) {
            p.name = c.name;
            p.args = c.args;
            p.source = Some(tag.clone());
            p.builtin = false;
            p.updated_at = Some(pf::now_str());
        } else {
            s.profiles.push(Profile {
                id,
                name: c.name,
                engine: engine.clone(),
                args: c.args,
                builtin: false,
                source: Some(tag.clone()),
                updated_at: Some(pf::now_str()),
            });
        }
    }
    s.save();
    Ok(s.profiles
        .iter()
        .filter(|p| p.source.as_deref() == Some(tag.as_str()))
        .cloned()
        .collect())
}

/// Оставляет только выбранного кандидата автоподбора (остальные `auto:<движок>:*`
/// удаляются). Профиль остаётся в «Стратегиях» с чипом «автоподбор».
#[tauri::command(async)]
fn autotune_keep(ga: State<'_, Global>, engine: String, id: String) -> Result<Vec<Profile>, String> {
    let g = ga.inner();
    let mut s = st(g);
    let tag = format!("auto:{engine}");
    if s.profile(&id).is_none() {
        return Err("профиль не найден".into());
    }
    s.profiles
        .retain(|p| p.id == id || p.source.as_deref() != Some(tag.as_str()));
    s.save();
    Ok(s.profiles.clone())
}

#[tauri::command(async)]
fn delete_profile(ga: State<'_, Global>, id: String) -> Result<Vec<Profile>, String> {
    let g = ga.inner();
    let mut s = st(g);
    if let Some(p) = s.profiles.iter().find(|p| p.id == id) {
        if is_author_profile(p) {
            return Ok(s.profiles.clone());
        }
    }
    s.profiles.retain(|p| p.id != id);
    // Удалили профиль, выбранный для автозапуска, — снимаем устаревшую ссылку.
    // Задачу планировщика снимет provision_boot при следующем старте GUI.
    if s.settings.autostart_profile.as_deref() == Some(id.as_str()) {
        s.settings.autostart_mode = "none".into();
        s.settings.autostart_profile = None;
    }
    s.save();
    Ok(s.profiles.clone())
}

// ---------------------------------------------------------------- запуск

fn locate_exe(root: &std::path::Path, exe_name: &str) -> Result<PathBuf, String> {
    crate::config::find_exe(root, exe_name)
        .map(|rel| root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
        .ok_or_else(|| format!("не найден {} в корне движка", exe_name))
}

fn stop_service(data: &std::path::Path) -> Result<(), String> {
    let p = data.join("logs").join(format!("svc_stop_{}.ps1", std::process::id()));
    std::fs::write(&p, format!("{}\nnet stop {} 2>$null | Out-Null\nexit 0", rn::PS_HEADER, SERVICE_NAME))
        .map_err(|e| e.to_string())?;
    let r = rn::run_script_privileged(&p);
    let _ = std::fs::remove_file(&p);
    r.map(|_| ())
}

fn do_stop(app: &AppHandle, g: &Global, silent: bool) -> Result<(), String> {
    let (rt, data) = {
        let s = st(g);
        (s.runtime.clone(), s.data.clone())
    };
    if let Some(rt) = rt {
        st(g).runtime = None;
        st(g).save();
        if rt.via == "service" {
            let _ = stop_service(&data);
        } else if rn::pid_alive(rt.pid) {
            let _ = rn::stop_pid(rt.pid, &data);
        }
        logger::log("info", "stop", &format!("остановлено: {} (pid {}, через {})", rt.profile_id, rt.pid, rt.via));
        emit(app, "zgui:status", serde_json::json!({"running": false, "pid": null, "profileId": null}));
        if !silent {
            emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text":"запрет остановлен"}));
        }
        Ok(())
    } else {
        // возможно работает служба — глушим её процесс
        let (running, data) = {
            let s = st(g);
            (s.service_running, s.data.clone())
        };
        if running == Some(true) {
            let _ = stop_service(&data);
            // Служба осталась установленной, но остановлена — фиксируем сразу,
            // иначе UI ~10 с показывает «служба запущена» после «Остановить».
            let mut s = st(g);
            s.service_running = Some(false);
            s.save();
            emit(app, "zgui:status", serde_json::json!({"running": false, "pid": null, "profileId": null}));
        }
        if !silent {
            emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text":"всё остановлено"}));
        }
        Ok(())
    }
}

/// Останавливает ВСЁ, что относится к нашему движку: профиль приложения, нашу
/// службу и winws, поднятые вне программы (ручной .bat/старая служба).
/// Без этого второй winws не виден GUI и конфликтует с первым — обход «не работает»,
/// пока процесс не убьют вручную (жалоба владельца).
fn stop_all_own(app: &AppHandle, g: &Global) -> Result<(), String> {
    let r = do_stop(app, g, true);
    let data = st(g).data.clone();
    // Кэш состояния мог устареть (GUI перезапускали) — проверяем службу фактом.
    let (installed, running) = svc::service_state();
    if installed && running {
        let _ = stop_service(&data);
    }
    let leftovers = if svc::any_winws_running() {
        svc::own_engine_pids(&data, None)
    } else {
        Vec::new()
    };
    if !leftovers.is_empty() {
        logger::log(
            "warn",
            "stop",
            &format!("останавливаю winws вне программы: {:?}", leftovers),
        );
        let _ = rn::stop_pids(&leftovers, &data);
    }
    {
        let mut s = st(g);
        if s.external_winws {
            s.external_winws = false;
            s.save();
        }
    }
    r
}

#[tauri::command(async)]
fn stop_running(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    // Взаимная блокировка: stop и start/test не пересекаются.
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "stop"}));
    let result = stop_all_own(&app, g);
    g.set_op_running(false);
    emit(&app, "zgui:op", serde_json::json!({"running": false, "kind": "stop"}));
    result?;
    emit(&app, "zgui:toast", serde_json::json!({"kind":"ok","text":"всё остановлено"}));
    Ok(())
}

fn do_start(app: &AppHandle, g: &Global, id: &str) -> Result<Runtime, String> {
    let (profile, root_path, settings, data) = {
        let s = st(g);
        let p = s.profile(id).cloned().ok_or_else(|| {
            logger::log("err", "start", &format!("профиль {id} не найден"));
            "профиль не найден — возможно, он был удалён в другой копии программы".to_string()
        })?;
        let root = s.roots.path(&p.engine).ok_or_else(|| {
            logger::log("warn", "start", &format!("корень движка «{}» не задан", p.engine));
            format!("корень «{}» не задан — нажмите «Скачать движок» на вкладке «Стратегии»", p.engine)
        })?;
        (p, root, s.settings.clone(), s.data.clone())
    };

    let _ = stop_all_own(app, g);

    let exe = locate_exe(&root_path, profile.exe_name()).map_err(|e| {
        logger::log("err", "start", &format!("{}: {e}", profile.name));
        human::humanize(&e)
    })?;
    let (tcp, udp) = pf::game_filter_ports(&settings.game_filter);
    let args = presets::prepare_args(&profile.args, &root_path, &tcp, &udp);

    let logs = data.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let out_log = logs.join(format!("stdout-{}.txt", profile.id));
    let err_log = logs.join(format!("stderr-{}.txt", profile.id));
    let pid_file = logs.join(format!("pid-{}.txt", profile.id));

    // Рабочий каталог: у flowseal exe в bin/, у новых движков — в корне.
    // Несуществующий wd ломает spawn (Windows «неверно задано имя папки»).
    let wd = {
        let bin = root_path.join("bin");
        if bin.is_dir() { bin } else { exe.parent().map(|p| p.to_path_buf()).unwrap_or(root_path.clone()) }
    };

    // Чистим старые логи: иначе при мгновенном выходе процесса в ошибку попадёт
    // содержимое прошлого запуска (в т.ч. в другой кодировке).
    let _ = std::fs::remove_file(&out_log);
    let _ = std::fs::remove_file(&err_log);

    // Если GUI уже запущен от администратора — стартуем winws НАПРЯМУЮ:
    // мгновенно, с логами и корректным квотингом, без UAC и launcher-скриптов.
    // Иначе — элевированный launcher (один UAC), логи в этом пути не собираются.
    let pid = if rn::is_elevated() {
        let pid = rn::spawn_direct(&exe, &wd, &args, &out_log, &err_log)?;
        std::thread::sleep(Duration::from_millis(900));
        pid
    } else {
        let launcher = rn::write_launcher(&exe, &wd, &args, &pid_file);
        let pid = rn::spawn_and_wait_pid(&launcher, &pid_file, Duration::from_secs(60))?;
        let _ = std::fs::remove_file(&launcher);
        pid
    };

    if !rn::pid_alive(pid) {
        // Показываем и stderr, и stdout: winws пишет диагностику в оба потока.
        let err = rn::tail(&err_log, 2000);
        let out = rn::tail(&out_log, 2000);
        let msg = [err, out]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        logger::log(
            "err",
            "start",
            &format!("«{}» сразу завершился: {}", profile.name, msg.trim()),
        );
        let hint = if rn::is_elevated() {
            "стратегия сразу завершилась — подробности в «Журнале»"
        } else {
            "для запуска обхода нужен запрос прав администратора — включите «Всегда запускать программу от администратора» в «Настройках»"
        };
        return Err(if msg.trim().is_empty() {
            format!("{hint} (движок не выдал ни одной строки вывода)")
        } else {
            format!("{hint}. Последние строки движка:\n{}", msg.trim())
        });
    }

    let runtime = Runtime {
        profile_id: profile.id.clone(),
        pid,
        started_at: now_ts(),
        via: "app".into(),
        alive: true,
    };
    st(g).runtime = Some(runtime.clone());
    st(g).save();
    logger::log(
        "ok",
        "start",
        &format!("запущена стратегия «{}» ({}, pid {})", profile.name, profile.engine, pid),
    );
    emit(app, "zgui:status", serde_json::json!({"running": true, "pid": pid, "profileId": profile.id}));
    emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text": format!("Запущена стратегия «{}»", profile.name)}));
    g.set_op_running(false);
    emit(app, "zgui:op", serde_json::json!({"running": false, "kind": "start"}));
    Ok(runtime)
}

/// Запускает профиль без тупиков на настройках: если установлена служба zapret —
/// переводит её на нужную стратегию (служба остаётся единственным механизмом
/// обхода), иначе поднимает winws как процесс программы.
fn start_or_switch(app: &AppHandle, g: &Global, id: &str) -> Result<Runtime, String> {
    // Во время прогона теста запуск запрещён: do_start/stop_all_own убили бы
    // winws теста. Взаимная блокировка: тест и запуск профиля исключают друг друга.
    let testing = g.testing.lock().unwrap_or_else(|e| e.into_inner()).running;
    if testing || tester::runner_alive(&st(g).data) {
        return Err("идёт тест стратегий — дождитесь окончания".into());
    }
    // Параллельная операция (обновления, DNS, сброс сети, служба): запрещаем
    // запуск, пока она не завершится — иначе старт/стоп могут пересечься.
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(app, "zgui:op", serde_json::json!({"running": true, "kind": "start"}));
    let result = if !svc::service_state().0 {
        do_start(app, g, id)
    } else {
        let (profile, root, args, data) = {
            let s = st(g);
            let p = s.profile(id).cloned().ok_or_else(|| "профиль не найден".to_string())?;
            let root = s
                .roots
                .path(&p.engine)
                .ok_or_else(|| format!("корень «{}» не задан — нажмите «Скачать движок»", p.engine))?;
            let (tcp, udp) = pf::game_filter_ports(&s.settings.game_filter);
            (p.clone(), root.clone(), presets::prepare_args(&p.args, &root, &tcp, &udp), s.data.clone())
        };
        // Один живой winws: снимаем процесс программы и старую службу перед пересозданием.
        let _ = stop_all_own(app, g);
        svc::install_service(&root, &profile, &args, &data).map_err(|e| {
            logger::log("err", "service", &format!("переключение службы не удалось: {e}"));
            human::with_context("не удалось переключить службу на эту стратегию", &e)
        })?;
        logger::log(
            "ok",
            "service",
            &format!("служба zapret переключена на «{}»", profile.name),
        );
        {
            let mut s = st(g);
            s.service_running = Some(true);
            s.service_strategy = Some(profile.id.clone());
            s.runtime = None;
            s.save();
        }
        emit(app, "zgui:status", serde_json::json!({"running": true, "pid": null, "profileId": profile.id}));
        emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text": format!("Служба переключена на «{}»", profile.name)}));
        g.set_op_running(false);
        emit(app, "zgui:op", serde_json::json!({"running": false, "kind": "start"}));
        Ok(Runtime {
            profile_id: profile.id,
            pid: 0,
            started_at: now_ts(),
            via: "service".into(),
            alive: true,
        })
    };
    g.set_op_running(false);
    emit(app, "zgui:op", serde_json::json!({"running": false, "kind": "start"}));
    result
}

#[tauri::command]
async fn start_profile(app: AppHandle, id: String) -> Result<Runtime, String> {
    // Запуск идёт в отдельном потоке: ожидание UAC (до 60 с) не блокирует UI,
    // окно остаётся отзывчивым и показывает спиннер на кнопке.
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ga = app2.state::<Global>();
        start_or_switch(&app2, ga.inner(), &id)
    })
    .await
    .map_err(|e| format!("запуск прерван: {}", e))?
}

#[tauri::command(async)]
fn current_status(ga: State<'_, Global>) -> Result<Option<Runtime>, String> {
    let s = st(ga.inner());
    Ok(s.runtime.clone().map(|mut r| {
        r.alive = rn::pid_alive(r.pid);
        r
    }))
}

// ---------------------------------------------------------------- тесты стратегий

fn test_marker(data: &std::path::Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        data.join("logs/test-stop.flag"),
        data.join("logs/test-runner.pid"),
        data.join("logs/test-current.pid"),
    )
}

fn set_test(app: &AppHandle, g: &Global, p: tester::TestProgress) {
    *g.testing.lock().unwrap_or_else(|e| e.into_inner()) = p.clone();
    emit(app, "zgui:test", p);
}

/// Свежесть прогресса теста: раннер пишет `logs/test-out.json` после каждой
/// стратегии. Если файл отсутствует (раннер только стартовал) — считаем живым;
/// если есть и старый (минуты без обновлений) — «живой» PID переиспользован
/// чужим процессом, это не наш раннер.
fn test_out_state(data: &std::path::Path) -> (bool, bool) {
    let p = data.join("logs/test-out.json");
    match std::fs::metadata(&p).and_then(|m| m.modified()) {
        Ok(t) => {
            let fresh = t.elapsed().map(|e| e.as_secs() < 180).unwrap_or(false);
            (true, fresh)
        }
        Err(_) => (false, false),
    }
}

/// Убирает файлы-маркеры теста (после отмены или «фантомного» раннера).
fn clear_test_markers(data: &std::path::Path) {
    let (flag, run_pid, win_pid) = test_marker(data);
    for f in [flag, run_pid, win_pid] {
        let _ = std::fs::remove_file(f);
    }
}

#[tauri::command(async)]
fn test_status(ga: State<'_, Global>) -> tester::TestProgress {
    let g = ga.inner();
    let cur = g.testing.lock().unwrap_or_else(|e| e.into_inner());
    if cur.running {
        return cur.clone();
    }
    // Раннер прошлой сессии живёт в фоне (GUI закрывали) — гасим его, не подхватываем.
    let data = st(g).data.clone();
    let (flag, run_pid, _win_pid) = test_marker(&data);
    let alive = std::fs::read_to_string(&run_pid)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(rn::pid_alive)
        .unwrap_or(false);
    let (out_exists, out_fresh) = test_out_state(&data);
    let live_runner = alive && !flag.exists() && (out_fresh || !out_exists);
    if live_runner {
        // Фоновый ранер прошлой сессии (GUI закрывали — он elevated и живёт вне
        // GUI). Не подхватываем и НЕ показываем «тест идёт»: гасим его.
        let _ = std::fs::write(&flag, now_ts().to_string());
        if let Ok(txt) = std::fs::read_to_string(&run_pid) {
            if let Ok(pid) = txt.trim().parse::<u32>() {
                let _ = rn::hidden_command("taskkill.exe")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .output();
            }
        }
        logger::log("info", "test", "фоновый ранер прошлой сессии остановлен");
        clear_test_markers(&data);
    } else if alive && !flag.exists() {
        // PID жив, но прогресс устарел → фантом (PID переиспользован). Чистим,
        // иначе «тест уже выполняется» блокирует новые прогоны.
        clear_test_markers(&data);
    }
    cur.clone()
}

/// Запускает каждую выбранную стратегию по очереди, проверяет контрольные домены
/// и определяет лучшую (аналог теста стратегий в GUI Flowseal).
/// `mode`: "main" — ручной список (выбирает лучшую), "geoblock" — онлайн-списки (диагностика).
/// Ключ аргументов стратегии: по нему результат теста переиспользуется
/// (мост «тест ⇄ автоподбор»), если аргументы не менялись.
fn args_key(p: &Profile, tcp: &str, udp: &str) -> String {
    let mut s = format!("{}\u{1}{}\u{1}{}\u{1}", p.engine, tcp, udp);
    for a in &p.args {
        s.push_str(a);
        s.push('\u{2}');
    }
    crate::config::sha256_hex(s.as_bytes())
}

#[tauri::command(async)]
fn test_strategies(
    app: AppHandle,
    ga: State<'_, Global>,
    ids: Vec<String>,
    domains_limit: Option<usize>,
    mode: Option<String>,
    reuse: Option<bool>,
) -> Result<bool, String> {
    let _ = domains_limit;
    let geoblock = mode.as_deref() == Some("geoblock");
    let g = ga.inner();
    {
        let data = st(g).data.clone();
        let mut cur = g.testing.lock().unwrap_or_else(|e| e.into_inner());
        if cur.running {
            // Живой ли это раннер на самом деле? Если нет — это фантом
            // (PID переиспользован чужим процессом): сбрасываем, чтобы не
            // блокировать новые прогоны «тест уже выполняется».
            if !tester::runner_alive(&data) {
                clear_test_markers(&data);
                *cur = tester::TestProgress::default();
            } else {
                return Err("тест уже выполняется".into());
            }
        }
    }
    // Взаимная блокировка: тест исключает запуск/остановку профилей и другие
    // долгие операции — иначе winws теста был бы убит или запущен рядом.
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "test"}));
    let (profiles, data, roots_ok) = {
        let s = st(g);
        let all = s.profiles.clone();
        // По умолчанию — все профили УСТАНОВЛЕННЫХ движков (матрица кандидатов
        // автоподбора). Неустановленный движок не ломает тест, его пресеты
        // просто не попадают в прогон. Явно выбранные id идут как есть.
        let roots = s.roots.clone();
        let selected: Vec<Profile> = if ids.is_empty() {
            all.iter()
                .filter(|p| {
                    crate::config::engine_def(&p.engine).is_some()
                        && roots.path(&p.engine).is_some()
                })
                .cloned()
                .collect()
        } else {
            all.iter().filter(|p| ids.contains(&p.id)).cloned().collect()
        };
        let selected = if ids.is_empty() {
            selected
        } else {
            // Явный выбор не должен падать молча: недоступный движок — честная ошибка.
            selected
                .into_iter()
                .filter(|p| roots.path(&p.engine).is_some())
                .collect()
        };
        (selected, s.data.clone(), roots)
    };
    if profiles.is_empty() {
        return Err("нет стратегий для теста".into());
    }
    // VPN мешает тесту — просим выгрузить (фронт показывает окно и вызывает kill_conflicts).
    let vpn = svc::detect_vpn();
    if !vpn.is_empty() {
        return Err(format!("VPN_RUNNING:{}", vpn.len()));
    }
    for p in &profiles {
        if roots_ok.path(&p.engine).is_none() {
            return Err(format!("для «{}» не задан корень движка", p.name));
        }
    }

    // Основной тест идёт ровно по ручному списку (обязательные + вшитые) — без геоблока.
    // Если есть калибровка geoblock-теста, недоступные домены (не обходятся Zapret) отсекаем,
    // но обязательные критические группы не трогаем.
    // Диагностический геоблок-тест берёт ВЕСЬ онлайн-список (без лимита) + базовую пробу.
    let custom = if geoblock {
        tester::load_geoblock_domains(&data, usize::MAX)
    } else {
        let mut list = tester::load_domains_from_lists(&data, usize::MAX);
        // Вырезаем ТОЛЬКО те домены, что калибровка отметила как «требует VPN»
        // (недоступны через Zapret). Reachable-список НЕ используется как белый:
        // иначе домены ручного списка, которых нет в онлайн-геоблоке, выпадали бы.
        // Обязательные критические группы не трогаем никогда.
        let vpn_only = tester::load_vpn_only(&data);
        if !vpn_only.is_empty() {
            let req: std::collections::HashSet<String> =
                tester::REQUIRED_DOMAINS.iter().map(|h| h.to_string()).collect();
            list.retain(|(_, host)| !vpn_only.contains(host) || req.contains(host));
        }
        list
    };
    if custom.is_empty() {
        return Err("нет доменов для теста (списки пусты)".into());
    }

    // Готовим шаги: exe, рабочий каталог, аргументы с game-filter.
    let (tcp, udp) = pf::game_filter_ports(&st(g).settings.game_filter);

    // Мост «тест ⇄ автоподбор»: при `reuse` не гоняем повторно стратегии, которые
    // уже проверялись с теми же аргументами (свежие результаты из tests.json).
    let (reused, profiles): (Vec<tester::StrategyResult>, Vec<Profile>) = if reuse == Some(true) {
        let cache = crate::tester::TestCache::load(&data);
        let fresh = cache
            .tested_at
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|t| now_ts().saturating_sub(t) < 3600)
            .unwrap_or(false);
        if fresh {
            let mut reused = Vec::new();
            let mut to_run = Vec::new();
            for p in profiles {
                let key = args_key(&p, &tcp, &udp);
                match cache.results.iter().find(|r| {
                    r.id == p.id
                        && r.started
                        // args_key может отсутствовать у старого кэша (до этой
                        // версии) — тогда доверяем совпадению id.
                        && (r.args_key.as_deref() == Some(key.as_str()) || r.args_key.is_none())
                }) {
                    Some(r) => reused.push(r.clone()),
                    None => to_run.push(p),
                }
            }
            if !reused.is_empty() {
                logger::log(
                    "info",
                    "test",
                    &format!("переиспользую прежние результаты: {} стратегий", reused.len()),
                );
            }
            (reused, to_run)
        } else {
            (Vec::new(), profiles)
        }
    } else {
        (Vec::new(), profiles)
    };

    let mut steps: Vec<tester::TestStep> = Vec::new();
    for p in &profiles {
        let root = st(g).roots.path(&p.engine).ok_or_else(|| {
            format!("для «{}» не задан корень движка", p.name)
        })?;
        let exe = locate_exe(&root, p.exe_name())?;
        let args = presets::prepare_args(&p.args, &root, &tcp, &udp);
        let wd = {
            let bin = root.join("bin");
            if bin.is_dir() { bin } else { exe.parent().map(|x| x.to_path_buf()).unwrap_or_else(|| PathBuf::from(root.to_string_lossy().into_owned())) }
        };
        steps.push(tester::TestStep {
            id: p.id.clone(),
            name: p.name.clone(),
            engine: p.engine.clone(),
            group: tester::group_of(p),
            exe: exe.to_string_lossy().into_owned(),
            workdir: wd.to_string_lossy().into_owned(),
            args,
        });
    }

    // Все кандидаты уже проверены — не поднимаем раннер вообще.
    if steps.is_empty() {
        let (sorted, best) = tester::summarize(&reused);
        let best_name = best
            .as_ref()
            .and_then(|id| reused.iter().find(|r| &r.id == id))
            .map(|r| r.name.clone());
        logger::log("info", "test", "повторный прогон не нужен — используем прежние результаты");
        set_test(
            &app,
            g,
            tester::TestProgress {
                running: false,
                phase: "done".into(),
                current_id: None,
                current_name: None,
                index: 0,
                total: 0,
                pct: 100,
                msg: "использованы прежние результаты (повторный прогон не нужен)".into(),
                results: sorted,
                best_id: best,
                best_name,
                done: true,
            },
        );
        g.set_op_running(false);
        emit(&app, "zgui:op", serde_json::json!({"running": false, "kind": "test"}));
        return Ok(true);
    }

    let (_plan, script, out_path) = tester::write_test_runner(&data, &steps, &custom, geoblock)?;
    let total = steps.len();
    // Тест поднимает свои winws: текущий обход (профиль, служба, ручной .bat)
    // обязан быть остановлен. Иначе winws выходит сразу с «A copy of winws is
    // already running with the same filter», а проба проходит «чужим» обходом.
    // Останавливаем ВСЕГДА: внешний winws в runtime не виден, а пустой
    // stop_all_own стоит копейки (tasklist/sc без запущенных процессов).
    let (had_runtime, had_service) = {
        let s = st(g);
        let (installed, running) = svc::service_state();
        (s.runtime.clone(), installed && running)
    };
    let _ = stop_all_own(&app, g);
    // Если winws всё ещё жив (чужой движок из другой папки или отказ UAC при
    // остановке) — тест даст мусор («A copy of winws is already running»).
    // Лучше честная ошибка, чем «ни одна стратегия не запустилась».
    if svc::any_winws_running() {
        logger::log("err", "test", "winws всё ещё запущен — тест отменён до остановки");
        return Err(
            "winws всё ещё запущен (обход вне программы или чужой процесс) — остановите его и повторите тест".into(),
        );
    }
    logger::log(
        "info",
        "test",
        &format!(
            "старт теста: {} стратегий, {} доменов{}",
            total,
            custom.len(),
            if geoblock { " (диагностика геоблока)" } else { "" }
        ),
    );

    // Если GUI уже запущен от администратора — не гоняем UAC-обёртку: она может
    // не стартовать дочерний процесс, и тест «зависает» на фазе запуска.
    let elevated = rn::is_elevated();

    let initial = tester::TestProgress {
        running: true,
        phase: "launch".into(),
        current_id: None,
        current_name: None,
        index: 0,
        total,
        pct: 0,
        msg: if elevated {
            "запускаю тест".into()
        } else {
            "запускаю тест (подтвердите права администратора один раз)".into()
        },
        results: Vec::new(),
        best_id: None,
        best_name: None,
        done: false,
    };
    {
        let mut cur = g.testing.lock().unwrap_or_else(|e| e.into_inner());
        *cur = initial.clone();
    }
    emit(&app, "zgui:test", initial);

    let app2 = app.clone();
    std::thread::spawn(move || {
        let ga2 = app2.state::<Global>();
        let g2 = ga2.inner();

        // Один UAC на весь тест: скрипт выполняет все стратегии внутри.
        // Если GUI уже админ — запускаем напрямую (без Start-Process -Verb RunAs).
        let launch = if elevated {
            rn::spawn_script_direct(&script).map(|_| ())
        } else {
            rn::spawn_elevated_script(&script)
        };
        if let Err(e) = launch {
            logger::log("err", "test", &format!("раннер теста не стартовал: {e}"));
            set_test(
                &app2,
                g2,
                tester::TestProgress {
                    running: false,
                    phase: "done".into(),
                    msg: format!("не удалось запустить тест: {}", e),
                    total,
                    results: Vec::new(),
                    done: true,
                    ..Default::default()
                },
            );
            return;
        }

        // Поллим промежуточный JSON, пока раннер пишет результаты.
        // `results` сразу содержит переиспользованные (из кэша) — они не гоняются.
        let mut results: Vec<tester::StrategyResult> = reused.clone();
        let mut parsed_count = 0usize;
        let started = std::time::Instant::now();
        // Таймаут — по НЕАКТИВНОСТИ, а не от старта: разовая неудачная чита
        // `test-out.json` (файл пишется после каждой стратегии, 2+ МБ) раньше
        // объявляла завершённый тест, хотя раннер продолжал работать —
        // остальные стратегии оставались «не тестировалась».
        let mut last_progress = started;
        loop {
            if test_marker(&data).0.exists() {
                break;
            }
            if let Some(v) = tester::read_test_progress(&out_path) {
                last_progress = std::time::Instant::now();
                // Фаза базовой пробы геоблок-теста (без Zapret): показываем прогресс,
                // иначе UI выглядит «замершим» до первого winws.
                if let Some(b) = v.get("baseline") {
                    let bd = b["done"].as_u64().unwrap_or(0);
                    let bt = b["total"].as_u64().unwrap_or(0);
                    let prog = tester::TestProgress {
                        running: true,
                        phase: "baseline".into(),
                        current_id: None,
                        current_name: None,
                        index: 0,
                        total,
                        pct: 0,
                        msg: format!("базовая проба (без Zapret): {}/{}", bd, bt),
                        results: Vec::new(),
                        best_id: None,
                        best_name: None,
                        done: false,
                    };
                    set_test(&app2, g2, prog);
                    std::thread::sleep(Duration::from_millis(700));
                    continue;
                }
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                let cur_id = v["currentId"].as_str().map(|s| s.to_string());
                let cur_name = v["currentName"].as_str().map(|s| s.to_string());
                if let Some(arr) = v["results"].as_array() {
                    let parsed: Vec<tester::StrategyResult> = arr
                        .iter()
                        .filter_map(|r| serde_json::from_value::<tester::StrategyResult>(r.clone()).ok())
                        .collect();
                    parsed_count = parsed.len();
                    let mut merged = reused.clone();
                    merged.extend(parsed);
                    results = merged;
                }
                let done = idx >= total && parsed_count >= total;
                let prog = tester::TestProgress {
                    running: !done,
                    phase: if done { "done".into() } else { "probe".into() },
                    current_id: cur_id,
                    current_name: cur_name.clone(),
                    index: idx,
                    total,
                    pct: ((idx as f64 / total.max(1) as f64) * 100.0) as i32,
                    msg: cur_name
                        .as_ref()
                        .map(|n| format!("тестирую «{}»", n))
                        .unwrap_or_else(|| "тест завершён".into()),
                    results: results.clone(),
                    best_id: None,
                    best_name: None,
                    done,
                };
                set_test(&app2, g2, prog);
                if done {
                    break;
                }
            } else {
                // Вотчдог запуска: если PID раннера так и не появился — не висим 120 с,
                // а выходим с понятной ошибкой. PID есть, но нет вывода — ждём, пока
                // не пройдёт 120 с БЕЗ успешных чтений (см. `last_progress`).
                let pid_seen = data.join("logs/test-runner.pid").exists();
                let (base, limit) = if pid_seen {
                    (last_progress, Duration::from_secs(120))
                } else {
                    (started, Duration::from_secs(20))
                };
                if base.elapsed() > limit {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(700));
        }
        let stopped = test_marker(&data).0.exists();
        let _ = std::fs::remove_file(&out_path);

        // Если раннер не оставил результатов — вероятнее всего UAC отклонён.
        // Переиспользованные (из кэша) оставляем, добавляем только «не стартовала».
        if parsed_count == 0 {
            let mut failed: Vec<tester::StrategyResult> = steps
                .iter()
                .map(|s| tester::StrategyResult {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    engine: s.engine.clone(),
                    group: s.group.clone(),
                    started: false,
                    score: 0,
                    max_score: custom.len() as u32,
                    domains: Vec::new(),
                    error: Some(if elevated {
                        "тест не запустился — раннер не стартовал (см. logs/test-runner.ps1 и run-*.err.txt)".into()
                    } else {
                        "тест не запустился — подтверждение прав администратора отклонено или раннер не стартовал".into()
                    }),
                    groups: Vec::new(),
                    critical_ok: false,
                    args_key: None,
                })
                .collect();
            results.append(&mut failed);
        }

        // Диагностика «стратегия не запустилась»: причины уже собраны раннером,
        // но в журнале их не было — при разборе жалоб не хватало фактов.
        for r in results.iter().filter(|r| !r.started) {
            let err: String = r.error.clone().unwrap_or_default().chars().take(300).collect();
            logger::log("err", "test", &format!("«{}» не запустилась: {}", r.name, err));
        }

        // Возвращаем обход, который остановили перед тестом: иначе пользователь
        // остаётся без защиты, а служба — в остановленном состоянии.
        if let Some(rt) = had_runtime {
            logger::log(
                "info",
                "test",
                &format!("возвращаю прежнюю стратегию «{}»", rt.profile_id),
            );
            if let Err(e) = do_start(&app2, g2, &rt.profile_id) {
                logger::log("err", "test", &format!("не удалось вернуть прежнюю стратегию: {e}"));
                emit(
                    &app2,
                    "zgui:toast",
                    serde_json::json!({"kind":"warn","text": format!("прежняя стратегия не вернулась: {e}")}),
                );
            }
        } else if had_service {
            if let Err(e) = svc::start_service(&data) {
                logger::log("err", "test", &format!("не удалось вернуть службу zapret: {e}"));
            }
        }

        // Калибровка геоблока: какие домены обходятся Zapret, а какие недоступны.
        if geoblock && !stopped {
            let mut passed: std::collections::HashSet<String> = std::collections::HashSet::new();
            for r in &results {
                for d in &r.domains {
                    if d.ok {
                        passed.insert(d.host.clone());
                    }
                }
            }
            let all_hosts: Vec<String> = custom.iter().map(|(_, h)| h.clone()).collect();
            let reachable: Vec<String> = passed.iter().cloned().collect();
            // «Недоступные» — не прошли НИ У ОДНОЙ стратегии (для них Zapret не помогает,
            // нужен VPN или сайт мёртв). Их вырезаем фильтром из основного теста.
            // Если прогон не дал ни одного успеха (тест не отработал) — ничего не помечаем,
            // иначе при сбое сети весь список стал бы «недоступным».
            let vpn_only: Vec<String> = if passed.is_empty() {
                Vec::new()
            } else {
                all_hosts
                    .iter()
                    .filter(|h| !passed.contains(*h))
                    .cloned()
                    .collect()
            };
            tester::save_reachability(&data, &reachable, &vpn_only);
            let msg = format!(
                "калибровка: обходится {} доменов, недоступны (только VPN) — {}",
                reachable.len(),
                vpn_only.len()
            );
            emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": msg}));
        }

        let (mut sorted, best) = tester::summarize(&results);
        // Помечаем ТОЛЬКО реально стартовавшие результаты ключом аргументов —
        // иначе «не запустилась» из кэша переиспользовалась бы как успешная.
        for r in sorted.iter_mut().filter(|r| r.started) {
            if let Some(p) = profiles.iter().find(|p| p.id == r.id) {
                r.args_key = Some(args_key(p, &tcp, &udp));
            }
        }
        let best_name = best
            .as_ref()
            .and_then(|id| results.iter().find(|r| &r.id == id))
            .map(|r| r.name.clone());
        // Диагностический геоблок-тест не влияет на «лучшую стратегию»/автостарт.
        let (best, best_name) = if geoblock { (None, None) } else { (best, best_name) };
        if stopped {
            logger::log("warn", "test", "тест остановлен пользователем");
            // Пользователь резко остановил тест: частичные результаты не считаем итоговыми.
            let msg = "тест остановлен пользователем".to_string();
            set_test(
                &app2,
                g2,
                tester::TestProgress {
                    running: false,
                    phase: "done".into(),
                    current_id: None,
                    current_name: None,
                    index: 0,
                    total,
                    pct: 0,
                    msg: msg.clone(),
                    results: Vec::new(),
                    best_id: None,
                    best_name: None,
                    done: true,
                },
            );
            emit(
                &app2,
                "zgui:toast",
                serde_json::json!({"kind":"info","text": msg}),
            );
            app2.state::<Global>().set_op_running(false);
            emit(&app2, "zgui:op", serde_json::json!({"running": false, "kind": "test"}));
            return;
        }
        if !geoblock {
            // Результаты ДОБАВляем к прежним: прогон одного движка не должен
            // стирать то, что уже намерено по другим (вкладка «Все» иначе
            // показывает только последний запуск).
            let mut cache = tester::TestCache::load(&data);
            for r in &sorted {
                match cache.results.iter_mut().find(|x| x.id == r.id) {
                    Some(old) => *old = r.clone(),
                    None => cache.results.push(r.clone()),
                }
            }
            cache.tested_at = Some(now_ts().to_string());
            if best.is_some() {
                cache.best_id = best.clone();
            }
            cache.save(&data);
        }
        let msg = if geoblock {
            "диагностика геоблок-списков завершена".to_string()
        } else {
            match &best_name {
                Some(n) => format!("лучшая стратегия: «{}»", n),
                None => "ни одна стратегия не набрала очков".to_string(),
            }
        };
        logger::log(
            if best_name.is_some() || geoblock { "ok" } else { "warn" },
            "test",
            &format!("тест завершён: {msg} (стратегий: {total})"),
        );
        set_test(
            &app2,
            g2,
            tester::TestProgress {
                running: false,
                phase: "done".into(),
                current_id: None,
                current_name: None,
                index: total,
                total,
                pct: 100,
                msg: msg.clone(),
                results: sorted,
                best_id: best,
                best_name,
                done: true,
            },
        );
        emit(
            &app2,
            "zgui:toast",
            serde_json::json!({"kind":"ok","text": msg}),
        );
        app2.state::<Global>().set_op_running(false);
        emit(&app2, "zgui:op", serde_json::json!({"running": false, "kind": "test"}));
    });
    Ok(true)
}

#[tauri::command(async)]
fn test_cache(ga: State<'_, Global>) -> tester::TestCache {
    tester::TestCache::load(&st(ga.inner()).data)
}

// ---------------------------------------------------------------- telegram

/// Открывает диспетчер задач Windows — запасной путь, если процесс не удаётся
/// завершить программно (защищённый/возрождаемый). Пользователь завершит вручную.
#[tauri::command(async)]
fn open_task_manager() -> Result<(), String> {
    rn::hidden_command("taskmgr.exe")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Открывает цель через оболочку Windows (ShellExecuteW). В отличие от
/// `cmd /c start`, не режет цель по `&` — ссылки с query-параметрами целы.
#[cfg(windows)]
fn shell_open(target: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = std::ffi::OsStr::new(target)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let op: Vec<u16> = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let h = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if (h as isize) <= 32 {
        return Err(format!("не удалось открыть (код {})", h as isize));
    }
    Ok(())
}

#[cfg(not(windows))]
fn shell_open(target: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Открывает внешнюю ссылку (http/https) или системное окно `.cpl` в оболочке
/// Windows. Разрешены только эти цели — произвольные команды из фронта закрыты.
/// Нужно для «Отправить отчёт» (GitHub Issues) и «Показать адаптеры» (`ncpa.cpl`).
#[tauri::command(async)]
fn open_external(target: String) -> Result<(), String> {
    // «Сетевые подключения»: ShellExecute по голому `ncpa.cpl` не находит файл —
    // надёжно открывается через `control.exe netconnections`.
    if target.eq_ignore_ascii_case("ncpa.cpl") {
        return std::process::Command::new("control.exe")
            .arg("netconnections")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    let ok = target.starts_with("https://") || target.starts_with("http://");
    if !ok {
        return Err("разрешены только ссылки http(s) и ncpa.cpl".into());
    }
    shell_open(&target)
}

#[tauri::command(async)]
fn tg_status(ga: State<'_, Global>) -> telegram::TgStatus {
    ga.inner().telegram.status()
}

#[tauri::command(async)]
fn tg_stats(ga: State<'_, Global>) -> String {
    ga.inner().telegram.stats()
}

/// Постоянный MTProto-секрет (32 hex) для бриджа. Генерируется один раз и
/// хранится в настройках: Telegram переиспользует одну запись прокси.
fn tg_secret_of(ga: &State<'_, Global>) -> String {
    let mut s = st(ga.inner());
    if s.settings.tg_secret.is_none() {
        s.settings.tg_secret = Some(gen_tg_secret());
        s.save();
    }
    s.settings.tg_secret.clone().unwrap_or_default()
}

/// 16 случайных байт в hex. RandomState засевается ОС на каждый экземпляр.
fn gen_tg_secret() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(32);
    for i in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(now_ts());
        h.write_u64(std::process::id() as u64 + i);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

#[tauri::command]
async fn tg_start(app: AppHandle, ga: State<'_, Global>, port: Option<u16>) -> Result<telegram::TgStatus, String> {
    let st = ga.inner().telegram.clone();
    let port = port.unwrap_or(1443);
    let secret = tg_secret_of(&ga);
    let result = st.start(port, None, Some(secret)).await;
    match &result {
        Ok(_) => {
            logger::log("ok", "telegram", &format!("прокси запущен на порту {port}"));
            emit(&app, "zgui:tg", serde_json::json!({"running": true}));
        }
        Err(e) => logger::log("err", "telegram", &format!("не удалось запустить прокси на порту {port}: {e}")),
    }
    result
}

#[tauri::command(async)]
fn tg_stop(app: AppHandle, ga: State<'_, Global>) -> telegram::TgStatus {
    let was_running = ga.inner().telegram.status().running;
    ga.inner().telegram.stop();
    logger::log("info", "telegram", "прокси остановлен");
    emit(&app, "zgui:tg", serde_json::json!({"running": false}));
    // Прокси в Telegram удалить программно нельзя — подсказываем, как выключить.
    if was_running {
        emit(&app, "zgui:toast", serde_json::json!({"kind":"info","text":"Чтобы Telegram перестал использовать прокси: Настройки → Продвинутые → Тип подключения → «Отключить прокси»."}));
    }
    ga.inner().telegram.status()
}

/// Следит: если включён наш Telegram-мост, а обнаружен VPN/туннель/сторонний
/// прокси — гасим мост (конфликт) и подсказываем отключить прокси в Telegram.
/// Троттлим, чтобы не дёргать tasklist на каждый bootstrap.
#[tauri::command(async)]
fn tg_vpn_guard(app: AppHandle, ga: State<'_, Global>) -> bool {
    let g = ga.inner();
    if !g.telegram.status().running {
        return false;
    }
    let now = now_ts();
    let last = g.tg_guard_checked.load(Ordering::SeqCst);
    if now < last + 20 {
        return false;
    }
    g.tg_guard_checked.store(now, Ordering::SeqCst);
    if svc::detect_vpn().is_empty() {
        return false;
    }
    g.telegram.stop();
    logger::log("warn", "telegram", "обнаружен VPN/туннель — Telegram-прокси выключен");
    emit(&app, "zgui:tg", serde_json::json!({"running": false}));
    emit(&app, "zgui:toast", serde_json::json!({"kind":"warn","text":"Обнаружен VPN/туннель — Telegram-прокси выключен. В Telegram отключите прокси: Настройки → Продвинутые → Тип подключения."}));
    true
}

/// Проверка обновления встроенного Telegram-моста (версия ZUI + коммиты Flowseal).
#[tauri::command(async)]
fn tg_check_update() -> updater::TgBridgeInfo {
    updater::check_tg_bridge()
}

/// Стоит ли предложить Telegram-мост: Telegram запущен И VPN/туннели не найдены,
/// мост ещё не включён и автозапуск не задан. Спрашиваем один раз за сессию
/// (флаг `tg_offer_shown`); проверку троттлим, чтобы не дёргать tasklist на каждый
/// опрос bootstrap.
#[tauri::command(async)]
fn tg_offer(ga: State<'_, Global>) -> bool {
    let g = ga.inner();
    if g.tg_offer_shown.load(Ordering::SeqCst) {
        return false;
    }
    let now = now_ts();
    let last = g.tg_offer_checked.load(Ordering::SeqCst);
    if now < last + 30 {
        return false;
    }
    g.tg_offer_checked.store(now, Ordering::SeqCst);
    if g.telegram.status().running {
        return false;
    }
    {
        let s = st(g);
        if !s.settings.tg_offer || s.settings.tg_autostart {
            return false;
        }
    }
    if !svc::telegram_running() {
        return false;
    }
    if !svc::detect_vpn().is_empty() {
        return false;
    }
    g.tg_offer_shown.store(true, Ordering::SeqCst);
    true
}

/// Сброс «предложение уже показывали»: вызывается при включении галочки
/// «предлагать автоматически», чтобы оффер сработал снова в этой сессии.
#[tauri::command(async)]
fn tg_offer_reset(ga: State<'_, Global>) {
    let g = ga.inner();
    g.tg_offer_shown.store(false, Ordering::SeqCst);
    g.tg_offer_checked.store(0, Ordering::SeqCst);
}

/// Текущее состояние watchdog (обход YouTube/Discord).
#[tauri::command(async)]
fn watchdog_status(ga: State<'_, Global>) -> watchdog::WatchdogStatus {
    ga.inner().watchdog.status()
}

/// Резко останавливает тест стратегий: стоп-флаг для раннера + мгновенный
/// kill деревьев (elevated раннер и активный winws) через один UAC.
#[tauri::command(async)]
fn cancel_test(ga: State<'_, Global>) -> Result<(), String> {
    let data = st(ga.inner()).data.clone();
    let (flag, run_pid, win_pid) = test_marker(&data);
    let read_pid = |p: &PathBuf| -> Vec<u32> {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .into_iter()
            .collect()
    };
    // Сначала маркер — поллер/раннер корректно завершат рабочий цикл сами.
    std::fs::write(&flag, now_ts().to_string()).map_err(|e| e.to_string())?;
    let mut pids = read_pid(&run_pid);
    pids.extend(read_pid(&win_pid));
    pids.dedup();
    if !pids.is_empty() {
        rn::stop_pids(&pids, &data)?;
    }
    // Фантомный статус: если поллер уже не крутится, cur.running иначе залипнет
    // навсегда и заблокирует новые прогоны. Гасим состояние и чистим маркеры.
    // (Берём ТОЛЬКО testing-лок — порядок testing→state фиксирован, иначе дедлок.)
    {
        let mut cur = ga.inner().testing.lock().unwrap_or_else(|e| e.into_inner());
        *cur = tester::TestProgress::default();
    }
    clear_test_markers(&data);
    Ok(())
}

/// Одно действие «применить лучшую стратегию»: включить автозапуск И запустить
/// сейчас. Раньше здесь только писался профиль автостарта — настройка висела
/// мёртвой (задача не создавалась), и запустить «прямо сейчас» мешал GUI-автозапуск.
#[tauri::command]
async fn apply_best_strategy(app: AppHandle, id: String) -> Result<(), String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ga = app2.state::<Global>();
        let g = ga.inner();
        if !st(g).profiles.iter().any(|p| p.id == id) {
            return Err("профиль не найден".into());
        }
        let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
        g.set_op_running(true);
        emit(&app2, "zgui:op", serde_json::json!({"running": true, "kind": "apply"}));
        let data = st(g).data.clone();
        {
            let mut s = st(g);
            s.settings.autostart_mode = "profile".into();
            s.settings.autostart_profile = Some(id.clone());
            s.save();
        }
        let mut c = tester::TestCache::load(&data);
        c.best_id = Some(id.clone());
        c.save(&data);
        sync_autostart(g);
        // Лок отпускаем ДО запуска: start_or_switch сам берёт OPS —
        // не-реентерабельный try_lock внутри того же лока всегда падал
        // «идёт другая операция» на самом себе.
        drop(_op);
        g.set_op_running(false);
        emit(&app2, "zgui:op", serde_json::json!({"running": false, "kind": "apply"}));
        let r = start_or_switch(&app2, g, &id);
        r?;
        emit(
            &app2,
            "zgui:toast",
            serde_json::json!({"kind":"ok","text":"Лучшая стратегия: автозапуск включён и запущена сейчас"}),
        );
        Ok(())
    })
    .await
    .map_err(|e| format!("применение стратегии прервано: {}", e))?
}

// -------------------------------------------------------- автозапуск, согласование

/// Валиден ли выбранный профиль автозапуска (существует среди профилей).
fn have_autostart_profile(s: &AppState) -> bool {
    s.settings.autostart_mode == "profile"
        && s
            .settings
            .autostart_profile
            .as_ref()
            .is_some_and(|id| s.profiles.iter().any(|p| &p.id == id))
}

/// Чистое решение: задачу планировщика держим только в программном режиме и
/// когда выбран существующий профиль автозапуска. Служба установлена — задача
/// не нужна (механизм обхода один).
fn autostart_wants_task(service_installed: bool, have_profile: bool) -> bool {
    !service_installed && have_profile
}

/// Приводит механизм автозапуска к единственному верному состоянию. Тихо и
/// best-effort: нехватка прав — в журнал, без ошибки-тупика в UI.
fn sync_autostart(g: &Global) {
    let (service_installed, have_profile, boot_app, data) = {
        let s = st(g);
        (s.service_running.is_some(), have_autostart_profile(&s), s.settings.boot_app, s.data.clone())
    };
    let want = autostart_wants_task(service_installed, have_profile);
    if want == boot_app {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            logger::log("warn", "boot", &format!("current_exe: {e}"));
            return;
        }
    };
    match rn::apply_boot_task(want, &exe, &data) {
        Ok(_) => {
            rn::remove_legacy_boot();
            let mut s = st(g);
            s.settings.boot_app = want;
            s.save();
            logger::log(
                "ok",
                "boot",
                if want {
                    "автозапуск: задача планировщика создана"
                } else {
                    "автозапуск: задача планировщика снята"
                },
            );
        }
        Err(e) => logger::log("warn", "boot", &format!("не удалось согласовать автозапуск: {e}")),
    }
}

// ---------------------------------------------------------------- конфликты

#[tauri::command(async)]
fn conflict_check(ga: State<'_, Global>) -> svc::ConflictReport {
    let s = st(ga.inner());
    let our_pid = s.runtime.as_ref().map(|r| r.pid);
    svc::detect_conflicts(&s.data, our_pid)
}

#[tauri::command(async)]
fn vpn_check() -> svc::ConflictReport {
    svc::ConflictReport {
        vpn: svc::detect_vpn(),
        ..Default::default()
    }
}

/// Полный безопасный сброс сети: снимает конфликты (zapret/VPN/WinDivert/прокси)
/// и восстанавливает интернет. Wi-Fi-пароли и настройки провайдера не трогаются.
/// Требует перезагрузку (winsock/int ip reset).
#[tauri::command(async)]
fn net_reset(ga: State<'_, Global>) -> Result<netreset::NetResetResult, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    let data = st(g).data.clone();
    logger::log("warn", "netreset", "запущено восстановление сети");
    let r = netreset::reset(&data);
    g.set_op_running(false);
    match r {
        Ok(r) => {
            logger::log("ok", "netreset", "восстановление сети завершено");
            Ok(r)
        }
        Err(e) => {
            logger::log("err", "netreset", &format!("восстановление сети не удалось: {e}"));
            Err(human::with_context("не удалось восстановить сеть", &e))
        }
    }
}

/// Создаёт точку восстановления Windows перед сбросом сети (не чаще раза в сутки).
#[tauri::command(async)]
fn net_create_restore_point(ga: State<'_, Global>) -> Result<String, String> {
    let data = st(ga.inner()).data.clone();
    netreset::create_restore_point(&data)
}

/// Список виртуальных сетевых адаптеров (только для информации — не удаляются).
#[tauri::command(async)]
fn virtual_adapters() -> Vec<String> {
    netreset::list_virtual_adapters()
}

/// Перезагрузка компьютера (с задержкой 5 с, чтобы успеть сохранить работу).
#[tauri::command(async)]
fn reboot_now() -> Result<(), String> {
    rn::hidden_command("shutdown")
        .args(["/r", "/t", "5"])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
fn kill_conflicts(app: AppHandle, ga: State<'_, Global>) -> Result<bool, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "kill"}));
    let (data, our_pid) = {
        let s = st(g);
        (s.data.clone(), s.runtime.as_ref().map(|r| r.pid))
    };
    let report = svc::detect_conflicts(&data, our_pid);
    if !report.has_conflicts() {
        g.set_op_running(false);
        emit(&app, "zgui:op", serde_json::json!({"running": false, "kind": "kill"}));
        return Ok(false);
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let r = svc::kill_conflicts(&report, our_pid, &data);
        // Итог считаем по факту: что было до и что осталось после (одно
        // уведомление в конце со списком выгруженных, а не по каждому процессу).
        let after = svc::detect_conflicts(&data, our_pid);
        let names_of = |rep: &svc::ConflictReport| -> std::collections::BTreeMap<String, u32> {
            let mut m = std::collections::BTreeMap::new();
            for p in rep.processes.iter().chain(rep.vpn.iter()) {
                if p.pid > 0 && !p.name.is_empty() && !p.name.starts_with("service:") {
                    *m.entry(p.name.clone()).or_insert(0) += 1;
                }
            }
            m
        };
        let before = names_of(&report);
        let remain = names_of(&after);
        let killed: Vec<String> = before
            .iter()
            .filter(|(n, _)| !remain.contains_key(*n))
            .map(|(n, c)| if *c > 1 { format!("{n} ×{c}") } else { n.clone() })
            .collect();
        let left: Vec<String> = remain.keys().cloned().collect();
        // После выгрузки VPN его системный прокси часто остаётся «висеть» —
        // чиним сразу, чтобы у пользователя не пропал интернет.
        let healed = heal_orphan_proxy();
        match r {
            Ok(_) => {
                if !killed.is_empty() {
                    emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": format!("выгружено: {}", killed.join(", "))}));
                }
                if !left.is_empty() {
                    emit(&app2, "zgui:toast", serde_json::json!({"kind":"warn","text": format!("не удалось выгрузить: {}", left.join(", "))}));
                } else if killed.is_empty() && !after.has_conflicts() {
                    emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text":"конфликтов нет"}));
                }
            }
            Err(e) => emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": format!("не удалось выгрузить: {e}")})),
        }
        if let Some(proxy) = healed {
            emit(
                &app2,
                "zgui:toast",
                serde_json::json!({"kind": "ok", "text": format!("сброшен нерабочий системный прокси {}", proxy)}),
            );
        }
        emit(&app2, "zgui:conflict", serde_json::json!({}));
        app2.state::<Global>().set_op_running(false);
        emit(&app2, "zgui:op", serde_json::json!({"running": false, "kind": "kill"}));
    });
    Ok(true)
}

// ---------------------------------------------------------------- служба

#[tauri::command(async)]
fn install_service(app: AppHandle, ga: State<'_, Global>, id: String) -> Result<(), String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "service"}));
    let (profile, root, args, data) = {
        let s = st(g);
        let p = s.profile(&id).cloned().ok_or("профиль не найден")?;
        let root = s.roots.path(&p.engine).ok_or("корень движка не задан — нажмите «Скачать движок»")?;
        let (tcp, udp) = pf::game_filter_ports(&s.settings.game_filter);
        (p.clone(), root.clone(), presets::prepare_args(&p.args, &root, &tcp, &udp), s.data.clone())
    };
    let r = svc::install_service(&root, &profile, &args, &data)
        .map_err(|e| {
            logger::log("err", "service", &format!("установка службы не удалась: {e}"));
            human::with_context("не удалось установить службу", &e)
        });
    if let Err(e) = r {
        g.set_op_running(false);
        emit(&app, "zgui:op", serde_json::json!({"running": false, "kind": "service"}));
        return Err(e);
    }
    logger::log("ok", "service", &format!("служба zapret установлена со стратегией «{}»", profile.name));
    {
        let mut s = st(g);
        s.service_running = Some(true);
        s.service_strategy = Some(profile.id.clone());
        s.settings.autostart_mode = "profile".into();
        s.settings.autostart_profile = Some(profile.id.clone());
        s.save();
    }
    // Служба — единственный механизм автозапуска: снимаем задачу планировщика,
    // если она была (раньше это был тупик с ошибкой «сначала отключите автозапуск»).
    sync_autostart(g);
    g.set_op_running(false);
    emit(&app, "zgui:op", serde_json::json!({"running": false, "kind": "service"}));
    emit(&app, "zgui:toast", serde_json::json!({"kind":"ok","text":"служба zapret установлена и запущена"}));
    Ok(())
}

#[tauri::command(async)]
fn remove_service(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "service"}));
    let data = st(g).data.clone();
    let r = svc::remove_service(&data).map_err(|e| {
        logger::log("err", "service", &format!("удаление службы не удалось: {e}"));
        human::with_context("не удалось удалить службу", &e)
    });
    if let Err(e) = r {
        g.set_op_running(false);
        emit(&app, "zgui:op", serde_json::json!({"running": false, "kind": "service"}));
        return Err(e);
    }
    logger::log("info", "service", "служба zapret удалена");
    let fallback = {
        let mut s = st(g);
        s.service_running = None;
        s.service_strategy = None;
        s.runtime = None;
        // Автозапуск не теряем: профиль сохраняем, дальше его подхватит программа.
        if s.settings.autostart_mode != "profile" {
            s.settings.autostart_profile = None;
        }
        let fallback = s.settings.autostart_mode == "profile";
        s.save();
        fallback
    };
    if fallback {
        sync_autostart(g);
    }
    g.set_op_running(false);
    emit(&app, "zgui:op", serde_json::json!({"running": false, "kind": "service"}));
    emit(&app, "zgui:status", serde_json::json!({"running": false, "pid": null, "profileId": null}));
    emit(
        &app,
        "zgui:toast",
        serde_json::json!({"kind":"ok","text": if fallback {
            "служба удалена — автозапуск теперь через программу"
        } else {
            "служба zapret удалена"
        }}),
    );
    Ok(())
}

// ---------------------------------------------------------------- обновления

/// Версия установленного движка Flowseal — из имени каталога релиза
/// (`zapret-discord-youtube-1.10.2`).
fn engine_version_from_root(root: &std::path::Path) -> Option<String> {
    root.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.rsplit_once('-').map(|(_, v)| v.to_string()))
        .filter(|v| v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EngineUpdateInfo {
    installed: Option<String>,
    latest: Option<String>,
    up_to_date: bool,
    error: Option<String>,
}

/// Проверка обновления движка: сравнение установленного релиза с последним на GitHub.
#[tauri::command(async)]
fn engine_check_update(ga: State<'_, Global>) -> EngineUpdateInfo {
    let (stored, root) = {
        let s = st(ga.inner());
        (
            s.engine_version.clone(),
            s.roots.path(ENGINE_FLOWSEAL),
        )
    };
    let installed = stored.or_else(|| root.as_ref().and_then(|r| engine_version_from_root(r)));
    match up::check_engine_latest() {
        Ok(latest) => {
            let up_to_date = installed.as_deref() == Some(latest.as_str());
            logger::log(
                "info",
                "engine",
                &format!(
                    "проверка обновления движка: установлен {}, последний {}",
                    installed.as_deref().unwrap_or("неизвестно"),
                    latest
                ),
            );
            EngineUpdateInfo {
                installed,
                latest: Some(latest),
                up_to_date,
                error: None,
            }
        }
        Err(e) => {
            logger::log("warn", "engine", &format!("проверка обновления движка: {e}"));
            EngineUpdateInfo {
                installed,
                latest: None,
                up_to_date: false,
                error: Some(e),
            }
        }
    }
}

#[tauri::command]
async fn check_updates(app: AppHandle, ga: State<'_, Global>) -> Result<bool, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "updates"}));
    let (data, roots, settings) = {
        let s = st(g);
        (s.data.clone(), s.roots.clone(), s.settings.clone())
    };
    let app2 = app.clone();
    std::thread::spawn(move || {
        match up::check_all(&data, &roots, &settings) {
            Ok(entries) => {
                let changed = entries
                    .iter()
                    .filter(|e| e.status == "avail" || e.status == "new")
                    .count();
                logger::log(
                    "info",
                    "updates",
                    &format!("проверка обновлений: {} файлов, требуют обновления {changed}", entries.len()),
                );
                let ga2 = app2.state::<Global>();
                let mut s = ga2.state.lock().unwrap();
                s.updater = UpdaterCache {
                    last_check: Some(crate::profiles::now_str()),
                    entries,
                    last_auto: s.updater.last_auto.clone(),
                    next_auto: s.updater.next_auto,
                };
                s.save();
                emit(&app2, "zgui:updates", updater_view(&s));
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text":"проверка обновлений конфигов завершена"}));
            }
            Err(e) => {
                logger::log("err", "updates", &format!("проверка обновлений не удалась: {e}"));
                log_updates(&data, &format!("check error: {}", e));
                // Событие нужно и при ошибке: фронт снимает им блокировку кнопок
                // (иначе «Проверить обновления» остаётся серой до таймаута).
                {
                    let ga2 = app2.state::<Global>();
                    let s = st(ga2.inner());
                    emit(&app2, "zgui:updates", updater_view(&s));
                }
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": human::with_context("не удалось проверить обновления", &e)}));
            }
        }
        app2.state::<Global>().set_op_running(false);
        emit(&app2, "zgui:op", serde_json::json!({"running": false, "kind": "updates"}));
    });
    Ok(true)
}

fn reload_bats_from_disk(s: &mut AppState) {
    let raw = s.raw_strategies_dir();
    let root = s.roots.path(ENGINE_FLOWSEAL);
    let existing = s.profiles.clone();
    if let Some(r) = root {
        let mut bats = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&raw) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.to_lowercase().ends_with(".bat") {
                    if let Ok(bytes) = std::fs::read(e.path()) {
                        bats.push((name, pf::decode_strategy_bytes(&bytes)));
                    }
                }
            }
        }
        let imported = pf::import_bat_profiles(&r, &bats, &existing);
        for p in imported {
            if let Some(ex) = s.profiles.iter_mut().find(|x| x.id == p.id) {
                if !ex.builtin {
                    ex.args = p.args;
                    ex.name = p.name;
                    ex.source = p.source;
                    ex.updated_at = p.updated_at;
                }
            } else {
                s.profiles.push(p);
            }
        }
    }
}

#[tauri::command]
async fn apply_updates(app: AppHandle, ga: State<'_, Global>, ids: Vec<String>) -> Result<bool, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    emit(&app, "zgui:op", serde_json::json!({"running": true, "kind": "updates"}));
    let (data, roots, settings) = {
        let s = st(g);
        (s.data.clone(), s.roots.clone(), s.settings.clone())
    };
    let app2 = app.clone();
    std::thread::spawn(move || {
        let wants_presets = ids.is_empty() || ids.iter().any(|i| i == up::PRESETS_ENTRY_ID);
        // OTA-набор пресетов качаем ЗАРАНЕЕ, вне блокировки state (сеть до 45 с).
        let (preset_set, preset_err) = if wants_presets {
            match up::fetch_preset_set() {
                Ok(Some(set)) => (Some(set), None),
                // Ассет ещё не опубликован — это не ошибка: встроенные пресеты актуальны.
                Ok(None) => (None, None),
                Err(e) => (None, Some(format!("не удалось скачать набор пресетов: {e}"))),
            }
        } else {
            (None, None)
        };
        // Файловые записи: id набора пресетов — не файл, исключаем. Если после
        // фильтра список пуст, а выбор был непустой — файловых записей не выбрано,
        // apply_updates не зовём (пустой список у него означает «все»).
        let file_ids: Vec<String> = ids.iter().filter(|i| *i != up::PRESETS_ENTRY_ID).cloned().collect();
        let file_applied = if ids.is_empty() || !file_ids.is_empty() {
            up::apply_updates(&data, &roots, &settings, file_ids)
        } else {
            Ok(Vec::new())
        };
        // Файловые записи и набор пресетов обрабатываем НЕЗАВИСИМО: падение
        // файловой части (сеть/лимит GitHub API) не должно терять уже скачанный
        // набор пресетов — иначе успешное обновление пресетов молча пропадало.
        let (mut entries, file_err) = match file_applied {
            Ok(e) => (e, None),
            Err(e) => {
                logger::log("err", "updates", &format!("применение файловых обновлений не удалось: {e}"));
                log_updates(&data, &format!("apply error: {e}"));
                (Vec::new(), Some(e))
            }
        };
        let ga2 = app2.state::<Global>();
        let mut s = ga2.state.lock().unwrap();
        let mut preset_note: Option<String> = None;
        if let Some(set) = &preset_set {
            let (updated, added) = up::apply_presets(&data, &mut s.profiles, set);
            preset_note = Some(format!("пресеты: обновлено {updated}, добавлено {added}"));
            entries.push(crate::config::UpdEntry {
                id: up::PRESETS_ENTRY_ID.into(),
                group: up::PRESETS_GROUP.into(),
                label: format!("presets.json ({updated} обновлено, {added} добавлено)"),
                dest: String::new(),
                exists: true,
                status: "ok".into(),
                remote_hash: Some(set.version.clone()),
                applied_hash: Some(set.version.clone()),
                local_hash: None,
                size: 0,
                error: None,
            });
        } else if let Some(err) = &preset_err {
            entries.push(crate::config::UpdEntry {
                id: up::PRESETS_ENTRY_ID.into(),
                group: up::PRESETS_GROUP.into(),
                label: "presets.json".into(),
                dest: String::new(),
                exists: false,
                status: "err".into(),
                remote_hash: None,
                applied_hash: None,
                local_hash: None,
                size: 0,
                error: Some(err.clone()),
            });
        }
        for e in &entries {
            if let Some(old) = s.updater.entries.iter_mut().find(|x| x.id == e.id) {
                *old = e.clone();
            } else {
                s.updater.entries.push(e.clone());
            }
        }
        if entries.iter().any(|e| e.group.starts_with("flowseal strategies") && e.status == "ok") {
            reload_bats_from_disk(&mut s);
        }
        s.updater.last_check = Some(crate::profiles::now_str());
        s.save();
        emit(&app2, "zgui:updates", updater_view(&s));
        let ok_count = entries.iter().filter(|e| e.status == "ok").count();
        let failed: Vec<String> = entries
            .iter()
            .filter(|e| e.status == "err")
            .map(|e| format!("{}: {}", e.label, e.error.clone().unwrap_or_default()))
            .collect();
        logger::log(
            if failed.is_empty() && file_err.is_none() { "ok" } else { "warn" },
            "updates",
            &format!(
                "обновлено записей: {ok_count}{}{}",
                preset_note.map(|n| format!(" ({n})")).unwrap_or_default(),
                if failed.is_empty() { String::new() } else { format!(", ошибки: {}", failed.join("; ")) }
            ),
        );
        if let Some(e) = &file_err {
            emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": human::with_context("не удалось применить обновления конфигов", e)}));
        } else if failed.is_empty() {
            emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": format!("обновлено записей: {}", ok_count)}));
        } else {
            // Часть записей (в т.ч. набор пресетов) не применилась — иначе тост
            // сообщал бы только «обновлено: N» и ошибка терялась для пользователя.
            emit(&app2, "zgui:toast", serde_json::json!({"kind":"warn","text": format!("обновлено записей: {ok_count}, ошибки: {}", failed.join("; "))}));
        }
        app2.state::<Global>().set_op_running(false);
        emit(&app2, "zgui:op", serde_json::json!({"running": false, "kind": "updates"}));
    });
    Ok(true)
}

// ---------------------------------------------------------------- настройки

#[tauri::command(async)]
fn set_settings(ga: State<'_, Global>, mut settings: Settings) -> Result<(), String> {
    // Флаг миграции не приходит с фронта — не сбрасываем его, иначе повторная
    // миграция 6→72 сработает после каждой сохранённой настройки.
    settings.interval_migrated = true;
    let autostart_changed = {
        let mut s = st(ga.inner());
        // Тема меняется только через `set_theme`: форма настроек её не присылает,
        // иначе сохранение сбрасывало бы выбор на дефолт. Флажок автозапуска GUI
        // задаётся не формой, а `sync_autostart` — здесь его просто сохраняем.
        settings.theme = s.settings.theme.clone();
        settings.boot_app = s.settings.boot_app;
        // Серверные поля, которых нет в форме настроек: иначе любое сохранение
        // обнуляло бы постоянный TG-секрет и сбрасывало флаг онбординга админа
        // (модалка «всегда от админа» всплывала бы снова).
        settings.tg_secret = s.settings.tg_secret.clone();
        settings.admin_onboarded = s.settings.admin_onboarded;
        let changed = s.settings.autostart_mode != settings.autostart_mode
            || s.settings.autostart_profile != settings.autostart_profile;
        s.settings = settings;
        s.save();
        changed
    };
    // Приводим задачу планировщика в соответствие только при смене полей
    // автозапуска: иначе любое сохранение (DNS, интервал) снова дёргало бы
    // создание задачи, то есть повторный UAC у не-админа.
    if autostart_changed {
        sync_autostart(ga.inner());
    }
    // Тост здесь не показываем: настройки применяются сразу при изменении поля,
    // и всплывашка на каждое переключение только мешала бы.
    Ok(())
}

/// Первый запуск (или повторное включение): «всегда от администратора» + фиксируем,
/// что предложение показано.
#[tauri::command(async)]
fn set_admin_prefs(ga: State<'_, Global>, always: bool) -> Result<(), String> {
    let mut s = st(ga.inner());
    s.settings.always_admin = always;
    s.settings.admin_onboarded = true;
    s.save();
    logger::log(
        "ok",
        "admin",
        if always {
            "включено «всегда запускать от администратора»"
        } else {
            "«всегда от администратора» выключено"
        },
    );
    Ok(())
}

/// Первый запуск: пользователь отложил выбор — больше не показываем предложение.
#[tauri::command(async)]
fn mark_admin_onboarded(ga: State<'_, Global>) {
    let mut s = st(ga.inner());
    s.settings.admin_onboarded = true;
    s.save();
}

#[tauri::command(async)]
fn set_theme(ga: State<'_, Global>, theme: String) -> Result<(), String> {
    let theme = match theme.as_str() {
        "grey" | "dark" | "light" => theme,
        other => return Err(format!("неизвестная тема: {}", other)),
    };
    let mut s = st(ga.inner());
    s.settings.theme = theme;
    s.save();
    Ok(())
}

#[tauri::command(async)]
fn read_log(ga: State<'_, Global>, kind: String, tail: usize) -> Result<String, String> {
    let s = st(ga.inner());
    let data = s.data.clone();
    let profile = s.runtime.as_ref().map(|r| r.profile_id.clone());
    let path = match kind.as_str() {
        "stdout" => profile.as_ref().map(|p| data.join("logs").join(format!("stdout-{}.txt", p))),
        "stderr" => profile.as_ref().map(|p| data.join("logs").join(format!("stderr-{}.txt", p))),
        "updates" => Some(data.join("logs").join("updates.log")),
        _ => None,
    };
    match path {
        Some(p) if p.exists() => Ok(crate::config::tail_file(&p, tail)),
        Some(_) => Ok(String::new()),
        None => Err("нет активной стратегии".into()),
    }
}

#[tauri::command(async)]
fn open_url(url: String) -> Result<(), String> {
    // Открываем только протокольные ссылки Telegram — не превращаем команду в «открой что угодно».
    if !url.starts_with("tg://") {
        return Err("разрешены только ссылки tg://".into());
    }
    // ВАЖНО: `cmd /c start` и `rundll32 url.dll` портят query-строку с `&`
    // (Telegram показывает «Некорректная ссылка»). ShellExecuteW передаёт URL
    // как есть — проверено вручную.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(&url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let op: Vec<u16> = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let h = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };
        // ShellExecuteW возвращает значение > 32 при успехе.
        if (h as isize) <= 32 {
            return Err(format!("не удалось открыть ссылку (код {})", h as isize));
        }
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command(async)]
fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Ok(());
    }
    let _ = std::process::Command::new("explorer.exe").arg(&p).spawn();
    Ok(())
}

#[tauri::command(async)]
fn ack_boot(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    let (pending, mode, profile) = {
        let s = st(g);
        (
            s.boot_pending,
            s.settings.autostart_mode.clone(),
            s.settings.autostart_profile.clone(),
        )
    };
    if !pending {
        return Ok(());
    }
    {
        let mut s = st(g);
        s.boot_pending = false;
        s.save();
    }
    if mode != "profile" {
        return Ok(());
    }
    let Some(pid) = profile else { return Ok(()) };
    // Если служба zapret установлена — обход при входе поднимает она.
    // Без этой проверки do_start() глушил службу (do_stop) и стартовал winws
    // как процесс приложения: «автозапуск службы» выглядел как «служба не работает».
    let (svc_installed, svc_running) = svc::service_state();
    if svc_installed {
        {
            let mut s = st(g);
            s.service_running = Some(svc_running);
            s.save();
        }
        logger::log(
            "info",
            "boot",
            if svc_running {
                "автозапуск: обход поднимает служба zapret — GUI-старт профиля пропущен"
            } else {
                "автозапуск: служба zapret установлена, но не запущена — GUI-старт профиля пропущен"
            },
        );
        return Ok(());
    }
    if !rn::is_elevated() {
        logger::log(
            "warn",
            "boot",
            "автозапуск без прав администратора — стратегия может не подняться",
        );
    }
    logger::log("info", "boot", &format!("автозапуск: старт профиля {pid}"));
    match do_start(&app, g, &pid) {
        Ok(_) => Ok(()),
        Err(e) => {
            logger::log("err", "boot", &format!("автозапуск стратегии не удался: {e}"));
            emit(
                &app,
                "zgui:toast",
                serde_json::json!({"kind":"err","text": format!("автозапуск стратегии не удался: {e}")}),
            );
            Err(e)
        }
    }
}

#[tauri::command(async)]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "Zapret GUI",
        "version": env!("CARGO_PKG_VERSION"),
        "portable": true,
        "elevated": rn::is_elevated(),
        "bundledConfigs": embedded::SNAPSHOT_INFO,
        "bundledEngines": embedded::ENGINE_INFO
    })
}

#[tauri::command(async)]
fn relaunch_as_admin(app: AppHandle) -> Result<(), String> {
    if rn::relaunch_as_admin()? {
        app.exit(0);
        Ok(())
    } else {
        logger::log("warn", "app", "запрос прав администратора отклонён");
        Err("запрос прав администратора отклонён — программа продолжает работу без прав".into())
    }
}

// ---------------------------------------------------------------- журнал

#[tauri::command(async)]
fn log_entries(after: u64) -> Vec<logger::Entry> {
    logger::entries(after)
}

/// Запись в журнал из интерфейса (ошибки команд, действия пользователя).
#[tauri::command(async)]
fn log_write(level: String, scope: String, msg: String) {
    logger::log(&level, &scope, &msg);
}

#[tauri::command(async)]
fn log_clear() {
    logger::clear();
    logger::log("info", "log", "журнал очищен");
}

#[tauri::command(async)]
fn log_dir_open() -> Result<String, String> {
    let dir = logger::dir().ok_or("журнал ещё не инициализирован")?;
    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
    Ok(dir.to_string_lossy().to_string())
}

/// Сохраняет текстовый отчёт о состоянии рядом с журналом и возвращает путь.
#[tauri::command(async)]
fn report_save(ga: State<'_, Global>) -> Result<String, String> {
    let s = st(ga.inner());
    let dir = logger::dir().unwrap_or_else(|| s.data.join("logs"));
    std::fs::create_dir_all(&dir).map_err(|e| human::with_context("не удалось создать папку отчёта", &e.to_string()))?;
    let path = dir.join(format!("отчёт-{}.txt", logger::now_stamp()));

    // Секция движков: все из реестра (готовность + путь).
    let engines_lines: String = config::engines()
        .iter()
        .map(|def| {
            let info = root_info(&s.roots, def.id, def.exe);
            format!(
                "  {:<12} : {} ({}: {})\r\n",
                def.label,
                info.path.unwrap_or_else(|| "не установлен".into()),
                def.exe,
                if info.exe.is_some() { "есть" } else { "НЕТ" }
            )
        })
        .collect();
    let runtime = match &s.runtime {
        Some(r) => format!("{} (pid {}, через {})", r.profile_id, r.pid, r.via),
        None => "не запущено".into(),
    };
    let service = match s.service_running {
        Some(true) => "установлена и работает",
        Some(false) => "установлена, остановлена",
        None => "не проверялась / не установлена",
    };
    let win = rn::hidden_command("cmd.exe")
        .args(["/c", "ver"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "неизвестно".into());

    let body = format!(
        "Zapret GUI — отчёт о состоянии\r\n\
         ============================================================\r\n\
         Версия программы : {ver}\r\n\
         Время отчёта     : {time}\r\n\
         Система          : {win}\r\n\
         Права админа     : {adm}\r\n\
         Папка программы  : {exe}\r\n\
         Папка данных     : {data}\r\n\
         ------------------------------------------------------------\r\n\
         Движки:\r\n{engines}         Профилей         : {total}\r\n\
         Сейчас запущено  : {runtime}\r\n\
         Служба Windows   : {service}\r\n\
         ------------------------------------------------------------\r\n\
         Настройки:\r\n\
           тема                : {theme}\r\n\
           автозапуск GUI      : {boot_gui} (задача планировщика {boot_task})\r\n\
           интервал обновлений : каждые {interval} ч\r\n\
           игровой фильтр      : {game}\r\n\
           режим ipset         : {ipset}\r\n\
           автозапуск движка   : {autostart} (профиль: {autostart_profile})\r\n\
           всегда от админа    : {always_admin}\r\n\
           Telegram-прокси     : порт {tg_port}, автозапуск: {tg_auto}\r\n\
         ============================================================\r\n\
         РЕЗУЛЬТАТЫ ТЕСТА СТРАТЕГИЙ\r\n\
         ============================================================\r\n\
         {tests}\r\n\
         ============================================================\r\n\
         ЖУРНАЛ (последние {count} записей)\r\n\
         ============================================================\r\n\
         {log}\r\n",
        ver = env!("CARGO_PKG_VERSION"),
        time = logger::stamp(
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        ),
        win = win,
        adm = if rn::is_elevated() { "да" } else { "НЕТ" },
        exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        data = s.data.display(),
        engines = engines_lines,
        total = s.profiles.len(),
        runtime = runtime,
        service = service,
        theme = s.settings.theme,
        boot_gui = if s.settings.boot_app { "включён" } else { "выключен" },
        boot_task = if rn::boot_task_exists() { "найдена" } else { "НЕ найдена" },
        interval = s.settings.update_interval_hours,
        game = s.settings.game_filter,
        ipset = s.settings.ipset_mode,
        autostart = s.settings.autostart_mode,
        autostart_profile = s.settings.autostart_profile.clone().unwrap_or_else(|| "нет".into()),
        always_admin = if s.settings.always_admin { "да" } else { "нет" },
        tg_port = s.settings.tg_port,
        tg_auto = if s.settings.tg_autostart { "да" } else { "нет" },
        count = logger::entries(0).len(),
        tests = {
            let cache = tester::TestCache::load(&s.data);
            tester::results_text(&cache.results, cache.best_id.as_deref())
        },
        log = logger::dump(),
    );

    std::fs::write(&path, body).map_err(|e| human::with_context("не удалось сохранить отчёт", &e.to_string()))?;
    logger::log("ok", "report", &format!("отчёт сохранён: {}", path.display()));
    let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
    Ok(path.to_string_lossy().to_string())
}

/// Кнопка «Результаты в журнал»: отдельный текстовый файл с таблицей теста
/// стратегий (очки, критические домены, не ответившие хосты) + запись в журнал.
#[tauri::command(async)]
fn test_report_save(ga: State<'_, Global>) -> Result<String, String> {
    let s = st(ga.inner());
    let cache = tester::TestCache::load(&s.data);
    let dir = logger::dir().unwrap_or_else(|| s.data.join("logs"));
    std::fs::create_dir_all(&dir)
        .map_err(|e| human::with_context("не удалось создать папку журнала", &e.to_string()))?;
    let path = dir.join(format!("тест-результаты-{}.txt", logger::now_stamp()));
    let body = format!(
        "Zapret GUI — результаты теста стратегий\r\n\
         Время        : {time}\r\n\
         Протестировано: {count} стратегий\r\n\
         Папка данных : {data}\r\n\
         ============================================================\r\n\
         {tests}",
        time = logger::stamp(
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        ),
        count = cache.results.len(),
        data = s.data.display(),
        tests = tester::results_text(&cache.results, cache.best_id.as_deref()),
    );
    std::fs::write(&path, body).map_err(|e| human::with_context("не удалось сохранить результаты", &e.to_string()))?;
    logger::log("ok", "report", &format!("результаты теста сохранены: {}", path.display()));
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(async)]
fn dns_providers() -> Vec<dns::DnsProvider> {
    dns::providers().to_vec()
}

#[tauri::command(async)]
fn apply_dns(ga: State<'_, Global>, provider: String, adapter: Option<String>) -> Result<String, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    let data = st(g).data.clone();
    let r = dns::apply(&data, &provider, adapter.as_deref());
    g.set_op_running(false);
    match r {
        Ok(msg) => {
            logger::log("ok", "dns", &format!("применён DNS {provider}: {msg}"));
            Ok(msg)
        }
        Err(e) => {
            logger::log("err", "dns", &format!("не удалось применить DNS {provider}: {e}"));
            Err(human::with_context("не удалось применить DNS", &e))
        }
    }
}

#[tauri::command(async)]
fn reset_dns(ga: State<'_, Global>, adapter: Option<String>) -> Result<String, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or("идёт другая операция — дождитесь завершения")?;
    g.set_op_running(true);
    let data = st(g).data.clone();
    let r = dns::reset(&data, adapter.as_deref());
    g.set_op_running(false);
    match r {
        Ok(msg) => {
            logger::log("info", "dns", "DNS возвращён на автоматический");
            Ok(msg)
        }
        Err(e) => {
            logger::log("err", "dns", &format!("не удалось сбросить DNS: {e}"));
            Err(human::with_context("не удалось вернуть стандартный DNS", &e))
        }
    }
}

/// Замеряет время отклика DNS-серверов (медиана из 3 UDP-запросов на адрес).
/// Выполняется в отдельном потоке, чтобы не блокировать UI.
#[tauri::command]
async fn dns_benchmark(ids: Option<Vec<String>>) -> Vec<dns::DnsPing> {
    tokio::task::spawn_blocking(move || dns::benchmark(ids))
        .await
        .unwrap_or_default()
}

// ---------------------------------------------------------------- фон

fn spawn_watchers(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_svc: u64 = 0;
        let mut startup_check = true;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let g = app.state::<Global>();

            let mut changed = false;
            {
                let mut s = st(&g);
                if let Some(rt) = s.runtime.clone() {
                    if rt.via == "app" {
                        let alive = rn::pid_alive(rt.pid);
                        if !alive {
                            s.runtime = None;
                            changed = true;
                            let _ = app.emit("zgui:status", serde_json::json!({"running": false, "pid": null, "profileId": null}));
                            let _ = app.emit("zgui:toast", serde_json::json!({"kind":"warn","text":"процесс запрета завершился — смотрите журнал"}));
                        }
                    }
                }
                if now_ts() - last_svc > 10 {
                    last_svc = now_ts();
                    let (installed, running) = svc::service_state();
                    let service_running = installed.then_some(running);
                    let strat = if installed {
                        svc::service_strategy(&s.data)
                    } else {
                        None
                    };
                    if s.service_running != service_running || s.service_strategy != strat {
                        s.service_running = service_running;
                        s.service_strategy = strat;
                        changed = true;
                    }
                }
                if changed {
                    s.save();
                }
            }
            // «Запущено вне программы»: winws нашего движка без записи runtime
            // (ручной .bat). Считаем владельца отдельно — current_owner сам берёт
            // state, вложенный захват дал бы дедлок. Тест и служба — не «внешние».
            let external = current_owner(&g) == WinwsOwner::External;
            {
                let mut s = st(&g);
                if s.external_winws != external {
                    s.external_winws = external;
                    s.save();
                }
            }

            // авто-проверка конфигов
            let (interval, last_auto, entries_empty, next_auto, roots, settings, data) = {
                let s = st(&g);
                (
                    s.settings.update_interval_hours,
                    s.updater.last_auto.clone(),
                    s.updater.entries.is_empty(),
                    s.updater.next_auto,
                    s.roots.clone(),
                    s.settings.clone(),
                    s.data.clone(),
                )
            };
            if interval > 0 && !g.is_busy() {
                let now = now_ts();
                let cooled = next_auto.is_none_or(|t| t <= now);
                let planned_due = match &last_auto {
                    Some(t) => t.parse::<u64>().unwrap_or(0) + interval as u64 * 3600 <= now,
                    None => true,
                };
                // Пустой каталог: игнорируем расписание (interval) и ждём только короткий
                // кулдаун повторной попытки, чтобы каталог заполнился сам при старте.
                let due = if startup_check {
                    // Первый тик после старта — один автоматический прогон (как кнопка).
                    true
                } else if entries_empty {
                    // Пустой каталог: игнорируем расписание и ждём короткий кулдаун
                    // повторной попытки, чтобы каталог заполнился сам.
                    cooled
                } else {
                    cooled && planned_due
                };
                if due {
                    startup_check = false;
                    g.set_busy(true);
                    let app2 = app.clone();
                    let mut s2 = st(&g);
                    let retry_secs = if entries_empty { 60 } else { 15 * 60 };
                    s2.updater.next_auto = Some(now + retry_secs);
                    s2.save();
                    drop(s2);
                    std::thread::spawn(move || {
                        let r = up::check_all(&data, &roots, &settings);
                        let ga3 = app2.state::<Global>();
                        let mut s3 = ga3.state.lock().unwrap();
                        match r {
                            Ok(entries) => {
                                s3.updater.entries = entries;
                                s3.updater.last_check = Some(crate::profiles::now_str());
                                s3.updater.last_auto = Some(crate::profiles::now_str());
                                s3.save();
                                let _ = app2.emit("zgui:updates", updater_view(&s3));
                                let _ = app2.emit("zgui:toast", serde_json::json!({"kind":"info","text":"автопроверка обновлений конфигов завершена"}));
                            }
                            Err(e) => {
                                logger::log("warn", "updates", &format!("автопроверка не удалась: {e}"));
                            }
                        }
                        app2.state::<Global>().set_busy(false);
                    });
                }
            }
        }
    });
}

/// Переносит старый автозапуск (HKCU\...\Run, версии ≤ 1.0.0) в задачу
/// планировщика и досоздаёт задачу, если автозапуск включён, а задачи нет.
/// Тихо, без UAC — досоздание возможно только при запуске от администратора.
fn provision_boot(state: &mut AppState) {
    if rn::legacy_boot_registered() && !state.settings.boot_app {
        state.settings.boot_app = true;
        logger::log(
            "info",
            "boot",
            "найден автозапуск из старой версии (реестр) — переношу в планировщик",
        );
    }
    // Профиль автозапуска пропал (вырезанный движок, удалённый профиль): задача
    // открывала бы GUI впустую при каждом входе. Снимаем её — механизм один.
    if state.settings.boot_app && !have_autostart_profile(state) {
        if rn::boot_task_exists() {
            if rn::is_elevated() {
                match std::env::current_exe() {
                    Ok(exe) => match rn::apply_boot_task(false, &exe, &state.data) {
                        Ok(_) => logger::log("ok", "boot", "задача планировщика снята: профиль автозапуска недоступен"),
                        Err(e) => logger::log("warn", "boot", &format!("не удалось снять задачу планировщика: {e}")),
                    },
                    Err(e) => logger::log("err", "boot", &format!("current_exe: {e}")),
                }
            } else {
                logger::log(
                    "warn",
                    "boot",
                    "автозапуск без выбранного профиля, но задача не снята — запустите GUI от администратора",
                );
            }
        }
        // Сбрасываем флаг, только если задачи действительно больше нет (иначе
        // повторим попытку при следующем запуске от администратора).
        if !rn::boot_task_exists() {
            state.settings.boot_app = false;
        }
        rn::remove_legacy_boot();
        return;
    }
    if !state.settings.boot_app {
        return;
    }
    if !rn::boot_task_exists() {
        if rn::is_elevated() {
            match std::env::current_exe() {
                Ok(exe) => match rn::apply_boot_task(true, &exe, &state.data) {
                    Ok(_) => logger::log("ok", "boot", "задача планировщика ZapretGUI создана"),
                    Err(e) => logger::log(
                        "err",
                        "boot",
                        &format!("не удалось создать задачу планировщика: {e}"),
                    ),
                },
                Err(e) => logger::log("err", "boot", &format!("current_exe: {e}")),
            }
        } else {
            logger::log(
                "warn",
                "boot",
                "автозапуск включён, но задача планировщика не найдена — запустите GUI от администратора",
            );
        }
    }
    // Старая запись реестра больше не нужна: иначе двойной запуск.
    rn::remove_legacy_boot();
}

/// Ставит окну «нативную» иконку из ресурсов exe — ту же, что видит Проводник.
///
/// Зачем: Tauri-кодоген берёт для окна **первый кадр** `icon.ico`
/// (`tauri-codegen/src/image.rs`: `icon_dir.entries()[0]`), а в нашем ICO первым
/// лежит 16×16. Панель задач рисует кнопку 24–48 px и растягивала этот 16-кадр —
/// отсюда «мыльная» иконка (жалоба владельца). Здесь Windows сама выбирает кадр
/// из группы иконок exe (как это делает Проводник): `LoadIconWithScaleDown`
/// (comctl32 v6, включён фичей `common-controls-v6`) + `WM_SETICON`.
/// Сам `icon.ico` при этом не меняется.
#[cfg(windows)]
fn apply_native_window_icon(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::LoadIconWithScaleDown;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LoadImageW, SendMessageW, HICON, ICON_BIG, ICON_SMALL, IMAGE_ICON, WM_SETICON,
    };

    // ID группы иконок, который встраивает tauri-build (проверено в собранном exe:
    // RT_GROUP_ICON = 32512).
    const ICON_GROUP_ID: usize = 32512;
    // Панель задач рисует 24–48 px: уменьшение всегда чётче растяжения.
    const TASKBAR_ICON_SIZE: i32 = 48;

    let hwnd = match window.hwnd() {
        Ok(h) => h.0,
        Err(e) => {
            logger::log("warn", "icon", &format!("не удалось получить HWND окна: {e}"));
            return;
        }
    };
    let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
    if hinst.is_null() {
        logger::log("warn", "icon", "не удалось получить HINSTANCE процесса");
        return;
    }
    // MAKEINTRESOURCE(32512): имя ресурса — число, упакованное в указатель.
    let name = ICON_GROUP_ID as *const u16;
    let mut hicon: HICON = std::ptr::null_mut();
    let hr = unsafe {
        LoadIconWithScaleDown(hinst, name, TASKBAR_ICON_SIZE, TASKBAR_ICON_SIZE, &mut hicon)
    };
    if hr < 0 || hicon.is_null() {
        // Фолбэк: LoadImage выберет ближайший кадр из группы.
        hicon = unsafe {
            LoadImageW(hinst, name, IMAGE_ICON, TASKBAR_ICON_SIZE, TASKBAR_ICON_SIZE, 0) as HICON
        };
    }
    if hicon.is_null() {
        logger::log("warn", "icon", "иконка из ресурсов не загрузилась — оставляю иконку Tauri");
        return;
    }
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, hicon as isize);
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, hicon as isize);
    }
    logger::log("info", "icon", "окну поставлена нативная иконка 48×48 из ресурсов exe");
}

#[cfg(not(windows))]
fn apply_native_window_icon(_window: &tauri::WebviewWindow) {}

// ---------------------------------------------------------------- builder

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logger::install_panic_hook();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = crate::config::portable_data_dir()?;
            let mut state = AppState::load(data);
            {
                // Логгер не знает про tauri — отдаём ему канал доставки в UI.
                let h = app.handle().clone();
                logger::init(
                    &state.data,
                    std::sync::Arc::new(move |e: logger::Entry| {
                        let _ = h.emit("zgui:log", &e);
                    }),
                );
            }
            logger::log(
                "info",
                "app",
                &format!(
                    "запуск Zapret GUI {} (админ: {}, портативно: {})",
                    env!("CARGO_PKG_VERSION"),
                    if rn::is_elevated() { "да" } else { "нет" },
                    state.data.display()
                ),
            );
            let elevated_flag = std::env::args().any(|a| a == "--elevated");
            let mut uac_declined = false;
            if state.settings.always_admin && !elevated_flag && !rn::is_elevated() {
                // Пользователь просил всегда работать от админа — перезапускаемся с UAC.
                // Если UAC отклонён, НЕ закрываем программу: работаем без прав и предупреждаем.
                match rn::relaunch_as_admin() {
                    Ok(true) => std::process::exit(0),
                    Ok(false) => {
                        uac_declined = true;
                        logger::log("warn", "app", "запрос прав администратора отклонён — запуск без прав");
                    }
                    Err(e) => {
                        uac_declined = true;
                        logger::log("err", "app", &format!("перезапуск от админа не удался: {e}"));
                    }
                }
            }
            // Защита от двух копий: вторая копия может перетереть state.json.
            // Не блокируем запуск (файл-замок может остаться от убитого процесса),
            // но предупреждаем пользователя.
            let lock_path = state.data.join("zgui.lock");
            let mut second_instance = false;
            if let Some(pid) = std::fs::read_to_string(&lock_path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
            {
                if pid != std::process::id() && rn::pid_alive(pid) {
                    second_instance = true;
                    logger::log(
                        "warn",
                        "app",
                        &format!("обнаружена уже запущенная копия программы (pid {pid})"),
                    );
                }
            }
            let _ = std::fs::write(&lock_path, std::process::id().to_string());
            embedded::seed_catalog(&state.data)?;
            provision_engines(&mut state);
            ensure_presets(&mut state);
            state.save();
            provision_boot(&mut state);
            // Чиним «осиротевший» системный прокси (остался от выгруженного VPN).
            let healed = heal_orphan_proxy();
            let webview_data = state.data.join("webview");
            let global = Global {
                state: Mutex::new(state),
                busy: AtomicBool::new(false),
                testing: Mutex::new(tester::TestProgress::default()),
                op_running: AtomicBool::new(false),
                telegram: telegram::TgState::default(),
                watchdog: std::sync::Arc::new(watchdog::WatchdogState::default()),
                tg_offer_shown: AtomicBool::new(false),
                tg_offer_checked: std::sync::atomic::AtomicU64::new(0),
                tg_guard_checked: std::sync::atomic::AtomicU64::new(0),
            };
            app.manage(global);
            // Автозапуск Telegram-прокси, если включён в настройках.
            {
                let g = app.state::<Global>();
                let (auto, port, secret, tg) = {
                    let mut s = st(g.inner());
                    if s.settings.tg_secret.is_none() {
                        s.settings.tg_secret = Some(gen_tg_secret());
                        s.save();
                    }
                    (
                        s.settings.tg_autostart,
                        s.settings.tg_port,
                        s.settings.tg_secret.clone().unwrap_or_default(),
                        g.inner().telegram.clone(),
                    )
                };
                if auto {
                    tauri::async_runtime::spawn(async move {
                        let _ = tg.start(port, None, Some(secret)).await;
                    });
                }
            }
            let window_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or("не найдена конфигурация главного окна")?;
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .data_directory(webview_data)
                .build()?;
            apply_native_window_icon(&window);
            let handle = app.handle().clone();
            spawn_watchers(handle.clone());
            watchdog::spawn(app.handle().clone(), app.state::<Global>().watchdog.clone());
            if let Some(proxy) = healed {
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(2500));
                    emit(
                        &h,
                        "zgui:toast",
                        serde_json::json!({
                            "kind": "ok",
                            "text": format!("сброшен нерабочий системный прокси {}", proxy)
                        }),
                    );
                });
            }
            if uac_declined || second_instance {
                let h = handle.clone();
                let text = if second_instance {
                    "уже запущена другая копия программы — закройте её, иначе настройки могут конфликтовать"
                } else {
                    "права администратора не получены — часть функций (запуск стратегий, тесты) недоступна"
                };
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(2500));
                    emit(
                        &h,
                        "zgui:toast",
                        serde_json::json!({ "kind": "warn", "text": text }),
                    );
                });
            }
            let boot = std::env::args().any(|a| a == "--boot");
            if boot {
                app.state::<Global>().state.lock().unwrap().boot_pending = true;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            log_entries,
            log_write,
            log_clear,
            log_dir_open,
            report_save,
            test_report_save,
            set_root,
            fetch_engine,
            refresh_catalog,
            save_profile,
            autotune_prepare,
            autotune_keep,
            delete_profile,
            start_profile,
            stop_running,
            current_status,
            test_strategies,
            test_status,
            test_cache,
            tg_status,
            tg_stats,
            tg_start,
            tg_stop,
            tg_check_update,
            tg_offer,
            tg_vpn_guard,
            watchdog_status,
            open_task_manager,
            open_external,
            tg_offer_reset,
            cancel_test,
            apply_best_strategy,
            install_service,
            remove_service,
            conflict_check,
            kill_conflicts,
            vpn_check,
            net_reset,
            net_create_restore_point,
            virtual_adapters,
            reboot_now,
            check_updates,
            apply_updates,
            engine_check_update,
            set_settings,
            set_theme,
            set_admin_prefs,
            mark_admin_onboarded,
            read_log,
            open_path,
            open_url,
            ack_boot,
            app_info,
            relaunch_as_admin,
            dns_providers,
            apply_dns,
            reset_dns,
            dns_benchmark
        ])
        .build(tauri::generate_context!())
        .expect("error while building zgui")
        .run(|app, event| {
            // При выходе гасим фоновый тестовый раннер: он elevated и сам по
            // закрытию GUI не умирает, а при следующем старте подхватывался как
            // «тест запустился сам». Пишем стоп-флаг (раннер сам завершит цикл);
            // PID убиваем без эскалации — если не хватит прав, сработает флаг.
            if matches!(event, tauri::RunEvent::Exit) {
                let data = app.state::<Global>().state.lock().unwrap_or_else(|e| e.into_inner()).data.clone();
                let (flag, run_pid, win_pid) = test_marker(&data);
                let _ = std::fs::write(&flag, now_ts().to_string());
                for p in [&run_pid, &win_pid] {
                    if let Ok(txt) = std::fs::read_to_string(p) {
                        if let Ok(pid) = txt.trim().parse::<u32>() {
                            let _ = rn::hidden_command("taskkill.exe")
                                .args(["/F", "/T", "/PID", &pid.to_string()])
                                .output();
                        }
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{
        autostart_wants_task, engine_meta, ensure_presets, local_proxy_port, unzip, winws_owner_of,
        WinwsOwner,
    };
    use crate::config;

    #[test]
    fn engine_meta_registry() {
        for def in crate::config::engines() {
            let id = def.id;
            let m = engine_meta(&id).unwrap_or_else(|e| panic!("нет meta для {id}: {e}"));
            let def = config::engine_def(&id).unwrap();
            assert_eq!(m.repo, def.repo);
            assert_eq!(m.exe, def.exe);
        }
        assert!(engine_meta("nope").is_err());
    }

    #[test]
    fn zapret2_fetched_from_self_repo() {
        // Релизный zip bol-van сносит Defender — zapret2 качаем нашей сборкой
        // из релиза нашего репо, asset engine-zapret2.zip.
        let m = engine_meta("zapret2").unwrap();
        assert_eq!(m.self_asset, Some("engine-zapret2.zip"));
        // Остальные движки — из репо автора.
        assert!(engine_meta("flowseal").unwrap().self_asset.is_none());
        assert!(engine_meta("goodbyedpi").unwrap().self_asset.is_none());
        assert!(engine_meta("dpibreak").unwrap().self_asset.is_none());
    }

    #[test]
    fn owner_precedence() {
        // Тест важнее всего: winws теста не «внешний» и не глушится тулбаром.
        assert_eq!(
            winws_owner_of(true, Some("p1"), Some(true), Some("p1"), true, true),
            WinwsOwner::Test
        );
        // Живой процесс программы.
        assert_eq!(
            winws_owner_of(false, Some("p1"), None, None, true, true),
            WinwsOwner::App("p1".into())
        );
        // Служба (в т.ч. когда winws ещё не успел появиться).
        assert_eq!(
            winws_owner_of(false, None, Some(true), Some("p2"), false, false),
            WinwsOwner::Service(Some("p2".into()))
        );
        // Свой winws вне программы — «внешний».
        assert_eq!(
            winws_owner_of(false, None, Some(false), None, true, true),
            WinwsOwner::External
        );
        // Служба установлена, но не запущена — не «внешний».
        assert_eq!(
            winws_owner_of(false, None, Some(false), Some("p2"), false, false),
            WinwsOwner::None
        );
        // Чужой winws (не нашего движка) владельцем не считается.
        assert_eq!(
            winws_owner_of(false, None, None, None, true, false),
            WinwsOwner::None
        );
    }

    #[test]
    fn autostart_task_only_in_program_mode() {
        assert!(autostart_wants_task(false, true));
        assert!(!autostart_wants_task(false, false));
        // Служба установлена — задача планировщика не нужна.
        assert!(!autostart_wants_task(true, true));
        assert!(!autostart_wants_task(true, false));
    }

    #[test]
    fn local_proxy_port_detects_only_local() {
        assert_eq!(local_proxy_port("127.0.0.1:10809"), Some(10809));
        assert_eq!(local_proxy_port("localhost:8080"), Some(8080));
        assert_eq!(local_proxy_port("http=127.0.0.1:1080;https=127.0.0.1:1080"), Some(1080));
        // Внешние/корпоративные прокси не трогаем.
        assert_eq!(local_proxy_port("proxy.corp.example:3128"), None);
        assert_eq!(local_proxy_port("10.0.0.1:8080"), None);
        assert_eq!(local_proxy_port("nonsense"), None);
    }

    #[test]
    fn ensure_presets_refreshes_builtin_args_and_purges_stale_cache() {
        // Регресс: исправленный в коде пресет оставался у пользователя в старой
        // (битой) версии из state.json — так GoodbyeDPI «RU + DNS» годами держал
        // несуществующий `-9`. Плюс кэш тестов хранил результаты удалённых пресетов.
        let data = std::env::temp_dir().join(format!("zgui-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let mut state = crate::config::AppState {
            data: data.clone(),
            roots: Default::default(),
            settings: Default::default(),
            profiles: vec![],
            runtime: None,
            updater: Default::default(),
            service_checked_at: 0,
            service_running: None,
            service_strategy: None,
            boot_pending: false,
            external_winws: false,
            engine_version: None,
        };
        state.profiles.push(crate::presets::preset_profile(
            "goodbyedpi-ru-dns",
            "goodbyedpi",
            "GoodbyeDPI · RU + DNS (старое)",
            vec!["-9".into(), "--dns-addr".into(), "77.88.8.8".into()],
        ));
        let mut own = crate::presets::preset_profile("my-own", "goodbyedpi", "Мой", vec!["-9".into()]);
        own.builtin = false;
        own.source = None;
        state.profiles.push(own);

        let mk = |id: &str| crate::tester::StrategyResult {
            id: id.into(),
            name: id.into(),
            engine: "flowseal".into(),
            group: "g".into(),
            started: true,
            score: 1,
            max_score: 2,
            domains: vec![],
            error: None,
            groups: vec![],
            critical_ok: false,
            args_key: None,
        };
        crate::tester::TestCache {
            tested_at: Some("1".into()),
            best_id: None,
            results: vec![mk("preset:goodbyedpi-9"), mk("preset:flowseal-general")],
        }
        .save(&data);

        ensure_presets(&mut state);

        let fixed = state
            .profiles
            .iter()
            .find(|p| p.id == "preset:goodbyedpi-ru-dns")
            .expect("пресет на месте");
        assert_eq!(fixed.args[0], "-5", "битый `-9` должен быть заменён на `-5`");
        assert_eq!(fixed.args.last().map(String::as_str), Some("1253"), "аргументы из таблицы");
        assert!(fixed.builtin, "пресет остаётся вшитым");
        let mine = state.profiles.iter().find(|p| p.id == "preset:my-own").expect("свой на месте");
        assert_eq!(mine.args, vec!["-9".to_string()], "пользовательский профиль не трогаем");
        assert_eq!(
            state.profiles.iter().filter(|p| p.builtin).count(),
            crate::presets::builtin_presets().len(),
            "все вшитые пресеты на месте"
        );
        let cache = crate::tester::TestCache::load(&data);
        assert_eq!(cache.results.len(), 1, "результат удалённого пресета вычищен");
        assert_eq!(cache.results[0].id, "preset:flowseal-general");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn unzip_handles_flat_wrapped_and_traversal_archives() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join(format!("zgui-unzip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = zip::write::SimpleFileOptions::default();
        let make = |zip_path: &std::path::Path, entries: &[(&str, &[u8])], dirs: &[&str]| {
            let mut w = zip::ZipWriter::new(std::fs::File::create(zip_path).unwrap());
            for d in dirs {
                w.add_directory(format!("{d}/"), opts).unwrap();
            }
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        };

        // Плоский архив (наш engine-zapret2.zip): без каталога-обёртки.
        let flat = dir.join("flat.zip");
        make(&flat, &[("winws2.exe", b"exe"), ("lua/x.lua", b"lua")], &["lua"]);
        let out_flat = dir.join("out-flat");
        unzip(&flat, &out_flat).expect("плоский архив должен распаковаться");
        assert!(out_flat.join("winws2.exe").is_file());
        assert!(out_flat.join("lua/x.lua").is_file());

        // Архив с каталогом-обёрткой: обёртка срезается.
        let wrapped = dir.join("wrapped.zip");
        make(&wrapped, &[("pkg/bin/winws.exe", b"exe")], &["pkg", "pkg/bin"]);
        let out_wrapped = dir.join("out-wrapped");
        unzip(&wrapped, &out_wrapped).expect("архив с обёрткой должен распаковаться");
        assert!(out_wrapped.join("bin/winws.exe").is_file());

        // Zip-slip: запись наружу не должна появиться рядом с целевым каталогом.
        let evil = dir.join("evil.zip");
        make(&evil, &[("../escape.txt", b"x"), ("ok.txt", b"y")], &[]);
        let out_evil = dir.join("out-evil");
        unzip(&evil, &out_evil).expect("zip-slip не должен ломать распаковку");
        assert!(!dir.join("escape.txt").exists(), "запись вне dest не должна создаваться");
        assert!(out_evil.join("ok.txt").is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
