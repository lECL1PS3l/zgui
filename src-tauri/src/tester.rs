use crate::config::Profile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainGroup {
    pub id: &'static str,
    pub label: &'static str,
    pub critical: bool,
    pub priority: u8,
}

pub const GROUP_YOUTUBE: DomainGroup = DomainGroup { id: "youtube", label: crate::texts::GROUP_LABEL_YOUTUBE, critical: true, priority: 1 };
pub const GROUP_YOUTUBE_MUSIC: DomainGroup = DomainGroup { id: "youtube-music", label: crate::texts::GROUP_LABEL_YOUTUBE_MUSIC, critical: true, priority: 1 };
pub const GROUP_DISCORD: DomainGroup = DomainGroup { id: "discord", label: crate::texts::GROUP_LABEL_DISCORD, critical: true, priority: 1 };
pub const GROUP_MICROSOFT_XBOX: DomainGroup = DomainGroup { id: "microsoft-xbox", label: crate::texts::GROUP_LABEL_MICROSOFT_XBOX, critical: false, priority: 2 };
pub const GROUP_GOOGLE: DomainGroup = DomainGroup { id: "google", label: crate::texts::GROUP_LABEL_GOOGLE, critical: false, priority: 2 };
pub const GROUP_CLOUDFLARE: DomainGroup = DomainGroup { id: "cloudflare", label: crate::texts::GROUP_LABEL_CLOUDFLARE, critical: false, priority: 2 };
pub const GROUP_OTHER: DomainGroup = DomainGroup { id: "other", label: crate::texts::GROUP_LABEL_OTHER, critical: false, priority: 3 };

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GroupResult {
    pub id: String,
    pub label: String,
    pub passed: u32,
    pub total: u32,
    pub ok: bool,
    pub critical: bool,
    pub priority: u8,
}

pub fn classify_domain(host: &str) -> DomainGroup {
    let h = host.to_ascii_lowercase();
    if h == "music.youtube.com" {
        return GROUP_YOUTUBE_MUSIC;
    }
    if ["youtube.com", "www.youtube.com", "youtu.be", "youtube-nocookie.com", "youtube.googleapis.com", "youtubei.googleapis.com", "googlevideo.com", "ytimg.com", "ytimg.l.google.com", "yt3.googleusercontent.com"].contains(&h.as_str())
        || h.ends_with(".youtube.com")
        || h.ends_with(".ytimg.com")
        || h.ends_with(".googlevideo.com")
    {
        return GROUP_YOUTUBE;
    }
    if ["discord.com", "discord.gg", "discord.media", "discordapp.com", "discordapp.net", "discordapp.io", "discordapp.org", "discordstatus.com", "discord.status", "gateway.discord.gg", "dl.discordapp.net", "images.discordapp.net", "status.discordapp.com"].contains(&h.as_str()) || h.ends_with(".discord.com") || h.ends_with(".discordapp.com") {
        return GROUP_DISCORD;
    }
    if ["login.live.com", "account.live.com", "microsoft.com", "www.microsoft.com", "xbox.com", "www.xbox.com", "xboxlive.com", "xboxservices.com"].contains(&h.as_str()) || h.ends_with(".xbox.com") || h.ends_with(".xboxlive.com") || h.ends_with(".xboxservices.com") {
        return GROUP_MICROSOFT_XBOX;
    }
    // Google AI projects are intentionally not part of the Google secondary group.
    if h.contains("gemini") || h.contains("aistudio") || h.contains("notebooklm") || h.ends_with(".ai.google") || h.contains("labs.google") {
        return GROUP_OTHER;
    }
    if h == "google.com" || h.ends_with(".google.com") || h.ends_with(".googleusercontent.com") || h.ends_with(".googleapis.com") || h == "gstatic.com" || h.ends_with(".gstatic.com") {
        return GROUP_GOOGLE;
    }
    if h == "cloudflare.com" || h.ends_with(".cloudflare.com") || h.ends_with(".cloudflare.net") || h == "cloudflare-dns.com" || h == "one.one.one.one" {
        return GROUP_CLOUDFLARE;
    }
    GROUP_OTHER
}

