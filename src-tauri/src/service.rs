use crate::config::{Profile, SERVICE_NAME};
use crate::runner::{hidden_command, run_script_privileged};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConflictProcess {
    pub pid: u32,
    pub name: String,
    pub note: String,
}

/// Известные VPN/прокси-клиенты (без .exe).
const VPN_PROCESSES: &[&str] = &[
    "happ",
    "happd",
    "amneziawg",
    "amneziavpn",
    "amnezia",
    "awg",
    "wireguard",
    "openvpn",
    "openvpn-gui",
    "openconnect",
    "outline",
    "outline-client",
    "tunnelblick",
    "sing-box",
    "singbox",
    "xray",
    "v2ray",
    "v2raya",
    "nekoray",
    "nekobox",
    "clash",
    "clash-verge",
    "clash-meta",
    "mihomo",
    "clashx",
    "shadowsocks",
    "shadowsocks-rust",
    "ss-local",
    "sslocal",
    "trojan",
    "trojan-go",
    "hysteria",
    "hysteria2",
    "tun2socks",
    "wintun",
    "proxifier",
    "windscribe",
    "nordvpn",
    "expressvpn",
    "surfshark",
    "protonvpn",
    "mullvad",
    "privatevpn",
    "hiddify",
    "v2rayng",
    "tunsafe",
    "vpngate",
    "softether",
];

/// Службы-туннели VPN.
const VPN_SERVICES: &[&str] = &[
    "WireGuardTunnel",
    "OpenVPNService",
    "OpenVPNServiceInteractive",
    "AmneziaWG",
    "amneziawg",
    "AmneziaVPN",
    "AmneziaVPN-service",
    "Happ",
    "HappService",
];

/// Разбирает строку `tasklist /FO CSV /NH`: `"имя","PID","...` в (имя, PID).
/// Простая резка по `","` ломалась на именах процессов с запятой и молча
/// пропускала их — PID не находился, процесс не считался конфликтом.
fn tasklist_name_pid(line: &str) -> Option<(String, u32)> {
    let rest = line.trim().strip_prefix('"')?;
    let (name, rest) = rest.split_once("\",\"")?;
    let (pid, _) = rest.split_once("\",\"")?;
    Some((name.to_string(), pid.parse::<u32>().ok()?))
}

/// Перечисляет процессы из tasklist, отфильтрованные по предикату имени.
fn list_processes(mut want: impl FnMut(&str) -> bool) -> Vec<(String, u32)> {
    let out = hidden_command("tasklist.exe")
        .args(["/FO", "CSV", "/NH"])
        .output();
    let Ok(o) = out else { return Vec::new() };
    let txt = String::from_utf8_lossy(&o.stdout);
    let mut found = Vec::new();
    for line in txt.lines() {
        let Some((name, pid)) = tasklist_name_pid(line) else { continue };
        if want(&name) {
            found.push((name, pid));
        }
    }
    found
}

fn is_vpn_process(name: &str) -> bool {
    let n = name.to_lowercase();
    let base = n.trim_end_matches(".exe");
    VPN_PROCESSES.iter().any(|v| base == *v || base.starts_with(&format!("{}-", v)))
}

/// Запущен ли Telegram Desktop (для предложения прокси-моста при свежем старте).
/// Имя процесса — Telegram.exe; сравнение по базовому имени без расширения.
pub fn telegram_running() -> bool {
    !list_processes(|n| n.to_lowercase().trim_end_matches(".exe") == "telegram").is_empty()
}

/// Находит запущенные VPN/прокси-клиенты.
pub fn detect_vpn() -> Vec<ConflictProcess> {
    let mut out: Vec<ConflictProcess> = list_processes(is_vpn_process)
        .into_iter()
        .map(|(name, pid)| ConflictProcess {
            pid,
            name,
            note: crate::texts::VPN_PROCESS_NOTE.into(),
        })
        .collect();

    for svc in VPN_SERVICES {
        let o = hidden_command("sc.exe").args(["query", svc]).output();
        if let Ok(o) = o {
            let txt = String::from_utf8_lossy(&o.stdout).to_uppercase();
            if o.status.success() && txt.contains("RUNNING") {
                out.push(ConflictProcess {
                    pid: 0,
                    name: format!("service:{}", svc),
                    note: crate::texts::VPN_SERVICE_NOTE.into(),
                });
            }
        }
    }
    out
}

