use crate::config::{
    AppState, Profile, Roots, Runtime, Settings, UpdaterCache, ENGINE_FLOWSEAL, SERVICE_NAME,
};
use crate::profiles as pf;
use crate::runner as rn;
use crate::service as svc;
use crate::updater as up;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Взаимная блокировка долгих операций: запуск/остановка стратегий, тест,
/// служба, DNS, обновления, сброс сети, восстановление интернета. Захват через
/// `lock()` — безвозвратно до отпускания; `try_lock()` — None, если уже занято.
/// Правило порядка: всегда берётся ДО `Global::state` (иначе дедлок).
static OPS: Mutex<()> = Mutex::new(());

/// Трей (экспериментальная фича): закрытие окна прячет его в трей, реальный
/// выход — только через пункт «Выход» в меню трея (флаг ниже).
static QUIT: AtomicBool = AtomicBool::new(false);

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

mod codes;
pub mod config;
mod diag;
mod dns;
mod human;
mod logger;
mod netreset;
mod presets;
mod profiles;
mod runner;
mod scanner;
mod service;
mod telegram;
mod tester;
mod texts;
mod tools;
mod updater;

pub struct Global {
    pub state: Mutex<AppState>,
    pub busy: AtomicBool,
    /// Идёт тест стратегий — взаимная блокировка с запуском/остановкой профилей.
    pub testing: Mutex<tester::TestProgress>,
    /// Идёт долгая операция (обновления, DNS, net_reset, service).
    pub op_running: AtomicBool,
    pub telegram: telegram::TgState,
    /// Предложение Telegram-моста уже сделано в этой сессии (спрашиваем один раз).
    pub tg_offer_shown: AtomicBool,
    /// Время последней проверки Telegram-предложения (throttle, epoch-сек).
    pub tg_offer_checked: std::sync::atomic::AtomicU64,
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
        emit(
            app,
            "zgui:op",
            serde_json::json!({"running": true, "kind": kind}),
        );
        Self {
            app: app.clone(),
            kind,
        }
    }
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        self.app.state::<Global>().set_op_running(false);
        logger::log(
            "info",
            "op",
            &format!("операция завершилась: {}", self.kind),
        );
        emit(
            &self.app,
            "zgui:op",
            serde_json::json!({"running": false, "kind": self.kind}),
        );
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
        if g.busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
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
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
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
        None => RootInfo {
            path: None,
            exe: None,
            ready: false,
        },
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
    /// Кто держит оптимизацию: факт «есть ли процесс движка» (без догадок).
    engine_active: bool,
    /// Идёт прогон теста стратегий.
    testing: bool,
    data_dir: String,
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

// ---------------------------------------------------------------- корневой

#[tauri::command]
async fn bootstrap(app: AppHandle) -> Result<Bootstrap, String> {
    // Блокирующие вызовы (tasklist/is_elevated) — в отдельном потоке: раньше
    // bootstrap занимал tokio-воркер и подвешивал параллельные команды.
    tauri::async_runtime::spawn_blocking(move || bootstrap_impl(app.state::<Global>().inner()))
        .await
        .map_err(|e| e.to_string())
}

fn bootstrap_impl(g: &Global) -> Bootstrap {
    // Факты о работающей оптимизации: только «есть ли процесс движка» и «идёт ли
    // тест» — никаких догадок о владельце.
    let engine_active = svc::any_winws_running();
    let testing = {
        let running = g.testing.lock().unwrap_or_else(|e| e.into_inner()).running;
        running || {
            let s = st(g);
            tester::runner_alive(&s.data)
        }
    };
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
            crate::config::engine_def(ENGINE_FLOWSEAL)
                .map(|d| d.exe)
                .unwrap_or("winws.exe"),
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
        engine_active,
        testing,
        data_dir: s.data.to_string_lossy().into_owned(),
        save_failed: crate::config::save_failed(),
    }
}

// ------------------------------------------------------------- orphan proxy

/// Проверяет системный прокси: если он включён, указывает на локальный адрес
/// (127.0.0.1/localhost), но на этом порту никто не слушает — значит, остался
/// «осиротевшим» от выгруженного VPN/движка, и браузер шлёт трафик в никуда
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
    let enabled = read("ProxyEnable")
        .map(|v| v.trim() == "0x1" || v.trim() == "1")
        .unwrap_or(false);
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