#[cfg(test)]
pub fn critical_group_ok(group: &DomainGroup, passed: u32, total: u32) -> bool {
    if group.id == GROUP_YOUTUBE_MUSIC.id {
        return total > 0 && passed > 0;
    }
    !group.critical || (total > 0 && passed * 2 >= total)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DomainResult {
    pub key: String,
    pub host: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub group_label: Option<String>,
    pub ok: bool,
    pub ms: u64,
    pub detail: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StrategyResult {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub group: String,
    pub started: bool,
    pub score: u32,
    pub max_score: u32,
    pub domains: Vec<DomainResult>,
    pub error: Option<String>,
    #[serde(default)]
    pub groups: Vec<GroupResult>,
    #[serde(default)]
    pub critical_ok: bool,
    /// Ключ аргументов стратегии на момент прогона. Позволяет переиспользовать
    /// результат (мост «тест ⇄ автоподбор»), если аргументы не изменились.
    #[serde(default)]
    pub args_key: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TestProgress {
    pub running: bool,
    pub phase: String,
    pub current_id: Option<String>,
    pub current_name: Option<String>,
    pub index: usize,
    pub total: usize,
    pub pct: i32,
    pub msg: String,
    pub results: Vec<StrategyResult>,
    pub best_id: Option<String>,
    pub best_name: Option<String>,
    pub done: bool,
    /// Прогон остановлен пользователем (отмена): итоговой «лучшей» нет,
    /// но частичные результаты показываются — фронт не пишет «ничего не подошло».
    #[serde(default)]
    pub stopped: bool,
}

/// TCP-connect с таймаутом (используется в юнит-тесте и как быстрый probe).
#[cfg(test)]
fn probe_tcp(host: &str, port: u16, timeout: Duration) -> (bool, u64, String) {
    use std::time::Instant;
    let start = Instant::now();
    let addr = match std::net::ToSocketAddrs::to_socket_addrs(&(host, port)) {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => return (false, 0, "DNS: пусто".into()),
        },
        Err(e) => return (false, 0, format!("DNS: {}", e)),
    };
    match std::net::TcpStream::connect_timeout(&addr, timeout) {
        Ok(stream) => {
            let ms = start.elapsed().as_millis() as u64;
            drop(stream);
            (true, ms, "подключено".into())
        }
        Err(e) => {
            let ms = start.elapsed().as_millis() as u64;
            let detail = match e.kind() {
                std::io::ErrorKind::TimedOut => "таймаут".to_string(),
                std::io::ErrorKind::ConnectionRefused => "отказ".to_string(),
                _ => e.to_string(),
            };
            (false, ms, detail)
        }
    }
}

/// Стандартный набор целей — 1:1 с `utils/targets.txt` оригинала flowseal
/// (только URL-цели; ICMP-цели по IP убраны — пинг DNS есть во вкладке «DNS»).
/// CDN-эндпоинты надёжнее «конкретных» доменов: их же проверяет авторский харнесс.
pub const REQUIRED_DOMAINS: &[&str] = &[
    "discord.com",
    "gateway.discord.gg",
    "cdn.discordapp.com",
    "updates.discord.com",
    "www.youtube.com",
    "youtu.be",
    "i.ytimg.com",
    "redirector.googlevideo.com",
    "www.google.com",
    "www.gstatic.com",
    "www.cloudflare.com",
    "cdnjs.cloudflare.com",
];

/// Онлайн-списки геоблока — отдельный диагностический тест (лимит задаёт UI).
/// Основной набор: критические группы (YouTube, Discord) и вторые по приоритету
/// (Google, Cloudflare — сообщаются, но для успеха не обязательны).
pub fn main_domains(limit: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for host in REQUIRED_DOMAINS {
        let host = host.to_string();
        // Ключ — полный слаг хоста: первые лейблы не уникальны ("www").
        let key = host.replace('.', "_");
        out.push((key, host));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Есть ли `curl.exe` (Windows 10 1803+ кладёт его в System32; иначе ищем в PATH).
/// Без curl проба не работает — тест обязан честно отказаться, а не «всё fail».
pub fn curl_available() -> bool {
    if let Some(root) = std::env::var_os("SystemRoot") {
        if std::path::Path::new(&root).join("System32").join("curl.exe").is_file() {
            return true;
        }
    }
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join("curl.exe").is_file()))
        .unwrap_or(false)
}

fn curl_exe() -> String {
    if let Some(root) = std::env::var_os("SystemRoot") {
        let p = std::path::Path::new(&root).join("System32").join("curl.exe");
        if p.is_file() {
            return p.to_string_lossy().into_owned();
        }
    }
    "curl.exe".into()
}

/// На время теста ipset переводится в режим «any» (пустой файл): прогон не
/// зависит от десятков тысяч IP-правил (так же делает авторский харнесс).
/// При выходе файл восстанавливается — даже при отмене/ошибке (Drop).
pub struct IpsetAnyGuard {
    restored: Vec<(std::path::PathBuf, std::path::PathBuf)>,
}

/// Запись с ретраями: антивирус/индексатор Windows может транзиентно держать
/// свежесозданный файл. Возврат false — файл остаётся как был (fail-safe).
fn write_retry(path: &std::path::Path, data: &[u8]) -> bool {
    for attempt in 0..3 {
        if std::fs::write(path, data).is_ok() {
            return true;
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    false
}

/// Переводит указанные `ipset-all.txt` в «any»; лечит последствия прошлого
/// сбоя (оставшийся `.test-backup` при пропавшем live-файле).
pub fn activate_ipset_any(paths: &[std::path::PathBuf]) -> IpsetAnyGuard {
    let mut restored = Vec::new();
    for live in paths {
        let backup = live.with_extension("txt.test-backup");
        // Лечение последствий прошлого сбоя: бэкап есть, а live пуст (остался
        // «any») или пропал — сначала возвращаем оригинал, потом переводим в «any»
        // заново. Без этого после неудачного Drop ipset оставался пустым навсегда.
        let live_empty = std::fs::read(live)
            .map(|c| c.iter().all(|b| b.is_ascii_whitespace()))
            .unwrap_or(false);
        if backup.is_file() && (!live.is_file() || live_empty) {
            if std::fs::copy(&backup, live).is_ok() {
                let _ = std::fs::remove_file(&backup);
            } else {
                continue; // вернуть нечего — live не трогаем
            }
        }
        let Ok(content) = std::fs::read(live) else { continue };
        if content.iter().all(|b| b.is_ascii_whitespace()) {
            continue; // уже «any» — трогать нечего
        }
        if write_retry(&backup, &content) && write_retry(live, b"") {
            restored.push((live.clone(), backup));
        }
    }
    IpsetAnyGuard { restored }
}

impl Drop for IpsetAnyGuard {
    fn drop(&mut self) {
        for (live, backup) in &self.restored {
            if backup.is_file() {
                let mut ok = false;
                for attempt in 0..3 {
                    if std::fs::copy(backup, live).is_ok() {
                        ok = true;
                        break;
                    }
                    if attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
                if ok {
                    let _ = std::fs::remove_file(backup);
                } else {
                    // Бэкап НЕ удаляем: следующий activate_ipset_any вылечит
                    // пустой live из него (см. выше), иначе список терялся.
                    crate::logger::log(
                        "err",
                        "test",
                        &format!("не удалось вернуть {} из .test-backup — восстановлю при следующем тесте", live.display()),
                    );
                }
            }
        }
    }
}

/// Запоминает, какие стратегии уже тестировались (id → ok).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TestCache {
    pub tested_at: Option<String>,
    pub best_id: Option<String>,
    pub results: Vec<StrategyResult>,
}

impl TestCache {
    pub fn load(data: &Path) -> Self {
        let p = data.join("tests.json");
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    /// Атомарная запись (tmp + rename): обрыв на середине не оставит
    /// повреждённый `tests.json`, который молча превращается в пустой кэш.
    pub fn save(&self, data: &Path) {
        let p = data.join("tests.json");
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = crate::config::atomic_write(&p, s.as_bytes());
        }
    }
}

/// Жив ли фоновый раннер теста: по маркерам `logs/test-runner.pid` и
/// `logs/test-stop.flag`.
pub fn runner_alive(data: &Path) -> bool {
    let pid = data.join("logs/test-runner.pid");
    let stop = data.join("logs/test-stop.flag");
    std::fs::read_to_string(&pid)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|p| !stop.exists() && crate::runner::pid_alive(p))
        .unwrap_or(false)
}

// ------------------------------------------------------------- раннер на Rust

/// Один запуск на стратегию.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TestStep {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub group: String,
    pub exe: String,
    pub workdir: String,
    pub args: Vec<String>,
}

/// Цель пробы с заранее вычисленной группой (чтобы раннер не зависел от реестра).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanDomain {
    pub key: String,
    pub host: String,
    pub group: String,
    pub group_label: String,
    pub critical: bool,
    pub priority: u8,
}

/// План теста: что запускать и что проверять. Раннер (`zgui.exe --test-runner`)
/// выполняет его целиком в одном elevated-процессе — один UAC на весь тест.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TestPlan {
    pub steps: Vec<TestStep>,
    pub domains: Vec<PlanDomain>,
    pub out: String,
    pub pid: String,
    pub win_pid: String,
    pub flag: String,
}