/// Все exe реестра движков (проверка «любой запущен» и тотальная остановка).
pub const ENGINE_EXES: [&str; 4] = ["winws.exe", "winws2.exe", "goodbyedpi.exe", "dpibreak.exe"];

/// Пути процессов по именам (pid → полный путь к exe) через WMI.
fn process_image_paths(names: &[&str]) -> HashMap<u32, PathBuf> {
    let filter = names
        .iter()
        .map(|n| format!("Name='{}'", n))
        .collect::<Vec<_>>()
        .join(" or ");
    // '|' в имени файла Windows запрещён — безопасный разделитель значений.
    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"{filter}\" -Property ProcessId,ExecutablePath \
         | Where-Object {{ $_.ExecutablePath }} \
         | ForEach-Object {{ '{{0}}|{{1}}' -f $_.ProcessId, $_.ExecutablePath }}"
    );
    let out = hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();
    let Ok(o) = out else { return HashMap::new() };
    let txt = String::from_utf8_lossy(&o.stdout);
    let mut map = HashMap::new();
    for line in txt.lines() {
        let Some(sep) = line.find('|') else { continue };
        let Ok(pid) = line[..sep].trim().parse::<u32>() else { continue };
        let path = PathBuf::from(&line[sep + 1..].trim());
        if path.is_absolute() {
            map.insert(pid, path);
        }
    }
    map
}

/// Дешёвая проверка: запущен ли хоть один exe движков (без WMI).
pub fn any_winws_running() -> bool {
    !list_processes(|n| ENGINE_EXES.iter().any(|e| n.eq_ignore_ascii_case(e)))
    .is_empty()
}

/// PID-ы ВСЕХ процессов-движков в системе — независимо от папки и прав:
/// любой winws/winws2/goodbyedpi/dpibreak конфликтует с нашим фильтром
/// WinDivert (два одновременно не работают). Решение владельца: гасим любой —
/// наша копия, старая версия, соло-харнесс или чужая сборка.
pub fn all_engine_pids(exclude: Option<u32>) -> Vec<u32> {
    let mut pids: Vec<u32> = list_processes(|n| ENGINE_EXES.iter().any(|e| n.eq_ignore_ascii_case(e)))
        .into_iter()
        .map(|(_, pid)| pid)
        .filter(|pid| Some(*pid) != exclude)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// Идентификатор нашего движка, если путь лежит в раскладке программы
/// (`...\data\engines\<движок>\...`). Ловит любую копию GUI, не только текущую.
fn layout_engine_id(path: &str) -> Option<&'static str> {
    let p = path.to_lowercase().replace('/', "\\");
    let (_, tail) = p.split_once("\\data\\engines\\")?;
    let id = tail.split('\\').next()?;
    crate::config::engine_ids().into_iter().find(|e| e.eq_ignore_ascii_case(id))
}

/// Службы-драйверы WinDivert: (имя, состояние, путь образа).
fn windivert_driver_services() -> Vec<(String, String, String)> {
    let script = r#"Get-CimInstance Win32_SystemDriver -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'WinDivert*' -or ($_.PathName -and $_.PathName -match '(?i)windivert\d*\.sys$') } | ForEach-Object { '{0}|{1}|{2}' -f $_.Name, $_.State, $_.PathName }"#;
    let out = hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output();
    let Ok(o) = out else { return Vec::new() };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '|');
            let name = it.next()?.trim();
            let state = it.next()?.trim();
            let path = it.next()?.trim();
            (!name.is_empty() && !path.is_empty())
                .then(|| (name.to_string(), state.to_string(), path.to_string()))
        })
        .collect()
}

/// Запущен ли хоть один движок из раскладки программы (любая копия GUI).
fn our_engines_running() -> bool {
    process_image_paths(&ENGINE_EXES)
        .values()
        .any(|p| layout_engine_id(&p.to_string_lossy()).is_some())
}

