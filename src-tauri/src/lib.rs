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

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

pub mod config;
mod diag;
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
mod texts;
mod tools;
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
    /// Эпоха теста стратегий: инкремент при отмене. Поллер прошлого прогона
    /// видит чужую эпоху и не трогает состояние/движок нового прогона.
    pub test_epoch: std::sync::atomic::AtomicU64,
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

/// RAII-гард долгой операции: ставит `op_running` и эмитит `zgui:op` при
/// создании, снимает — при выходе из области видимости (в том числе при
/// раннем `return`, ошибке или панике). Раньше флаг ставился/снимался руками
/// в каждой команде, и любой пропущенный путь оставлял GUI в «идёт операция…»
/// навсегда. Для фоновых операций гард ПЕРЕНОСИТСЯ в поток
/// (`std::thread::spawn(move || { let _g = guard; ... })`) и снимается при
/// завершении потока; `Global` берётся из `AppHandle` в момент срабатывания.
struct OpGuard {
    app: AppHandle,
    kind: &'static str,
}

impl OpGuard {
    fn new(app: &AppHandle, kind: &'static str) -> Self {
        app.state::<Global>().set_op_running(true);
        // Диагностика «зависло идёт операция…»: в журнале видно начало/конец
        // каждой операции — застрявшая останется без строки «завершилась».
        logger::log("info", "op", &format!("операция началась: {kind}"));
        emit(app, "zgui:op", serde_json::json!({"running": true, "kind": kind}));
        Self { app: app.clone(), kind }
    }
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        self.app.state::<Global>().set_op_running(false);
        logger::log("info", "op", &format!("операция завершилась: {}", self.kind));
        emit(&self.app, "zgui:op", serde_json::json!({"running": false, "kind": self.kind}));
    }
}

/// RAII-гард скачивания (`busy`): ставится при старте загрузки/авто-проверки,
/// снимается при выходе из области видимости (в том числе при панике потока).
/// Раньше флаг ставился/снимался руками — пропущенный путь оставлял «идёт
/// загрузка» до перезапуска. Для фонового потока гард переносится в поток.
struct BusyGuard(AppHandle);

impl BusyGuard {
    fn try_new(app: &AppHandle, busy_msg: &str) -> Result<Self, String> {
        let g = app.state::<Global>();
        if g.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(busy_msg.to_string());
        }
        Ok(Self(app.clone()))
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.state::<Global>().set_busy(false);
    }
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
    out.push(texts::VPN_CONFLICT.into());
    if s.external_winws {
        out.push(texts::EXTERNAL_WINWS.into());
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
            out.push(texts::author_bats(def.id));
        }
    }
    // Диагностика окружения (BFE, timestamps, конфликты, пути, hosts) — как
    // в «Run Diagnostics» автора; снимок кэшируется на процесс.
    out.extend(crate::diag::warnings(&s.data));
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
    /// Последняя запись state.json не удалась — фронт покажет предупреждение.
    save_failed: bool,
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
    // Снимок состояния — и лок сразу отпускаем: any_winws_running/own_engine_pids
    // спавнят tasklist, держать мьютекс state на время спавна нельзя (UI ждёт).
    let (data, app, service_running, service_strategy) = {
        let s = st(g);
        (
            s.data.clone(),
            s.runtime
                .as_ref()
                .filter(|r| rn::pid_alive(r.pid))
                .map(|r| r.profile_id.clone()),
            s.service_running,
            s.service_strategy.clone(),
        )
    };
    let testing = testing_flag || tester::runner_alive(&data);
    let any = svc::any_winws_running();
    let own = any && !svc::own_engine_pids(&data, None).is_empty();
    winws_owner_of(
        testing,
        app.as_deref(),
        service_running,
        service_strategy.as_deref(),
        any,
        own,
    )
}

/// Владелец по снимку state — без tasklist/WMI (дорогое сканирование делает
/// фоновый наблюдатель раз в несколько секунд). Для bootstrap, который
/// вызывается из UI каждые 4 с: пометка `external_winws` обновляется
/// наблюдателем, поэтому отдельный скан процессов здесь не нужен.
fn owner_from_state(g: &Global) -> WinwsOwner {
    let testing_flag = g.testing.lock().unwrap_or_else(|e| e.into_inner()).running;
    let (data, app, service_running, service_strategy, external) = {
        let s = st(g);
        (
            s.data.clone(),
            s.runtime
                .as_ref()
                .filter(|r| rn::pid_alive(r.pid))
                .map(|r| r.profile_id.clone()),
            s.service_running,
            s.service_strategy.clone(),
            s.external_winws,
        )
    };
    let testing = testing_flag || tester::runner_alive(&data);
    winws_owner_of(
        testing,
        app.as_deref(),
        service_running,
        service_strategy.as_deref(),
        external,
        external,
    )
}

// ---------------------------------------------------------------- корневой

#[tauri::command]
async fn bootstrap(app: AppHandle) -> Result<Bootstrap, String> {
    // Блокирующие вызовы (tasklist/WMI/powershell в current_owner/is_elevated) —
    // в отдельном потоке: раньше bootstrap занимал tokio-воркер и подвешивал
    // параллельные команды при старте.
    tauri::async_runtime::spawn_blocking(move || bootstrap_impl(app.state::<Global>().inner()))
        .await
        .map_err(|e| e.to_string())
}

fn bootstrap_impl(g: &Global) -> Bootstrap {
    // Владельца берём из state (наблюдатель уже сканировал процессы), а не
    // через current_owner: тот каждые 4 с спавнил tasklist/WMI.
    let owner = owner_name(&owner_from_state(g)).to_string();
    // is_elevated кэширует результат (элевация не меняется за жизнь процесса).
    let elevated = rn::is_elevated();
    let s = st(g);
    let engines = engines_info(&s);
    let (tcp, udp) = game_filter_ports_from(&s.settings);
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
        elevated,
        op_running: g.op_running(),
        owner,
        data_dir: s.data.to_string_lossy().into_owned(),
        warnings: collect_warnings(&s),
        save_failed: crate::config::save_failed(),
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
        return Err(texts::FOLDER_MISSING.into());
    }
    let exe_name = meta.exe;
    if crate::config::find_exe(&selected, exe_name).is_none() {
        return Err(texts::exe_missing_in_folder(exe_name));
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
    emit(&app, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::engine_root_set(&engine, &root.display().to_string())}));
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
        .ok_or_else(|| texts::unknown_engine(engine))?;
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
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let busy_guard = BusyGuard::try_new(&app, texts::BUSY_DOWNLOAD)?;
    let op_guard = OpGuard::new(&app, "fetch");
    let dest = dest.unwrap_or_else(|| st(g).data.join("engines").join(&engine).to_string_lossy().into_owned());
    let data_dir = st(g).data.clone();
    let app2 = app.clone();
    let engine2 = engine.clone();
    std::thread::spawn(move || {
        let _op_guard = op_guard;
        let _busy_guard = busy_guard;
        let tag = format!("fetch:{}", engine2);
        emit(&app2, "zgui:prog", serde_json::json!({"id": tag, "phase": "meta", "msg": texts::FETCH_META, "pct": 0}));
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
                emit(&app2, "zgui:prog", serde_json::json!({"id": tag, "phase": "done", "msg": texts::engine_installed_short(&engine2), "pct": 100}));
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::engine_installed(&engine2, &path)}));
            }
            Err(e) => {
                let friendly = human::with_context(&texts::engine_install_failed(&engine2), &e);
                logger::log("err", "engine", &format!("установка {engine2} не удалась: {e}"));
                emit(&app2, "zgui:prog", serde_json::json!({"id": tag, "phase": "error", "msg": friendly.clone(), "pct": -1}));
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": friendly}));
            }
        }
    });
    Ok(texts::engine_download_started(&engine))
}