/// Промежуточный/итоговый прогресс прогона (пишет раннер, читает GUI).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProgressState {
    pub index: usize,
    pub total: usize,
    pub current_id: Option<String>,
    pub current_name: Option<String>,
    pub results: Vec<StrategyResult>,
    pub done: bool,
}

/// Пишет план теста и чистит маркеры прошлого прогона. Возвращает
/// (путь к плану, путь к файлу прогресса).
pub fn write_plan(data: &Path, steps: &[TestStep], domains: &[(String, String)]) -> Result<(PathBuf, PathBuf), String> {
    let logs = data.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("logs: {e}"))?;
    let plan_path = logs.join("test-plan.json");
    let out_path = logs.join("test-out.json");
    let pid_path = logs.join("test-runner.pid");
    let win_pid = logs.join("test-current.pid");
    let flag = logs.join("test-stop.flag");
    for f in [&out_path, &pid_path, &win_pid, &flag] {
        let _ = std::fs::remove_file(f);
    }
    let plan = TestPlan {
        steps: steps.to_vec(),
        domains: domains
            .iter()
            .map(|(k, h)| {
                let g = classify_domain(h);
                PlanDomain {
                    key: k.clone(),
                    host: h.clone(),
                    group: g.id.to_string(),
                    group_label: g.label.to_string(),
                    critical: g.critical,
                    priority: g.priority,
                }
            })
            .collect(),
        out: out_path.to_string_lossy().into_owned(),
        pid: pid_path.to_string_lossy().into_owned(),
        win_pid: win_pid.to_string_lossy().into_owned(),
        flag: flag.to_string_lossy().into_owned(),
    };
    let json = serde_json::to_vec(&plan).map_err(|e| e.to_string())?;
    std::fs::write(&plan_path, json).map_err(|e| format!("план теста: {e}"))?;
    Ok((plan_path, out_path))
}