/// PowerShell-скрипт тотального добивания движков: гасит ЛЮБОЙ winws/winws2/
/// goodbyedpi/dpibreak в системе (`taskkill /F /T /IM` по именам — работает и для
/// процессов с нечитаемым путём) и останавливает службы, чей ImagePath в
/// реестре ссылается на exe движка (любое имя службы, включая чужой `zapret`).
/// ASCII-only.
pub(crate) fn kill_engines_script() -> String {
    let mut s = String::new();
    s.push_str(&format!("{}\n", crate::runner::PS_HEADER));
    s.push_str("$zguiStopped = @()\n");
    s.push_str("foreach ($zguiName in @('winws.exe','winws2.exe','goodbyedpi.exe','dpibreak.exe')) {\n");
    s.push_str("  try { taskkill /F /T /IM $zguiName 2>$null | Out-Null } catch {}\n");
    s.push_str("}\n");
    s.push_str("$zguiServices = @(Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Services' -ErrorAction SilentlyContinue | ForEach-Object {\n");
    s.push_str("  $zguiIp = (Get-ItemProperty -Path $_.PSPath -Name ImagePath -ErrorAction SilentlyContinue).ImagePath\n");
    s.push_str("  if ($zguiIp -and ($zguiIp -match '(?i)(winws2?|goodbyedpi|dpibreak)\\.exe')) { $_.PSChildName }\n");
    s.push_str("})\n");
    s.push_str("foreach ($zguiSvc in $zguiServices) {\n");
    s.push_str("  try { sc.exe stop $zguiSvc 2>$null | Out-Null } catch {}\n");
    s.push_str("  $zguiStopped += $zguiSvc\n");
    s.push_str("}\n");
    s.push_str("$zguiStopped -join ','\n");
    s.push_str("exit 0\n");
    s
}

/// Тотальное добивание движков под администратором (см. `kill_engines_script`):
/// процессы по именам + службы по ImagePath из реестра. Возвращает имена
/// найденных служб (для журнала).
pub fn sweep_our_engines(data_dir: &Path) -> Result<Vec<String>, String> {
    let script = crate::runner::LockedScript::write(
        data_dir.join("logs").join(format!("sweep_engines_{}.ps1", std::process::id())),
        &kill_engines_script(),
    )?;
    let out = hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script.path())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .replace('\r', "")
        .split([',', '\n'])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect())
}

/// Гасит зависшие драйверы WinDivert (ЛЮБЫЕ: наши, чужие, из других программ),
/// если ни один движок сейчас не запущен (иначе фильтр занят законно). Решение
/// владельца: два фильтра WinDivert не работают одновременно, висячий драйвер
/// обязан быть убран независимо от того, чей он. Возвращает имена убранных
/// служб; детали — в журнале.
pub fn cleanup_own_stray_drivers() -> Vec<String> {
    // Служба владеет движком (в т.ч. «запускается») — драйвер ей нужен, не трогаем.
    if service_active() {
        return Vec::new();
    }
    if our_engines_running() {
        return Vec::new();
    }
    let mut cleaned = Vec::new();
    for (name, state, path) in windivert_driver_services() {
        if !state.eq_ignore_ascii_case("running") {
            continue;
        }
        let _ = hidden_command("sc.exe").args(["stop", &name]).output();
        let _ = hidden_command("sc.exe").args(["delete", &name]).output();
        // Успех — по факту состояния, а не по коду возврата: `sc delete` может
        // вернуть «marked for deletion», хотя служба уже снимается.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let running = hidden_command("sc.exe")
            .args(["query", &name])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_uppercase().contains("RUNNING"))
            .unwrap_or(false);
        if running {
            // Смена состояния драйвера не мгновенна, а без админа sc stop вообще
            // не проходит — текст честный в обоих случаях (раньше и с правами
            // писалось «нужны права администратора»).
            let why = if crate::runner::is_elevated() {
                "ещё выгружается — повторю при остановке/старте"
            } else {
                "нет прав администратора — снимется со следующего запуска от админа"
            };
            crate::logger::log_code(
                "warn",
                "windivert",
                "W-STOP-008",
                &format!("зависший драйвер {name} не убран: {why}"),
            );
        } else {
            crate::logger::log_code("warn", "windivert", "W-WIN-001", &format!("убран зависший драйвер: {name} ({path})"));
            cleaned.push(name);
        }
    }
    cleaned
}

/// Запускает установленную службу zapret (нужна после теста, который её глушил).
///
/// Успех — по факту (`sc query` показывает RUNNING), а не по коду `net start`:
/// «уже запущена» и «не запустилась» иначе выглядят одинаково.
pub fn start_service(data_dir: &Path) -> Result<(), String> {
    let body = format!(
        "{}\nnet start {} 2>$null | Out-Null\n\
         if ((& sc.exe query {} | Out-String) -match 'RUNNING') {{ exit 0 }} else {{ exit 1 }}",
        crate::runner::PS_HEADER,
        SERVICE_NAME,
        SERVICE_NAME
    );
    let script = crate::runner::LockedScript::write(
        data_dir.join("logs").join(format!("svc_start_{}.ps1", std::process::id())),
        &body,
    )?;
    let code = run_script_privileged(script.path())?;
    if code != 0 {
        return Err(crate::texts::service_start_failed_code(code));
    }
    Ok(())
}