fn fetch_engine_impl(app: &AppHandle, meta: &EngineMeta, dest: &str, data: &std::path::Path, engine: &str) -> Result<(String, String), String> {
    fn asset_url_of(x: &serde_json::Value) -> String {
        x["browser_download_url"].as_str().unwrap_or("").to_string()
    }
    fn asset_digest_of(x: &serde_json::Value) -> Option<String> {
        x["digest"].as_str().map(str::to_string)
    }
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
    // зваться по архитектуре. Вместе с URL берём `digest` из GitHub API —
    // независимый эталон SHA-256 для проверки скачанного файла.
    let zip_with_exe = |name: &str| -> Option<(String, Option<String>)> {
        rel["assets"].as_array().and_then(|a| {
            a.iter()
                .find(|x| {
                    x["name"].as_str().map(|n| n.to_lowercase().contains(name)).unwrap_or(false)
                        && x["name"].as_str().map(|n| n.ends_with(".zip")).unwrap_or(false)
                })
                .map(|x| (asset_url_of(x), asset_digest_of(x)))
        })
    };
    let asset = meta
        .self_asset
        .and_then(|name| zip_with_exe(name.trim_end_matches(".zip")))
        .or_else(|| zip_with_exe(meta.exe.trim_end_matches(".exe")))
        .or_else(|| zip_with_exe("win"))
        .or_else(|| {
            rel["assets"].as_array().and_then(|a| {
                a.iter()
                    .find(|x| x["name"].as_str().map(|n| n.ends_with(".zip")).unwrap_or(false))
                    .map(|x| (asset_url_of(x), asset_digest_of(x)))
            })
        })
        .unwrap_or_default();
    let (asset_url, api_digest) = asset;
    if asset_url.is_empty() {
        return Err(texts::NO_ZIP_IN_RELEASE.into());
    }

    let tmp_zip = data.join("tmp").join(format!("{}-{}.zip", engine, tag_name));
    // Удаляется при выходе из функции (в т.ч. при ошибке скачивания/распаковки).
    let _zip_guard = rn::TempFile::new(tmp_zip.clone());
    let mut resp = cli.get(&asset_url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(texts::download_http_error(resp.status()));
    }
    // Кап размера: и заявленный Content-Length, и фактически прочитанное —
    // вредоносный/битый релиз не должен забить диск.
    let total = resp.content_length().unwrap_or(0);
    let max_fetch_mb = up::MAX_FETCH / (1024 * 1024);
    if total > up::MAX_FETCH {
        return Err(texts::engine_too_large(max_fetch_mb));
    }
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
        if downloaded > up::MAX_FETCH {
            return Err(texts::engine_too_large(max_fetch_mb));
        }
        if total > 0 {
            let pct = ((downloaded as f64 / total as f64) * 100.0) as i32;
            emit(app, "zgui:prog", serde_json::json!({"id": tag, "phase": "download", "msg": texts::downloading(&tag_name, pct), "pct": pct}));
        }
    }
    drop(f);

    // Контур целостности: наш собственный ассет сверяем с закреплённым хешем,
    // сторонний релиз — с digest из GitHub API (независимый эталон, не «из
    // тех же байтов»). Нет эталона — не блокируем, но пишем предупреждение.
    let got = crate::config::file_sha256(&tmp_zip).ok_or_else(|| texts::engine_integrity_failed(engine))?;
    let expected: Option<String> = meta
        .self_asset
        .and_then(crate::config::pinned_asset_sha256)
        .map(str::to_string)
        .or_else(|| api_digest.as_deref().map(|d| d.strip_prefix("sha256:").unwrap_or(d).to_string()));
    match expected {
        Some(want) if !want.eq_ignore_ascii_case(&got) => {
            logger::log("err", "engine", &format!("{engine}: SHA-256 не совпал с эталоном — установка отменена"));
            return Err(texts::engine_integrity_failed(engine));
        }
        Some(_) => {}
        None => logger::log("warn", "engine", &format!("{engine}: эталонный хеш недоступен — целостность не подтверждена")),
    }

    // Staging: распаковка идёт в `<dest>.new`, живой каталог подменяется только
    // после успеха. При любой ошибке рабочий движок остаётся нетронутым.
    let dest_path = std::path::Path::new(dest);
    let staging = PathBuf::from(format!("{dest}.new"));
    let backup = PathBuf::from(format!("{dest}.old"));
    let _ = std::fs::remove_dir_all(&staging);
    emit(app, "zgui:prog", serde_json::json!({"id": tag, "phase": "unzip", "msg": texts::UNPACKING, "pct": 90}));
    let unpack = std::fs::create_dir_all(&staging)
        .map_err(|e| e.to_string())
        .and_then(|_| unzip(&tmp_zip, &staging).map_err(texts::unpack_failed));
    let _ = std::fs::remove_file(&tmp_zip);
    unpack?;

    // Корень ищем в staging до подмены: exe движка (рекурсивно — архив может
    // быть с каталогом-обёрткой). Нейтрализация автоапдейта — только Flowseal.
    let rel = crate::config::find_exe(&staging, meta.exe)
        .ok_or_else(|| texts::exe_missing_in_archive(meta.exe))?;
    let replace = || -> Result<(), String> {
        let _ = std::fs::remove_dir_all(&backup);
        if dest_path.exists() {
            std::fs::rename(dest_path, &backup).map_err(|e| {
                let _ = std::fs::remove_dir_all(&staging);
                format!("не удалось заменить каталог движка (занят?): {e}")
            })?;
        }
        if let Err(e) = std::fs::rename(&staging, dest_path) {
            // Вернуть рабочий каталог на место.
            if backup.exists() {
                let _ = std::fs::rename(&backup, dest_path);
            }
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("не удалось заменить каталог движка: {e}"));
        }
        let _ = std::fs::remove_dir_all(&backup);
        Ok(())
    };
    replace()?;

    let exe_path = dest_path.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let actual_root = exe_path
        .parent()
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| dest_path.to_path_buf());
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

/// Лимиты распаковки zip — защита от zip-бомбы (тысячи записей / гигабайты
/// распакованного объёма). Настоящие движки укладываются с большим запасом.
const UNZIP_MAX_ENTRIES: usize = 5000;
const UNZIP_MAX_TOTAL: u64 = 256 * 1024 * 1024;