/// Читает файл прогресса (атомарная запись раннером: tmp + rename).
pub fn read_progress(out_path: &Path) -> Option<ProgressState> {
    let text = std::fs::read_to_string(out_path).ok()?;
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// Частичные результаты из файла прогресса — для сохранения при отмене теста.
pub fn salvage_progress(out_path: &Path) -> Vec<StrategyResult> {
    read_progress(out_path).map(|st| st.results).unwrap_or_default()
}

/// Спит `ms`, но просыпается раньше, если появился стоп-флаг.
/// Возвращает true, если сон прерван остановкой.
fn sleep_or_stop(flag: &Path, ms: u64) -> bool {
    let mut left = ms;
    while left > 0 {
        if flag.exists() {
            return true;
        }
        let d = left.min(100);
        std::thread::sleep(Duration::from_millis(d));
        left -= d;
    }
    flag.exists()
}

/// Точка входа режима `--test-runner` (вызывается из main до старта Tauri).
pub fn run_test_runner(plan_path: &Path) -> i32 {
    let Ok(text) = std::fs::read_to_string(plan_path) else { return 2 };
    let Ok(plan) = serde_json::from_str::<TestPlan>(&text) else { return 2 };
    let _ = std::fs::write(&plan.pid, std::process::id().to_string());
    let flag = PathBuf::from(&plan.flag);
    let out = PathBuf::from(&plan.out);
    let total = plan.steps.len();
    let mut results: Vec<StrategyResult> = Vec::new();
    write_progress(&out, 0, total, plan.steps.first(), &results, false);

    let mut index = 0usize;
    let mut stopped = false;
    for step in &plan.steps {
        if flag.exists() {
            stopped = true;
            break;
        }
        index += 1;
        let mut r = measure_step(&plan, step);
        // Второй шанс для почти нулевого результата: первый прогон сразу после
        // старта часто не применяется из-за гонки загрузки WinDivert.
        // При остановке второй шанс НЕ гоняем (иначе поднимаем движок зря).
        if !flag.exists() && r.started && r.score <= 1 {
            let r2 = measure_step(&plan, step);
            if r2.started && r2.score > r.score {
                r = r2;
            }
        }
        std::thread::sleep(Duration::from_millis(400));
        results.push(r);
        write_progress(&out, index, total, Some(step), &results, false);
        if flag.exists() {
            stopped = true;
            break;
        }
    }
    let _ = std::fs::remove_file(&plan.win_pid);
    write_progress(&out, index, total, plan.steps.last(), &results, true);
    if stopped {
        crate::logger::log("info", "test", "раннер остановлен по флагу");
    }
    0
}

fn write_progress(out: &Path, index: usize, total: usize, cur: Option<&TestStep>, results: &[StrategyResult], done: bool) {
    let state = ProgressState {
        index,
        total,
        current_id: cur.map(|s| s.id.clone()),
        current_name: cur.map(|s| s.name.clone()),
        results: results.to_vec(),
        done,
    };
    if let Ok(json) = serde_json::to_vec(&state) {
        let tmp = out.with_extension("tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, out);
        }
    }
}

/// Запускает движок шага; stdout/stderr — в диагностические файлы.
fn spawn_engine(step: &TestStep, err_file: &Path, out_file: &Path) -> Result<Child, String> {
    let out = std::fs::File::create(out_file).map_err(|e| format!("лог: {e}"))?;
    let err = std::fs::File::create(err_file).map_err(|e| format!("лог: {e}"))?;
    let mut cmd = crate::runner::hidden_command(&step.exe);
    cmd.current_dir(&step.workdir)
        .args(&step.args)
        .stdout(out)
        .stderr(err);
    cmd.spawn().map_err(|e| format!("не удалось запустить движок: {e}"))
}

fn child_alive(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}

fn tail_of(path: &Path, max_chars: usize) -> String {
    std::fs::read(path)
        .ok()
        .map(|b| {
            let s = crate::config::decode_text(&b);
            let t = s.trim();
            let chars: Vec<char> = t.chars().collect();
            let start = chars.len().saturating_sub(max_chars);
            chars[start..].iter().collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug)]
struct Probe {
    label: &'static str,
    ok: bool,
    code: String,
    ms: u64,
}

/// Разбирает stdout curl: `"<code> <time_total>"` (формат `-w`).
fn parse_curl_out(s: &str) -> Option<(String, u64)> {
    let t = s.trim();
    let mut it = t.split_whitespace();
    let code = it.next()?;
    let time = it.next()?;
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let secs: f64 = time.parse().ok()?;
    Some((code.to_string(), (secs * 1000.0) as u64))
}

fn curl_probe(exe: &str, host: &str, label: &'static str, proto_args: &[&str]) -> Probe {
    let mut args: Vec<String> = vec![
        "-sS".into(),
        "-o".into(),
        "NUL".into(),
        "-m".into(),
        "4".into(),
        "--connect-timeout".into(),
        "3".into(),
    ];
    args.extend(proto_args.iter().map(|s| s.to_string()));
    args.push("-w".into());
    args.push("%{http_code} %{time_total}".into());
    args.push(format!("https://{host}/"));
    let out = crate::runner::hidden_command(exe).args(&args).output();
    let fail = Probe { label, ok: false, code: "000".into(), ms: 0 };
    match out {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            match parse_curl_out(&stdout) {
                Some((code, ms)) => Probe { label, ok: code != "000", code, ms },
                None => fail,
            }
        }
        Err(_) => fail,
    }
}

/// Матрица проб: HTTP/1.1 + TLS1.2 + TLS1.3 на каждый хост, батчами по 8
/// процессов (как авторский харнесс — большие пачки перегружают WinDivert).
fn curl_matrix(hosts: &[String], flag: &Path) -> HashMap<String, Vec<Probe>> {
    let protos: Vec<(&'static str, Vec<&'static str>)> = vec![
        ("HTTP1.1", vec!["--http1.1"]),
        ("TLS1.2", vec!["--tlsv1.2", "--tls-max=1.2"]),
        ("TLS1.3", vec!["--tlsv1.3", "--tls-max=1.3"]),
    ];
    struct Q {
        host: String,
        label: &'static str,
        args: Vec<&'static str>,
    }
    let mut queue: Vec<Q> = Vec::new();
    for h in hosts {
        for (label, args) in &protos {
            queue.push(Q { host: h.clone(), label, args: args.clone() });
        }
    }
    let exe = curl_exe();
    let mut res: HashMap<String, Vec<Probe>> = hosts.iter().map(|h| (h.clone(), Vec::new())).collect();
    let mut i = 0usize;
    while i < queue.len() {
        if flag.exists() {
            break;
        }
        let end = (i + 8).min(queue.len());
        let batch = &queue[i..end];
        let got: Vec<(usize, Probe)> = std::thread::scope(|s| {
            let handles: Vec<_> = batch
                .iter()
                .enumerate()
                .map(|(j, q)| {
                    let exe = exe.clone();
                    s.spawn(move || (j, curl_probe(&exe, &q.host, q.label, &q.args)))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or((usize::MAX, Probe { label: "?", ok: false, code: "000".into(), ms: 0 })))
                .collect()
        });
        for (j, p) in got {
            if j != usize::MAX {
                if let Some(v) = res.get_mut(&batch[j].host) {
                    v.push(p);
                }
            }
        }
        i = end;
    }
    res
}

/// Замер сайта для сканера диагностики: ok = хоть один протокол ответил,
/// ms = самый быстрый живой протокол. Обёртка над curl-матрицей тестера.
pub(crate) fn site_measure(host: &str) -> crate::scanner::PhaseMeasure {
    let exe = curl_exe();
    let mut ok = false;
    let mut best: Option<u64> = None;
    for args in [
        vec!["--http1.1"],
        vec!["--tlsv1.2", "--tls-max=1.2"],
        vec!["--tlsv1.3", "--tls-max=1.3"],
    ] {
        let p = curl_probe(&exe, host, "probe", &args);
        if p.ok {
            ok = true;
            best = Some(best.map_or(p.ms, |b| b.min(p.ms)));
        }
    }
    crate::scanner::PhaseMeasure {
        ok,
        ms: best.unwrap_or(0),
        detail: if ok { "есть ответ".into() } else { "нет ответа".into() },
    }
}

struct Row {
    host: String,
    ok: bool,
    ms: u64,
    detail: String,
}

/// host → строка результата: ok = хоть одна проба прошла, ms = самая быстрая.
fn rows_from_matrix(hosts: &[String], m: &HashMap<String, Vec<Probe>>) -> Vec<Row> {
    hosts
        .iter()
        .map(|h| {
            let probes = m.get(h).cloned().unwrap_or_default();
            let mut ok = false;
            let mut best = 0u64;
            let mut parts: Vec<String> = Vec::new();
            for p in &probes {
                let mark = if p.ok { "ok" } else { "fail" };
                parts.push(format!("{} {} {}", p.label, p.code, mark));
                if p.ok {
                    ok = true;
                    if best == 0 || p.ms < best {
                        best = p.ms;
                    }
                }
            }
            Row { host: h.clone(), ok, ms: best, detail: parts.join("; ") }
        })
        .collect()
}

/// ICMP-пинг хостов (информационно, как у автора; «TTL=» не зависит от локали).
fn ping_pass(hosts: &[String]) -> HashMap<String, bool> {
    std::thread::scope(|s| {
        let handles: Vec<_> = hosts
            .iter()
            .map(|h| {
                let h2 = h.clone();
                s.spawn(move || {
                    let out = crate::runner::hidden_command("ping.exe")
                        .args(["-n", "1", "-w", "1200", &h2])
                        .output();
                    let ok = out
                        .map(|o| String::from_utf8_lossy(&o.stdout).contains("TTL="))
                        .unwrap_or(false);
                    (h2, ok)
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect()
    })
}

/// Один прогон стратегии: старт движка (с одним ретраем мгновенной смерти),
/// матрица проб + повтор упавших, ICMP-пинг, сборка групп. Движок гасится.
fn measure_step(plan: &TestPlan, step: &TestStep) -> StrategyResult {
    let out_path = PathBuf::from(&plan.out);
    let dir = out_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let safe: String = step.id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' }).collect();
    let err_file = dir.join(format!("run-{safe}.err.txt"));
    let out_file = dir.join(format!("run-{safe}.out.txt"));
    let flag_path = PathBuf::from(&plan.flag);

    let mut res = StrategyResult {
        id: step.id.clone(),
        name: step.name.clone(),
        engine: step.engine.clone(),
        group: step.group.clone(),
        started: false,
        score: 0,
        max_score: plan.domains.len() as u32,
        domains: Vec::new(),
        error: None,
        groups: Vec::new(),
        critical_ok: false,
        args_key: None,
    };

    let mut child = match spawn_engine(step, &err_file, &out_file) {
        Ok(c) => c,
        Err(e) => {
            res.error = Some(e);
            return res;
        }
    };
    let _ = std::fs::write(&plan.win_pid, child.id().to_string());
    if sleep_or_stop(&flag_path, 1800) {
        // Остановка до проб: движок гасим, результат — «не тестировалась».
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&plan.win_pid);
        return res;
    }
    if !child_alive(&mut child) {
        // Один ретрай: гонка загрузки WinDivert/транзиентное состояние.
        match spawn_engine(step, &err_file, &out_file) {
            Ok(c2) => {
                child = c2;
                let _ = std::fs::write(&plan.win_pid, child.id().to_string());
                if sleep_or_stop(&flag_path, 1800) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&plan.win_pid);
                    return res;
                }
            }
            Err(e) => {
                res.error = Some(e);
                return res;
            }
        }
    }
    if !child_alive(&mut child) {
        let code = child.wait().ok().and_then(|s| s.code());
        let tail = {
            let mut t = tail_of(&err_file, 300);
            if t.is_empty() {
                t = tail_of(&out_file, 300);
            }
            t
        };
        res.error = Some(if !crate::runner::is_elevated() {
            format!("ADMIN_REQUIRED: winws needs administrator rights — run the GUI as admin (exit {code:?}) {tail}")
        } else {
            format!("process exited immediately (exit {code:?}) {tail}")
        });
        return res;
    }

    res.started = true;
    let hosts: Vec<String> = plan.domains.iter().map(|d| d.host.clone()).collect();
    let flag = flag_path.as_path();
    if !flag.exists() {
        let matrix = curl_matrix(&hosts, flag);
        let mut rows = rows_from_matrix(&hosts, &matrix);
        // Повтор для упавших хостов: одна оборванная сессия не приговор.
        let failed: Vec<String> = rows.iter().filter(|r| !r.ok).map(|r| r.host.clone()).collect();
        if !failed.is_empty() && !flag.exists() {
            let m2 = curl_matrix(&failed, flag);
            let rows2 = rows_from_matrix(&failed, &m2);
            for r2 in &rows2 {
                if r2.ok {
                    if let Some(r) = rows.iter_mut().find(|r| r.host == r2.host) {
                        r.ok = true;
                        r.ms = r2.ms;
                        r.detail = format!("{} | retry: {}", r.detail, r2.detail);
                    }
                }
            }
        }
        let ping = if flag.exists() { HashMap::new() } else { ping_pass(&hosts) };
        let mut doms: Vec<DomainResult> = Vec::new();
        for d in &plan.domains {
            let (ok, ms, detail) = rows
                .iter()
                .find(|r| r.host == d.host)
                .map(|r| (r.ok, r.ms, r.detail.clone()))
                .unwrap_or((false, 0, String::new()));
            let pingmark = if ping.get(&d.host).copied().unwrap_or(false) { "ping ok" } else { "ping fail" };
            doms.push(DomainResult {
                key: d.key.clone(),
                host: d.host.clone(),
                group: Some(d.group.clone()),
                group_label: Some(d.group_label.clone()),
                ok,
                ms,
                detail: format!("{detail}; {pingmark}"),
            });
        }
        res.score = doms.iter().filter(|d| d.ok).count() as u32;
        // Группы — в порядке первого появления в плане.
        let mut order: Vec<String> = Vec::new();
        for d in &plan.domains {
            if !order.contains(&d.group) {
                order.push(d.group.clone());
            }
        }
        for gid in order {
            let items: Vec<&PlanDomain> = plan.domains.iter().filter(|d| d.group == gid).collect();
            let first = items[0];
            let total = items.len() as u32;
            let passed = doms
                .iter()
                .filter(|d| d.group.as_deref() == Some(gid.as_str()) && d.ok)
                .count() as u32;
            let is_music = gid == GROUP_YOUTUBE_MUSIC.id;
            let ok = if is_music { passed > 0 } else { passed > 0 && passed * 2 >= total };
            res.groups.push(GroupResult {
                id: gid,
                label: first.group_label.clone(),
                passed,
                total,
                ok,
                critical: first.critical,
                priority: first.priority,
            });
        }
        res.critical_ok = res.groups.iter().all(|g| !g.critical || g.ok);
        res.domains = doms;
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&plan.win_pid);
    res
}

/// Группирует профили по типу стратегии для UI.
pub fn group_of(p: &Profile) -> String {
    let kind = if p.source.as_deref().is_some_and(|s| s.starts_with("preset:")) {
        "preset"
    } else {
        "bat"
    };
    format!("{kind} · {}", crate::config::engine_def(&p.engine).map(|d| d.label).unwrap_or(p.engine.as_str()))
}

/// Текстовая сводка результатов теста для журнала/отчёта: таблица стратегий
/// (очки, критические домены) + список не ответивших критических доменов.
/// Общая для кнопки «Результаты в журнал» и полного отчёта.
pub fn results_text(results: &[StrategyResult], best_id: Option<&str>) -> String {
    if results.is_empty() {
        return format!("{}\r\n", crate::texts::RESULTS_EMPTY);
    }
    let mut sorted: Vec<&StrategyResult> = results.iter().collect();
    sorted.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let best = best_id
        .and_then(|id| results.iter().find(|r| r.id == id))
        .map(|r| crate::texts::results_best_line(&r.name, r.score, r.max_score))
        .unwrap_or_else(|| crate::texts::BEST_NONE.into());
    let width = sorted
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(10)
        .min(48);

    let mut out = String::new();
    out.push_str(&format!("{best}\r\n\r\n"));
    for r in &sorted {
        let state = if !r.started {
            let err: String = r.error.clone().unwrap_or_default().replace(['\r', '\n'], " ");
            let err: String = err.chars().take(120).collect();
            crate::texts::result_not_started(&crate::human::humanize(&err))
        } else if r.critical_ok {
            crate::texts::RESULT_CRIT_OK.to_string()
        } else {
            crate::texts::RESULT_CRIT_FAIL.to_string()
        };
        out.push_str(&format!(
            "  {:<width$}  {:>3}/{:<3}  {}\r\n",
            r.name,
            r.score,
            r.max_score,
            state,
            width = width
        ));
    }

    let mut bad: Vec<String> = Vec::new();
    for r in &sorted {
        if !r.started {
            continue;
        }
        let failed_groups: Vec<&GroupResult> =
            r.groups.iter().filter(|g| g.critical && !g.ok).collect();
        if failed_groups.is_empty() {
            continue;
        }
        let mut hosts: Vec<String> = Vec::new();
        for g in failed_groups {
            hosts.extend(
                r.domains
                    .iter()
                    .filter(|d| !d.ok && d.group.as_deref() == Some(g.id.as_str()))
                    .map(|d| d.host.clone()),
            );
        }
        if hosts.is_empty() {
            continue;
        }
        let total = hosts.len();
        hosts.truncate(12);
        let more = if total > 12 { format!(" … ещё {}", total - 12) } else { String::new() };
        bad.push(format!(
            "  {} ({}/{}): {}{}",
            r.name,
            r.score,
            r.max_score,
            hosts.join(", "),
            more
        ));
    }
    if !bad.is_empty() {
        out.push_str(&format!("\r\n{}\r\n", crate::texts::RESULTS_FAILED_HEADER));
        out.push_str(&bad.join("\r\n"));
        out.push_str("\r\n");
    }
    out
}

/// Формирует сводку: отсортированные результаты + лучшая стратегия.
/// При равных очках выигрывает та, что прошла больше критических групп, затем —
/// с меньшей средней задержкой; имя лишь последний детерминированный tie-break.
pub fn summarize(results: &[StrategyResult]) -> (Vec<StrategyResult>, Option<String>) {
    let mut v = results.to_vec();
    // Не стартовавшая стратегия не может считаться прошедшей критические группы.
    let critical_passed =
        |r: &StrategyResult| if r.started { r.groups.iter().filter(|g| g.critical && g.ok).count() } else { 0 };
    let avg_ms = |r: &StrategyResult| -> u64 {
        if r.domains.is_empty() {
            u64::MAX
        } else {
            r.domains.iter().map(|d| d.ms).sum::<u64>() / r.domains.len() as u64
        }
    };
    v.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| critical_passed(b).cmp(&critical_passed(a)))
            .then_with(|| avg_ms(a).cmp(&avg_ms(b)))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    // первая в отсортированном списке, прошедшая критические группы — лучшая
    let best = v.iter().find(|r| r.started && r.critical_ok).map(|r| r.id.clone());
    (v, best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipset_heal_restores_backup_when_live_empty() {
        let dir = std::env::temp_dir().join(format!("zgui-ipset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let live = dir.join("ipset-all.txt");
        let backup = live.with_extension("txt.test-backup");
        // Последствие прошлого сбоя: live пуст, бэкап с оригиналом на месте.
        std::fs::write(&live, b"").unwrap();
        std::fs::write(&backup, b"1.2.3.4/32\n").unwrap();
        {
            let guard = activate_ipset_any(std::slice::from_ref(&live));
            // Активация вылечила live из бэкапа и снова перевела в «any».
            assert!(std::fs::read(&live).unwrap().is_empty());
            assert_eq!(std::fs::read(&backup).unwrap(), b"1.2.3.4/32\n");
            drop(guard);
        }
        // Drop вернул оригинал и убрал бэкап.
        assert_eq!(std::fs::read(&live).unwrap(), b"1.2.3.4/32\n");
        assert!(!backup.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summarize_picks_best() {
        let mk = |id: &str, name: &str, score: u32| StrategyResult {
            id: id.into(),
            name: name.into(),
            engine: "flowseal".into(),
            group: "flowseal bat".into(),
            started: true,
            score,
            max_score: 5,
            domains: vec![],
            error: None,
            groups: vec![],
            critical_ok: true,
            args_key: None,
        };
        let (v, best) = summarize(&[mk("a", "A", 2), mk("b", "B", 5), mk("c", "C", 5)]);
        assert_eq!(best.as_deref(), Some("b"));
        assert_eq!(v[0].score, 5);
    }

    #[test]
    fn summarize_breaks_ties_by_critical_groups_then_latency() {
        let mk = |id: &str, crit_group_ok: bool, ms: u64| StrategyResult {
            id: id.into(),
            name: id.into(),
            engine: "flowseal".into(),
            group: "flowseal bat".into(),
            started: true,
            score: 5,
            max_score: 5,
            domains: vec![DomainResult {
                key: "d".into(),
                host: "d.example".into(),
                group: Some("youtube".into()),
                group_label: Some("YouTube".into()),
                ok: true,
                ms,
                detail: "http 200".into(),
            }],
            error: None,
            groups: vec![GroupResult {
                id: "youtube".into(),
                label: "YouTube".into(),
                passed: 1,
                total: 1,
                ok: crit_group_ok,
                critical: true,
                priority: 1,
            }],
            critical_ok: true,
            args_key: None,
        };
        let (v, best) = summarize(&[
            mk("slow", true, 900),
            mk("fast", true, 100),
            mk("no-critical", false, 10),
        ]);
        assert_eq!(best.as_deref(), Some("fast"), "при равных очках выигрывает быстрая");
        assert_eq!(v[0].id, "fast");
        assert_eq!(v[2].id, "no-critical", "без критических групп — в конце");
    }

    #[test]
    fn results_text_lists_scores_and_failed_critical_hosts() {
        let mk = |id: &str, name: &str, score: u32, crit_ok: bool, started: bool| StrategyResult {
            id: id.into(),
            name: name.into(),
            engine: "flowseal".into(),
            group: "bat · Flowseal".into(),
            started,
            score,
            max_score: 115,
            domains: vec![
                DomainResult {
                    key: "yt".into(),
                    host: "youtube.com".into(),
                    group: Some("youtube".into()),
                    group_label: Some("YouTube".into()),
                    ok: crit_ok,
                    ms: 10,
                    detail: "http 200".into(),
                },
                DomainResult {
                    key: "dc".into(),
                    host: "discord.com".into(),
                    group: Some("discord".into()),
                    group_label: Some("Discord".into()),
                    ok: crit_ok,
                    ms: 10,
                    detail: "timeout".into(),
                },
            ],
            error: if started { None } else { Some("process exited immediately".into()) },
            groups: vec![
                GroupResult {
                    id: "youtube".into(),
                    label: "YouTube".into(),
                    passed: if crit_ok { 1 } else { 0 },
                    total: 1,
                    ok: crit_ok,
                    critical: true,
                    priority: 1,
                },
                GroupResult {
                    id: "discord".into(),
                    label: "Discord".into(),
                    passed: if crit_ok { 1 } else { 0 },
                    total: 1,
                    ok: crit_ok,
                    critical: true,
                    priority: 1,
                },
            ],
            critical_ok: crit_ok,
            args_key: None,
        };
        let text = results_text(
            &[
                mk("a", "general · ALT11", 84, true, true),
                mk("b", "general · ALT2", 3, false, true),
                mk("c", "zz-broken", 0, false, false),
            ],
            Some("a"),
        );
        assert!(text.contains("Лучшая стратегия: general · ALT11 (84/115)"), "{text}");
        assert!(text.contains(crate::texts::RESULT_CRIT_OK), "{text}");
        assert!(text.contains(crate::texts::RESULT_CRIT_FAIL), "{text}");
        assert!(text.contains("не запустилась: Движок сразу завершился"), "{text}");
        assert!(
            text.contains(&format!("{} (3/115): youtube.com, discord.com", "general · ALT2")),
            "упавшие критические хосты должны быть перечислены: {text}"
        );
        // Порядок строк — по очкам (лучший выше), не стартовавшая в конце.
        let (a, b, c) = (
            text.find("general · ALT11").unwrap(),
            text.find("general · ALT2").unwrap(),
            text.find("zz-broken").unwrap(),
        );
        assert!(a < b && b < c, "порядок строк: {text}");
    }

    #[test]
    fn groups() {
        let p = Profile {
            id: "x".into(),
            name: "x".into(),
            engine: crate::config::ENGINE_FLOWSEAL.into(),
            args: vec!["--x".into()],
            builtin: false,
            source: Some("general.bat".into()),
            updated_at: None,
        };
        assert_eq!(group_of(&p), "bat · Flowseal (zapret winws)");
        let q = Profile {
            engine: crate::config::ENGINE_ZAPRET2.into(),
            source: Some("preset:zapret2-general".into()),
            ..p
        };
        assert_eq!(group_of(&q), "preset · zapret2 (winws2)");
    }

    #[test]
    fn probe_localhost_ok() {
        let (ok, _ms, _d) = probe_tcp("localhost", 1, Duration::from_millis(300));
        assert!(!ok, "порт 1 не должен быть открыт");
    }

    #[test]
    fn main_domains_match_author_targets() {
        // URL-часть списка 1:1 с utils/targets.txt оригинала; ICMP-цели по IP
        // убраны из теста (пинг DNS — во вкладке «DNS»).
        let d = main_domains(500);
        assert_eq!(d.len(), REQUIRED_DOMAINS.len());
        assert!(d.iter().any(|(_, h)| h == "cdn.discordapp.com"));
        assert!(d.iter().any(|(_, h)| h == "cdnjs.cloudflare.com"));
        assert!(!d.iter().any(|(_, h)| h.chars().all(|c| c.is_ascii_digit() || c == '.')), "IP-целей в тесте быть не должно");
        // Ключи уникальны (ключ — полный слаг хоста).
        let keys: std::collections::HashSet<&String> = d.iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), d.len(), "ключи целей должны быть уникальны");
        for (_, host) in &d {
            let group = classify_domain(host);
            assert!(
                group.critical || group.priority == 2,
                "{host} не должен попадать в стандартный тест (группа {})",
                group.id
            );
        }
    }

    #[test]
    fn cache_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("zgui-cache-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let c = TestCache {
            best_id: Some("general".into()),
            ..Default::default()
        };
        c.save(&tmp);
        let back = TestCache::load(&tmp);
        assert_eq!(back.best_id.as_deref(), Some("general"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn critical_groups_require_youtube_music() {
        assert!(critical_group_ok(&GROUP_YOUTUBE, 3, 5));
        assert!(!critical_group_ok(&GROUP_YOUTUBE_MUSIC, 0, 1));
        assert!(critical_group_ok(&GROUP_YOUTUBE_MUSIC, 1, 1));
        assert!(!critical_group_ok(&GROUP_DISCORD, 1, 3));
    }

    #[test]
    fn classifies_priority_groups_and_excludes_google_ai() {
        assert_eq!(classify_domain("music.youtube.com").id, "youtube-music");
        assert_eq!(classify_domain("discordapp.com").id, "discord");
        assert_eq!(classify_domain("xboxservices.com").id, "microsoft-xbox");
        assert_eq!(classify_domain("www.google.com").id, "google");
        assert_eq!(classify_domain("gemini.google.com").id, "other");
        assert_eq!(classify_domain("www.cloudflare.com").id, "cloudflare");
        assert_eq!(classify_domain("1.1.1.1").id, "other");
        assert_eq!(classify_domain("i.ytimg.com").id, "youtube");
        assert_eq!(classify_domain("redirector.googlevideo.com").id, "youtube");
        assert_eq!(classify_domain("www.gstatic.com").id, "google");
        assert_eq!(classify_domain("cdn.discordapp.com").id, "discord");
        assert_eq!(GROUP_OTHER.label, "Другие сайты");
    }

    #[test]
    fn parses_curl_output() {
        assert_eq!(parse_curl_out("200 1.234"), Some(("200".into(), 1234)));
        assert_eq!(parse_curl_out("000 0.000000"), Some(("000".into(), 0)));
        assert_eq!(parse_curl_out("404 0.05"), Some(("404".into(), 50)));
        assert_eq!(parse_curl_out(""), None);
        assert_eq!(parse_curl_out("garbage"), None);
        assert_eq!(parse_curl_out("20 1.0"), None, "код короче трёх цифр");
    }

    #[test]
    fn plan_roundtrip_and_progress() {
        let base = std::env::temp_dir().join(format!("zgui-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let steps = vec![TestStep {
            id: "s1".into(),
            name: "Шаг".into(),
            engine: "flowseal".into(),
            group: "g".into(),
            exe: "C:\\Windows\\System32\\cmd.exe".into(),
            workdir: "C:\\Windows".into(),
            args: vec!["/c".into(), "echo hi".into()],
        }];
        let domains = vec![
            ("y".to_string(), "www.youtube.com".to_string()),
            ("d".to_string(), "discord.com".to_string()),
        ];
        let (plan_path, out) = write_plan(&base, &steps, &domains).unwrap();
        let plan: TestPlan = serde_json::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.domains.len(), 2);
        assert!(plan.domains.iter().all(|d| d.critical), "обе цели критические");
        assert!(plan.out.ends_with("test-out.json"));
        // План пишется без маркеров прошлого прогона.
        assert!(!base.join("logs/test-stop.flag").exists());
        let _ = std::fs::remove_dir_all(&base);
        let _ = out;
    }

    #[cfg(windows)]
    #[test]
    fn runner_measures_offline_step() {
        // Герметичный прогон: движок — cmd.exe с долгим ping, домены — без
        // сервера. Раннер обязан корректно стартовать, замерить нули и завершиться.
        let base = std::env::temp_dir().join(format!("zgui-runner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let steps = vec![TestStep {
            id: "t".into(),
            name: "Тест".into(),
            engine: "flowseal".into(),
            group: "g".into(),
            exe: "C:\\Windows\\System32\\cmd.exe".into(),
            workdir: "C:\\Windows".into(),
            args: vec!["/c".into(), "ping -n 6 127.0.0.1 >nul".into()],
        }];
        let domains = vec![
            ("y".to_string(), "127.0.0.1".to_string()),
            ("x".to_string(), "example.invalid".to_string()),
        ];
        let (plan_path, out) = write_plan(&base, &steps, &domains).unwrap();
        let code = run_test_runner(&plan_path);
        assert_eq!(code, 0, "раннер завершился с ошибкой");
        let st = read_progress(&out).expect("нет файла прогресса");
        assert!(st.done, "прогон должен быть завершён");
        assert_eq!(st.total, 1);
        assert_eq!(st.index, 1);
        let r = &st.results[0];
        assert!(r.started, "процесс не запустился: {:?}", r.error);
        assert_eq!(r.score, 0, "без сервера домены не проходят");
        assert_eq!(r.max_score, 2);
        // Группа «other» некритична — критические прошли вакуумно.
        assert!(r.critical_ok);
        assert!(!r.domains.is_empty());
        assert!(r.domains.iter().all(|d| d.detail.contains("ping")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ipset_guard_sets_any_and_restores() {
        // Windows-антивирус может транзиентно держать файл — читаем с ретраями.
        fn read_retry(p: &std::path::Path) -> Vec<u8> {
            for _ in 0..50 {
                if let Ok(b) = std::fs::read(p) {
                    return b;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            panic!("файл не читается: {}", p.display());
        }
        fn gone_retry(p: &std::path::Path) -> bool {
            for _ in 0..50 {
                if !p.exists() {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            false
        }
        let tmp = std::env::temp_dir().join(format!("zgui-ipset-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let live = tmp.join("ipset-all.txt");
        let backup = tmp.join("ipset-all.txt.test-backup");
        assert!(write_retry(&live, b"1.2.3.0/24\n5.6.7.0/24\n"));
        {
            let _g = activate_ipset_any(std::slice::from_ref(&live));
            assert!(read_retry(&live).is_empty(), "на время теста ipset пуст (any)");
            assert!(backup.is_file(), "оригинал сохранён в .test-backup");
        }
        assert_eq!(
            read_retry(&live),
            b"1.2.3.0/24\n5.6.7.0/24\n",
            "после теста ipset восстановлен"
        );
        assert!(gone_retry(&backup), "бэкап убран");
        // Уже «any» (пустой) — файл не трогаем, бэкап не создаём.
        assert!(write_retry(&live, b""));
        {
            let _g = activate_ipset_any(std::slice::from_ref(&live));
            assert!(!backup.exists());
        }
        // Хвост прошлого сбоя: бэкап есть, live пропал — лечим при активации.
        assert!(gone_retry(&live) || std::fs::remove_file(&live).is_ok());
        assert!(write_retry(&backup, b"9.9.9.0/24\n"));
        {
            let _g = activate_ipset_any(std::slice::from_ref(&live));
            assert_eq!(
                read_retry(&backup),
                b"9.9.9.0/24\n",
                "застрявший бэкап вернулся и снова сохранён"
            );
            assert!(read_retry(&live).is_empty(), "live снова в «any»");
        }
        assert_eq!(read_retry(&live), b"9.9.9.0/24\n", "после Drop — снова валидный список");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