pub(crate) fn build_service_cmdline(root: &Path, profile: &Profile, args: &[String]) -> String {
    let bin = crate::config::find_exe(root, profile.exe_name())
        .map(|rel| root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
        .unwrap_or_else(|| root.join("bin").join(profile.exe_name()));
    let mut s = format!("\"{}\"", bin.to_string_lossy());
    for a in args {
        s.push(' ');
        s.push_str(&quote_win_arg(a));
    }
    s
}

/// Квотирование аргумента для Windows-командной строки по правилам MS:
/// кавычки экранируются `\"`, а бэкслеши перед кавычкой — включая хвостовые
/// (`C:\dir\`) — удваиваются. Иначе закрывающая кавычка «съедала» бы хвостовой
/// бэкслеш и путь ломался.
pub(crate) fn quote_win_arg(a: &str) -> String {
    if !a.is_empty() && !a.chars().any(char::is_whitespace) && !a.contains('"') {
        return a.to_string();
    }
    let mut out = String::with_capacity(a.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for c in a.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                for _ in 0..(backslashes * 2 + 1) {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            _ => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(c);
            }
        }
    }
    for _ in 0..(backslashes * 2) {
        out.push('\\');
    }
    out.push('"');
    out
}

/// Экранирует строку для вставки в PowerShell-литерал в одинарных кавычках.
fn ps_single_quote(text: &str) -> String {
    text.replace('\'', "''")
}

pub fn install_service(root: &Path, profile: &Profile, args: &[String], data_dir: &Path) -> Result<(), String> {
    let cmdline = build_service_cmdline(root, profile, args);
    // ВАЖНО (почему не sc.exe): `sc` в PowerShell — алиас Set-Content, а передать
    // binPath с кавычками через нативную командную строку PS 5.1 надёжно нельзя.
    // New-Service принимает готовую строку как есть — кавычки и пробелы не ломаются.
    //
    // Перед New-Service добиваем любые движки (4 имени, политика A6): winws не
    // допускает второй экземпляр с тем же фильтром, и служба, стартуя поверх
    // уже запущенного winws, мгновенно падала с ошибкой SCM 7023 («служба падает
    // сама»). Основную работу делает stop_all_own на стороне Rust, это — пояс.
    // Ошибку самого Start-Service забираем текстом: SCM отдаёт её одной строкой
    // (дубль фильтра, драйвер, права), и «код 1» без причины ничего не объясняет.
    let err_file = data_dir.join("logs").join("svc_start_error.txt");
    let _ = std::fs::remove_file(&err_file);
    let body = format!(
        "{}\nStop-Service -Name '{}' -Force -ErrorAction SilentlyContinue\n& sc.exe delete {} 2>$null | Out-Null\nStart-Sleep -Milliseconds 600\ntry {{ taskkill /F /IM winws.exe /IM winws2.exe /IM goodbyedpi.exe /IM dpibreak.exe 2>$null | Out-Null }} catch {{}}\nStart-Sleep -Milliseconds 400\nNew-Service -Name '{}' -BinaryPathName '{}' -StartupType Automatic -Description 'Local traffic optimization helper (zgui)' | Out-Null\n$zguiErr = ''\ntry {{ Start-Service -Name '{}' }} catch {{ $zguiErr = $_.Exception.Message }}\nif ($zguiErr) {{ $zguiErr | Set-Content -Path {} -Encoding UTF8; exit 3 }}\n",
        crate::runner::PS_HEADER,
        SERVICE_NAME,
        SERVICE_NAME,
        SERVICE_NAME,
        ps_single_quote(&cmdline),
        SERVICE_NAME,
        ps_single_quote(&err_file.to_string_lossy()),
    );
    let script = crate::runner::LockedScript::write(
        data_dir.join("logs").join(format!("svc_install_{}.ps1", std::process::id())),
        &body,
    )?;
    let code = run_script_privileged(script.path()).map_err(|e| e.to_string())?;
    if code == 3 {
        let detail = std::fs::read_to_string(&err_file)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        crate::logger::log_code("err", "service", "E-SVC-005", &format!("Start-Service не удался: {detail}"));
        return Err(crate::texts::service_start_failed_detail(&detail));
    }
    if code != 0 {
        return Err(crate::texts::service_install_failed_code(code));
    }
    // Start-Service возвращает 0 ещё до того, как процесс службы реально устоится:
    // если winws вышел сразу (ошибка драйвера/дубль), SCM регистрирует 7023, а GUI
    // рапортовал бы «установлена». Ждём фактического RUNNING.
    let mut running = false;
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if service_state().1 {
            running = true;
            break;
        }
    }
    if !running {
        return Err(crate::texts::SERVICE_STARTED_THEN_DIED.into());
    }
    // Прямой вызов reg.exe: profile.id — внешние данные (имя .bat или OTA-пресета),
    // в `powershell -Command` он парсился бы как код (инъекция). argv безопасен.
    match hidden_command("reg.exe")
        .args([
            "add",
            "HKLM\\System\\CurrentControlSet\\Services\\zapret",
            "/v",
            "zgui-strategy",
            "/t",
            "REG_SZ",
            "/d",
            profile.id.as_str(),
            "/f",
        ])
        .output()
    {
        Ok(o) if !o.status.success() => {
            crate::logger::log(
                "err",
                "service",
                "служба установлена, но стратегию записать не удалось — выберите её заново",
            );
        }
        Err(_) => crate::logger::log(
            "err",
            "service",
            "служба установлена, но reg.exe недоступен — стратегия не записана",
        ),
        _ => {}
    }
    Ok(())
}