fn unzip(zip_path: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    if archive.len() > UNZIP_MAX_ENTRIES {
        return Err(format!("слишком много файлов в архиве: {}", archive.len()));
    }
    let mut total: u64 = 0;

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
        if total.saturating_add(entry.size()) > UNZIP_MAX_TOTAL {
            return Err("архив превышает допустимый размер распаковки".into());
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        let copied = std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        total += copied;
        if total > UNZIP_MAX_TOTAL {
            return Err("архив превышает допустимый размер распаковки".into());
        }
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
    // Чистим: (1) явно вырезанные вшитые пресеты (id из REMOVED_PRESET_IDS) —
    // иначе остаются мёртвые стратегии вроде «GoodbyeDPI - 9» → `unknown option`;
    // (2) профили-кандидаты удалённого автоподбора (`auto:<движок>:*`) — функционал
    // убран, иначе они висят мусором в «Стратегиях» и кэше тестов.
    // OTA-пресеты не трогаем: их id могут не входить во вшитую таблицу, но они
    // актуальны.
    let removed: std::collections::HashSet<String> = presets::REMOVED_PRESET_IDS
        .iter()
        .map(|id| format!("preset:{}", id))
        .collect();
    let before = s.profiles.len();
    s.profiles.retain(|p| {
        if removed.contains(&p.id) {
            return false;
        }
        !p.source.as_deref().is_some_and(|src| src.starts_with("auto:"))
    });
    if s.profiles.len() != before {
        logger::log(
            "info",
            "profiles",
            &format!("удалено устаревших пресетов: {}", before - s.profiles.len()),
        );
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
    // Кэш тестов: результаты по профилям, которых больше нет (вырезанные пресеты),
    // иначе висят в списке как «битые» стратегии.
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


/// Пользовательские профили больше не поддерживаются (программа работает
/// только с авторскими конфигами). Удаляем ТОЛЬКО реально кастомные
/// (`source` пустой): профили из авторских `.bat` (`builtin: false`,
/// `source: "general (ALT).bat"`) и OTA-пресеты (`preset:*`) сохраняются.
/// Регресс 25.09: удаляли всё `!builtin` — flowseal-стратегии пропадали
/// до следующего «Проверить обновления → Применить».
fn drop_custom_profiles(s: &mut AppState) {
    let before = s.profiles.len();
    s.profiles.retain(|p| p.builtin || p.source.is_some());
    let removed = before - s.profiles.len();
    if removed > 0 {
        logger::log("info", "profiles", &format!("удалено пользовательских профилей: {removed}"));
    }
}

// ---------------------------------------------------------------- запуск

fn locate_exe(root: &std::path::Path, exe_name: &str) -> Result<PathBuf, String> {
    crate::config::find_exe(root, exe_name)
        .map(|rel| root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
        .ok_or_else(|| texts::engine_exe_missing(exe_name))
}

fn stop_service(data: &std::path::Path) -> Result<(), String> {
    let script = rn::LockedScript::write(
        data.join("logs").join(format!("svc_stop_{}.ps1", std::process::id())),
        &format!(
            "{}\nnet stop {} 2>$null | Out-Null\n\
             if ((& sc.exe query {} | Out-String) -match 'RUNNING') {{ exit 1 }} else {{ exit 0 }}",
            rn::PS_HEADER, SERVICE_NAME, SERVICE_NAME
        ),
    )?;
    let code = rn::run_script_privileged(script.path())?;
    if code != 0 {
        return Err(texts::STOP_FAILED.into());
    }
    Ok(())
}

fn do_stop(app: &AppHandle, g: &Global, silent: bool) -> Result<(), String> {
    let (rt, data) = {
        let s = st(g);
        (s.runtime.clone(), s.data.clone())
    };
    if let Some(rt) = rt {
        // Сначала реально останавливаем, потом чистим состояние и рапортуем:
        // при отказе UAC прежний код всё равно писал «остановлено», а winws
        // продолжал жить и выглядел «внешним» (жалоба владельца).
        let stop_res = if rt.via == "service" {
            stop_service(&data)
        } else if rn::pid_alive(rt.pid) {
            rn::stop_pid(rt.pid, &data)
        } else {
            Ok(()) // процесс уже завершился сам — останавливать нечего
        };
        // Процесс мог завершиться прямо в момент попытки (taskkill вернул
        // «доступ запрещён» уже умирающему) — судим по факту, а не по коду:
        // иначе «стратегия активна» висела бы при мёртвом движке.
        let stop_res = stop_res.or_else(|e| {
            if rt.via != "service" && !rn::pid_alive(rt.pid) {
                logger::log(
                    "warn",
                    "stop",
                    &format!("остановка «{}»: процесс уже завершён ({e})", rt.profile_id),
                );
                Ok(())
            } else {
                Err(e)
            }
        });
        if let Err(e) = stop_res {
            logger::log("err", "stop", &format!("остановка «{}» не удалась: {e}", rt.profile_id));
            return Err(texts::STOP_FAILED.into());
        }
        st(g).runtime = None;
        st(g).save();
        logger::log("info", "stop", &format!("остановлено: {} (pid {}, через {})", rt.profile_id, rt.pid, rt.via));
        emit(app, "zgui:status", serde_json::json!({"running": false, "pid": null, "profileId": null}));
        if !silent {
            emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::STOPPED_ONE}));
        }
        Ok(())
    } else {
        // возможно работает служба — глушим её процесс
        let (running, data) = {
            let s = st(g);
            (s.service_running, s.data.clone())
        };
        if running == Some(true) {
            if let Err(e) = stop_service(&data) {
                logger::log("err", "stop", &format!("остановка службы не удалась: {e}"));
                return Err(texts::STOP_FAILED.into());
            }
            // Служба осталась установленной, но остановлена — фиксируем сразу,
            // иначе UI ~10 с показывает «служба запущена» после «Остановить».
            let mut s = st(g);
            s.service_running = Some(false);
            s.save();
            emit(app, "zgui:status", serde_json::json!({"running": false, "pid": null, "profileId": null}));
        }
        if !silent {
            emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::ALL_STOPPED}));
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
        if let Err(e) = stop_service(&data) {
            logger::log("err", "stop", &format!("служба не остановилась: {e}"));
        }
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
        if let Err(e) = rn::stop_pids(&leftovers, &data) {
            logger::log("err", "stop", &format!("winws вне программы не остановился: {e}"));
        }
    }
    {
        let mut s = st(g);
        // Пометку «вне программы» снимаем только когда движков действительно
        // не осталось: иначе UI решит, что всё остановлено, при живом winws.
        if s.external_winws && !svc::any_winws_running() {
            s.external_winws = false;
            s.save();
        }
    }
    r
}

/// Снимает зависшие драйверы WinDivert нашей раскладки (после остановки —
/// движков уже нет). Раньше они оставались RUNNING до следующего старта GUI:
/// «драйвер висит, снять нельзя». Без прав — только запись в журнал.
fn cleanup_stray_drivers_after_stop() {
    let cleaned = svc::cleanup_own_stray_drivers();
    if !cleaned.is_empty() {
        logger::log("info", "windivert", &format!("сняты зависшие драйверы: {}", cleaned.join(", ")));
    }
}

#[tauri::command(async)]
fn stop_running(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    // Во время теста движок принадлежит тесту: тулбарная «Остановить» не должна
    // убивать winws теста (иначе тест продолжается «без обхода» и врёт цифрами).
    {
        let testing = g.testing.lock().unwrap_or_else(|e| e.into_inner()).running;
        if testing || tester::runner_alive(&st(g).data) {
            return Err(texts::TEST_RUNNING.into());
        }
    }
    // Взаимная блокировка: stop и start/test не пересекаются.
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let _guard = OpGuard::new(&app, "stop");
    let result = stop_all_own(&app, g);
    result?;
    // Движок остановлен — снимаем зависшие драйверы WinDivert нашей раскладки
    // (при старте стратегии этого не делаем: драйвер понадобится через секунду).
    cleanup_stray_drivers_after_stop();
    emit(&app, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::ALL_STOPPED}));
    Ok(())
}

/// Диапазоны Game Filter из настроек — единая точка вызова. Нельзя собирать
/// поля через несколько `st(g)` в одном выражении: мьютекс state не
/// реентерабельный, это дедлок (регресс 25.09: тест зависал на «идёт операция…»).
fn game_filter_ports_from(s: &Settings) -> (String, String) {
    pf::game_filter_ports(&s.game_filter, &s.game_filter_tcp, &s.game_filter_udp)
}

fn do_start(app: &AppHandle, g: &Global, id: &str) -> Result<Runtime, String> {
    let (profile, root_path, settings, data) = {
        let s = st(g);
        let p = s.profile(id).cloned().ok_or_else(|| {
            logger::log("err", "start", &format!("профиль {id} не найден"));
            texts::PROFILE_NOT_FOUND.to_string()
        })?;
        let root = s.roots.path(&p.engine).ok_or_else(|| {
            logger::log("warn", "start", &format!("корень движка «{}» не задан", p.engine));
            texts::engine_root_missing(&p.engine)
        })?;
        (p, root, s.settings.clone(), s.data.clone())
    };

    let _ = stop_all_own(app, g);

    let exe = locate_exe(&root_path, profile.exe_name()).map_err(|e| {
        logger::log("err", "start", &format!("{}: {e}", profile.name));
        human::humanize(&e)
    })?;
    let (tcp, udp) = game_filter_ports_from(&settings);
    let args = presets::prepare_args(&profile.args, &root_path, &tcp, &udp);

    let logs = data.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let log_id = pf::log_file_component(&profile.id);
    let out_log = logs.join(format!("stdout-{}.txt", log_id));
    let err_log = logs.join(format!("stderr-{}.txt", log_id));
    let pid_file = logs.join(format!("pid-{}.txt", log_id));

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
        let launcher = rn::write_launcher(&exe, &wd, &args, &pid_file)?;
        let pid = rn::spawn_and_wait_pid(launcher.path(), &pid_file, Duration::from_secs(60))?;
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
        return Err(texts::strategy_died(rn::is_elevated(), msg.trim()));
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
    emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::strategy_started(&profile.name)}));
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
        return Err(texts::TEST_RUNNING.into());
    }
    // Параллельная операция (обновления, DNS, сброс сети, служба): запрещаем
    // запуск, пока она не завершится — иначе старт/стоп могут пересечься.
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let _guard = OpGuard::new(app, "start");
    let result = if !svc::service_state().0 {
        do_start(app, g, id)
    } else {
        let (profile, root, args, data) = {
            let s = st(g);
            let p = s.profile(id).cloned().ok_or_else(|| texts::PROFILE_NOT_FOUND.to_string())?;
            let root = s
                .roots
                .path(&p.engine)
                .ok_or_else(|| texts::engine_root_missing(&p.engine))?;
            let (tcp, udp) = game_filter_ports_from(&s.settings);
            (p.clone(), root.clone(), presets::prepare_args(&p.args, &root, &tcp, &udp), s.data.clone())
        };
        // Один живой winws: снимаем процесс программы и старую службу перед пересозданием.
        let _ = stop_all_own(app, g);
        svc::install_service(&root, &profile, &args, &data).map_err(|e| {
            logger::log("err", "service", &format!("переключение службы не удалось: {e}"));
            human::with_context(texts::SERVICE_SWITCH_CONTEXT, &e)
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
        emit(app, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::service_switched(&profile.name)}));
        Ok(Runtime {
            profile_id: profile.id,
            pid: 0,
            started_at: now_ts(),
            via: "service".into(),
            alive: true,
        })
    };
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
    .map_err(|e| texts::start_interrupted(&e.to_string()))?
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