/// Копирует отсутствующие файлы дерева (существующие не трогает).
fn copy_tree_missing(from: &std::path::Path, to: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(from) else {
        return;
    };
    for e in entries.flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        if src.is_dir() {
            let _ = std::fs::create_dir_all(&dst);
            copy_tree_missing(&src, &dst);
        } else if !dst.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

/// Отключает авторскую авто-проверку обновлений Flowseal: `utils/check_updates.enabled`
/// заставляет каждый *.bat вызывать `service.bat check_updates`, который открывает
/// страницу релиза в браузере. Обновлениями занимается наш GUI — флаг гасим.
fn neutralize_author_autoupdate(root: &std::path::Path) {
    let flag = root.join("utils").join("check_updates.enabled");
    if flag.exists() {
        let _ = std::fs::rename(
            &flag,
            root.join("utils")
                .join("check_updates.enabled.zgui_disabled"),
        );
        if flag.exists() {
            let _ = std::fs::remove_file(&flag);
        }
    }
}

/// Реальный корень движка Flowseal внутри распакованной папки — каталог с `bin`.
fn engine_root_of(dir: &std::path::Path) -> Option<PathBuf> {
    let rel = crate::config::find_exe(dir, "winws.exe")?;
    let exe_path = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let mut cur = exe_path.parent().map(|p| p.to_path_buf())?;
    loop {
        if cur.join("bin").is_dir() {
            return Some(cur);
        }
        match cur.parent() {
            Some(p) if p.starts_with(dir) => cur = p.to_path_buf(),
            _ => break,
        }
    }
    Some(dir.to_path_buf())
}

fn seed_flowseal_configs(root: &std::path::Path, data: &std::path::Path, settings: &Settings) {
    let lists = root.join("lists");
    let _ = std::fs::create_dir_all(&lists);
    copy_tree_missing(&data.join("catalog/flowseal/lists"), &lists);
    let seeds: [(&str, &str); 4] = [
        ("ipset-exclude-user.txt", "203.0.113.113/32\n"),
        (
            "list-general-user.txt",
            "# Never leave this file empty\ndomain.example.abc\n",
        ),
        (
            "list-exclude-user.txt",
            "domain.example.abc\nsteamstatic.com\nsteamcontent.com\nsteamcdn-a.akamaihd.net\n",
        ),
        (
            "ipset-all-user.txt",
            "# Ваши подсети для оптимизации (по одной в строке)\n",
        ),
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
fn set_root(
    app: AppHandle,
    ga: State<'_, Global>,
    engine: String,
    path: String,
) -> Result<RootInfo, String> {
    let g = ga.inner();
    let def = crate::config::engine_def(&engine).ok_or_else(|| texts::unknown_engine(&engine))?;
    let selected = PathBuf::from(&path);
    if !selected.is_dir() {
        return Err(texts::FOLDER_MISSING.into());
    }
    let exe_name = def.exe;
    if crate::config::find_exe(&selected, exe_name).is_none() {
        return Err(texts::exe_missing_in_folder(exe_name));
    }
    let root = engine_root_of(&selected).unwrap_or(selected);
    let (data, settings) = {
        let s = st(g);
        (s.data.clone(), s.settings.clone())
    };
    st(g)
        .roots
        .set(&engine, Some(root.to_string_lossy().into_owned()));
    if engine == ENGINE_FLOWSEAL {
        neutralize_author_autoupdate(&root);
        seed_flowseal_configs(&root, &data, &settings);
    }
    let mut s = st(g);
    reload_bats_from_disk(&mut s);
    s.save();
    let info = root_info(&s.roots, &engine, exe_name);
    logger::log(
        "ok",
        "engine",
        &format!("корень {engine} задан: {}", root.display()),
    );
    emit(
        &app,
        "zgui:toast",
        serde_json::json!({"kind":"ok","text": texts::engine_root_set(&engine, &root.display().to_string())}),
    );
    Ok(info)
}

// ---------------------------------------------------------------- профили

/// Подхватывает движки, уже разложенные в `data/engines/<id>`: всё лежит
/// рядом в комплекте (zip), из exe ничего не распаковывается и не качается.
fn provision_engines(s: &mut AppState) {
    let data = s.data.clone();
    for id in config::engine_ids() {
        let Some(def) = crate::config::engine_def(id) else {
            continue;
        };
        // Уже заданный корень: для flowseal нормализуем (мог быть записан как
        // каталог-обёртка) и обновляем списки/профили.
        if let Some(p) = s.roots.path(id) {
            let cur = PathBuf::from(&p);
            let norm = if id == ENGINE_FLOWSEAL {
                engine_root_of(&cur).unwrap_or_else(|| cur.clone())
            } else {
                cur.clone()
            };
            if norm != cur {
                s.roots.set(id, Some(norm.to_string_lossy().into_owned()));
            }
            if id == ENGINE_FLOWSEAL {
                neutralize_author_autoupdate(&norm);
                seed_flowseal_configs(&norm, &data, &s.settings);
                reload_bats_from_disk(s);
            }
            continue;
        }
        // Корня нет — ищем готовый движок в data/engines/<id>.
        let base = data.join("engines").join(id);
        let root = if id == ENGINE_FLOWSEAL {
            engine_root_of(&base)
        } else {
            crate::config::find_exe(&base, def.exe).map(|rel| {
                let p = base.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                p.parent()
                    .map(|d| d.to_path_buf())
                    .unwrap_or_else(|| base.clone())
            })
        };
        let Some(root) = root else { continue };
        s.roots.set(id, Some(root.to_string_lossy().into_owned()));
        if id == ENGINE_FLOWSEAL {
            neutralize_author_autoupdate(&root);
            seed_flowseal_configs(&root, &data, &s.settings);
            reload_bats_from_disk(s);
        }
        logger::log(
            "ok",
            "engine",
            &format!("движок {id} подхвачен: {}", root.display()),
        );
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
        !p.source
            .as_deref()
            .is_some_and(|src| src.starts_with("auto:"))
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
    let ota_version = up::preset_stamp(&s.data);
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
                if existing.args != args
                    || existing.name != def.name
                    || existing.engine != def.engine
                {
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
    cache
        .results
        .retain(|r| s.profiles.iter().any(|p| p.id == r.id));
    if cache.results.len() != before {
        logger::log(
            "info",
            "test",
            &format!(
                "удалены устаревшие результаты тестов: {}",
                before - cache.results.len()
            ),
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
        logger::log(
            "info",
            "profiles",
            &format!("удалено пользовательских профилей: {removed}"),
        );
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
        data.join("logs")
            .join(format!("svc_stop_{}.ps1", std::process::id())),
        &format!(
            "{}\nnet stop {} 2>$null | Out-Null\n\
             if ((& sc.exe query {} | Out-String) -match 'RUNNING') {{ exit 1 }} else {{ exit 0 }}",
            rn::PS_HEADER,
            SERVICE_NAME,
            SERVICE_NAME
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
                logger::log_code(
                    "warn",
                    "stop",
                    "W-STOP-010",
                    &format!("остановка «{}»: процесс уже завершён ({e})", rt.profile_id),
                );
                Ok(())
            } else {
                Err(e)
            }
        });
        if let Err(e) = stop_res {
            logger::log_code(
                "err",
                "stop",
                "E-STOP-001",
                &format!("остановка «{}» не удалась: {e}", rt.profile_id),
            );
            return Err(texts::STOP_FAILED.into());
        }
        st(g).runtime = None;
        st(g).save();
        // Секционный лог: вывод движка + итог по времени работы.
        if rt.via != "service" {
            let logs = data.join("logs");
            let log_id = pf::log_file_component(&rt.profile_id);
            let out = rn::tail(&logs.join(format!("stdout-{log_id}.txt")), 800);
            let err = rn::tail(&logs.join(format!("stderr-{log_id}.txt")), 800);
            let uptime = now_ts().saturating_sub(rt.started_at);
            let started = out.contains("capture is started") || err.contains("capture is started");
            logger::log("info", "launch", "=== ВЫВОД ДВИЖКА ===");
            let cap = if started {
                "перехват: запущен (capture is started)".to_string()
            } else if uptime < 3 && out.trim().is_empty() && err.trim().is_empty() {
                "перехват: движок остановлен слишком быстро — вывод не успел появиться".to_string()
            } else {
                "перехват: строка «capture is started» не найдена".to_string()
            };
            logger::log("info", "launch", &cap);
            for (tag, txt) in [("stdout", out), ("stderr", err)] {
                let t = one_line(&txt, 400);
                if !t.is_empty() {
                    logger::log("info", "launch", &format!("{tag}: {t}"));
                }
            }
            logger::log("info", "launch", "=== СТОП ===");
        }
        logger::log(
            "info",
            "launch",
            &format!(
                "стоп: профиль=«{}»; работал ~{} с; способ={}",
                rt.profile_id,
                now_ts().saturating_sub(rt.started_at),
                rt.via
            ),
        );
        logger::log(
            "info",
            "stop",
            &format!(
                "остановлено: {} (pid {}, через {})",
                rt.profile_id, rt.pid, rt.via
            ),
        );
        emit(
            app,
            "zgui:status",
            serde_json::json!({"running": false, "pid": null, "profileId": null}),
        );
        if !silent {
            emit(
                app,
                "zgui:toast",
                serde_json::json!({"kind":"ok","text": texts::STOPPED_ONE}),
            );
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
                logger::log_code(
                    "err",
                    "stop",
                    "E-STOP-002",
                    &format!("остановка службы не удалась: {e}"),
                );
                return Err(texts::STOP_FAILED.into());
            }
            // Служба осталась установленной, но остановлена — фиксируем сразу,
            // иначе UI ~10 с показывает «служба запущена» после «Остановить».
            let mut s = st(g);
            s.service_running = Some(false);
            s.save();
            emit(
                app,
                "zgui:status",
                serde_json::json!({"running": false, "pid": null, "profileId": null}),
            );
        }
        if !silent {
            emit(
                app,
                "zgui:toast",
                serde_json::json!({"kind":"ok","text": texts::ALL_STOPPED}),
            );
        }
        Ok(())
    }
}

/// Останавливает ВСЁ, что относится к нашему движку: профиль приложения, нашу
/// службу и winws, поднятые вне программы (ручной .bat/старая служба).
/// Без этого второй winws не виден GUI и конфликтует с первым — оптимизация «не работает»,
/// пока процесс не убьют вручную (жалоба владельца).
fn stop_all_own(app: &AppHandle, g: &Global) -> Result<(), String> {
    let r = do_stop(app, g, true);
    let data = st(g).data.clone();
    // Кэш состояния мог устареть (GUI перезапускали) — проверяем службу фактом.
    let (installed, running) = svc::service_state();
    if installed && running {
        if let Err(e) = stop_service(&data) {
            logger::log_code(
                "err",
                "stop",
                "E-STOP-003",
                &format!("служба не остановилась: {e}"),
            );
        }
    }
    let any_left = svc::any_winws_running();
    if any_left {
        // Гасим ЛЮБОЙ процесс движка (winws/winws2/goodbyedpi/dpibreak):
        // два фильтра WinDivert одновременно не работают, поэтому это всегда
        // конфликт — неважно, наша копия, старая версия или чужой запуск.
        let pids = svc::all_engine_pids(None);
        logger::log_code(
            "warn",
            "stop",
            "W-STOP-004",
            &format!("останавливаю движки: {:?}", pids),
        );
        if let Err(e) = rn::stop_pids(&pids, &data) {
            logger::log_code(
                "err",
                "stop",
                "E-STOP-005",
                &format!("движки не остановились: {e}"),
            );
        }
    }
    // Под админом — тотальное добивание скриптом (имена + службы из реестра):
    // ловит процессы, чьи пути/права не видны нашему уровню, и службы с любым
    // именем, чей ImagePath ссылается на exe движка.
    if rn::is_elevated() && (any_left || installed) {
        match svc::sweep_our_engines(&data) {
            Ok(services) if !services.is_empty() => {
                logger::log_code(
                    "warn",
                    "stop",
                    "W-STOP-006",
                    &format!("остановлены службы-движки: {:?}", services),
                );
            }
            Ok(_) => {}
            Err(e) => {
                // taskkill шумит на «процесс не найден» (PS 5.1 EAP=Stop даже при
                // 2>$null); если движков в итоге нет — это не ошибка остановки.
                if svc::any_winws_running() {
                    logger::log_code(
                        "err",
                        "stop",
                        "E-STOP-005",
                        &format!("тотальное добивание движков не удалось: {e}"),
                    );
                } else {
                    logger::log(
                        "info",
                        "stop",
                        &format!("sweep завершился с шумом, движков нет: {e}"),
                    );
                }
            }
        }
        // Даём процессам умереть: следующий шаг (старт/тест) не должен
        // увидеть их живыми в tasklist.
        for _ in 0..8 {
            if !svc::any_winws_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    } else if svc::any_winws_running() {
        logger::log_code(
            "warn",
            "stop",
            "W-STOP-009",
            "движки не удалось остановить полностью — нужны права администратора",
        );
    }
    r
}

/// Единый maintenance-sweep (старт, выход, стоп): добить любые чужие/зависшие
/// движки (служба — исключение: её движок живёт без GUI) и снять зависшие
/// драйверные службы WinDivert. UAC здесь не поднимаем: в этих точках диалог
/// был бы неожиданным, эскалация — в stop_all_own по явному действию.
fn maintenance_sweep() {
    // Служба владеет движком (RUNNING или START_PENDING): не трогаем ни
    // процессы, ни драйвер. Раньше состояние START_PENDING считалось
    // «остановлена», и sweep убивал только что поднятый winws службы.
    if svc::service_active() {
        logger::log(
            "info",
            "sweep",
            "служба владеет движком (запущена/запускается) — не трогаем",
        );
        return;
    }
    let pids = svc::all_engine_pids(None);
    for pid in &pids {
        if rn::kill_pid_direct(*pid) {
            logger::log("info", "sweep", &format!("движок остановлен (pid {pid})"));
        } else {
            logger::log_code(
                "warn",
                "sweep",
                "W-STOP-009",
                &format!("движок (pid {pid}) не остановлен — нужны права администратора"),
            );
        }
    }
    let cleaned = svc::cleanup_own_stray_drivers();
    if !cleaned.is_empty() {
        logger::log(
            "info",
            "windivert",
            &format!("сняты зависшие драйверы: {}", cleaned.join(", ")),
        );
    }
}

#[tauri::command(async)]
fn stop_running(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    // Во время теста движок принадлежит тесту: тулбарная «Остановить» не должна
    // убивать winws теста (иначе тест продолжается «без движка» и врёт цифрами).
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
    // Движок остановлен — единый sweep (добить остатки, снять драйверы).
    // При старте стратегии sweep не делаем: драйвер понадобится через секунду.
    maintenance_sweep();
    emit(
        &app,
        "zgui:toast",
        serde_json::json!({"kind":"ok","text": texts::ALL_STOPPED}),
    );
    Ok(())
}

/// Диапазоны Game Filter из настроек — единая точка вызова. Нельзя собирать
/// поля через несколько `st(g)` в одном выражении: мьютекс state не
/// реентерабельный, это дедлок (регресс 25.09: тест зависал на «идёт операция…»).
fn game_filter_ports_from(s: &Settings) -> (String, String) {
    pf::game_filter_ports(&s.game_filter, &s.game_filter_tcp, &s.game_filter_udp)
}

/// Сжимает многострочный вывод движка в одну строку (для журнала).
fn one_line(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    let mut n = 0usize;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
            continue;
        }
        if n >= max {
            out.push('…');
            break;
        }
        out.push(ch);
        n += 1;
        prev_space = false;
    }
    out.trim_end().to_string()
}

fn do_start(app: &AppHandle, g: &Global, id: &str) -> Result<Runtime, String> {
    let (profile, root_path, settings, data) = {
        let s = st(g);
        let p = s.profile(id).cloned().ok_or_else(|| {
            logger::log_code(
                "err",
                "start",
                "E-START-001",
                &format!("профиль {id} не найден"),
            );
            texts::PROFILE_NOT_FOUND.to_string()
        })?;
        let root = s.roots.path(&p.engine).ok_or_else(|| {
            logger::log_code(
                "warn",
                "start",
                "W-START-002",
                &format!("корень движка «{}» не задан", p.engine),
            );
            texts::engine_root_missing(&p.engine)
        })?;
        (p, root, s.settings.clone(), s.data.clone())
    };

    stop_all_own(app, g)?;

    let exe = locate_exe(&root_path, profile.exe_name()).map_err(|e| {
        logger::log_code(
            "err",
            "start",
            "E-START-003",
            &format!("{}: {e}", profile.name),
        );
        human::humanize(&e)
    })?;
    let (tcp, udp) = game_filter_ports_from(&settings);
    let args = presets::prepare_args(&profile.args, &root_path, &tcp, &udp, &settings.filter_mode);

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
        if bin.is_dir() {
            bin
        } else {
            exe.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(root_path.clone())
        }
    };

    // Чистим старые логи: иначе при мгновенном выходе процесса в ошибку попадёт
    // содержимое прошлого запуска (в т.ч. в другой кодировке).
    let _ = std::fs::remove_file(&out_log);
    let _ = std::fs::remove_file(&err_log);

    // Секционный лог запуска: понятный разбор «почему стартовало / не стартовало».
    logger::log("info", "launch", "=== ЗАПУСК СТРАТЕГИИ ===");
    logger::log(
        "info",
        "launch",
        &format!(
            "система: права администратора={}; движок={}; папка движка={}",
            if rn::is_elevated() { "да" } else { "нет" },
            profile.engine,
            root_path.display()
        ),
    );
    logger::log(
        "info",
        "launch",
        &format!("конфиг: профиль=«{}» ({})", profile.name, profile.id),
    );
    logger::log("info", "launch", &format!("аргументы: {}", args.join(" ")));

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
        &format!(
            "запущена стратегия «{}» ({}, pid {})",
            profile.name, profile.engine, pid
        ),
    );
    emit(
        app,
        "zgui:status",
        serde_json::json!({"running": true, "pid": pid, "profileId": profile.id}),
    );
    emit(
        app,
        "zgui:toast",
        serde_json::json!({"kind":"ok","text": texts::strategy_started(&profile.name)}),
    );
    Ok(runtime)
}

