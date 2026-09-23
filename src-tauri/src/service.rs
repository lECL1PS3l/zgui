use crate::config::{Profile, SERVICE_NAME};
use crate::runner::{hidden_command, run_powershell, run_script_privileged};
use std::collections::HashMap;
use std::fs;
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
];

/// Перечисляет процессы из tasklist, отфильтрованные по предикату имени.
fn list_processes(mut want: impl FnMut(&str) -> bool) -> Vec<(String, u32)> {
    let out = hidden_command("tasklist.exe")
        .args(["/FO", "CSV", "/NH"])
        .output();
    let Ok(o) = out else { return Vec::new() };
    let txt = String::from_utf8_lossy(&o.stdout);
    let mut found = Vec::new();
    for line in txt.lines() {
        let cols: Vec<&str> = line.split("\",\"").collect();
        if cols.len() < 2 {
            continue;
        }
        let name = cols[0].trim_matches('"').to_string();
        let Ok(pid) = cols[1].trim_matches('"').parse::<u32>() else { continue };
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
            note: "VPN/прокси — конфликтует с zapret".into(),
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
                    note: "VPN-служба запущена".into(),
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
    report.own_service = strategy.is_some();
    report.foreign_service = installed && strategy.is_none();
    if report.foreign_service {
        report
            .processes
            .push(ConflictProcess {
                pid: 0,
                name: format!("service:{}", SERVICE_NAME),
                note: if running {
                    "служба zapret (не от нашего GUI) запущена".into()
                } else {
                    "служба zapret (не от нашего GUI) установлена".into()
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
            let cols: Vec<&str> = line.split("\",\"").collect();
            if cols.len() < 2 {
                continue;
            }
            let name = cols[0].trim_matches('"').to_string();
            let Ok(pid) = cols[1].trim_matches('"').parse::<u32>() else { continue };
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
                note: "чужой процесс zapret (не запущен нашим GUI)".into(),
            });
        }
    }

    report.vpn = detect_vpn();

    if !report.processes.is_empty() || !report.vpn.is_empty() {
        report.message = "Обнаружено конфликтующее ПО".into();
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

/// Запускает установленную службу zapret (нужна после теста, который её глушил).
pub fn start_service(data_dir: &Path) -> Result<(), String> {
    let script = data_dir.join("logs").join(format!("svc_start_{}.ps1", std::process::id()));
    let body = format!(
        "{}\nnet start {} 2>$null | Out-Null\nexit 0",
        crate::runner::PS_HEADER,
        SERVICE_NAME
    );
    crate::runner::write_ps1(&script, &body)?;
    let r = run_script_privileged(&script);
    let _ = fs::remove_file(&script);
    r.map(|_| ())
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
    // Сначала ОСТАНАВЛИВАЕМ службы и ждём — иначе служба перезапустит свой процесс
    // (например, AmneziaVPN-service возрождает AmneziaVPN-service.exe после taskkill).
    for svc in &vpn_services {
        body.push_str(&format!(
            "Stop-Service -Name '{}' -Force -ErrorAction SilentlyContinue\n\
             sc.exe stop '{}' 2>$null | Out-Null\n",
            ps_single_quote(svc),
            ps_single_quote(svc)
        ));
    }
    // Службы, чей путь ведёт к найденным VPN-процессам: массив + `-contains`.
    if !names.is_empty() {
        let arr = names
            .iter()
            .map(|n| format!("'{}'", ps_single_quote(n.trim_end_matches(".exe").to_lowercase().as_str())))
            .collect::<Vec<_>>()
            .join(",");
        body.push_str(&format!(
            "$zguiNames = @({arr})\n\
             Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | ForEach-Object {{ \
               $leaf = [System.IO.Path]::GetFileNameWithoutExtension((($_.PathName -replace '\"','') -split ' ')[0]); \
               if ($leaf -and ($zguiNames -contains $leaf.ToLower())) {{ \
                 Stop-Service -Name $_.Name -Force -ErrorAction SilentlyContinue; \
                 sc.exe stop $_.Name 2>$null | Out-Null; sc.exe delete $_.Name 2>$null | Out-Null }} }}\n"
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
    // Убиваем по имени (все копии) массивом, затем по PID (остатки/деревья).
    if !names.is_empty() {
        let arr = names
            .iter()
            .map(|n| format!("'{}'", ps_single_quote(n)))
            .collect::<Vec<_>>()
            .join(",");
        body.push_str(&format!(
            "$zguiKill = @({arr})\n\
             foreach ($n in $zguiKill) {{ taskkill /F /T /IM $n 2>$null | Out-Null }}\n"
        ));
    }
    for pid in &pids {
        body.push_str(&format!("taskkill /F /T /PID {} 2>$null | Out-Null\n", pid));
    }
    body.push_str("exit 0\n");
    Some((body, names))
}

/// Гасит чужие процессы и VPN «намертво» (taskkill по PID и по имени, стоп служб).
/// Свои pid-ы не трогает. Возвращает список имён, по которым выдана команда
/// завершения (для итогового уведомления в UI).
pub fn kill_conflicts(report: &ConflictReport, our_pid: Option<u32>, data_dir: &Path) -> Result<Vec<String>, String> {
    let Some((body, names)) = build_kill_script(report, our_pid) else {
        return Ok(Vec::new());
    };
    let script = data_dir
        .join("logs")
        .join(format!("conflict_kill_{}.ps1", std::process::id()));
    crate::runner::write_ps1(&script, &body)?;
    let r = run_script_privileged(&script);
    let _ = fs::remove_file(&script);
    r.map(|_| names)
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
        if a.contains(' ') {
            s.push_str(" \"");
            s.push_str(a);
            s.push('"');
        } else {
            s.push(' ');
            s.push_str(a);
        }
    }
    s
}

/// Экранирует строку для вставки в PowerShell-литерал в одинарных кавычках.
fn ps_single_quote(text: &str) -> String {
    text.replace('\'', "''")
}

pub fn install_service(root: &Path, profile: &Profile, args: &[String], data_dir: &Path) -> Result<(), String> {
    let cmdline = build_service_cmdline(root, profile, args);
    let script = data_dir.join("logs").join(format!("svc_install_{}.ps1", std::process::id()));
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
    crate::runner::write_ps1(&script, &body)?;
    let code = run_script_privileged(&script).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&script);
    if code != 0 {
        return Err(format!("установка службы завершилась с кодом {}", code));
    }
    run_powershell(&[
        "reg".into(),
        "add".into(),
        "HKLM\\System\\CurrentControlSet\\Services\\zapret".into(),
        "/v".into(),
        "zgui-strategy".into(),
        "/t".into(),
        "REG_SZ".into(),
        "/d".into(),
        profile.id.clone(),
        "/f".into(),
    ])
    .ok();
    Ok(())
}

pub fn remove_service(data_dir: &Path) -> Result<(), String> {
    let script = data_dir.join("logs").join(format!("svc_remove_{}.ps1", std::process::id()));
    let body = format!(
        "{}\nStop-Service -Name '{}' -Force -ErrorAction SilentlyContinue\n& sc.exe delete {} 2>$null | Out-Null\nexit 0",
        crate::runner::PS_HEADER,
        SERVICE_NAME,
        SERVICE_NAME
    );
    crate::runner::write_ps1(&script, &body).map_err(|e| e.to_string())?;
    let r = run_script_privileged(&script);
    let _ = fs::remove_file(&script);
    r.map(|_| ())
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
    let line = txt.lines().find(|l| l.contains("zgui-strategy"))?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    parts.last().map(|s| s.to_string())
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
}