/// Убирает файлы-маркеры теста (после отмены или «фантомного» раннера).
fn clear_test_markers(data: &std::path::Path) {
    let (flag, run_pid, win_pid) = test_marker(data);
    for f in [flag, run_pid, win_pid] {
        let _ = std::fs::remove_file(f);
    }
}

/// ЕДИНСТВЕННАЯ точка остановки фонового раннера теста. Порядок: стоп-флаг
/// (раннер проверяет его в каждом шаге и внутри проб — останавливается за
/// секунды) → гашение процесса с деревом (там же его winws) → чистка маркеров.
/// Идемпотентна и вызывается отовсюду: кнопка «Остановить», таймаут ожидания,
/// выход из GUI, старт нового теста. `allow_uac` — разрешить элевированный
/// kill (единственный способ убить раннер немедленно, если GUI не админ).
fn stop_test_runner(data: &std::path::Path, allow_uac: bool) {
    let (flag, run_pid, win_pid) = test_marker(data);
    let _ = std::fs::write(&flag, now_ts().to_string());
    let pid = std::fs::read_to_string(&run_pid)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if let Some(pid) = pid {
        if rn::pid_alive(pid) {
            let _ = rn::hidden_command("taskkill.exe")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
            // Раннер теста запускается с элевацией: непривилегированный taskkill
            // его не берёт, нужен админский скрипт (один UAC-запрос).
            if rn::pid_alive(pid) && allow_uac {
                match rn::stop_pids(&[pid], data) {
                    Ok(()) => {}
                    Err(e) => logger::log("warn", "test", &format!("раннер теста {pid}: {e}")),
                }
            }
            if rn::pid_alive(pid) {
                logger::log(
                    "warn",
                    "test",
                    &format!("раннер теста {pid} ещё жив — остановится по стоп-флагу"),
                );
            } else {
                logger::log("info", "test", &format!("раннер теста остановлен (pid {pid})"));
            }
            // Даём процессу исчезнуть, чтобы маркеры не «ожили» следом.
            for _ in 0..30 {
                if !rn::pid_alive(pid) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    let _ = std::fs::remove_file(&win_pid);
    let _ = std::fs::remove_file(&run_pid);
    // Флаг не оставляем: иначе следующий прогон остановится на первом же шаге.
    let _ = std::fs::remove_file(&flag);
}

/// Есть ли признаки (живого или мёртвого) раннера — маркеры/флаг.
fn test_markers_present(data: &std::path::Path) -> bool {
    let (flag, run_pid, win_pid) = test_marker(data);
    flag.exists() || run_pid.exists() || win_pid.exists()
}

#[tauri::command(async)]
fn test_status(ga: State<'_, Global>) -> tester::TestProgress {
    let g = ga.inner();
    let cur = g.testing.lock().unwrap_or_else(|e| e.into_inner());
    if cur.running {
        return cur.clone();
    }
    // Фоновый раннер прошлой сессии (GUI закрывали, он elevated и живёт вне GUI):
    // не подхватываем и НЕ показываем «тест идёт» — гасим единой процедурой
    // (непривилегированный taskkill его не убивает, а маркеры нельзя стирать,
    // пока процесс жив: иначе раннер становится невидимым и блокирует движок).
    let data = st(g).data.clone();
    if test_markers_present(&data) {
        stop_test_runner(&data, false);
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
    reuse: Option<bool>,
) -> Result<bool, String> {
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
                return Err(texts::TEST_ALREADY.into());
            }
        }
    }
    // Взаимная блокировка: тест исключает запуск/остановку профилей и другие
    // долгие операции — иначе winws теста был бы убит или запущен рядом.
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let op_guard = OpGuard::new(&app, "test");
    // Диагностика зависаний старта: эта строка есть = команда дошла до подготовки.
    logger::log("info", "test", "тест: запрос принят, идёт подготовка");
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
        return Err(texts::TEST_NO_STRATEGIES.into());
    }
    // Без curl.exe проба не работает: честная ошибка вместо «все домены fail».
    if !tester::curl_available() {
        return Err(texts::CURL_MISSING.into());
    }
    // VPN мешает тесту — просим выгрузить (фронт показывает окно и вызывает kill_conflicts).
    // Чистота перед стартом: остатки прошлого прогона (осиротевший elevated-
    // раннер, его движок) гасим ДО начала — иначе новый тест упрётся в защиту
    // «winws всё ещё запущен», а два раннера подерутся за движок. Здесь UAC
    // допустим: пользователь сам только что инициировал тест.
    if test_markers_present(&data) {
        stop_test_runner(&data, true);
    }
    let vpn = svc::detect_vpn();
    if !vpn.is_empty() {
        return Err(format!("VPN_RUNNING:{}", vpn.len()));
    }
    for p in &profiles {
        if roots_ok.path(&p.engine).is_none() {
            return Err(texts::engine_root_missing(&p.name));
        }
    }

    // Стандартный набор целей — 1:1 с `utils/targets.txt` автора flowseal
    // (критические CDN + вторые по приоритету).
    let custom = tester::main_domains(usize::MAX);
    if custom.is_empty() {
        return Err(texts::TEST_NO_DOMAINS.into());
    }

    // Готовим шаги: exe, рабочий каталог, аргументы с game-filter.
    // Один лок state на все поля: несколько st(g) в одном выражении — дедлок
    // (std::sync::Mutex не реентерабельный).
    let (tcp, udp) = {
        let s = st(g);
        game_filter_ports_from(&s.settings)
    };

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
            texts::engine_root_missing(&p.name)
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
                msg: texts::TEST_REUSED.into(),
                results: sorted,
                best_id: best,
                best_name,
                done: true,
            },
        );
        return Ok(true);
    }

    let (_plan, script, out_path) = tester::write_test_runner(&data, &steps, &custom)?;
    // Защита от подмены раннера между записью и UAC-запуском (TOCTOU):
    // файл держится открытым с запретом записи до конца потока теста.
    let script_lock = rn::LockedScript::lock(script.clone())?;
    logger::log("info", "test", &format!("тест: план готов ({total_hint} шагов), пишу раннер", total_hint = steps.len()));
    // Паритет с автором: на время прогона `ipset-all.txt` переводится в «any»
    // (пустой), после — возврат; Drop гарантирует восстановление при отмене.
    let ipset_paths: Vec<std::path::PathBuf> = {
        let mut seen = std::collections::HashSet::new();
        let mut v = Vec::new();
        for p in &profiles {
            let Some(root) = roots_ok.path(&p.engine) else { continue };
            let f = root.join("lists").join("ipset-all.txt");
            if seen.insert(f.clone()) {
                v.push(f);
            }
        }
        v
    };
    let ipset_guard = tester::activate_ipset_any(&ipset_paths);
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
        return Err(texts::WINWS_RUNNING.into());
    }
    logger::log(
        "info",
        "test",
        &format!("старт теста: {} стратегий, {} доменов", total, custom.len()),
    );

    // Если GUI уже запущен от администратора — не гоняем UAC-обёртку: она может
    // не стартовать дочерний процесс, и тест «зависает» на фазе запуска.
    let elevated = rn::is_elevated();
    // Эпоха прогона: cancel_test() её увеличивает, и тогда этот поллер на выходе
    // не трогает состояние/движок (см. проверки ниже).
    let epoch = g.test_epoch.load(Ordering::SeqCst);

    let initial = tester::TestProgress {
        running: true,
        phase: "launch".into(),
        current_id: None,
        current_name: None,
        index: 0,
        total,
        pct: 0,
        msg: if elevated {
            texts::TEST_STARTING.into()
        } else {
            texts::TEST_STARTING_UAC.into()
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
        let _op_guard = op_guard;
        let _ipset_guard = ipset_guard;
        let _script_lock = script_lock;
        logger::log("info", "test", "тест: поток раннера стартовал");
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
                    msg: texts::test_start_failed(&e.to_string()),
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
        // Две фазы ожидания. Фаза 1 — появление PID раннера (UAC + старт
        // PowerShell + чтение плана): до 60 с, это не ошибка. Фаза 2 — прогресс;
        // лимит по НЕАКТИВНОСТИ и от размера плана: шаг с 20+ доменами, повтором
        // и DNS/проб-таймаутами занимает до ~2-3 минут, поэтому фиксированные
        // 120 с ложно объявляли «раннер не стартовал» (тест шёл, движок жил,
        // а результаты терялись).
        let batches = (custom.len() / 8 + 1) as u64;
        let idle_limit = Duration::from_secs(std::cmp::max(300, batches * 40 + 60));
        let mut last_progress = started;
        let mut pid_seen_at: Option<std::time::Instant> = None;
        let mut timed_out = false;
        loop {
            // Отмена (cancel_test) сменила эпоху: поллер прошлого прогона не
            // владеет ни состоянием, ни движком — молча выходим, уборку сделала отмена.
            if g2.test_epoch.load(Ordering::SeqCst) != epoch {
                logger::log("warn", "test", "поллер отменённого прогона завершился — уборку пропускаю");
                return;
            }
            if test_marker(&data).0.exists() {
                break;
            }
            if let Some(v) = tester::read_test_progress(&out_path) {
                last_progress = std::time::Instant::now();
                if pid_seen_at.is_none() {
                    pid_seen_at = Some(last_progress);
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
                        .map(|n| texts::testing_strategy(n))
                        .unwrap_or_else(|| texts::TEST_DONE.into()),
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
                // Фаза 1: PID ещё не появился (UAC/старт PowerShell) — ждём 60 с.
                // Фаза 2: PID есть, прогресса нет — ждём idle_limit без успешных чтений.
                if pid_seen_at.is_none() && data.join("logs/test-runner.pid").exists() {
                    pid_seen_at = Some(std::time::Instant::now());
                    last_progress = std::time::Instant::now();
                }
                let (base, limit) = match pid_seen_at {
                    Some(t) => {
                        let base = if last_progress > t { last_progress } else { t };
                        (base, idle_limit)
                    }
                    None => (started, Duration::from_secs(60)),
                };
                if base.elapsed() > limit {
                    timed_out = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(700));
        }
        // Эпоха могла смениться на последней итерации (отмена): тогда не трогаем
        // ни маркеры, ни движок, ни итоговое состояние — это уже не наш прогон.
        if g2.test_epoch.load(Ordering::SeqCst) != epoch {
            logger::log("warn", "test", "поллер отменённого прогона завершился — уборку пропускаю");
            return;
        }
        let stopped = test_marker(&data).0.exists();
        let _ = std::fs::remove_file(&out_path);
        // Раннер жив (таймаут/отмена/фантом) — гасим его вместе с движком:
        // осиротевший раннер живёт elevated и дальше блокирует тесты («winws всё
        // ещё запущен»), а маркеры без процесса не дают его найти. UAC здесь не
        // поднимаем: стоп-флаг останавливает раннер за секунды (проверки в
        // шагах и внутри проб), а гнать запрос прав в конце теста — плохой UX.
        if tester::runner_alive(&data) {
            stop_test_runner(&data, false);
        } else {
            clear_test_markers(&data);
        }

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
                    error: Some(if !elevated {
                        texts::TEST_NOT_STARTED_UAC.into()
                    } else if timed_out {
                        texts::test_timed_out(idle_limit.as_secs())
                    } else {
                        texts::TEST_NOT_STARTED.into()
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
                    serde_json::json!({"kind":"warn","text": texts::prev_strategy_failed(&e)}),
                );
            }
        } else if had_service {
            if let Err(e) = svc::start_service(&data) {
                logger::log("err", "test", &format!("не удалось вернуть службу zapret: {e}"));
            }
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
        if stopped {
            logger::log("warn", "test", "тест остановлен пользователем");
            // Пользователь резко остановил тест: частичные результаты не считаем итоговыми.
            let msg = texts::TEST_STOPPED.to_string();
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
            // Гард операции снимет «идёт операция» сам (любой выход из потока).
            return;
        }
        {
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
        let msg = match &best_name {
            Some(n) => texts::best_strategy(n),
            // Очки могли быть набраны, но «лучшая» — только та, что прошла
            // критические домены (summarize: started && critical_ok).
            None => texts::NO_BEST.to_string(),
        };
        logger::log(
            if best_name.is_some() { "ok" } else { "warn" },
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
        return Err(texts::open_failed(h as isize));
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
        return Err(texts::ONLY_HTTP_LINKS.into());
    }
    shell_open(&target)
}

#[tauri::command(async)]
fn tg_status(ga: State<'_, Global>) -> telegram::TgStatus {
    ga.inner().telegram.status()
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
        emit(&app, "zgui:toast", serde_json::json!({"kind":"info","text": texts::TG_PROXY_OFF_HINT}));
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
    emit(&app, "zgui:toast", serde_json::json!({"kind":"warn","text": texts::TG_VPN_OFF}));
    true
}

/// Проверка обновления встроенного Telegram-моста (версия ZUI + коммиты Flowseal).
#[tauri::command]
async fn tg_check_update() -> updater::TgBridgeInfo {
    // Сетевой запрос (reqwest::blocking) — не в tokio-воркере.
    tauri::async_runtime::spawn_blocking(updater::check_tg_bridge)
        .await
        .unwrap_or_else(|_| updater::TgBridgeInfo::default())
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

/// Резко останавливает тест стратегий: стоп-флаг для раннера + мгновенный
/// kill деревьев (elevated раннер и активный winws) через один UAC.
#[tauri::command(async)]
fn cancel_test(ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    let data = st(g).data.clone();
    // Новая эпоха: поллер отменённого прогона больше не владеет состоянием и
    // движком — без этого он на выходе убивал winws уже нового теста/профиля.
    g.test_epoch.fetch_add(1, Ordering::SeqCst);
    // Единая процедура: стоп-флаг + гашение раннера/движка + чистка маркеров.
    // UAC не поднимаем (стоп-флаг останавливает раннер за секунды: проверки в
    // шагах и внутри проб), но если GUI уже админ — kill сработает сразу.
    stop_test_runner(&data, false);
    // Фантомный статус: если поллер уже не крутится, cur.running иначе залипнет
    // навсегда и заблокирует новые прогоны.
    // (Берём ТОЛЬКО testing-лок — порядок testing→state фиксирован, иначе дедлок.)
    {
        let mut cur = g.testing.lock().unwrap_or_else(|e| e.into_inner());
        *cur = tester::TestProgress::default();
    }
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
            return Err(texts::PROFILE_NOT_FOUND.into());
        }
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let guard = OpGuard::new(&app2, "apply");
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
        drop(guard);
        let r = start_or_switch(&app2, g, &id);
        r?;
        emit(
            &app2,
            "zgui:toast",
            serde_json::json!({"kind":"ok","text": texts::BEST_APPLIED}),
        );
        Ok(())
    })
    .await
    .map_err(|e| texts::apply_failed(&e.to_string()))?
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
#[tauri::command]
async fn net_reset(app: AppHandle) -> Result<netreset::NetResetResult, String> {
    // Скрипт сброса с UAC (минуты) — не занимаем tokio-воркер.
    tauri::async_runtime::spawn_blocking(move || {
        let ga = app.state::<Global>();
        let g = ga.inner();
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "netreset");
        let data = st(g).data.clone();
        logger::log("warn", "netreset", "запущено восстановление сети");
        let r = netreset::reset(&data);
        match r {
            Ok(r) => {
                logger::log("ok", "netreset", "восстановление сети завершено");
                Ok(r)
            }
            Err(e) => {
                logger::log("err", "netreset", &format!("восстановление сети не удалось: {e}"));
                Err(human::with_context(texts::NET_RESET_FAILED, &e))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Создаёт точку восстановления Windows перед сбросом сети (не чаще раза в сутки).
#[tauri::command]
async fn net_create_restore_point(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = st(app.state::<Global>().inner()).data.clone();
        netreset::create_restore_point(&data)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Список виртуальных сетевых адаптеров (только для информации — не удаляются).
#[tauri::command]
async fn virtual_adapters() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(netreset::list_virtual_adapters)
        .await
        .unwrap_or_default()
}

/// Корень flowseal — в нём `bin\` с фейками (ACTIVE_*.bin).
fn flowseal_root(g: &Global) -> Result<std::path::PathBuf, String> {
    st(g)
        .roots
        .path(crate::config::ENGINE_FLOWSEAL)
        .ok_or_else(|| texts::engine_root_missing(crate::config::ENGINE_FLOWSEAL))
}

/// Чистка кэша Discord (аналог пункта меню автора): гасит клиент, удаляет кэши.
#[tauri::command]
async fn discord_cache_clear() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let r = tools::clear_discord_cache();
        match &r {
            Ok(m) => logger::log("ok", "tools", m),
            Err(e) => logger::log("err", "tools", &format!("кэш Discord: {e}")),
        }
        r
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Фейки flowseal: список `bin\*.bin` и активные файлы (по SHA-256, как автор).
#[tauri::command]
async fn fakes_view(app: AppHandle) -> Result<tools::FakesView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = flowseal_root(app.state::<Global>().inner())?;
        Ok(tools::fakes_view(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Замена `ACTIVE_*_UDP.bin` выбранным фейком (kind: discord|game).
#[tauri::command]
async fn replace_fake(app: AppHandle, kind: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = flowseal_root(app.state::<Global>().inner())?;
        let r = tools::replace_active_fake(&root, &kind, &name);
        match &r {
            Ok(m) => logger::log("ok", "tools", m),
            Err(e) => logger::log("err", "tools", &format!("замена фейка: {e}")),
        }
        r
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Скачивает свежий hosts автора и сравнивает с системным (замена — вручную).
#[tauri::command]
async fn hosts_update(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = st(app.state::<Global>().inner()).data.clone();
        let r = tools::hosts_update(&data);
        match &r {
            Ok(m) => logger::log("ok", "tools", m),
            Err(e) => logger::log("err", "tools", &format!("обновление hosts: {e}")),
        }
        r
    })
    .await
    .map_err(|e| e.to_string())?
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
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let op_guard = OpGuard::new(&app, "kill");
    let (data, our_pid) = {
        let s = st(g);
        (s.data.clone(), s.runtime.as_ref().map(|r| r.pid))
    };
    let report = svc::detect_conflicts(&data, our_pid);
    if !report.has_conflicts() {
        return Ok(false);
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let _op_guard = op_guard;
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
                    emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::killed_list(&killed.join(", "))}));
                }
                if !left.is_empty() {
                    emit(&app2, "zgui:toast", serde_json::json!({"kind":"warn","text": texts::kill_left(&left.join(", "))}));
                } else if killed.is_empty() && !after.has_conflicts() {
                    emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::NO_CONFLICTS}));
                }
            }
            Err(e) => emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": texts::kill_failed(&e)})),
        }
        if let Some(proxy) = healed {
            emit(
                &app2,
                "zgui:toast",
                serde_json::json!({"kind": "ok", "text": texts::proxy_healed(&proxy)}),
            );
        }
        emit(&app2, "zgui:conflict", serde_json::json!({}));
    });
    Ok(true)
}