/// Запускает профиль без тупиков на настройках: если установлена служба zapret —
/// переводит её на нужную стратегию (служба остаётся единственным механизмом
/// оптимизации), иначе поднимает winws как процесс программы.
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
            let p = s
                .profile(id)
                .cloned()
                .ok_or_else(|| texts::PROFILE_NOT_FOUND.to_string())?;
            let root = s
                .roots
                .path(&p.engine)
                .ok_or_else(|| texts::engine_root_missing(&p.engine))?;
            let (tcp, udp) = game_filter_ports_from(&s.settings);
            (
                p.clone(),
                root.clone(),
                presets::prepare_args(&p.args, &root, &tcp, &udp, &s.settings.filter_mode),
                s.data.clone(),
            )
        };
        // Один живой winws: снимаем процесс программы и старую службу перед пересозданием.
        stop_all_own(app, g)?;
        svc::install_service(&root, &profile, &args, &data).map_err(|e| {
            logger::log_code(
                "err",
                "service",
                "E-SVC-001",
                &format!("переключение службы не удалось: {e}"),
            );
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
        emit(
            app,
            "zgui:status",
            serde_json::json!({"running": true, "pid": null, "profileId": profile.id}),
        );
        emit(
            app,
            "zgui:toast",
            serde_json::json!({"kind":"ok","text": texts::service_switched(&profile.name)}),
        );
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
    // Консервативно: нет pid-файла — считаем, что раннер ещё стартует (флаг нельзя
    // стирать: он его прочитает первым делом). Есть pid и он умер — точно мёртв.
    let mut alive = pid.is_none();
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
                    Err(e) => logger::log_code(
                        "warn",
                        "test",
                        "W-TEST-001",
                        &format!("раннер теста {pid}: {e}"),
                    ),
                }
            }
            // Даём процессу исчезнуть, чтобы маркеры не «ожили» следом.
            for _ in 0..30 {
                if !rn::pid_alive(pid) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            alive = rn::pid_alive(pid);
            if alive {
                logger::log(
                    "warn",
                    "test",
                    &format!("раннер теста {pid} ещё жив — остановится по стоп-флагу"),
                );
            } else {
                logger::log(
                    "info",
                    "test",
                    &format!("раннер теста остановлен (pid {pid})"),
                );
            }
        }
    }
    let _ = std::fs::remove_file(&win_pid);
    if alive {
        // ВАЖНО: флаг оставляем. Он единственный сигнал живому раннеру (проверки
        // в шагах и пробах); ранняя чистка флага — причина «остановка не работает».
        return;
    }
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
            all.iter()
                .filter(|p| ids.contains(&p.id))
                .cloned()
                .collect()
        };
        // Явный выбор с недоступным движком отсеивается ниже ЧЕСТНОЙ ошибкой
        // (engine_root_missing): молчаливая фильтрация прятала причину.
        (selected, s.data.clone(), roots)
    };
    if profiles.is_empty() {
        return Err(texts::TEST_NO_STRATEGIES.into());
    }
    // Без curl.exe проба не работает: честная ошибка вместо «все домены fail».
    if !tester::curl_available() {
        return Err(texts::CURL_MISSING.into());
    }
    // Чистота перед стартом: остатки прошлого прогона (осиротевший elevated-
    // раннер, его движок) гасим ДО начала — иначе новый тест упрётся в защиту
    // «winws всё ещё запущен», а два раннера подерутся за движок. Здесь UAC
    // допустим: пользователь сам только что инициировал тест.
    if test_markers_present(&data) {
        stop_test_runner(&data, true);
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
    let (tcp, udp, filter_mode) = {
        let s = st(g);
        let (t, u) = game_filter_ports_from(&s.settings);
        (t, u, s.settings.filter_mode.clone())
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
                    &format!(
                        "переиспользую прежние результаты: {} стратегий",
                        reused.len()
                    ),
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
        let root = st(g)
            .roots
            .path(&p.engine)
            .ok_or_else(|| texts::engine_root_missing(&p.name))?;
        let exe = locate_exe(&root, p.exe_name())?;
        let args = presets::prepare_args(&p.args, &root, &tcp, &udp, &filter_mode);
        let wd = {
            let bin = root.join("bin");
            if bin.is_dir() {
                bin
            } else {
                exe.parent()
                    .map(|x| x.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from(root.to_string_lossy().into_owned()))
            }
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
        logger::log(
            "info",
            "test",
            "повторный прогон не нужен — используем прежние результаты",
        );
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
                stopped: false,
            },
        );
        return Ok(true);
    }

    let (plan_path, out_path) = tester::write_plan(&data, &steps, &custom)?;
    let plan_arg = plan_path.to_string_lossy().into_owned();
    logger::log(
        "info",
        "test",
        &format!(
            "тест: план готов ({total_hint} шагов)",
            total_hint = steps.len()
        ),
    );
    // Паритет с автором: на время прогона `ipset-all.txt` переводится в «any»
    // (пустой), после — возврат; Drop гарантирует восстановление при отмене.
    let ipset_paths: Vec<std::path::PathBuf> = {
        let mut seen = std::collections::HashSet::new();
        let mut v = Vec::new();
        for p in &profiles {
            let Some(root) = roots_ok.path(&p.engine) else {
                continue;
            };
            let f = root.join("lists").join("ipset-all.txt");
            if seen.insert(f.clone()) {
                v.push(f);
            }
        }
        v
    };
    let ipset_guard = tester::activate_ipset_any(&ipset_paths);
    let total = steps.len();
    // Тест поднимает свои winws: текущий движок (профиль, служба, ручной .bat)
    // обязан быть остановлен. Иначе winws выходит сразу с «A copy of winws is
    // already running with the same filter», а проба проходит «чужим» движком.
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
        logger::log_code(
            "err",
            "test",
            "E-TEST-002",
            "winws всё ещё запущен — тест отменён до остановки",
        );
        return Err(texts::WINWS_RUNNING.into());
    }
    logger::log(
        "info",
        "test",
        &format!("старт теста: {} стратегий, {} доменов", total, custom.len()),
    );

    // С какой-то версии GUI всегда работает от администратора (иначе не
    // стартует) — UAC-обёртка для дочернего раннера больше не нужна.
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
        msg: texts::TEST_STARTING.into(),
        results: Vec::new(),
        best_id: None,
        best_name: None,
        done: false,
        stopped: false,
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
        logger::log("info", "test", "тест: поток раннера стартовал");
        let ga2 = app2.state::<Global>();
        let g2 = ga2.inner();

        // Раннер теста — дочерний режим `zgui.exe --test-runner`: запускаем
        // напрямую. GUI всегда работает от администратора (иначе не стартует),
        // поэтому UAC-обёртки здесь больше нет.
        let launch = match std::env::current_exe() {
            Ok(exe) => {
                let child_args = ["--test-runner".to_string(), plan_arg.clone()];
                rn::hidden_command(&exe.to_string_lossy())
                    .args(&child_args)
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        };
        if let Err(e) = launch {
            logger::log_code(
                "err",
                "test",
                "E-TEST-003",
                &format!("раннер теста не стартовал: {e}"),
            );
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
                logger::log_code(
                    "warn",
                    "test",
                    "W-TEST-004",
                    "поллер отменённого прогона завершился — уборку пропускаю",
                );
                return;
            }
            if test_marker(&data).0.exists() {
                break;
            }
            if let Some(st) = tester::read_progress(&out_path) {
                last_progress = std::time::Instant::now();
                if pid_seen_at.is_none() {
                    pid_seen_at = Some(last_progress);
                }
                let idx = st.index;
                let cur_id = st.current_id.clone();
                let cur_name = st.current_name.clone();
                parsed_count = st.results.len();
                let mut merged = reused.clone();
                merged.extend(st.results.iter().cloned());
                results = merged;
                let done = st.done || (idx >= total && parsed_count >= total);
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
                        .map(|_| texts::TESTING.to_string())
                        .unwrap_or_else(|| texts::TEST_DONE.into()),
                    results: results.clone(),
                    best_id: None,
                    best_name: None,
                    done,
                    stopped: false,
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
            logger::log_code(
                "warn",
                "test",
                "W-TEST-004",
                "поллер отменённого прогона завершился — уборку пропускаю",
            );
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
                    error: Some(if timed_out {
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
            let err: String = r
                .error
                .clone()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();
            logger::log_code(
                "err",
                "test",
                "E-TEST-005",
                &format!("«{}» не запустилась: {}", r.name, err),
            );
        }

        // Возвращаем движок, который остановили перед тестом: иначе пользователь
        // остаётся без защиты, а служба — в остановленном состоянии.
        if let Some(rt) = had_runtime {
            logger::log(
                "info",
                "test",
                &format!("возвращаю прежнюю стратегию «{}»", rt.profile_id),
            );
            if let Err(e) = do_start(&app2, g2, &rt.profile_id) {
                logger::log_code(
                    "err",
                    "test",
                    "E-TEST-006",
                    &format!("не удалось вернуть прежнюю стратегию: {e}"),
                );
                emit(
                    &app2,
                    "zgui:toast",
                    serde_json::json!({"kind":"warn","text": texts::prev_strategy_failed(&e)}),
                );
            }
        } else if had_service {
            if let Err(e) = svc::start_service(&data) {
                logger::log_code(
                    "err",
                    "test",
                    "E-TEST-007",
                    &format!("не удалось вернуть службу zapret: {e}"),
                );
            }
        }

        // Смена режима ipsets во время теста откладывалась (файл был «any») —
        // приводим list/ipset-all.txt к текущему режиму пользователя.
        {
            let (root, settings_now) = {
                let s = st(g2);
                (s.roots.path("flowseal"), s.settings.clone())
            };
            if let Some(root) = root {
                up::sync_ipset(&root, &data, &settings_now);
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
            logger::log_code(
                "warn",
                "test",
                "W-TEST-008",
                "тест остановлен пользователем",
            );
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
                    stopped: true,
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
                stopped: false,
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
    // Криптостойкий источник ОС (CSPRNG); фоллбэк — только при отказе системы.
    let mut bytes = [0u8; 16];
    if getrandom::getrandom(&mut bytes).is_err() {
        let t = now_ts();
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = ((t >> ((i % 8) * 8)) as u8) ^ (i as u8).wrapping_mul(31);
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tauri::command]
async fn tg_start(
    app: AppHandle,
    ga: State<'_, Global>,
    port: Option<u16>,
) -> Result<telegram::TgStatus, String> {
    let st = ga.inner().telegram.clone();
    let port = port.unwrap_or(1443);
    let secret = tg_secret_of(&ga);
    let result = st.start(port, None, Some(secret)).await;
    match &result {
        Ok(_) => {
            logger::log("ok", "telegram", &format!("прокси запущен на порту {port}"));
            emit(&app, "zgui:tg", serde_json::json!({"running": true}));
        }
        Err(e) => logger::log_code(
            "err",
            "telegram",
            "E-TG-002",
            &format!("не удалось запустить прокси на порту {port}: {e}"),
        ),
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
        emit(
            &app,
            "zgui:toast",
            serde_json::json!({"kind":"info","text": texts::TG_PROXY_OFF_HINT}),
        );
    }
    ga.inner().telegram.status()
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
fn cancel_test(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    let data = st(g).data.clone();
    // Новая эпоха: поллер отменённого прогона больше не владеет состоянием и
    // движком — без этого он на выходе убивал winws уже нового теста/профиля.
    g.test_epoch.fetch_add(1, Ordering::SeqCst);
    // Спасаем уже измеренное: раннер может остановиться не мгновенно, но всё,
    // что успело записаться в прогресс, обязано остаться в кэше (раньше отмена
    // стирала результаты — «остановил, а они исчезли»).
    let out_path = data.join("logs/test-out.json");
    let salvage = tester::salvage_progress(&out_path);
    let salvage_count = salvage.iter().filter(|r| r.started).count();
    if salvage_count > 0 {
        let mut cache = tester::TestCache::load(&data);
        for r in salvage.into_iter().filter(|r| r.started) {
            match cache.results.iter_mut().find(|x| x.id == r.id) {
                Some(old) => *old = r,
                None => cache.results.push(r),
            }
        }
        cache.tested_at = Some(now_ts().to_string());
        cache.save(&data);
        logger::log(
            "info",
            "test",
            &format!("отмена: сохранено частичных результатов: {salvage_count}"),
        );
    }
    // Единая процедура: стоп-флаг + гашение раннера/движка + чистка маркеров.
    // UAC не поднимаем (стоп-флаг останавливает раннер за секунды: проверки в
    // шагах и внутри проб), но если GUI уже админ — kill сработает сразу.
    stop_test_runner(&data, false);
    // Финальное событие: поллер отменённого прогона выходит молча, поэтому
    // состояние «тест остановлен» обязан объявить сам cancel. Без этого фронт
    // оставался с last running:true — кнопка «Запустить тест» залипала в busy.
    set_test(
        &app,
        g,
        tester::TestProgress {
            running: false,
            phase: "done".into(),
            pct: 100,
            msg: texts::TEST_STOPPED.into(),
            done: true,
            stopped: true,
            ..Default::default()
        },
    );
    Ok(())
}

/// Одно действие «применить лучшую стратегию»: запомнить её как выбранную и
/// сразу запустить. Автозапуск — отдельная галочка службы на карточке.
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

// -------------------------------------------------------- автозапуск (служба)

/// Синхронизирует поля службы в state с фактом системы (и сбрасывает автозапуск,
/// если службы нет). Применяется при старте и после операций, которые могли
/// удалить службу («Восстановление интернета»), чтобы карточка не врала.
fn sync_service_state(state: &mut AppState) {
    let (installed, running) = svc::service_state();
    let live = installed.then_some(running);
    let strat = if installed {
        svc::service_strategy(&state.data)
    } else {
        None
    };
    if state.service_running != live || state.service_strategy != strat {
        state.service_running = live;
        state.service_strategy = strat;
        if live.is_none() {
            // Службы нет — автозапуск (единственный механизм) снимаем.
            state.settings.autostart_mode = "none".into();
            state.settings.autostart_profile = None;
        }
        state.save();
    }
}

/// Автозапуск GUI в трей: держит задачу планировщика в согласии с настройкой
/// `boot_app` (включена — пересоздаём на текущий exe; выключена — снимаем).
/// Заодно разово убирает запись HKCU\Run от прежних версий.
fn sync_boot_task(data: &std::path::Path, boot_app: bool) {
    rn::remove_legacy_boot();
    if boot_app {
        if let Err(e) = rn::apply_boot_task(data) {
            logger::log_code(
                "warn",
                "boot",
                "W-BOOT-001",
                &format!("автозапуск в трей не подтвердился: {e}"),
            );
        }
    } else if rn::boot_task_exists() {
        match rn::remove_boot_task(data) {
            Ok(_) => logger::log("info", "boot", "старая задача автозапуска снята"),
            Err(e) => logger::log_code(
                "warn",
                "boot",
                "W-BOOT-001",
                &format!("не удалось снять старую задачу автозапуска: {e}"),
            ),
        }
    }
}

// ---------------------------------------------------------------- VPN

#[tauri::command(async)]
fn vpn_check() -> Vec<svc::ConflictProcess> {
    svc::detect_vpn()
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
                // Сброс сети удаляет службу zapret — мгновенно приводим state
                // в соответствие (иначе карточка врёт до следующего скана).
                {
                    let mut s = st(g);
                    s.runtime = None;
                    sync_service_state(&mut s);
                }
                Ok(r)
            }
            Err(e) => {
                logger::log_code(
                    "err",
                    "netreset",
                    "E-NET-002",
                    &format!("восстановление сети не удалось: {e}"),
                );
                Err(human::with_context(texts::NET_RESET_FAILED, &e))
            }
        }
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
async fn discord_cache_clear(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Инструменты под общим замком: не работают во время теста/службы/старта
        // (закрытие Discord и чистка кэша во время прогона сбивают картину).
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "tools");
        let r = tools::clear_discord_cache();
        match &r {
            Ok(m) => logger::log("ok", "tools", m),
            Err(e) => logger::log_code("err", "tools", "E-TOOL-001", &format!("кэш Discord: {e}")),
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
        // Подмена .bin при работающем движке/тесте недопустима: следующая
        // запущенная стратегия получит другие фейки — картина прогона поедет.
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "tools");
        let root = flowseal_root(app.state::<Global>().inner())?;
        let r = tools::replace_active_fake(&root, &kind, &name);
        match &r {
            Ok(m) => logger::log("ok", "tools", m),
            Err(e) => logger::log_code("err", "tools", "E-TOOL-002", &format!("замена фейка: {e}")),
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
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "tools");
        let data = st(app.state::<Global>().inner()).data.clone();
        let r = tools::hosts_update(&data);
        match &r {
            Ok(m) => logger::log("ok", "tools", m),
            Err(e) => logger::log_code(
                "err",
                "tools",
                "E-TOOL-003",
                &format!("обновление hosts: {e}"),
            ),
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

// ---------------------------------------------------------------- служба

#[tauri::command(async)]
fn install_service(app: AppHandle, ga: State<'_, Global>, id: String) -> Result<(), String> {
    let g = ga.inner();
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let _guard = OpGuard::new(&app, "service");
    let (profile, root, args, data) = {
        let s = st(g);
        let p = s.profile(&id).cloned().ok_or(texts::PROFILE_NOT_FOUND)?;
        let root = s
            .roots
            .path(&p.engine)
            .ok_or_else(|| texts::engine_root_missing(&p.engine))?;
        let (tcp, udp) = game_filter_ports_from(&s.settings);
        (
            p.clone(),
            root.clone(),
            presets::prepare_args(&p.args, &root, &tcp, &udp, &s.settings.filter_mode),
            s.data.clone(),
        )
    };
    // Служба должна остаться единственным движком: гасим текущий движок (профиль
    // или внешний winws) ДО установки — иначе winws службы выходит сразу с
    // «duplicate filter», SCM пишет 7023, и юзер видит «служба падает сама».
    // Тест сюда не попадёт — его держит op-лок. Ошибку уборки не считаем
    // фатальной: пояс по четырём именам в install_service добьёт остатки.
    if let Err(e) = stop_all_own(&app, g) {
        logger::log_code(
            "warn",
            "service",
            "W-SVC-002",
            &format!("перед установкой службы уборка движков не удалась: {e}"),
        );
    }
    let r = svc::install_service(&root, &profile, &args, &data).map_err(|e| {
        logger::log_code(
            "err",
            "service",
            "E-SVC-003",
            &format!("установка службы не удалась: {e}"),
        );
        human::with_context(texts::SERVICE_INSTALL_CONTEXT, &e)
    });
    r?;
    logger::log(
        "ok",
        "service",
        &format!("служба zapret установлена со стратегией «{}»", profile.name),
    );
    // Запись стратегии в реестр могла не пройти (см. service.rs): предупреждаем
    // явно, иначе GUI покажет «служба без стратегии» без объяснения.
    if svc::service_strategy(&data).is_none() {
        emit(
            &app,
            "zgui:toast",
            serde_json::json!({"kind":"warn","text": texts::SERVICE_STRATEGY_MISSING}),
        );
    }
    {
        let mut s = st(g);
        s.service_running = Some(true);
        s.service_strategy = Some(profile.id.clone());
        s.settings.autostart_mode = "profile".into();
        s.settings.autostart_profile = Some(profile.id.clone());
        // Одна галочка = служба + программа в трее при входе (решение юзера).
        s.settings.boot_app = true;
        s.save();
    }
    if let Err(e) = rn::apply_boot_task(&data) {
        logger::log_code(
            "warn",
            "boot",
            "W-BOOT-001",
            &format!("автозапуск в трей не включился: {e}"),
        );
    }
    emit(
        &app,
        "zgui:toast",
        serde_json::json!({"kind":"ok","text": texts::SERVICE_INSTALLED}),
    );
    Ok(())
}

#[tauri::command(async)]
fn remove_service(app: AppHandle, ga: State<'_, Global>) -> Result<(), String> {
    let g = ga.inner();
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    let _guard = OpGuard::new(&app, "service");
    let data = st(g).data.clone();
    let r = svc::remove_service(&data).map_err(|e| {
        logger::log_code(
            "err",
            "service",
            "E-SVC-004",
            &format!("удаление службы не удалось: {e}"),
        );
        human::with_context(texts::SERVICE_REMOVE_CONTEXT, &e)
    });
    r?;
    logger::log("info", "service", "служба zapret удалена");
    {
        let mut s = st(g);
        s.service_running = None;
        s.service_strategy = None;
        s.runtime = None;
        // Автозапуск снят целиком: и служба, и окно в трее при входе.
        s.settings.autostart_mode = "none".into();
        s.settings.autostart_profile = None;
        s.settings.boot_app = false;
        s.save();
    }
    if let Err(e) = rn::remove_boot_task(&data) {
        logger::log_code(
            "warn",
            "boot",
            "W-BOOT-001",
            &format!("не удалось снять автозапуск в трей: {e}"),
        );
    }
    emit(
        &app,
        "zgui:status",
        serde_json::json!({"running": false, "pid": null, "profileId": null}),
    );
    emit(
        &app,
        "zgui:toast",
        serde_json::json!({"kind":"ok","text": texts::SERVICE_REMOVED}),
    );
    Ok(())
}

// ---------------------------------------------------------------- обновления

#[tauri::command]
async fn check_updates(app: AppHandle, ga: State<'_, Global>) -> Result<bool, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    // Авто-проверка в фоне держит только BusyGuard: ручную проверку/применение
    // не пускаем рядом — оба качают один и тот же архив, это лишняя сеть.
    if g.is_busy() {
        return Err(texts::BUSY_OTHER_OP.into());
    }
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
                    .filter(|e| e.status == "avail" || e.status == "new" || e.status == "modified")
                    .count();
                logger::log(
                    "info",
                    "updates",
                    &format!(
                        "проверка обновлений: {} файлов, требуют обновления {changed}",
                        entries.len()
                    ),
                );
                let ga2 = app2.state::<Global>();
                let mut s = ga2.state.lock().unwrap_or_else(|e| e.into_inner());
                s.updater = UpdaterCache {
                    last_check: Some(crate::profiles::now_str()),
                    entries,
                    last_auto: s.updater.last_auto.clone(),
                    next_auto: s.updater.next_auto,
                };
                s.save();
                emit(&app2, "zgui:updates", updater_view(&s));
                emit(
                    &app2,
                    "zgui:toast",
                    serde_json::json!({"kind":"ok","text": texts::UPDATES_CHECKED}),
                );
            }
            Err(e) => {
                logger::log_code(
                    "err",
                    "updates",
                    "E-UPD-001",
                    &format!("проверка обновлений не удалась: {e}"),
                );
                log_updates(&data, &format!("check error: {}", e));
                // Событие нужно и при ошибке: фронт снимает им блокировку кнопок
                // (иначе «Проверить обновления» остаётся серой до таймаута).
                {
                    let ga2 = app2.state::<Global>();
                    let s = st(ga2.inner());
                    emit(&app2, "zgui:updates", updater_view(&s));
                }
                emit(
                    &app2,
                    "zgui:toast",
                    serde_json::json!({"kind":"err","text": human::with_context(texts::UPDATES_CHECK_CONTEXT, &e)}),
                );
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
async fn apply_updates(
    app: AppHandle,
    ga: State<'_, Global>,
    ids: Vec<String>,
) -> Result<bool, String> {
    let g = ga.inner();
    let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
    // Авто-проверка в фоне держит только BusyGuard: одновременно с ней не
    // применяем — оба качают один архив, это лишняя сеть и гонка итогов.
    if g.is_busy() {
        return Err(texts::BUSY_OTHER_OP.into());
    }
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
        let file_ids: Vec<String> = ids
            .iter()
            .filter(|i| *i != up::PRESETS_ENTRY_ID)
            .cloned()
            .collect();
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
                logger::log_code(
                    "err",
                    "updates",
                    "E-UPD-002",
                    &format!("применение файловых обновлений не удалось: {e}"),
                );
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
                added: 0,
                removed: 0,
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
                added: 0,
                removed: 0,
            });
        }
        for e in &entries {
            if let Some(old) = s.updater.entries.iter_mut().find(|x| x.id == e.id) {
                *old = e.clone();
            } else {
                s.updater.entries.push(e.clone());
            }
        }
        if entries
            .iter()
            .any(|e| e.group.starts_with("flowseal strategies") && e.status == "ok")
        {
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
            if failed.is_empty() && file_err.is_none() {
                "ok"
            } else {
                "warn"
            },
            "updates",
            &format!(
                "обновлено записей: {ok_count}{}{}",
                preset_note.map(|n| format!(" ({n})")).unwrap_or_default(),
                if failed.is_empty() {
                    String::new()
                } else {
                    format!(", ошибки: {}", failed.join("; "))
                }
            ),
        );
        if let Some(e) = &file_err {
            emit(
                &app2,
                "zgui:toast",
                serde_json::json!({"kind":"err","text": human::with_context(texts::UPDATES_APPLY_CONTEXT, e)}),
            );
        } else if failed.is_empty() {
            emit(
                &app2,
                "zgui:toast",
                serde_json::json!({"kind":"ok","text": texts::updates_applied(ok_count)}),
            );
        } else {
            // Часть записей (в т.ч. набор пресетов) не применилась — иначе тост
            // сообщал бы только «обновлено: N» и ошибка терялась для пользователя.
            emit(
                &app2,
                "zgui:toast",
                serde_json::json!({"kind":"warn","text": texts::updates_applied_partial(ok_count, &failed.join("; "))}),
            );
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
    let g = ga.inner();
    let mut s = st(g);
    // Тема меняется только через `set_theme`: форма настроек её не присылает,
    // иначе сохранение сбрасывало бы выбор на дефолт.
    settings.theme = s.settings.theme.clone();
    // Серверные поля, которых нет в форме настроек: иначе любое сохранение
    // обнуляло бы постоянный TG-секрет. Режим администратора закреплён
    // навсегда (решение юзера 26.09) — форма его больше не присылает.
    settings.tg_secret = s.settings.tg_secret.clone();
    settings.always_admin = true;
    settings.admin_onboarded = true;
    // Автозапуск в трей — не поле формы: сохраняем текущее значение, иначе
    // любое сохранение настроек выключало бы задачу планировщика.
    settings.boot_app = s.settings.boot_app;
    // Пройденное обучение — тоже не поле формы: иначе следующее сохранение
    // настроек сбрасывало бы флаг и тур предлагался бы при каждом запуске.
    settings.tour_done = s.settings.tour_done;
    // Смена режима ipsets должна сразу пересобрать list/ipset-all.txt движка:
    // иначе переключатель «не работает» до перезапуска или применения обновлений.
    // Сам движок подхватит новый список при следующем запуске стратегии.
    let ipset_changed = settings.ipset_mode != s.settings.ipset_mode;
    let mode = settings.ipset_mode.clone();
    s.settings = settings;
    s.save();
    let (data, root) = (s.data.clone(), s.roots.path("flowseal"));
    drop(s); // порядок блокировок: state → testing, не наоборот
    if ipset_changed {
        // Во время прогона теста файл ipset намеренно пуст («any»): не перетираем
        // его до конца теста — поллер теста сам синхронизирует файл на выходе.
        let testing = g.testing.lock().unwrap_or_else(|e| e.into_inner()).running
            || tester::runner_alive(&data);
        if testing {
            logger::log(
                "info",
                "settings",
                "ipsets: смена режима отложена до конца теста",
            );
        } else if let Some(root) = root {
            let settings_now = st(g).settings.clone();
            up::sync_ipset(&root, &data, &settings_now);
            logger::log(
                "info",
                "settings",
                &format!("ipsets: режим «{mode}» применён к list/ipset-all.txt"),
            );
        }
    }
    // Тост здесь не показываем: настройки применяются сразу при изменении поля,
    // и всплывашка на каждое переключение только мешала бы.
    Ok(())
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
        // Папку создаём, а не молчим: кнопка «Открыть папку» должна что-то делать.
        std::fs::create_dir_all(&p).map_err(|_| "Папка ещё не создана".to_string())?;
    }
    std::process::Command::new("explorer.exe")
        .arg(&p)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "Z-GUI",
        "version": env!("CARGO_PKG_VERSION"),
        "portable": true,
        "elevated": rn::is_elevated()
    })
}

/// Отмечает обучение-тур пройденным (показываем один раз за установку).
#[tauri::command]
fn tour_done_set(ga: State<'_, Global>) -> Result<(), String> {
    let mut s = st(ga.inner());
    s.settings.tour_done = true;
    s.save();
    Ok(())
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
    std::fs::create_dir_all(&dir)
        .map_err(|e| human::with_context(texts::REPORT_DIR_FAILED, &e.to_string()))?;
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
                if info.exe.is_some() {
                    "есть"
                } else {
                    "НЕТ"
                }
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

    // Коды из журнала — с расшифровкой: юзеру и агенту сразу видно, что чинить.
    let log_dump = logger::dump();
    let codes_lines = {
        let found = codes::codes_in(&log_dump);
        if found.is_empty() {
            "  (кодов в этом отчёте нет)".to_string()
        } else {
            found
                .iter()
                .map(|c| {
                    format!(
                        "  {c} — {}",
                        codes::explain(c).unwrap_or("(описание не найдено)")
                    )
                })
                .collect::<Vec<_>>()
                .join("\r\n")
        }
    };

    let body = format!(
        "Z-GUI — отчёт о состоянии\r\n\
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
           автозапуск          : {autostart} (профиль: {autostart_profile})\r\n\
           интервал обновлений : каждые {interval} ч\r\n\
           игровой фильтр      : {game}\r\n\
           режим ipset         : {ipset}\r\n\
           всегда от админа    : {always_admin}\r\n\
           Telegram-прокси     : порт {tg_port}, автозапуск: {tg_auto}\r\n\
         ============================================================\r\n\
         РЕЗУЛЬТАТЫ ТЕСТА СТРАТЕГИЙ\r\n\
         ============================================================\r\n\
         {tests}\r\n\
         ============================================================\r\n\
         ЖУРНАЛ (последние {count} записей)\r\n\
         ============================================================\r\n\
         {log}\r\n\
         ============================================================\r\n\
         КОДЫ В ЭТОМ ОТЧЁТЕ (расшифровка)\r\n\
         ============================================================\r\n\
         {codes}\r\n",
        ver = env!("CARGO_PKG_VERSION"),
        time = logger::stamp(
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        ),
        win = win,
        adm = if rn::is_elevated() { "да" } else { "НЕТ" },
        exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        data = s.data.display(),
        engines = engines_lines,
        total = s.profiles.len(),
        runtime = runtime,
        service = service,
        theme = s.settings.theme,
        interval = s.settings.update_interval_hours,
        game = s.settings.game_filter,
        ipset = s.settings.ipset_mode,
        autostart = s.settings.autostart_mode,
        autostart_profile = s
            .settings
            .autostart_profile
            .clone()
            .unwrap_or_else(|| "нет".into()),
        always_admin = if s.settings.always_admin {
            "да"
        } else {
            "нет"
        },
        tg_port = s.settings.tg_port,
        tg_auto = if s.settings.tg_autostart {
            "да"
        } else {
            "нет"
        },
        count = logger::entries(0).len(),
        tests = {
            let cache = tester::TestCache::load(&s.data);
            tester::results_text(&cache.results, cache.best_id.as_deref())
        },
        log = log_dump,
        codes = codes_lines,
    );

    std::fs::write(&path, body)
        .map_err(|e| human::with_context(texts::REPORT_SAVE_FAILED, &e.to_string()))?;
    logger::log(
        "ok",
        "report",
        &format!("отчёт сохранён: {}", path.display()),
    );
    let _ = std::process::Command::new("explorer.exe")
        .arg(&path)
        .spawn();
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
                .unwrap_or(0),
        ),
        cache.results.len(),
        &s.data.display().to_string(),
        &tester::results_text(&cache.results, cache.best_id.as_deref()),
    );
    std::fs::write(&path, body)
        .map_err(|e| human::with_context(texts::RESULTS_SAVE_FAILED, &e.to_string()))?;
    logger::log(
        "ok",
        "report",
        &format!("результаты теста сохранены: {}", path.display()),
    );
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(async)]
fn dns_providers() -> Vec<dns::DnsProvider> {
    dns::providers().to_vec()
}