pub fn remove_service(data_dir: &Path) -> Result<(), String> {
    // Успех — «службы больше нет» (1060) или «помечена на удаление» (1072):
    // `sc delete` может вернуть «marked for deletion» ещё до фактического
    // снятия, и это не ошибка.
    let body = format!(
        "{}\nStop-Service -Name '{}' -Force -ErrorAction SilentlyContinue\n\
         & sc.exe delete {} 2>$null | Out-Null\nStart-Sleep -Milliseconds 500\n\
         & sc.exe query {} 2>$null | Out-Null\n\
         if ($LASTEXITCODE -eq 1060 -or $LASTEXITCODE -eq 1072) {{ exit 0 }} else {{ exit 1 }}",
        crate::runner::PS_HEADER,
        SERVICE_NAME,
        SERVICE_NAME,
        SERVICE_NAME
    );
    let script = crate::runner::LockedScript::write(
        data_dir.join("logs").join(format!("svc_remove_{}.ps1", std::process::id())),
        &body,
    )?;
    let code = run_script_privileged(script.path())?;
    if code != 0 {
        return Err(crate::texts::service_remove_failed_code(code));
    }
    Ok(())
}

/// Состояние RUNNING/START_PENDING по сырому выводу `sc query`.
/// Вынесено отдельно: на русской Windows заголовок «STATE» локализован,
/// а сами состояния всегда латиницей — тест фиксирует оба случая.
fn running_from_sc_output(txt: &str) -> bool {
    let t = txt.to_uppercase();
    t.contains("RUNNING") || t.contains("START_PENDING")
}

pub fn service_state() -> (bool, bool) {
    let out = hidden_command("sc.exe")
        .args(["query", SERVICE_NAME])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            // SCM печатает сами состояния латиницей (RUNNING/START_PENDING),
            // но заголовок строки локализован («СОСТОЯНИЕ» на русской Windows),
            // поэтому ищем только токены состояния. Проверка по слову "STATE"
            // давала вечное «служба остановлена» при живой службе — жалоба
            // «служба падает сама».
            let txt = String::from_utf8_lossy(&o.stdout).to_string();
            (true, running_from_sc_output(&txt))
        }
        _ => (false, false),
    }
}

/// Владеет ли служба движком прямо сейчас: RUNNING или START_PENDING.
/// В этом окне ЛЮБАЯ уборка (движки/драйверы) обязана отступить.
pub fn service_active() -> bool {
    let out = hidden_command("sc.exe")
        .args(["query", SERVICE_NAME])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let txt = String::from_utf8_lossy(&o.stdout).to_uppercase();
            txt.contains("RUNNING") || txt.contains("START_PENDING")
        }
        _ => false,
    }
}