// ---------------------------------------------------------------- служба

#[tauri::command(async)]
fn install_service(app: AppHandle, ga: State<'_, Global>, id: String) -> Result<(), String> {
    let g = ga.inner();
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let _guard = OpGuard::new(&app, "service");
    let (profile, root, args, data) = {
        let s = st(g);
        let p = s.profile(&id).cloned().ok_or(texts::PROFILE_NOT_FOUND)?;
        let root = s.roots.path(&p.engine).ok_or_else(|| texts::engine_root_missing(&p.engine))?;
        let (tcp, udp) = game_filter_ports_from(&s.settings);
        (p.clone(), root.clone(), presets::prepare_args(&p.args, &root, &tcp, &udp), s.data.clone())
    };
    let r = svc::install_service(&root, &profile, &args, &data)
        .map_err(|e| {
            logger::log("err", "service", &format!("установка службы не удалась: {e}"));
            human::with_context(texts::SERVICE_INSTALL_CONTEXT, &e)
        });
    r?;
    logger::log("ok", "service", &format!("служба zapret установлена со стратегией «{}»", profile.name));
    // Запись стратегии в реестр могла не пройти (см. service.rs): предупреждаем
    // явно, иначе GUI покажет «служба без стратегии» без объяснения.
    if svc::service_strategy(&data).is_none() {
        emit(&app, "zgui:toast", serde_json::json!({"kind":"warn","text": texts::SERVICE_STRATEGY_MISSING}));
    }
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
    emit(&app, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::SERVICE_INSTALLED}));
    Ok(())
}