#[tauri::command]
async fn apply_dns(
    app: AppHandle,
    provider: String,
    adapter: Option<String>,
) -> Result<String, String> {
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
                logger::log_code(
                    "err",
                    "dns",
                    "E-DNS-001",
                    &format!("не удалось применить DNS {provider}: {e}"),
                );
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
                logger::log_code(
                    "err",
                    "dns",
                    "E-DNS-002",
                    &format!("не удалось сбросить DNS: {e}"),
                );
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
        let mut last_engine_scan: u64 = 0;
        let mut startup_check = true;
        // Был ли движок в прошлом скане: переход «есть → нет» = движок исчез
        // вне нашего управления, значит пора снять и загруженный драйвер; переход
        // в любую сторону — факт для UI (событие `zgui:status`).
        let mut had_engines = false;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let g = app.state::<Global>();

            let mut changed = false;
            let mut runtime_died = false;
            {
                let mut s = st(&g);
                if let Some(rt) = s.runtime.clone() {
                    if rt.via == "app" && !rn::pid_alive(rt.pid) {
                        s.runtime = None;
                        changed = true;
                        runtime_died = true;
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
            if changed {
                let _ = app.emit(
                    "zgui:status",
                    serde_json::json!({"running": false, "pid": null, "profileId": null}),
                );
            }
            if runtime_died {
                let _ = app.emit(
                    "zgui:toast",
                    serde_json::json!({"kind":"warn","text": texts::ENGINE_STOPPED}),
                );
            }
            // Факт «есть ли процесс движка» для шапки: скан раз в 5 с. Переход
            // в любую сторону — событие UI (окно само ничего не опрашивает).
            if now_ts() - last_engine_scan >= 5 {
                last_engine_scan = now_ts();
                let any = svc::any_winws_running();
                if any != had_engines {
                    had_engines = any;
                    let _ = app.emit(
                        "zgui:status",
                        serde_json::json!({"running": any, "pid": null, "profileId": null}),
                    );
                    if !any {
                        // Движок закончился (убили извне/сам завершился) — снимаем
                        // загруженный драйвер WinDivert. Если движком владеет служба
                        // (в т.ч. только поднимается), sweep обязан отступить.
                        if !svc::service_active() {
                            std::thread::spawn(maintenance_sweep);
                        }
                    }
                }
                // Античит-пауза (опция): при обнаружении античита один раз гасим
                // НАШУ оптимизацию. Берём OPS — не мешаем другой операции; после
                // остановки runtime пуст, поэтому повторно не срабатывает.
                if st(&g).settings.anticheat_pause && st(&g).runtime.is_some() {
                    if let Some(name) = svc::anticheat_running() {
                        if let Some(_op) = ops_try() {
                            let ga = app.state::<Global>();
                            match do_stop(&app, ga.inner(), true) {
                                Ok(()) => {
                                    let _ = app.emit("zgui:toast", serde_json::json!({"kind":"warn","text": texts::anticheat_paused(&name)}));
                                    logger::log(
                                        "warn",
                                        "anticheat",
                                        "обнаружен античит — оптимизация приостановлена",
                                    );
                                }
                                Err(e) => {
                                    logger::log(
                                        "warn",
                                        "anticheat",
                                        &format!("античит: остановить не удалось: {e}"),
                                    );
                                }
                            }
                        }
                    }
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
            if interval > 0 && !g.is_busy() && !g.op_running() {
                let now = now_ts();
                let cooled = next_auto.is_none_or(|t| t <= now);
                let planned_due = match &last_auto {
                    Some(t) => t.parse::<u64>().unwrap_or(0) + interval as u64 * 3600 <= now,
                    None => true,
                };
                // Пустой каталог: игнорируем расписание (interval) и ждём только короткий
                // кулдаун повторной попытки, чтобы каталог заполнился сам при старте.
                // Стартовая проверка — не чаще раза в час: иначе каждое открытие
                // программы заново качает zip-архив (~1.6 МБ).
                let startup_due = startup_check
                    && last_auto
                        .as_deref()
                        .and_then(|t| t.parse::<u64>().ok())
                        .is_none_or(|t| t + 3600 <= now);
                let due = if startup_due {
                    true
                } else if entries_empty {
                    // Пустой каталог (оффлайн/нет GitHub): пробуем не чаще раза
                    // в 5 минут, а не каждую минуту.
                    cooled
                } else {
                    cooled && planned_due
                };
                if due {
                    let app2 = app.clone();
                    // Гард берётся атомарно: если фоновая проверка/обновление уже
                    // идёт, тик пропускается, а не перебивает флаг чужой операции.
                    if let Ok(busy) = BusyGuard::try_new(&app2, texts::BUSY_DOWNLOAD) {
                        startup_check = false;
                        let mut s2 = st(&g);
                        let retry_secs = if entries_empty { 300 } else { 15 * 60 };
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
                                    logger::log_code(
                                        "warn",
                                        "updates",
                                        "W-UPD-003",
                                        &format!("автопроверка не удалась: {e}"),
                                    );
                                }
                            }
                        });
                    }
                }
            }
        }
    });
}