/// Разбирает значение `zgui-strategy` из вывода `reg query`.
/// Строка вида `    zgui-strategy    REG_SZ    general (ALT)` — id может
/// содержать пробелы, поэтому режем ровно по типу, а не по whitespace
/// (иначе «general (ALT)» превращался в «(ALT)»).
fn parse_strategy_value(txt: &str) -> Option<String> {
    let line = txt.lines().find(|l| l.contains("zgui-strategy"))?;
    let (_, val) = line.split_once("REG_SZ")?;
    let val = val.trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

pub fn service_strategy(_data_dir: &Path) -> Option<String> {
    let out = hidden_command("reg.exe")
        .args([
            "query",
            "HKLM\\System\\CurrentControlSet\\Services\\zapret",
            "/v",
            "zgui-strategy",
        ])
        .output()
        .ok()?;
    let txt = String::from_utf8_lossy(&out.stdout);
    parse_strategy_value(&txt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_exe_list_covers_registry() {
        for d in crate::config::engines() {
            assert!(ENGINE_EXES.contains(&d.exe), "нет в конфликтах: {}", d.exe);
        }
    }

    #[test]
    fn sc_running_parses_localized_output() {
        // Русская Windows: заголовок «СОСТОЯНИЕ», состояние — латиницей.
        let ru = "ИМЯ_СЛУЖБЫ: zapret\r\n        ТИП               : 10  WIN32_OWN_PROCESS  \r\n        СОСТОЯНИЕ         : 4  RUNNING \r\n";
        assert!(running_from_sc_output(ru), "живая служба на русской локали");
        let ru_start = "        СОСТОЯНИЕ         : 2  START_PENDING \r\n";
        assert!(running_from_sc_output(ru_start), "поднимается — уже занята");
        let stopped = "        СОСТОЯНИЕ         : 1  STOPPED \r\n";
        assert!(!running_from_sc_output(stopped));
        assert!(!running_from_sc_output(""), "пустой вывод — не запущена");
    }

    #[test]
    fn strategy_value_keeps_spaces_and_semicolons() {
        // id с пробелами — раньше резался по whitespace («general (ALT)» → «(ALT)»).
        assert_eq!(
            parse_strategy_value("    zgui-strategy    REG_SZ    general (ALT)\r\n"),
            Some("general (ALT)".into())
        );
        // Внешне опасные символы остаются данными: reg.exe получает их argv.
        assert_eq!(
            parse_strategy_value("    zgui-strategy    REG_SZ    x; Start-Process calc"),
            Some("x; Start-Process calc".into())
        );
        assert_eq!(parse_strategy_value("    zgui-strategy    REG_SZ    "), None);
        assert_eq!(parse_strategy_value("нет такой строки"), None);
    }

    #[test]
    fn service_cmdline_quotes_spaces() {
        let p = Profile {
            id: "z2".into(),
            name: "z".into(),
            engine: crate::config::ENGINE_ZAPRET2.into(),
            args: vec!["--lua-init=@C:\\lua lib\\zapret-lib.lua".into()],
            builtin: true,
            source: None,
            updated_at: None,
        };
        let c = build_service_cmdline(Path::new(r"D:\e"), &p, &p.args);
        assert!(c.contains("winws2.exe"));
        assert!(c.contains("\"--lua-init=@C:\\lua lib\\zapret-lib.lua\""));
    }

    #[test]
    fn quote_win_arg_doubles_trailing_backslash() {
        assert_eq!(quote_win_arg("plain"), "plain");
        // Без пробелов/кавычек — как есть.
        assert_eq!(quote_win_arg(r"C:\dir\"), r"C:\dir\");
        // Хвостовой `\` перед закрывающей кавычкой удваивается.
        assert_eq!(quote_win_arg(r"C:\my dir\"), r#""C:\my dir\\""#);
        // Кавычка внутри — \", бэкслеши перед ней удваиваются.
        assert_eq!(quote_win_arg(r#"--x="q""#), r#""--x=\"q\"""#);
    }

    #[test]
    fn kill_engines_script_is_ascii_and_total() {
        let body = kill_engines_script();
        assert!(body.is_ascii(), "скрипт должен остаться ASCII-only: {body}");
        // Тотально: все 4 имени процессов + службы по ImagePath (любое имя).
        for n in ["winws.exe", "winws2.exe", "goodbyedpi.exe", "dpibreak.exe"] {
            assert!(body.contains(n), "{n}");
        }
        assert!(body.contains("taskkill /F /T /IM"));
        assert!(body.contains("HKLM:\\SYSTEM\\CurrentControlSet\\Services"));
        assert!(body.contains("sc.exe stop"));
        // Синтаксис PowerShell — реальным парсером.
        let path = std::env::temp_dir().join(format!("zgui-sweep-{}.ps1", std::process::id()));
        crate::runner::write_ps1(&path, &body).unwrap();
        let check = format!(
            "$t=$null; $e=$null; [void][System.Management.Automation.Language.Parser]::ParseFile({}, [ref]$t, [ref]$e); if ($e.Count) {{ $e[0].Message; exit 1 }} else {{ exit 0 }}",
            crate::runner::ps_quote(&path.to_string_lossy())
        );
        let out = hidden_command("powershell.exe")
            .args(["-NoProfile", "-Command", &check])
            .output()
            .expect("powershell");
        let _ = std::fs::remove_file(&path);
        assert!(
            out.status.success(),
            "синтаксис скрипта: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn detects_vpn_process_names() {
        assert!(is_vpn_process("Happ.exe"));
        assert!(is_vpn_process("happd.exe"));
        assert!(is_vpn_process("wireguard.exe"));
        assert!(is_vpn_process("AmneziaVPN.exe"));
        assert!(is_vpn_process("sing-box.exe"));
        assert!(is_vpn_process("clash-verge.exe"));
        assert!(!is_vpn_process("winws.exe"));
        assert!(!is_vpn_process("chrome.exe"));
        assert!(!is_vpn_process("explorer.exe"));
    }

    #[test]
    fn build_cmdline_quotes_spaces() {
        let p = Profile {
            id: "p".into(),
            name: "p".into(),
            engine: crate::config::ENGINE_FLOWSEAL.into(),
            args: vec!["--filter-tcp=80 --new".into()],
            builtin: false,
            source: None,
            updated_at: None,
        };
        let c = build_service_cmdline(Path::new("C:\\z"), &p, &p.args);
        assert!(c.contains("winws.exe"));
        assert!(c.contains("\"--filter-tcp=80 --new\""));
    }

    #[test]
    fn tasklist_csv_parses_names_with_commas() {
        let (name, pid) =
            tasklist_name_pid(r#""my,app.exe","1234","Console","1","12,345 К""#).expect("строка должна разобраться");
        assert_eq!(name, "my,app.exe");
        assert_eq!(pid, 1234);
        assert!(tasklist_name_pid("ИНФО: нет задач для заданных критериев.").is_none());
        // Экранированные кавычки внутри имени тоже не ломают разбор.
        let (n2, p2) = tasklist_name_pid(r#""odd""name.exe","7","Console","1","1 К""#).unwrap();
        assert_eq!((n2.as_str(), p2), ("odd\"\"name.exe", 7));
    }

    #[test]
    fn build_cmdline_finds_nested_exe() {
        // Пользователь может указать корень-обёртку (распакованный релиз с каталогом
        // внутри) — exe ищется рекурсивно, иначе служба ссылалась бы в пустоту.
        let root = std::env::temp_dir().join(format!("zgui-svc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("zapret-discord-youtube-1.10.2").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("winws.exe"), b"").unwrap();
        let p = Profile {
            id: "g".into(),
            name: "g".into(),
            engine: crate::config::ENGINE_FLOWSEAL.into(),
            args: vec!["--wf-tcp-out=443".into()],
            builtin: false,
            source: None,
            updated_at: None,
        };
        let c = build_service_cmdline(&root, &p, &p.args);
        assert!(c.contains("zapret-discord-youtube-1.10.2"));
        assert!(c.contains("winws.exe"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn layout_engine_id_matches_any_copy_only_for_our_layout() {
        assert_eq!(
            layout_engine_id(r"\??\E:\Z GUI\target\release\data\engines\flowseal\bin\winws.exe"),
            Some("flowseal")
        );
        assert_eq!(layout_engine_id(r"D:\ZGUI 2\data\engines\ZAPRET2\winws2.exe"), Some("zapret2"));
        assert_eq!(layout_engine_id(r"E:\mydata\engines\flowseal\winws.exe"), None);
        assert_eq!(layout_engine_id(r"C:\Windows\System32\drivers\windivert.sys"), None);
    }
}
