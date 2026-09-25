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

#[derive(serde::Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    pub processes: Vec<ConflictProcess>,
    pub vpn: Vec<ConflictProcess>,
    pub foreign_service: bool,
    pub own_service: bool,
    pub message: String,
}

impl ConflictReport {
    pub fn has_conflicts(&self) -> bool {
        !self.processes.is_empty() || !self.vpn.is_empty() || self.foreign_service
    }
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

/// Ищет чужие процессы winws.exe/winws2.exe (не наши), чужую службу `zapret` и VPN.
/// Все exe реестра движков (конфликт-чек и остановка внешнего обхода).
pub const ENGINE_EXES: [&str; 4] = ["winws.exe", "winws2.exe", "goodbyedpi.exe", "dpibreak.exe"];

pub fn detect_conflicts(data_dir: &Path, our_pid: Option<u32>) -> ConflictReport {
    let mut report = ConflictReport::default();

    let (installed, running) = service_state();
    let strategy = service_strategy(data_dir);
    // Наша служба: есть наш reg-ключ стратегии ИЛИ её ImagePath указывает в наш
    // layout data\engines\ (служба, поставленная прошлой копией GUI — тогда
    // ключа может не быть, и её нельзя считать «чужой»).
    let own = strategy.is_some() || service_image_is_ours();
    report.own_service = own;
    report.foreign_service = installed && !own;
    if report.foreign_service {
        report
            .processes
            .push(ConflictProcess {
                pid: 0,
                name: format!("service:{}", SERVICE_NAME),
                note: if running {
                    crate::texts::FOREIGN_SERVICE_RUNNING.into()
                } else {
                    crate::texts::FOREIGN_SERVICE_INSTALLED.into()
                },
            });
    }

    // Пути процессов winws/winws2 через WMI (tasklist не отдаёт путь к exe).
    // Свои экземпляры — из папки движка под <data> — не считаем чужими, даже если
    // их подняла наша служба zapret или UAC-раннер (тогда PID не совпадает с GUI).
    let image_paths = process_image_paths(&ENGINE_EXES);

    for exe in ENGINE_EXES {
        let out = hidden_command("tasklist.exe")
            .args(["/FI", &format!("IMAGENAME eq {}", exe), "/FO", "CSV", "/NH"])
            .output();
        let Ok(o) = out else { continue };
        let txt = String::from_utf8_lossy(&o.stdout);
        for line in txt.lines() {
            let Some((name, pid)) = tasklist_name_pid(line) else { continue };
            if Some(pid) == our_pid {
                continue;
            }
            let own_path = image_paths
                .get(&pid)
                .map(|p| is_own_engine(p, data_dir))
                .unwrap_or(false);
            // Путь не смогли узнать (нет прав на WMI), но наша служба установлена и
            // запущена — почти наверняка это winws службы, трогать его нельзя.
            let assumed_service = report.own_service && running && !image_paths.contains_key(&pid);
            if own_path || assumed_service {
                continue;
            }
            report.processes.push(ConflictProcess {
                pid,
                name: name.clone(),
                note: crate::texts::FOREIGN_ENGINE_PROCESS.into(),
            });
        }
    }

    report.vpn = detect_vpn();

    if !report.processes.is_empty() || !report.vpn.is_empty() {
        report.message = crate::texts::CONFLICTS_FOUND.into();
    }
    report
}

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

/// Свой ли движок: exe лежит под папкой данных программы (data/engines/...).
/// Сравнение регистронезависимое; префикс с разделителем, чтобы `data` не
/// совпало с чужим `data2`.
fn is_own_engine(path: &Path, data_dir: &Path) -> bool {
    let p = path.to_string_lossy().to_lowercase();
    let d = data_dir.to_string_lossy().to_lowercase();
    if d.is_empty() {
        return false;
    }
    let prefix = if d.ends_with('\\') { d } else { format!("{}\\", d) };
    p.starts_with(&prefix)
}

/// PID-ы winws/winws2, запущенных из наших данных (движок GUI или нашей службы).
/// Нужны, чтобы находить обход, поднятый ВНЕ программы (ручной запуск .bat,
/// старая служба): GUI его не видит и может запустить второй winws — два
/// фильтра WinDivert дерутся, и обход перестаёт работать.
pub fn own_engine_pids(data_dir: &Path, exclude: Option<u32>) -> Vec<u32> {
    let mut pids: Vec<u32> = process_image_paths(&ENGINE_EXES)
        .into_iter()
        .filter(|(pid, p)| Some(*pid) != exclude && is_own_engine(p, data_dir))
        .map(|(pid, _)| pid)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// Дешёвая проверка: запущен ли хоть один exe движков (без WMI).
pub fn any_winws_running() -> bool {
    !list_processes(|n| ENGINE_EXES.iter().any(|e| n.eq_ignore_ascii_case(e)))
    .is_empty()
}

/// Идентификатор нашего движка, если путь лежит в раскладке программы
/// (`...\data\engines\<движок>\...`). Ловит любую копию GUI, не только текущую.
fn layout_engine_id(path: &str) -> Option<&'static str> {
    let p = path.to_lowercase().replace('/', "\\");
    let (_, tail) = p.split_once("\\data\\engines\\")?;
    let id = tail.split('\\').next()?;
    crate::config::engine_ids().into_iter().find(|e| e.eq_ignore_ascii_case(id))
}

/// Наш ли драйвер: файл `WinDivert*.sys` из раскладки программы — включая
/// копии GUI в других папках. Чужие WinDivert (System32, другие программы)
/// не трогаем.
pub fn is_own_windivert_path(path: &str) -> bool {
    let p = path.to_lowercase().replace('/', "\\");
    let file = p.rsplit('\\').next().unwrap_or("");
    file.starts_with("windivert") && file.ends_with(".sys") && layout_engine_id(&p).is_some()
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

/// Гасит наши зависшие драйверы WinDivert (из комплектов движков), если ни
/// один движок сейчас не запущен (иначе фильтр занят законно). Возвращает
/// имена убранных служб; детали — в журнале.
pub fn cleanup_own_stray_drivers() -> Vec<String> {
    if our_engines_running() {
        return Vec::new();
    }
    let mut cleaned = Vec::new();
    for (name, state, path) in windivert_driver_services() {
        if !state.eq_ignore_ascii_case("running") || !is_own_windivert_path(&path) {
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
            crate::logger::log(
                "warn",
                "windivert",
                &format!("зависший драйвер {name} не убран — нужны права администратора"),
            );
        } else {
            crate::logger::log("warn", "windivert", &format!("убран зависший драйвер: {name} ({path})"));
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

/// Строит PowerShell-скрипт выгрузки конфликтов и список имён, по которым выдана
/// команда завершения. `None` — выгружать нечего. Имена процессов приходят из
/// tasklist (их задаёт внешний exe), поэтому они НЕ подставляются в regex или
/// командную строку как есть — только массивом с экранированием одинарных кавычек
/// (иначе имя вида `x'-...` = инъекция в админский скрипт).
pub(crate) fn build_kill_script(report: &ConflictReport, our_pid: Option<u32>) -> Option<(String, Vec<String>)> {
    let mut pids: Vec<u32> = report
        .processes
        .iter()
        .chain(report.vpn.iter())
        .filter(|p| p.pid > 0 && Some(p.pid) != our_pid)
        .map(|p| p.pid)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    let foreign_service = report.foreign_service;
    let vpn_services: Vec<String> = report
        .vpn
        .iter()
        .filter(|p| p.pid == 0)
        .filter_map(|p| p.name.strip_prefix("service:").map(|s| s.to_string()))
        .collect();
    let mut names: Vec<String> = report
        .processes
        .iter()
        .chain(report.vpn.iter())
        .filter(|p| p.pid > 0 && !p.name.is_empty() && !p.name.starts_with("service:"))
        .map(|p| p.name.clone())
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();

    if pids.is_empty() && !foreign_service && vpn_services.is_empty() && names.is_empty() {
        return None;
    }

    let mut body = format!("{}\n", crate::runner::PS_HEADER);
    body.push_str("$zguiFailed = 0\n");
    // Сначала ОСТАНАВЛИВАЕМ службы и ждём — иначе служба перезапустит свой процесс
    // (например, AmneziaVPN-service возрождает AmneziaVPN-service.exe после taskkill).
    // НЕ отключаем и НЕ удаляем: у VPN-клиентов (Happ, Amnezia и др.) служба —
    // их привилегированный компонент для TUN/DNS; GUI поднимает её сам при
    // следующем подключении, а удалённую восстановить без переустановки не может.
    for svc in &vpn_services {
        body.push_str(&format!(
            "Stop-Service -Name '{}' -Force -ErrorAction SilentlyContinue\n\
             sc.exe stop '{}' 2>$null | Out-Null\n",
            ps_single_quote(svc),
            ps_single_quote(svc)
        ));
    }
    // Службы, чей путь ведёт к найденным VPN-процессам: массив + `-contains`.
    // Только ОСТАНАВЛИВАЕМ (без `Set Disabled`/`sc delete`); их exe запоминаем
    // в `$zguiSvcExes`, чтобы ниже не добивать процессы служб taskkill-ом —
    // штатный stop даёт службе прибрать TUN-адаптер и маршруты, а жёсткое
    // убийство оставляет их висеть и (у Happ) ломает запуск VPN до переустановки.
    if !names.is_empty() {
        let arr = names
            .iter()
            .map(|n| format!("'{}'", ps_single_quote(n.trim_end_matches(".exe").to_lowercase().as_str())))
            .collect::<Vec<_>>()
            .join(",");
        body.push_str(&format!(
            "$zguiNames = @({arr})\n\
             $zguiSvcExes = @()\n\
             Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | ForEach-Object {{ \
               $m = [regex]::Match([string]$_.PathName, '^\\s*\"([^\"]+)\"'); \
               $exe = if ($m.Success) {{ $m.Groups[1].Value }} elseif ($_.PathName) {{ ($_.PathName -split '\\s+')[0] }} else {{ '' }}; \
               $leaf = [System.IO.Path]::GetFileNameWithoutExtension($exe); \
               if ($leaf -and ($zguiNames -contains $leaf.ToLower())) {{ \
                 $zguiSvcExes += $leaf.ToLower(); \
                 Stop-Service -Name $_.Name -Force -ErrorAction SilentlyContinue; \
                 sc.exe stop $_.Name 2>$null | Out-Null }} }}\n"
        ));
    }
    if !vpn_services.is_empty() || !names.is_empty() {
        body.push_str("Start-Sleep -Seconds 2\n");
    }
    if foreign_service {
        // ВАЖНО: `sc` в PowerShell 5.1 — это алиас Set-Content, а не sc.exe:
        // `sc delete zapret` молча писал файл и НИЧЕГО не удалял. Только `sc.exe`.
        body.push_str(&format!(
            "net stop {} 2>$null | Out-Null\n& sc.exe delete {} 2>$null | Out-Null\n",
            SERVICE_NAME, SERVICE_NAME
        ));
    }
    // Убиваем по имени (все копии) массивом, с повтором (демон может успеть
    // возродить процесс), затем по PID. Службы к этому моменту остановлены,
    // их процессы не добиваем (см. $zguiSvcExes выше).
    if !names.is_empty() {
        let arr = names
            .iter()
            .map(|n| format!("'{}'", ps_single_quote(n)))
            .collect::<Vec<_>>()
            .join(",");
        body.push_str(&format!(
            "$zguiKill = @({arr})\n\
             for ($i = 0; $i -lt 2; $i++) {{ \
               foreach ($n in $zguiKill) {{ \
                 $leaf = [System.IO.Path]::GetFileNameWithoutExtension($n).ToLower(); \
                 if ($zguiSvcExes -contains $leaf) {{ continue }}; \
                 taskkill /F /T /IM $n 2>$null | Out-Null }}; \
               Start-Sleep -Milliseconds 700 }}\n\
             # Фоллбэк: taskkill может получить отказ у завершающегося процесса —\
             # добиваем через Stop-Process по имени.\n\
             foreach ($n in $zguiKill) {{ \
               $leaf = [System.IO.Path]::GetFileNameWithoutExtension($n).ToLower(); \
               if ($zguiSvcExes -contains $leaf) {{ continue }}; \
               $pn = [System.IO.Path]::GetFileNameWithoutExtension($n); \
               Get-Process -Name $pn -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue }}\n"
        ));
    }
    for pid in &pids {
        body.push_str(&format!(
            "taskkill /F /T /PID {pid} 2>$null | Out-Null\n\
             if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ Stop-Process -Id {pid} -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 400 }}\n\
             if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ $zguiFailed++ }}\n"
        ));
    }
    body.push_str("if ($zguiFailed -gt 0) { exit 1 } else { exit 0 }\n");
    Some((body, names))
}

/// Гасит чужие процессы и VPN «намертво» (taskkill по PID и по имени, стоп служб).
/// Свои pid-ы не трогает. Возвращает список имён, по которым выдана команда
/// завершения (для итогового уведомления в UI).
pub fn kill_conflicts(report: &ConflictReport, our_pid: Option<u32>, data_dir: &Path) -> Result<Vec<String>, String> {
    let Some((body, names)) = build_kill_script(report, our_pid) else {
        return Ok(Vec::new());
    };
    let script = crate::runner::LockedScript::write(
        data_dir.join("logs").join(format!("conflict_kill_{}.ps1", std::process::id())),
        &body,
    )?;
    let code = run_script_privileged(script.path())?;
    if code != 0 {
        // Часть процессов устояла: не выдаём это за полный успех, но и не
        // срываем повтор действия — остаток UI покажет через conflict_check.
        crate::logger::log(
            "warn",
            "conflict",
            &format!("часть процессов не закрылась (код {code})"),
        );
    }
    Ok(names)
}

/// Строит командную строку службы: "C:\...\winws.exe" --arg "v" ...
/// exe ищется так же, как при обычном запуске, — рекурсивно от корня:
/// пользователь мог указать каталог-обёртку распакованного релиза.
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
    let body = format!(
        "{}\nStop-Service -Name '{}' -Force -ErrorAction SilentlyContinue\n& sc.exe delete {} 2>$null | Out-Null\nStart-Sleep -Milliseconds 600\nNew-Service -Name '{}' -BinaryPathName '{}' -StartupType Automatic -Description 'Zapret DPI bypass software (zgui)' | Out-Null\nStart-Service -Name '{}'\n",
        crate::runner::PS_HEADER,
        SERVICE_NAME,
        SERVICE_NAME,
        SERVICE_NAME,
        ps_single_quote(&cmdline),
        SERVICE_NAME
    );
    let script = crate::runner::LockedScript::write(
        data_dir.join("logs").join(format!("svc_install_{}.ps1", std::process::id())),
        &body,
    )?;
    let code = run_script_privileged(script.path()).map_err(|e| e.to_string())?;
    if code != 0 {
        return Err(crate::texts::service_install_failed_code(code));
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

/// Служба `zapret` с путём в наш layout (`...\data\engines\...`) — наша, даже
/// если reg-ключ стратегии отсутствует (осталась от прошлой копии GUI).
fn service_image_is_ours() -> bool {
    let out = hidden_command("reg.exe")
        .args([
            "query",
            r"HKLM\System\CurrentControlSet\Services\zapret",
            "/v",
            "ImagePath",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let txt = String::from_utf8_lossy(&o.stdout).to_lowercase();
            txt.contains(r"\data\engines\")
        }
        _ => false,
    }
}

pub fn service_state() -> (bool, bool) {
    let out = hidden_command("sc.exe")
        .args(["query", SERVICE_NAME])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let txt = String::from_utf8_lossy(&o.stdout).to_uppercase();
            let running = txt.contains("STATE") && txt.contains("RUNNING");
            (true, running)
        }
        _ => (false, false),
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
    fn conflict_report_flags() {
        let mut r = ConflictReport::default();
        assert!(!r.has_conflicts());
        r.processes.push(ConflictProcess { pid: 42, name: "winws.exe".into(), note: "x".into() });
        assert!(r.has_conflicts());
        let r2 = ConflictReport { foreign_service: true, ..Default::default() };
        assert!(r2.has_conflicts());
    }

    #[test]
    fn own_engine_recognised_by_data_prefix() {
        let data = Path::new("D:\\Zapret\\data");
        assert!(is_own_engine(Path::new("D:\\Zapret\\data\\engines\\flowseal\\bin\\winws.exe"), data));
        assert!(is_own_engine(Path::new("d:\\zapret\\DATA\\engines\\flowseal\\WINWS.EXE"), data));
        // Чужие папки и «похожие» префиксы — чужие.
        assert!(!is_own_engine(Path::new("D:\\Zapret\\data2\\winws.exe"), data));
        assert!(!is_own_engine(Path::new("D:\\Zapret\\data-x\\bin\\winws.exe"), data));
        assert!(!is_own_engine(Path::new("C:\\Users\\x\\Downloads\\zapret\\bin\\winws.exe"), data));
        assert!(!is_own_engine(Path::new("C:\\winws.exe"), Path::new("")));
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
    fn vpn_report_flags() {
        let r = ConflictReport {
            vpn: vec![ConflictProcess { pid: 7, name: "Happ.exe".into(), note: "vpn".into() }],
            ..Default::default()
        };
        assert!(r.has_conflicts());
        assert!(!r.vpn.is_empty());
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
    fn kill_script_escapes_process_names() {
        // Имя exe задаёт внешний файл: кавычка в имени не должна ломать
        // админский PowerShell-скрипт (инъекция).
        let malicious = "evil'-x.exe".to_string();
        let report = ConflictReport {
            vpn: vec![ConflictProcess { pid: 4242, name: malicious.clone(), note: "vpn".into() }],
            ..Default::default()
        };
        let (body, names) = build_kill_script(&report, None).expect("должен быть скрипт");
        assert_eq!(names, vec![malicious.clone()], "список имён для UI — как есть");
        assert!(body.contains("evil''-x.exe"), "кавычка в имени должна быть удвоена: {body}");
        assert!(!body.contains("'evil'-x"), "неэкранированной кавычки быть не должно");
    }

    #[test]
    fn kill_script_reads_pathname_quoted() {
        // Путь службы с пробелом: имя exe берём из кавычек, а не из первого
        // слова строки (иначе "C:\Program Files\..." → "C:\Program").
        let report = ConflictReport {
            vpn: vec![ConflictProcess { pid: 1, name: "app.exe".into(), note: "vpn".into() }],
            ..Default::default()
        };
        let (body, _) = build_kill_script(&report, None).expect("должен быть скрипт");
        assert!(body.contains(r#"[regex]::Match([string]$_.PathName, '^\s*"([^"]+)"')"#), "разбор PathName по кавычкам: {body}");
    }

    #[test]
    fn kill_script_keeps_services_stopped_not_deleted() {
        // Службы VPN-клиентов (HappService и др.) только останавливаем:
        // `Set Disabled`/`sc delete` ломали клиента до переустановки, а exe
        // службы не должен добиваться taskkill-ом — процесс гасит сам SCM.
        let report = ConflictReport {
            vpn: vec![
                ConflictProcess { pid: 7, name: "Happ.exe".into(), note: String::new() },
                ConflictProcess { pid: 8, name: "happd.exe".into(), note: String::new() },
            ],
            ..Default::default()
        };
        let (body, _) = build_kill_script(&report, None).expect("должен быть скрипт");
        assert!(!body.contains("StartupType Disabled"), "службу нельзя отключать: {body}");
        assert!(!body.contains("sc.exe delete"), "службу нельзя удалять: {body}");
        assert!(body.contains("Stop-Service"), "службу нужно останавливать: {body}");
        assert!(body.contains("$zguiSvcExes"), "exe служб нужно запоминать: {body}");
        assert!(
            body.contains("if ($zguiSvcExes -contains $leaf)"),
            "процессы остановленных служб не добиваем: {body}"
        );
    }

    #[test]
    fn kill_script_is_valid_powershell() {
        // Скрипт клеится из строк с regex и экранированием — проверяем, что
        // PowerShell его реально разбирает (без выполнения).
        let report = ConflictReport {
            processes: vec![ConflictProcess { pid: 42, name: "evil'-x.exe".into(), note: String::new() }],
            vpn: vec![ConflictProcess { pid: 7, name: "Happ.exe".into(), note: String::new() }],
            foreign_service: true,
            ..Default::default()
        };
        let (body, _) = build_kill_script(&report, None).expect("должен быть скрипт");
        let path = std::env::temp_dir().join(format!("zgui-kill-parse-{}.ps1", std::process::id()));
        crate::runner::write_ps1(&path, &body).unwrap();
        let script = format!(
            "$t=$null; $e=$null; [void][System.Management.Automation.Language.Parser]::ParseFile({}, [ref]$t, [ref]$e); if ($e.Count) {{ $e[0].Message; exit 1 }} else {{ exit 0 }}",
            crate::runner::ps_quote(&path.to_string_lossy())
        );
        let out = hidden_command("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .expect("powershell");
        let _ = std::fs::remove_file(&path);
        assert!(
            out.status.success(),
            "скрипт не разбирается PowerShell: {}",
            String::from_utf8_lossy(&out.stdout)
        );
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

    #[test]
    fn own_windivert_path_detects_our_copies_not_foreign() {
        // Наша раскладка: любая копия программы, включая старые сборки.
        assert!(is_own_windivert_path(
            r"\??\E:\Base\opencode\Z GUI\src-tauri\target\release\data\engines\flowseal\zapret-discord-youtube-1.10.2\bin\WinDivert64.sys"
        ));
        assert!(is_own_windivert_path(
            r"E:\Base\ZGUI_Stable_24.09.2026_15-36\data\engines\goodbyedpi\WinDivert64.sys"
        ));
        assert!(is_own_windivert_path(r"D:\ZGUI-copy\data\engines\dpibreak\WinDivert.sys"));
        assert!(is_own_windivert_path(r"x:/tmp/Z GUI 2/data/engines/zapret2/bin/windivert64.sys"));
        // Чужие драйверы не трогаем.
        assert!(!is_own_windivert_path(r"\SystemRoot\System32\drivers\WinDivert.sys"));
        assert!(!is_own_windivert_path(r"C:\Program Files\FlyFrogLLC\Happ\windivert.sys"));
        assert!(!is_own_windivert_path(r"E:\tools\data\engines\unknown\WinDivert64.sys"));
        assert!(!is_own_windivert_path(
            r"E:\Base\opencode\Z GUI\src-tauri\target\release\data\engines\flowseal\bin\fake.sys"
        ));
    }
}