/// Нативное окно-предупреждение перед выходом: UI ещё не создан, показать иначе
/// нечего. Используется, когда пользователь отклонил запрос прав администратора.
#[cfg(windows)]
fn fatal_dialog(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
    };
    let title: Vec<u16> = "Z-GUI".encode_utf16().chain(std::iter::once(0)).collect();
    let body: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(windows))]
fn fatal_dialog(_text: &str) {}

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
            logger::log_code(
                "warn",
                "icon",
                "W-SYS-006",
                &format!("не удалось получить HWND окна: {e}"),
            );
            return;
        }
    };
    let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
    if hinst.is_null() {
        logger::log_code(
            "warn",
            "icon",
            "W-SYS-007",
            "не удалось получить HINSTANCE процесса",
        );
        return;
    }
    // MAKEINTRESOURCE(32512): имя ресурса — число, упакованное в указатель.
    let name = ICON_GROUP_ID as *const u16;
    let mut hicon: HICON = std::ptr::null_mut();
    let hr = unsafe {
        LoadIconWithScaleDown(
            hinst,
            name,
            TASKBAR_ICON_SIZE,
            TASKBAR_ICON_SIZE,
            &mut hicon,
        )
    };
    if hr < 0 || hicon.is_null() {
        // Фолбэк: LoadImage выберет ближайший кадр из группы.
        hicon = unsafe {
            LoadImageW(
                hinst,
                name,
                IMAGE_ICON,
                TASKBAR_ICON_SIZE,
                TASKBAR_ICON_SIZE,
                0,
            ) as HICON
        };
    }
    if hicon.is_null() {
        logger::log_code(
            "warn",
            "icon",
            "W-SYS-008",
            "иконка из ресурсов не загрузилась — оставляю иконку Tauri",
        );
        return;
    }
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, hicon as isize);
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, hicon as isize);
    }
    logger::log(
        "info",
        "icon",
        "окну поставлена нативная иконка 48×48 из ресурсов exe",
    );
}