#[tauri::command(async)]
fn remove_service(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let _guard = OpGuard::new(&app, "service");
    let data = st(g).data.clone();
    let r = svc::remove_service(&data).map_err(|e| {
        logger::log("err", "service", &format!("удаление службы не удалось: {e}"));
        human::with_context(texts::SERVICE_REMOVE_CONTEXT, &e)
    });
    r?;
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
    emit(&app, "zgui:status", serde_json::json!({"running": false, "pid": null, "profileId": null}));
    emit(
        &app,
        "zgui:toast",
        serde_json::json!({"kind":"ok","text": if fallback {
            texts::SERVICE_REMOVED_FALLBACK
        } else {
            texts::SERVICE_REMOVED
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
#[tauri::command]
async fn engine_check_update(app: AppHandle) -> EngineUpdateInfo {
    tauri::async_runtime::spawn_blocking(move || engine_check_update_impl(app.state::<Global>().inner()))
        .await
        .unwrap_or_else(|_| EngineUpdateInfo {
            installed: None,
            latest: None,
            up_to_date: false,
            error: Some(texts::CHECK_INTERRUPTED.into()),
        })
}

fn engine_check_update_impl(g: &Global) -> EngineUpdateInfo {
    let (stored, root) = {
        let s = st(g);
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
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let op_guard = OpGuard::new(&app, "updates");
    let (data, roots, settings) = {
        let s = st(g);
        (s.data.clone(), s.roots.clone(), s.settings.clone())
    };
    let app2 = app.clone();
    std::thread::spawn(move || {
        let _op_guard = op_guard;
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
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::UPDATES_CHECKED}));
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
                emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": human::with_context(texts::UPDATES_CHECK_CONTEXT, &e)}));
            }
        }
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
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let op_guard = OpGuard::new(&app, "updates");
    let (data, roots, settings) = {
        let s = st(g);
        (s.data.clone(), s.roots.clone(), s.settings.clone())
    };
    let app2 = app.clone();
    std::thread::spawn(move || {
        let _op_guard = op_guard;
        let wants_presets = ids.is_empty() || ids.iter().any(|i| i == up::PRESETS_ENTRY_ID);
        // OTA-набор пресетов качаем ЗАРАНЕЕ, вне блокировки state (сеть до 45 с).
        let (preset_set, preset_err) = if wants_presets {
            match up::fetch_preset_set() {
                Ok(Some(set)) => (Some(set), None),
                // Ассет ещё не опубликован — это не ошибка: встроенные пресеты актуальны.
                Ok(None) => (None, None),
                Err(e) => (None, Some(texts::presets_download_failed(&e))),
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
                let mut s = ga2.state.lock().unwrap_or_else(|e| e.into_inner());
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
            emit(&app2, "zgui:toast", serde_json::json!({"kind":"err","text": human::with_context(texts::UPDATES_APPLY_CONTEXT, e)}));
        } else if failed.is_empty() {
            emit(&app2, "zgui:toast", serde_json::json!({"kind":"ok","text": texts::updates_applied(ok_count)}));
        } else {
            // Часть записей (в т.ч. набор пресетов) не применилась — иначе тост
            // сообщал бы только «обновлено: N» и ошибка терялась для пользователя.
            emit(&app2, "zgui:toast", serde_json::json!({"kind":"warn","text": texts::updates_applied_partial(ok_count, &failed.join("; "))}));
        }
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
        other => return Err(texts::unknown_theme(other)),
    };
    let mut s = st(ga.inner());
    s.settings.theme = theme;
    s.save();
    Ok(())
}

#[tauri::command(async)]
fn open_url(url: String) -> Result<(), String> {
    // Открываем только протокольные ссылки Telegram — не превращаем команду в «открой что угодно».
    if !url.starts_with("tg://") {
        return Err(texts::ONLY_TG_LINKS.into());
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
            return Err(texts::open_link_failed(h as isize));
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

#[tauri::command]
async fn ack_boot(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
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
                "автозапуск: оптимизацию поднимает служба zapret — GUI-старт профиля пропущен"
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
    // Через start_or_switch (а не do_start напрямую): проверка теста, ops_try и
    // OpGuard — иначе автозапуск мог стартовать winws поверх теста или операции.
    // Блокирующий путь (UAC до 60 с) — в spawn_blocking, tokio-воркер не занят.
    let app2 = app.clone();
    let pid2 = pid.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        let ga = app2.state::<Global>();
        start_or_switch(&app2, ga.inner(), &pid2)
    })
    .await
    .map_err(|e| texts::start_interrupted(&e.to_string()))?;
    match res {
        Ok(_) => Ok(()),
        Err(e) => {
            logger::log("err", "boot", &format!("автозапуск стратегии не удался: {e}"));
            emit(
                &app,
                "zgui:toast",
                serde_json::json!({"kind":"err","text": texts::autostart_failed(&e)}),
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
        Err(texts::ADMIN_DECLINED.into())
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
fn log_dir_open() -> Result<String, String> {
    let dir = logger::dir().ok_or(texts::LOG_NOT_READY)?;
    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
    Ok(dir.to_string_lossy().to_string())
}

/// Сохраняет текстовый отчёт о состоянии рядом с журналом и возвращает путь.
#[tauri::command]
async fn report_save(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || report_save_impl(app.state::<Global>().inner()))
        .await
        .map_err(|e| e.to_string())?
}

fn report_save_impl(g: &Global) -> Result<String, String> {
    let s = st(g);
    let dir = logger::dir().unwrap_or_else(|| s.data.join("logs"));
    std::fs::create_dir_all(&dir).map_err(|e| human::with_context(texts::REPORT_DIR_FAILED, &e.to_string()))?;
    let path = dir.join(format!("report-{}.txt", logger::now_stamp()));

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

    std::fs::write(&path, body).map_err(|e| human::with_context(texts::REPORT_SAVE_FAILED, &e.to_string()))?;
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
        .map_err(|e| human::with_context(texts::RESULTS_DIR_FAILED, &e.to_string()))?;
    let path = dir.join(format!("tests-{}.txt", logger::now_stamp()));
    let body = texts::test_report_body(
        &logger::stamp(
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        ),
        cache.results.len(),
        &s.data.display().to_string(),
        &tester::results_text(&cache.results, cache.best_id.as_deref()),
    );
    std::fs::write(&path, body).map_err(|e| human::with_context(texts::RESULTS_SAVE_FAILED, &e.to_string()))?;
    logger::log("ok", "report", &format!("результаты теста сохранены: {}", path.display()));
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(async)]
fn dns_providers() -> Vec<dns::DnsProvider> {
    dns::providers().to_vec()
}

#[tauri::command]
async fn apply_dns(app: AppHandle, provider: String, adapter: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let g = app.state::<Global>();
        let g = g.inner();
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "dns");
        let data = st(g).data.clone();
        let r = dns::apply(&data, &provider, adapter.as_deref());
        match r {
            Ok(msg) => {
                logger::log("ok", "dns", &format!("применён DNS {provider}: {msg}"));
                Ok(msg)
            }
            Err(e) => {
                logger::log("err", "dns", &format!("не удалось применить DNS {provider}: {e}"));
                Err(human::with_context(texts::DNS_APPLY_FAILED, &e))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn reset_dns(app: AppHandle, adapter: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let g = app.state::<Global>();
        let g = g.inner();
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "dns");
        let data = st(g).data.clone();
        let r = dns::reset(&data, adapter.as_deref());
        match r {
            Ok(msg) => {
                logger::log("info", "dns", "DNS возвращён на автоматический");
                Ok(msg)
            }
            Err(e) => {
                logger::log("err", "dns", &format!("не удалось сбросить DNS: {e}"));
                Err(human::with_context(texts::DNS_RESET_FAILED, &e))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
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
        // Сканирование процессов (tasklist/WMI в current_owner) дорогое: раз в
        // 5 с достаточно — UI узнаёт «вне программы» максимум с этой задержкой.
        let mut next_owner_scan = Instant::now();
        // Был ли движок в прошлом скане: переход «есть → нет» = движок исчез
        // вне нашего управления, значит пора снять и загруженный драйвер.
        let mut had_engines = false;
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
                            let _ = app.emit("zgui:toast", serde_json::json!({"kind":"warn","text": texts::ENGINE_STOPPED}));
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
            if Instant::now() >= next_owner_scan {
                next_owner_scan = Instant::now() + Duration::from_secs(5);
                let owner = current_owner(&g);
                let external = matches!(owner, WinwsOwner::External);
                let has_engines = !matches!(owner, WinwsOwner::None);
                if had_engines && !has_engines {
                    // Движок закончился (его убили извне/он завершился сам), а
                    // GUI жив: снимаем загруженный драйвер WinDivert — пока
                    // служба RUNNING, её .sys нельзя ни удалить, ни очистить.
                    std::thread::spawn(cleanup_stray_drivers_after_stop);
                }
                had_engines = has_engines;
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
                    let app2 = app.clone();
                    // Гард берётся атомарно: если загрузка уже идёт (fetch_engine),
                    // тик пропускается, а не перебивает флаг чужой операции.
                    if let Ok(busy) = BusyGuard::try_new(&app2, texts::BUSY_DOWNLOAD) {
                        startup_check = false;
                        let mut s2 = st(&g);
                        let retry_secs = if entries_empty { 60 } else { 15 * 60 };
                        s2.updater.next_auto = Some(now + retry_secs);
                        s2.save();
                        drop(s2);
                        std::thread::spawn(move || {
                            let _busy = busy;
                            let r = up::check_all(&data, &roots, &settings);
                            let ga3 = app2.state::<Global>();
                            let mut s3 = ga3.state.lock().unwrap_or_else(|e| e.into_inner());
                            match r {
                                Ok(entries) => {
                                    s3.updater.entries = entries;
                                    s3.updater.last_check = Some(crate::profiles::now_str());
                                    s3.updater.last_auto = Some(crate::profiles::now_str());
                                    s3.save();
                                    let _ = app2.emit("zgui:updates", updater_view(&s3));
                                    let _ = app2.emit("zgui:toast", serde_json::json!({"kind":"info","text": texts::AUTOUPDATE_CHECKED}));
                                }
                                Err(e) => {
                                    logger::log("warn", "updates", &format!("автопроверка не удалась: {e}"));
                                }
                            }
                        });
                    }
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
            drop_custom_profiles(&mut state);
            state.save();
            provision_boot(&mut state);
            // Чиним «осиротевший» системный прокси (остался от выгруженного VPN).
            let healed = heal_orphan_proxy();
            // Убираем наши зависшие драйверы WinDivert (после сбоев/убитых
            // движков и копий программы), пока обход не запущен.
            std::thread::spawn(|| {
                svc::cleanup_own_stray_drivers();
                if rn::is_elevated() {
                    // TCP timestamps — как автор включает при каждом запуске .bat.
                    diag::ensure_tcp_timestamps();
                }
            });
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
                test_epoch: std::sync::atomic::AtomicU64::new(0),
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
                            "text": texts::proxy_healed(&proxy)
                        }),
                    );
                });
            }
            if uac_declined || second_instance {
                let h = handle.clone();
                let text = if second_instance {
                    texts::SECOND_COPY
                } else {
                    texts::NO_ADMIN_START
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
                // Не `unwrap()`: паника на отравленном мьютексе уронила бы запуск.
                app.state::<Global>()
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .boot_pending = true;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            log_entries,
            log_write,
            log_dir_open,
            report_save,
            test_report_save,
            set_root,
            fetch_engine,
            start_profile,
            stop_running,
            current_status,
            test_strategies,
            test_status,
            test_cache,
            tg_status,
            tg_start,
            tg_stop,
            tg_check_update,
            tg_offer,
            tg_vpn_guard,
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
            discord_cache_clear,
            fakes_view,
            replace_fake,
            hosts_update,
            reboot_now,
            check_updates,
            apply_updates,
            engine_check_update,
            set_settings,
            set_theme,
            set_admin_prefs,
            mark_admin_onboarded,
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
            // «тест запустился сам». Единая процедура: стоп-флаг (раннер завершит
            // цикл сам) + попытка kill без эскалации (UAC на выходе не поднимаем).
            if matches!(event, tauri::RunEvent::Exit) {
                let data = app.state::<Global>().state.lock().unwrap_or_else(|e| e.into_inner()).data.clone();
                stop_test_runner(&data, false);
                // Гасим winws, поднятый приложением: без этого он остаётся
                // сиротой и держит драйвер WinDivert («висящий драйвер»).
                // Без UAC — на выходе диалоги не показываем.
                let rt = {
                    let g = app.state::<Global>();
                    let s = st(&g);
                    s.runtime.clone()
                };
                if let Some(rt) = rt {
                    if rt.via != "service" && rn::pid_alive(rt.pid) {
                        if rn::kill_pid_direct(rt.pid) {
                            logger::log("info", "exit", &format!("движок остановлен при выходе (pid {})", rt.pid));
                        } else {
                            logger::log(
                                "warn",
                                "exit",
                                "движок не остановлен при выходе — запустите программу от администратора и нажмите «Остановить»",
                            );
                        }
                    }
                }
                // Убираем наши зависшие драйверы WinDivert (если движки не
                // запущены — иначе функция сама пропустит).
                svc::cleanup_own_stray_drivers();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{
        autostart_wants_task, drop_custom_profiles, engine_meta, ensure_presets, local_proxy_port, unzip, winws_owner_of,
        WinwsOwner,
    };
    use crate::config;

    #[test]
    fn engine_meta_registry() {
        for def in crate::config::engines() {
            let id = def.id;
            let m = engine_meta(id).unwrap_or_else(|e| panic!("нет meta для {id}: {e}"));
            let def = config::engine_def(id).unwrap();
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
        // Вырезанный вшитый пресет (наш старый «Flowseal · General» — дубликат
        // general.bat из движка): профиль и его результат теста должны исчезнуть.
        state.profiles.push(crate::presets::preset_profile(
            "flowseal-general",
            "flowseal",
            "Flowseal · General",
            vec!["--dpi-desync=fake".into()],
        ));
        // Кандидаты удалённого автоподбора (`auto:*`) чистятся при старте —
        // функционал убран, профили не должны висеть мусором.
        let mut stale_auto = crate::presets::preset_profile(
            "goodbyedpi-9",
            "goodbyedpi",
            "GoodbyeDPI · 9 (максимальный)",
            vec!["-9".into()],
        );
        stale_auto.id = "auto:goodbyedpi:goodbyedpi-9".into();
        stale_auto.builtin = false;
        stale_auto.source = Some("auto:goodbyedpi".into());
        state.profiles.push(stale_auto);
        let mut live_auto = crate::presets::preset_profile(
            "gd-ttl5",
            "goodbyedpi",
            "GoodbyeDPI · -5 + TTL 5",
            vec!["-5".into(), "--set-ttl".into(), "5".into()],
        );
        live_auto.id = "auto:goodbyedpi:gd-ttl5".into();
        live_auto.builtin = false;
        live_auto.source = Some("auto:goodbyedpi".into());
        state.profiles.push(live_auto);

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
            results: vec![
                mk("preset:goodbyedpi-9"),
                mk("auto:goodbyedpi:goodbyedpi-9"),
                mk("preset:flowseal-general"),
            ],
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
        assert!(
            !state.profiles.iter().any(|p| p.id == "auto:goodbyedpi:goodbyedpi-9"),
            "кандидат автоподбора удалён (функционал убран)"
        );
        assert!(
            !state.profiles.iter().any(|p| p.id == "auto:goodbyedpi:gd-ttl5"),
            "любой auto:-профиль удаляется, а не остаётся"
        );
        assert!(
            !state.profiles.iter().any(|p| p.id == "preset:flowseal-general"),
            "вырезанный пресет удалён из профилей"
        );
        assert_eq!(
            state.profiles.iter().filter(|p| p.builtin).count(),
            crate::presets::builtin_presets().len(),
            "все вшитые пресеты на месте"
        );
        let cache = crate::tester::TestCache::load(&data);
        assert!(cache.results.is_empty(), "результаты удалённых пресетов вычищены");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn drop_custom_profiles_keeps_author_bats_and_ota_presets() {
        // Регресс 25.09: удаляли всё `!builtin` — авторские .bat-профили
        // flowseal (builtin: false, source: "*.bat") пропадали из UI до
        // следующего «Проверить обновления → Применить».
        let data = std::env::temp_dir().join(format!("zgui-drop-prof-{}", std::process::id()));
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
        // Авторский .bat-профиль (так его создаёт reload_bats_from_disk).
        state.profiles.push(crate::config::Profile {
            id: "general (ALT)".into(),
            name: "general · ALT".into(),
            engine: crate::config::ENGINE_FLOWSEAL.into(),
            args: vec!["--wf-tcp-out=443".into()],
            builtin: false,
            source: Some("general (ALT).bat".into()),
            updated_at: None,
        });
        // OTA-пресет (builtin: true).
        state.profiles.push(crate::presets::preset_profile(
            "z2-x",
            "zapret2",
            "zapret2 · General",
            vec!["--wf-tcp-out=443".into()],
        ));
        // Старый кастомный профиль из прежних версий (source пустой) — удаляется.
        let mut custom = crate::presets::preset_profile("custom-old", "flowseal", "Мой", vec!["--x".into()]);
        custom.builtin = false;
        custom.source = None;
        state.profiles.push(custom);

        drop_custom_profiles(&mut state);

        assert!(
            state.profiles.iter().any(|p| p.id == "general (ALT)"),
            "авторский .bat-профиль должен остаться"
        );
        assert!(state.profiles.iter().any(|p| p.id == "preset:z2-x"), "OTA-пресет остаётся");
        assert!(
            !state.profiles.iter().any(|p| p.id == "preset:custom-old"),
            "кастом без source удаляется"
        );
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