#[cfg(not(windows))]
fn apply_native_window_icon(_window: &tauri::WebviewWindow) {}

// ---------------------------------------------------------------- builder

/// Точка входа дочернего режима `--test-runner`: выполняет план теста целиком
/// (процесс запускается с UAC один раз) и возвращает код выхода.
pub fn test_runner_main(plan_path: &std::path::Path) -> i32 {
    tester::run_test_runner(plan_path)
}

fn scanner_lists_dir(g: &Global) -> Option<PathBuf> {
    let s = st(g);
    s.roots.path(ENGINE_FLOWSEAL).map(|root| root.join("lists"))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScannerPage {
    strategies: Vec<Profile>,
    current: Option<String>,
    game_filter: String,
}

#[tauri::command]
fn scanner_page(ga: State<'_, Global>) -> ScannerPage {
    let g = ga.inner();
    let s = st(g);
    ScannerPage {
        strategies: s
            .profiles
            .iter()
            .filter(|p| p.engine == ENGINE_FLOWSEAL)
            .cloned()
            .collect(),
        current: s.runtime.as_ref().map(|r| r.profile_id.clone()),
        game_filter: s.settings.game_filter.clone(),
    }
}

#[tauri::command(async)]
async fn scanner_capture(name: String, secs: u64) -> Result<Vec<scanner::Endpoint>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(scanner::capture_process_endpoints(&name, secs.min(120)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ждёт, пока winws напишет «capture is started» (движок готов к фильтрации).
fn wait_engine_ready(out_log: &std::path::Path, pid: u32, secs: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if let Ok(t) = std::fs::read_to_string(out_log) {
            if t.contains("capture is started") {
                std::thread::sleep(Duration::from_millis(300));
                return;
            }
        }
        if !rn::pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Перебор `--dpi-desync-ttl` для цели: запускает выбранную стратегию с разными
/// TTL, проверяет TLS 1.3 к хосту и возвращает таблицу. Профиль не меняется.
fn ttl_sweep(
    app: &AppHandle,
    g: &Global,
    host: &str,
    id: &str,
) -> Result<scanner::TtlReport, String> {
    let (profile, root, settings, data) = {
        let s = st(g);
        let p = s
            .profile(id)
            .cloned()
            .ok_or_else(|| texts::PROFILE_NOT_FOUND.to_string())?;
        let root = s
            .roots
            .path(&p.engine)
            .ok_or_else(|| texts::engine_root_missing(&p.engine))?;
        (p, root, s.settings.clone(), s.data.clone())
    };
    let exe = locate_exe(&root, profile.exe_name())?;
    let (tcp, udp) = game_filter_ports_from(&settings);
    let base = presets::prepare_args(&profile.args, &root, &tcp, &udp, &settings.filter_mode);
    let wd = {
        let bin = root.join("bin");
        if bin.is_dir() {
            bin
        } else {
            exe.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(root.clone())
        }
    };
    let logs = data.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let out_log = logs.join("ttl-sweep.out.txt");
    let err_log = logs.join("ttl-sweep.err.txt");
    let pid_file = logs.join("ttl-sweep.pid.txt");
    let _ = std::fs::remove_file(&out_log);
    let _ = std::fs::remove_file(&err_log);

    // Прежнюю стратегию вернём после подбора; отмена — через тот же флаг, что у сканера.
    let prev = st(g).runtime.clone();
    let flag = data.join("logs").join("scan-stop.flag");
    let _ = std::fs::remove_file(&flag);

    stop_all_own(app, g)?;
    let elevated = rn::is_elevated();
    let mut results = Vec::new();
    let total = scanner::TTL_CANDIDATES.len();
    for (idx, ttl) in scanner::TTL_CANDIDATES.iter().enumerate() {
        if flag.exists() {
            break;
        }
        let ttl = *ttl;
        emit(
            app,
            "zgui:scan",
            serde_json::json!({"phase": "ttl", "done": idx, "total": total, "msg": format!("TTL {ttl}: проверяю…")}),
        );
        let args = scanner::inject_ttl(&base, ttl);
        let launched = if elevated {
            rn::spawn_direct(&exe, &wd, &args, &out_log, &err_log)
        } else {
            rn::write_launcher(&exe, &wd, &args, &pid_file)
                .and_then(|l| rn::spawn_and_wait_pid(l.path(), &pid_file, Duration::from_secs(60)))
        };
        let pid = match launched {
            Ok(p) => p,
            Err(_) => {
                results.push(scanner::TtlResult {
                    ttl,
                    ok: false,
                    ms: 0,
                });
                continue;
            }
        };
        // winws поднимает WinDivert ~2 с: без ожидания готовности все TTL «не сработали».
        if elevated {
            wait_engine_ready(&out_log, pid, 8);
        } else {
            std::thread::sleep(Duration::from_millis(1800));
        }
        let (mut ok, mut ms) = if rn::pid_alive(pid) {
            tester::tls13_measure(host)
        } else {
            (false, 0)
        };
        if !ok && rn::pid_alive(pid) {
            std::thread::sleep(Duration::from_millis(700));
            let (ok2, ms2) = tester::tls13_measure(host);
            if ok2 {
                ok = true;
                ms = ms2;
            }
        }
        let _ = rn::kill_pid_direct(pid);
        std::thread::sleep(Duration::from_millis(400));
        results.push(scanner::TtlResult { ttl, ok, ms });
    }
    let _ = std::fs::remove_file(&flag);
    let _ = stop_all_own(app, g);
    if let Some(rt) = prev {
        if let Err(e) = do_start(app, g, &rt.profile_id) {
            logger::log(
                "warn",
                "ttl",
                &format!(
                    "не удалось вернуть прежнюю стратегию «{}»: {e}",
                    rt.profile_id
                ),
            );
        }
    }
    emit(
        app,
        "zgui:scan",
        serde_json::json!({"phase": "ttl", "done": total, "total": total, "msg": "Готово"}),
    );
    let best = results
        .iter()
        .filter(|r| r.ok)
        .min_by_key(|r| r.ms)
        .map(|r| r.ttl);
    Ok(scanner::TtlReport {
        host: host.to_string(),
        strategy: profile.name.clone(),
        results,
        best,
    })
}

#[tauri::command(async)]
async fn scanner_method(target: String) -> Result<scanner::MethodReport, String> {
    let host = scanner::parse_site_target(&target)
        .ok_or_else(|| "Укажите домен или ссылку".to_string())?;
    tauri::async_runtime::spawn_blocking(move || scanner::diagnose_method(&host))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
async fn scanner_ttl(
    app: AppHandle,
    host: String,
    strategy_id: String,
) -> Result<scanner::TtlReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = scanner::parse_site_target(&host)
            .ok_or_else(|| "Укажите домен или ссылку".to_string())?;
        let ga = app.state::<Global>();
        let g = ga.inner();
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "ttl");
        ttl_sweep(&app, g, &target, &strategy_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
async fn scanner_run(
    app: AppHandle,
    kind: String,
    target: String,
    strategy_id: String,
    focus: bool,
) -> Result<scanner::ScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ga = app.state::<Global>();
        let g = ga.inner();
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "scan");
        scanner::run_scan(&app, g, &kind, &target, &strategy_id, focus)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
async fn scanner_apply(app: AppHandle, report: scanner::ScanReport) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ga = app.state::<Global>();
        let g = ga.inner();
        let _op = ops_try().ok_or(texts::BUSY_OTHER_OP)?;
        let _guard = OpGuard::new(&app, "scan-apply");
        scanner::apply_report(g, &report)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
async fn scanner_report_save(
    app: AppHandle,
    report: scanner::ScanReport,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = st(app.state::<Global>().inner()).data.clone();
        scanner::save_report(&data, &report)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
async fn scanner_processes() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let out = crate::runner::run_powershell(&[
            "-Command".into(),
            "$sys=[System.IO.Path]::GetFullPath($env:SystemRoot); Get-Process | Where-Object { $_.MainWindowTitle -ne '' -and $_.Path -and (-not $_.Path.StartsWith($sys,[System.StringComparison]::OrdinalIgnoreCase)) -and ($_.Name -notin @('zgui','NVIDIA Overlay')) } | Select-Object -ExpandProperty Name | Sort-Object -Unique".into(),
        ])
        .unwrap_or_default();
        out.lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
fn scanner_cancel(ga: State<'_, Global>) -> Result<(), String> {
    let data = st(ga.inner()).data.clone();
    let dir = data.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("scan-stop.flag"), b"1").map_err(|e| e.to_string())
}

/// Поднимает на передний план главное окно уже запущенной копии (по pid).
/// Нужно для режима «один экземпляр»: закрытие теперь прячет окно в трей,
/// поэтому повторный запуск должен вернуть существующее окно, а не плодить
/// вторую копию и вторую иконку в трее.
#[cfg(windows)]
fn focus_window_of_pid(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
        SetWindowPos, ShowWindow, GW_OWNER, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
    };
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> i32 {
        let t = &mut *(lparam as *mut (u32, bool, HWND));
        let mut wpid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut wpid);
        let owned = GetWindow(hwnd, GW_OWNER).is_null();
        let vis = IsWindowVisible(hwnd) != 0;
        if wpid == t.0 && owned && (!t.1 || vis) {
            t.2 = hwnd;
            return 0;
        }
        1
    }
    // Проход 1 — видимое окно; проход 2 — любое (копия могла спрятаться в трей).
    for require_visible in [true, false] {
        let mut t: (u32, bool, HWND) = (pid, require_visible, std::ptr::null_mut());
        unsafe {
            EnumWindows(Some(cb), &mut t as *mut _ as LPARAM);
            if !t.2.is_null() {
                // Крестик прячет окно (SW_HIDE), поэтому именно SW_SHOW; SW_RESTORE
                // лишь разворачивает свёрнутое. Затем на миг topmost — окно
                // гарантированно выходит вперёд даже когда Windows блокирует
                // SetForegroundWindow из чужого процесса.
                ShowWindow(t.2, SW_SHOW);
                ShowWindow(t.2, SW_RESTORE);
                SetWindowPos(t.2, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                SetWindowPos(t.2, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                SetForegroundWindow(t.2);
                return true;
            }
        }
    }
    false
}

#[cfg(not(windows))]
fn focus_window_of_pid(_pid: u32) -> bool {
    false
}

/// Закрывает все прочие копии программы (любые папки/сборки) — иначе в трее
/// копятся иконки старых тестовых версий. Новый процесс всегда от админа,
/// поэтому вправе снять прежние. Себя и фоновые тест-раннеры не трогаем.
fn close_other_instances() {
    let me = std::process::id();
    let runners = svc::zgui_runner_pids();
    for pid in svc::zgui_pids() {
        if pid == me || runners.contains(&pid) {
            continue;
        }
        let _ = rn::hidden_command("taskkill.exe")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
        logger::log(
            "info",
            "app",
            &format!("закрыта прежняя копия программы (pid {pid})"),
        );
    }
}

/// Проверка свежего релиза самой программы (плитка в «Обновлениях»).
#[tauri::command]
async fn app_update_info() -> Result<up::AppUpdate, String> {
    tauri::async_runtime::spawn_blocking(up::app_update_info)
        .await
        .map_err(|e| e.to_string())
}

/// Скачивание архива последнего релиза в `<data>/updates/` (без установки).
#[tauri::command(async)]
async fn app_update_download(app: AppHandle, ga: State<'_, Global>) -> Result<String, String> {
    let data = st(ga.inner()).data.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // OPS-лок берём внутри рабочего потока — guard не пересекает await.
        let _op = ops_try().ok_or_else(|| texts::BUSY_OTHER_OP.to_string())?;
        let _guard = OpGuard::new(&app, "app-update");
        up::app_update_download(&data)
    })
    .await
    .map_err(|e| e.to_string())?
}

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
                    "запуск Z-GUI {} (админ: {}, портативно: {})",
                    env!("CARGO_PKG_VERSION"),
                    if rn::is_elevated() { "да" } else { "нет" },
                    state.data.display()
                ),
            );
            // Состояние службы — из системы, а не из скопированного state.json:
            // при копировании папки данные могут говорить «служба есть», хотя её
            // нет, и на плитке висит фантомный чип «служба» до первого скана.
            sync_service_state(&mut state);
            let elevated_flag = std::env::args().any(|a| a == "--elevated");
            // Автозапуск при входе запускает программу скрыто в трей (задача
            // планировщика «ZapretGUI»). `--tray` — окно не показываем, живёт
            // только иконка; окно появляется по клику.
            let tray_flag = std::env::args().any(|a| a == "--tray");
            if !elevated_flag && !rn::is_elevated() {
                // Без прав администратора программа не работает: один запрос UAC
                // при запуске, режим закрепляется навсегда (решение юзера 26.09).
                // Отказ — нативное окно и выход: «полурежим» без прав плодил
                // нестабильные сценарии (тест через UAC-обёртку не стартовал).
                state.settings.always_admin = true;
                state.settings.admin_onboarded = true;
                let _ = state.save();
                match rn::relaunch_as_admin() {
                    Ok(true) => std::process::exit(0),
                    Ok(false) => {
                        logger::log_code("warn", "app", "W-SYS-009", "запрос прав администратора отклонён — выход");
                        fatal_dialog(texts::NEED_ADMIN_DIALOG);
                        std::process::exit(1);
                    }
                    Err(e) => {
                        logger::log_code("err", "app", "E-SYS-010", &format!("перезапуск от админа не удался: {e}"));
                        fatal_dialog(texts::NEED_ADMIN_DIALOG);
                        std::process::exit(1);
                    }
                }
            }
            // Вторая копия: не убиваем и не блокируем запуск (файл-замок может
            // остаться от убитого процесса), но честно предупреждаем — две копии
            // могут конфликтовать настройками. Решение юзера 26.09: жёсткое
            // закрытие откатили (старая копия под админом запирала запуск новой).
            let lock_path = state.data.join("zgui.lock");
            // Один экземпляр: если жива другая копия — поднимаем её окно и выходим.
            if let Some(pid) = std::fs::read_to_string(&lock_path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
            {
                if pid != std::process::id() && svc::pid_is_zgui(pid) {
                    // Автозапуск (`--tray`): копия уже живёт — молча выходим,
                    // чтобы не выдёргивать её окно при входе (решение юзера).
                    if tray_flag {
                        logger::log(
                            "info",
                            "app",
                            &format!("автозапуск: копия (pid {pid}) уже работает — тихо выхожу"),
                        );
                        std::process::exit(0);
                    }
                    let focused = focus_window_of_pid(pid);
                    logger::log(
                        "info",
                        "app",
                        &format!(
                            "уже запущена копия (pid {pid}) — открываю её окно{}",
                            if focused { "" } else { " (окно не найдено)" }
                        ),
                    );
                    std::process::exit(0);
                }
            }
            // Живой копии нет — закрываем прежние (тестовые/забытые), не трогая
            // фоновые тест-раннеры: чтобы в трее не копились иконки старых версий.
            close_other_instances();
            let _ = std::fs::write(&lock_path, std::process::id().to_string());
            provision_engines(&mut state);
            ensure_presets(&mut state);
            drop_custom_profiles(&mut state);
            state.save();
            // Настройка автозапуска в трей не должна держать старт — в фоне.
            let boot_data = state.data.clone();
            let boot_app = state.settings.boot_app;
            // Чиним «осиротевший» системный прокси (остался от выгруженного VPN).
            let healed = heal_orphan_proxy();
            // Единый sweep: зависшие движки чужих копий/прошлых сбоев и их
            // драйверы WinDivert, пока оптимизация не запущена.
            std::thread::spawn(move || {
                maintenance_sweep();
                sync_boot_task(&boot_data, boot_app);
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
                tg_offer_shown: AtomicBool::new(false),
                tg_offer_checked: std::sync::atomic::AtomicU64::new(0),
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
                // `--tray` (автозапуск при входе): окно создаётся скрытым —
                // показываем его только по клику на иконку или из меню трея.
                .visible(!tray_flag)
                .build()?;
            apply_native_window_icon(&window);
            // Трей-иконка: меню «Показать окно» / «Выход». Закрытие окна (крестик)
            // по умолчанию прячет его в трей, а не выходит из программы.
            {
                use tauri::menu::{MenuBuilder, MenuItemBuilder};
                use tauri::tray::TrayIconBuilder;
                let show_item = MenuItemBuilder::with_id("tray_show", "Показать окно").build(app)?;
                let quit_item = MenuItemBuilder::with_id("tray_quit", "Выход").build(app)?;
                let menu = MenuBuilder::new(app)
                    .item(&show_item)
                    .separator()
                    .item(&quit_item)
                    .build()?;
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
                let tray = TrayIconBuilder::with_id("main")
                    .icon(icon)
                    .tooltip("Z-GUI")
                    .menu(&menu)
                    // Левый клик по иконке показывает окно — то же, что пункт меню.
                    .on_tray_icon_event(|tray, event| {
                        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(w) = tray.app_handle().get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                    })
                    .on_menu_event(|app, ev| match ev.id().as_ref() {
                        "tray_show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        "tray_quit" => {
                            QUIT.store(true, Ordering::Relaxed);
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .build(app)?;
                // Держим иконку живой: при drop она исчезает из трея.
                app.manage(tray);
            }
            let handle = app.handle().clone();
            spawn_watchers(handle.clone());
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
            tg_offer_reset,
            cancel_test,
            apply_best_strategy,
            install_service,
            remove_service,
            vpn_check,
            net_reset,
            virtual_adapters,
            discord_cache_clear,
            fakes_view,
            replace_fake,
            hosts_update,
            reboot_now,
            check_updates,
            apply_updates,
            app_update_info,
            app_update_download,
            set_settings,
            set_theme,
            open_path,
            open_external,
            open_url,
            app_info,
            dns_providers,
            apply_dns,
            reset_dns,
            dns_benchmark,
            scanner_page,
            scanner_processes,
            scanner_capture,
            scanner_cancel,
            scanner_method,
            tour_done_set,
            scanner_ttl,
            scanner_run,
            scanner_apply,
            scanner_report_save
        ])
        .build(tauri::generate_context!())
        .expect("error while building zgui")
        .run(|app, event| {
            // Крестик главного окна прячет окно в трей (реальный выход — «Выход» в трее).
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } = &event
            {
                if label == "main" && !QUIT.load(Ordering::Relaxed) {
                    api.prevent_close();
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            }
            // При выходе гасим фоновый тестовый раннер: он elevated и сам по
            // закрытию GUI не умирает, а при следующем старте подхватывался как
            // «тест запустился сам». Единая процедура: стоп-флаг (раннер завершит
            // цикл сам) + попытка kill без эскалации (UAC на выходе не поднимаем).
            if matches!(event, tauri::RunEvent::Exit) {
                let data = app.state::<Global>().state.lock().unwrap_or_else(|e| e.into_inner()).data.clone();
                // Снимаем наш lock-файл, чтобы он не остался устаревшим.
                let lock = data.join("zgui.lock");
                if std::fs::read_to_string(&lock).ok().map(|s| s.trim() == std::process::id().to_string()).unwrap_or(false) {
                    let _ = std::fs::remove_file(&lock);
                }
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
                // Тотально: движки, поднятые другой копией/прошлым сбоем, не
                // должны переживать выход GUI (иначе «повисший winws» держит
                // драйвер). Служба — исключение: её движок живёт без программы.
                // Единая процедура: движки + зависшие драйверы WinDivert.
                maintenance_sweep();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{drop_custom_profiles, ensure_presets, local_proxy_port};

    #[test]
    fn one_line_collapses_whitespace_and_truncates() {
        assert_eq!(super::one_line("a\n  b\t c ", 100), "a b c");
        let long = super::one_line(&"x".repeat(500), 10);
        assert!(long.ends_with('…') && long.chars().count() <= 11);
    }

    #[test]
    fn local_proxy_port_detects_only_local() {
        assert_eq!(local_proxy_port("127.0.0.1:10809"), Some(10809));
        assert_eq!(local_proxy_port("localhost:8080"), Some(8080));
        assert_eq!(
            local_proxy_port("http=127.0.0.1:1080;https=127.0.0.1:1080"),
            Some(1080)
        );
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
        };
        state.profiles.push(crate::presets::preset_profile(
            "goodbyedpi-ru-dns",
            "goodbyedpi",
            "GoodbyeDPI · RU + DNS (старое)",
            vec!["-9".into(), "--dns-addr".into(), "77.88.8.8".into()],
        ));
        let mut own =
            crate::presets::preset_profile("my-own", "goodbyedpi", "Мой", vec!["-9".into()]);
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
        assert_eq!(
            fixed.args[0], "-5",
            "битый `-9` должен быть заменён на `-5`"
        );
        assert_eq!(
            fixed.args.last().map(String::as_str),
            Some("1253"),
            "аргументы из таблицы"
        );
        assert!(fixed.builtin, "пресет остаётся вшитым");
        let mine = state
            .profiles
            .iter()
            .find(|p| p.id == "preset:my-own")
            .expect("свой на месте");
        assert_eq!(
            mine.args,
            vec!["-9".to_string()],
            "пользовательский профиль не трогаем"
        );
        assert!(
            !state
                .profiles
                .iter()
                .any(|p| p.id == "auto:goodbyedpi:goodbyedpi-9"),
            "кандидат автоподбора удалён (функционал убран)"
        );
        assert!(
            !state
                .profiles
                .iter()
                .any(|p| p.id == "auto:goodbyedpi:gd-ttl5"),
            "любой auto:-профиль удаляется, а не остаётся"
        );
        assert!(
            !state
                .profiles
                .iter()
                .any(|p| p.id == "preset:flowseal-general"),
            "вырезанный пресет удалён из профилей"
        );
        assert_eq!(
            state.profiles.iter().filter(|p| p.builtin).count(),
            crate::presets::builtin_presets().len(),
            "все вшитые пресеты на месте"
        );
        let cache = crate::tester::TestCache::load(&data);
        assert!(
            cache.results.is_empty(),
            "результаты удалённых пресетов вычищены"
        );
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
        let mut custom =
            crate::presets::preset_profile("custom-old", "flowseal", "Мой", vec!["--x".into()]);
        custom.builtin = false;
        custom.source = None;
        state.profiles.push(custom);

        drop_custom_profiles(&mut state);

        assert!(
            state.profiles.iter().any(|p| p.id == "general (ALT)"),
            "авторский .bat-профиль должен остаться"
        );
        assert!(
            state.profiles.iter().any(|p| p.id == "preset:z2-x"),
            "OTA-пресет остаётся"
        );
        assert!(
            !state.profiles.iter().any(|p| p.id == "preset:custom-old"),
            "кастом без source удаляется"
        );
        let _ = std::fs::remove_dir_all(&data);
    }
}
