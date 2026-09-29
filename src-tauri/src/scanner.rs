//! Диагностика сервиса: замер цели с обходом и без, вердикт и рекомендация.
//!
//! Модуль аддитивный: существующую логику движка/службы/теста не меняет.

/// Значимые строки пользовательского include (`ipset-all-user.txt`):
/// комментарии (`#`) и пустые строки игнорируются.
pub fn read_user_include(path: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PhaseMeasure {
    pub ok: bool,
    pub ms: u64,
    pub detail: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    NoEffect,
    Collateral,
    Covered,
    NotCovered,
    Unrelated,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Recommendation {
    pub file: Option<String>,
    pub lines: Vec<String>,
    pub note: String,
}

fn none_note(note: &str) -> Recommendation {
    Recommendation { file: None, lines: Vec::new(), note: note.to_string() }
}

/// Нормализует URL/домен в host (без схемы/пути/порта, нижний регистр).
pub fn parse_site_target(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let s = s.split(['/', '?', '#']).next().unwrap_or(s);
    let host = s
        .rsplit('@')
        .next()
        .unwrap_or(s)
        .split(':')
        .next()
        .unwrap_or(s)
        .to_ascii_lowercase();
    if host.is_empty()
        || !host.contains('.')
        || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return None;
    }
    Some(host)
}

fn ipv4_u32(ip: &str) -> Option<u32> {
    let o: Vec<u32> = ip.trim().split('.').map(|p| p.parse().ok()).collect::<Option<Vec<_>>>()?;
    if o.len() != 4 || o.iter().any(|x| *x > 255) {
        return None;
    }
    Some((o[0] << 24) | (o[1] << 16) | (o[2] << 8) | o[3])
}

/// Входит ли IPv4-адрес в любой диапазон `ipset-all.txt` (строки `ip` или `ip/len`).
pub fn ip_in_ipset(ip: &str, ipset_path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(ipset_path) else {
        return false;
    };
    let Some(u) = ipv4_u32(ip) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.split_whitespace().next().unwrap_or("");
        let (base, mask) = match line.split_once('/') {
            Some((b, m)) => (b, m.parse::<u32>().unwrap_or(32)),
            None => (line, 32),
        };
        if let Some(b) = ipv4_u32(base) {
            let sh = 32u32.saturating_sub(mask.min(32));
            if (u >> sh) == (b >> sh) {
                return true;
            }
        }
    }
    false
}

/// Вердикт для сайта: `without` — замер без обхода, `with` — с обходом.
pub fn decide_site(
    without: &PhaseMeasure,
    with: &PhaseMeasure,
    in_ipset: bool,
    excluded: bool,
) -> (Verdict, Recommendation) {
    if !without.ok && with.ok {
        (Verdict::Covered, none_note("оптимизация нужна и работает"))
    } else if without.ok && !with.ok {
        let note = if excluded {
            "домен уже в исключениях — проверьте, применился ли список (перезапуск стратегии)"
        } else if in_ipset {
            "IP сайта в списке IP — оптимизация ломает незаблокированный сайт; исключаем домен"
        } else {
            "оптимизация ломает сайт; исключаем домен"
        };
        (
            Verdict::Collateral,
            Recommendation { file: Some("list-exclude-user.txt".into()), lines: Vec::new(), note: note.into() },
        )
    } else if !without.ok && !with.ok {
        (Verdict::Unrelated, none_note("не похоже на проблему оптимизации"))
    } else {
        (Verdict::NoEffect, none_note("оптимизация цели не мешает"))
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Endpoint {
    pub ip: String,
    pub port: u16,
    pub state: String,
}

/// Ждёт появления TCP-соединения к `ip:port` и измеряет, сколько оно живёт.
/// Наблюдает РЕАЛЬНОЕ соединение приложения (не синтетическую пробу): подходит
/// играм, где сервер не шлёт данные до клиентского рукопожатия.
/// `cancel_flag` — файл, при появлении которого прерываемся.
pub fn wait_and_measure(
    ip: &str,
    port: u16,
    wait_secs: u64,
    measure_secs: u64,
    cancel_flag: &std::path::Path,
) -> PhaseMeasure {
    if ipv4_u32(ip).is_none() {
        return PhaseMeasure { ok: false, ms: 0, detail: "некорректный адрес".into() };
    }
    let script = format!(
        "$flag='{flag}'; $rip='{ip}'; $rport={port}; $wait={wait}; $meas={meas}; \
         $t0=Get-Date; $appeared=$false; \
         while(((Get-Date)-$t0).TotalSeconds -lt $wait){{ \
           if(Test-Path $flag){{ 'cancelled'; exit }}; \
           $c=Get-NetTCPConnection -RemoteAddress $rip -RemotePort $rport -State Established -ErrorAction SilentlyContinue; \
           if($c){{ $appeared=$true; break }}; Start-Sleep -Milliseconds 1000 }}; \
         if(-not $appeared){{ 'no;0;0'; exit }}; \
         $t1=Get-Date; $alive=$true; \
         while(((Get-Date)-$t1).TotalSeconds -lt $meas){{ \
           if(Test-Path $flag){{ 'cancelled'; exit }}; \
           $c=Get-NetTCPConnection -RemoteAddress $rip -RemotePort $rport -State Established -ErrorAction SilentlyContinue; \
           if(-not $c){{ $alive=$false; break }}; Start-Sleep -Milliseconds 1000 }}; \
         '{{0}};{{1}};{{2}}' -f 'yes',[int](((Get-Date)-$t1).TotalMilliseconds),$alive",
        flag = cancel_flag.display(),
        ip = ip,
        port = port,
        wait = wait_secs,
        meas = measure_secs,
    );
    let out = crate::runner::run_powershell(&["-Command".into(), script]).unwrap_or_default();
    let line = out.trim();
    if line == "cancelled" {
        return PhaseMeasure { ok: false, ms: 0, detail: "проверка отменена".into() };
    }
    let parts: Vec<&str> = line.split(';').collect();
    if parts.len() != 3 || parts[0] != "yes" {
        return PhaseMeasure { ok: false, ms: 0, detail: "соединение не появилось (создайте сессию)".into() };
    }
    let ms: u64 = parts[1].parse().unwrap_or(0);
    let alive = parts[2].trim().eq_ignore_ascii_case("true");
    if alive {
        PhaseMeasure { ok: true, ms, detail: format!("держится ≥ {} с", ms / 1000) }
    } else {
        PhaseMeasure { ok: false, ms, detail: format!("оборвалось через {} с", ms / 1000) }
    }
}

/// Наблюдение за процессом: ждёт появления ЛЮБОГО не-443 соединения и меряет,
/// сколько оно живёт (устойчиво к смене серверного IP — Photon выдаёт разные).
pub fn watch_process(
    process: &str,
    wait_secs: u64,
    measure_secs: u64,
    cancel_flag: &std::path::Path,
) -> PhaseMeasure {
    let name = process.trim();
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return PhaseMeasure { ok: false, ms: 0, detail: "некорректное имя процесса".into() };
    }
    let script = format!(
        "$flag='{flag}'; $proc='{proc}'; $wait={wait}; $meas={meas}; \
         $t0=Get-Date; $appeared=$false; \
         while(((Get-Date)-$t0).TotalSeconds -lt $wait){{ \
           if(Test-Path $flag){{ 'cancelled'; exit }}; \
           $p=Get-Process -Name $proc -ErrorAction SilentlyContinue | Select-Object -First 1; \
           if($p){{ $c=Get-NetTCPConnection -OwningProcess $p.Id -State Established -ErrorAction SilentlyContinue | Where-Object {{ $_.RemotePort -ne 443 -and -not $_.RemoteAddress.StartsWith('127.') }}; if($c){{ $appeared=$true; break }} }}; \
           Start-Sleep -Milliseconds 1000 }}; \
         if(-not $appeared){{ 'no;0;0'; exit }}; \
         $t1=Get-Date; $alive=$true; \
         while(((Get-Date)-$t1).TotalSeconds -lt $meas){{ \
           if(Test-Path $flag){{ 'cancelled'; exit }}; \
           $p=Get-Process -Name $proc -ErrorAction SilentlyContinue | Select-Object -First 1; $c=$null; \
           if($p){{ $c=Get-NetTCPConnection -OwningProcess $p.Id -State Established -ErrorAction SilentlyContinue | Where-Object {{ $_.RemotePort -ne 443 -and -not $_.RemoteAddress.StartsWith('127.') }} }}; \
           if(-not $c){{ $alive=$false; break }}; Start-Sleep -Milliseconds 1000 }}; \
         '{{0}};{{1}};{{2}}' -f 'yes',[int](((Get-Date)-$t1).TotalMilliseconds),$alive",
        flag = cancel_flag.display(),
        proc = name,
        wait = wait_secs,
        meas = measure_secs,
    );
    let out = crate::runner::run_powershell(&["-Command".into(), script]).unwrap_or_default();
    parse_watch(&out)
}

fn parse_watch(out: &str) -> PhaseMeasure {
    let line = out.trim();
    if line == "cancelled" {
        return PhaseMeasure { ok: false, ms: 0, detail: "проверка отменена".into() };
    }
    let parts: Vec<&str> = line.split(';').collect();
    if parts.len() != 3 || parts[0] != "yes" {
        return PhaseMeasure { ok: false, ms: 0, detail: "соединение не появилось (создайте сессию)".into() };
    }
    let ms: u64 = parts[1].parse().unwrap_or(0);
    let alive = parts[2].trim().eq_ignore_ascii_case("true");
    if alive {
        PhaseMeasure { ok: true, ms, detail: format!("держится ≥ {} с", ms / 1000) }
    } else {
        PhaseMeasure { ok: false, ms, detail: format!("оборвалось через {} с", ms / 1000) }
    }
}

/// Поднимает главное окно на передний план — только если включена галочка
/// «Поднимать окно» в «Диагностике» (опционально, т.к. кража фокуса мешает игре).
fn focus_main(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Захват живых TCP-коннектов процесса по имени (без 443/локальных) за `secs` секунд.
pub fn capture_process_endpoints(name: &str, secs: u64) -> Vec<Endpoint> {
    let name = name.trim();
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Vec::new();
    }
    let script = format!(
        "$p=Get-Process -Name '{name}' -ErrorAction SilentlyContinue | Select-Object -First 1; \
         if(-not $p){{return}}; $tid=$p.Id; $d=(Get-Date).AddSeconds({secs}); $seen=@{{}}; \
         while((Get-Date) -lt $d){{ \
           if(-not (Get-Process -Id $tid -ErrorAction SilentlyContinue)){{break}}; \
           Get-NetTCPConnection -OwningProcess $tid -State Established -ErrorAction SilentlyContinue | \
             Where-Object {{ $_.RemoteAddress -notmatch '^(127\\.|10\\.|192\\.168\\.|172\\.(1[6-9]|2[0-9]|3[0-1])\\.)' }} | \
             ForEach-Object {{ $seen[\"$($_.RemoteAddress):$($_.RemotePort)\"]=1 }}; \
           Start-Sleep -Milliseconds 1000 }}; $seen.Keys"
    );
    let out = crate::runner::run_powershell(&["-Command".into(), script]).unwrap_or_default();
    let mut eps: Vec<Endpoint> = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if let Some((ip, port)) = line.rsplit_once(':') {
            if let Ok(p) = port.parse::<u16>() {
                if !ip.is_empty() {
                    eps.push(Endpoint { ip: ip.to_string(), port: p, state: "Established".into() });
                }
            }
        }
    }
    eps
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ScanReport {
    pub kind: String,
    pub target: String,
    pub strategy: String,
    pub without: PhaseMeasure,
    pub with: PhaseMeasure,
    pub verdict: Verdict,
    pub recommendation: Recommendation,
    pub in_ipset: bool,
}

pub fn parse_endpoint(s: &str) -> Option<(String, u16)> {
    let (ip, port) = s.trim().rsplit_once(':')?;
    let port: u16 = port.trim().parse().ok()?;
    ipv4_u32(ip)?;
    Some((ip.trim().to_string(), port))
}

fn subnet24(ip: &str) -> String {
    let o: Vec<&str> = ip.split('.').collect();
    if o.len() == 4 {
        format!("{}.{}.{}.0/24", o[0], o[1], o[2])
    } else {
        format!("{ip}/24")
    }
}

fn resolve_first_ip(host: &str) -> Option<String> {
    use std::net::ToSocketAddrs;
    (host, 443u16)
        .to_socket_addrs()
        .ok()?
        .find_map(|a| match a.ip() {
            std::net::IpAddr::V4(v4) => Some(v4.to_string()),
            _ => None,
        })
}

/// Домен уже в списках исключений движка (`list-exclude.txt` / `-user`).
pub fn domain_excluded(host: &str, lists_dir: &std::path::Path) -> bool {
    for f in ["list-exclude.txt", "list-exclude-user.txt"] {
        if let Ok(text) = std::fs::read_to_string(lists_dir.join(f)) {
            for line in text.lines() {
                let l = line.trim().trim_start_matches('^').to_ascii_lowercase();
                if l.is_empty() || l.starts_with('#') {
                    continue;
                }
                if host == l || host.ends_with(&format!(".{l}")) {
                    return true;
                }
            }
        }
    }
    false
}

fn measure_site(target: &str) -> PhaseMeasure {
    match parse_site_target(target) {
        Some(host) => crate::tester::site_measure(&host),
        None => PhaseMeasure { ok: false, ms: 0, detail: "некорректный домен".into() },
    }
}

/// Вердикт для программы/игры по жизнеспособности реального соединения
/// (`ok` = держится весь замер, `!ok` = оборвалось/не появилось).
pub fn decide_process(
    without: &PhaseMeasure,
    with: &PhaseMeasure,
    in_ipset: bool,
    game_filter: bool,
) -> (Verdict, Recommendation) {
    if !without.ok && with.ok {
        (Verdict::Covered, none_note("оптимизация нужна и удерживает соединение"))
    } else if without.ok && !with.ok {
        (
            Verdict::Collateral,
            Recommendation {
                file: Some("ipset-exclude-user.txt".into()),
                lines: Vec::new(),
                note: "оптимизация рвёт рабочее соединение — исключаем адрес".into(),
            },
        )
    } else if !without.ok && !with.ok {
        let note = if !game_filter {
            "соединение рвётся и с оптимизацией; включите «Игровой фильтр» и добавьте подсеть в обход"
        } else if !in_ipset {
            "адреса нет в списке IP — добавляем подсеть в обход и включаем «Игровой фильтр»"
        } else {
            "адрес в списке IP и «Игровой фильтр» включён, но соединение всё равно рвётся — попробуйте другую стратегию"
        };
        (
            Verdict::NotCovered,
            Recommendation { file: Some("ipset-all-user.txt".into()), lines: Vec::new(), note: note.into() },
        )
    } else {
        (Verdict::NoEffect, none_note("соединение держится и без оптимизации — проблема не в ней"))
    }
}

/// Полный прогон: снимок → фазы → возврат прежнего состояния → вердикт.
/// Возврат состояния делается ВСЕГДА (даже при ошибке/отмене фаз).
pub fn run_scan(
    app: &tauri::AppHandle,
    g: &crate::Global,
    kind: &str,
    target: &str,
    strategy_id: &str,
    focus: bool,
) -> Result<ScanReport, String> {
    let (prev_profile, prev_service) = {
        let s = crate::st(g);
        (s.runtime.as_ref().map(|r| r.profile_id.clone()), s.service_running.unwrap_or(false))
    };
    let data = crate::st(g).data.clone();
    let flag = data.join("logs/scan-stop.flag");
    let _ = std::fs::remove_file(&flag);
    let lists = crate::scanner_lists_dir(g);

    let report = scan_inner(app, g, kind, target, strategy_id, focus, &lists, &flag);

    // Возврат прежнего состояния: служба важнее профиля (они взаимоисключающие).
    if prev_service {
        if let Err(e) = crate::service::start_service(&data) {
            crate::logger::log("warn", "scanner", &format!("служба не вернулась после скана: {e}"));
        }
    } else if let Some(id) = prev_profile {
        let _ = crate::do_start(app, g, &id);
    }
    let _ = std::fs::remove_file(&flag);
    report
}

#[allow(clippy::too_many_arguments)]
fn scan_inner(
    app: &tauri::AppHandle,
    g: &crate::Global,
    kind: &str,
    target: &str,
    strategy_id: &str,
    focus: bool,
    lists: &Option<std::path::PathBuf>,
    flag: &std::path::Path,
) -> Result<ScanReport, String> {
    if kind == "site" {
        if !crate::tester::curl_available() {
            return Err("не найден curl.exe — проба сайта невозможна".into());
        }
        crate::stop_all_own(app, g)?;
        if focus { focus_main(app); }
        crate::emit(app, "zgui:scan", serde_json::json!({"phase": "without", "msg": "Замер без оптимизации…"}));
        let without = measure_site(target);
        let start_res = crate::do_start(app, g, strategy_id);
        if focus { focus_main(app); }
        crate::emit(app, "zgui:scan", serde_json::json!({"phase": "with", "msg": "Замер с оптимизацией…"}));
        let with = if start_res.is_ok() {
            measure_site(target)
        } else {
            PhaseMeasure { ok: false, ms: 0, detail: "стратегия не запустилась".into() }
        };
        let _ = crate::stop_all_own(app, g);

        let (in_ipset, excluded) = match lists {
            Some(l) => {
                let host = parse_site_target(target).unwrap_or_default();
                let hit = resolve_first_ip(&host).map(|ip| ip_in_ipset(&ip, &l.join("ipset-all.txt"))).unwrap_or(false);
                (hit, domain_excluded(&host, l))
            }
            None => (false, false),
        };
        let (verdict, mut rec) = decide_site(&without, &with, in_ipset, excluded);
        if matches!(verdict, Verdict::Collateral) {
            rec.lines = vec![parse_site_target(target).unwrap_or_default()];
        }
        start_res?;
        Ok(ScanReport {
            kind: kind.into(),
            target: target.into(),
            strategy: strategy_id.into(),
            without,
            with,
            verdict,
            recommendation: rec,
            in_ipset,
        })
    } else {
        // Цель — `ip:порт` (наблюдаем адрес) либо имя процесса (наблюдаем процесс:
        // Photon выдаёт разные серверные IP, фиксировать один нельзя).
        let endpoint = parse_endpoint(target);
        let desc = endpoint
            .as_ref()
            .map(|(ip, p)| format!("{ip}:{p}"))
            .unwrap_or_else(|| target.to_string());
        let do_measure = |ip: &str, port: u16| wait_and_measure(ip, port, 30, 40, flag);

        crate::stop_all_own(app, g)?;
        if focus { focus_main(app); }
        crate::emit(app, "zgui:scan", serde_json::json!({"phase": "without", "msg": format!("Фаза 1/2 (без оптимизации): создайте сессию — {desc}")}));
        let without = match &endpoint {
            Some((ip, p)) => wait_and_measure(ip, *p, 45, 40, flag),
            None => watch_process(target, 45, 40, flag),
        };
        if without.detail == "проверка отменена" {
            let _ = crate::stop_all_own(app, g);
            return Err("проверка отменена".into());
        }
        let start_res = crate::do_start(app, g, strategy_id);
        if focus { focus_main(app); }
        crate::emit(app, "zgui:scan", serde_json::json!({"phase": "with", "msg": format!("Фаза 2/2 (с оптимизацией): пересоздайте сессию — {desc}")}));
        let with = if start_res.is_ok() {
            match &endpoint {
                Some((ip, p)) => do_measure(ip, *p),
                None => watch_process(target, 30, 40, flag),
            }
        } else {
            PhaseMeasure { ok: false, ms: 0, detail: "стратегия не запустилась".into() }
        };
        let _ = crate::stop_all_own(app, g);
        if with.detail == "проверка отменена" {
            return Err("проверка отменена".into());
        }

        let in_ipset = match (&endpoint, lists) {
            (Some((ip, _)), Some(l)) => ip_in_ipset(ip, &l.join("ipset-all.txt")),
            _ => false,
        };
        let gf = crate::st(g).settings.game_filter != "off";
        let (verdict, mut rec) = decide_process(&without, &with, in_ipset, gf);
        if matches!(verdict, Verdict::NotCovered) {
            if let Some((ip, _)) = &endpoint {
                rec.lines = vec![subnet24(ip)];
            }
        }
        start_res?;
        Ok(ScanReport {
            kind: kind.into(),
            target: desc,
            strategy: strategy_id.into(),
            without,
            with,
            verdict,
            recommendation: rec,
            in_ipset,
        })
    }
}

/// Применяет рекомендацию: домен в исключения либо подсеть в include + Game Filter.
pub fn apply_report(g: &crate::Global, report: &ScanReport) -> Result<String, String> {
    let lists = crate::scanner_lists_dir(g).ok_or("движок flowseal не настроен — применить некуда")?;
    match report.verdict {
        Verdict::Collateral if report.kind == "site" => {
            let host = report.recommendation.lines.first().cloned().unwrap_or_default();
            if host.is_empty() {
                return Err("пустой домен".into());
            }
            let f = lists.join("list-exclude-user.txt");
            let mut cur = std::fs::read_to_string(&f).unwrap_or_default();
            if !cur.lines().any(|l| l.trim().eq_ignore_ascii_case(&host)) {
                if !cur.is_empty() && !cur.ends_with('\n') {
                    cur.push('\n');
                }
                cur.push_str(&host);
                cur.push('\n');
                std::fs::write(&f, cur).map_err(|e| e.to_string())?;
            }
            Ok(format!("Добавлено в исключения: {host}. Перезапустите стратегию."))
        }
        Verdict::NotCovered => {
            let line = report.recommendation.lines.first().cloned().unwrap_or_default();
            if line.is_empty() {
                return Err("пустая подсеть".into());
            }
            let f = lists.join("ipset-all-user.txt");
            let mut cur = std::fs::read_to_string(&f).unwrap_or_default();
            if !cur.lines().any(|l| l.trim() == line) {
                if !cur.is_empty() && !cur.ends_with('\n') {
                    cur.push('\n');
                }
                cur.push_str(&line);
                cur.push('\n');
                std::fs::write(&f, cur).map_err(|e| e.to_string())?;
            }
            let (root, data, settings) = {
                let mut s = crate::st(g);
                s.settings.game_filter = "all".into();
                s.save();
                (s.roots.path(crate::config::ENGINE_FLOWSEAL), s.data.clone(), s.settings.clone())
            };
            if let Some(root) = root {
                crate::updater::sync_ipset(&root, &data, &settings);
            }
            Ok(format!("Добавлено в обход: {line}; включён «Игровой фильтр». Перезапустите стратегию."))
        }
        _ => Ok("Рекомендация не требует правок.".into()),
    }
}

/// Сохраняет отчёт скана в `data/logs/scan-<epoch>.txt`, возвращает путь.
pub fn save_report(data: &std::path::Path, report: &ScanReport) -> Result<String, String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = data.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("scan-{ts}.txt"));
    let ph = |name: &str, m: &PhaseMeasure| {
        format!("{name}: {} ({}), {} мс", if m.ok { "есть ответ" } else { "нет ответа" }, m.detail, m.ms)
    };
    let text = format!(
        "Диагностика сервиса\r\nЦель: {} ({})\r\nСтратегия: {}\r\nБез оптимизации: {}\r\nС оптимизацией: {}\r\nВердикт: {:?}\r\nРекомендация: {}\r\nФайл: {}\r\nСтроки: {}\r\n",
        report.target,
        report.kind,
        report.strategy,
        ph("замер", &report.without).trim_start_matches("замер: "),
        ph("замер", &report.with).trim_start_matches("замер: "),
        report.verdict,
        report.recommendation.note,
        report.recommendation.file.clone().unwrap_or_else(|| "—".into()),
        report.recommendation.lines.join(", "),
    );
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_site_target_normalizes_urls() {
        assert_eq!(
            parse_site_target("https://store.steampowered.com/app/1?x=2").as_deref(),
            Some("store.steampowered.com")
        );
        assert_eq!(
            parse_site_target("cdn.cloudflare.steamstatic.com:443").as_deref(),
            Some("cdn.cloudflare.steamstatic.com")
        );
        assert_eq!(parse_site_target("  steamcommunity.com  ").as_deref(), Some("steamcommunity.com"));
        assert_eq!(parse_site_target(""), None);
        assert_eq!(parse_site_target("bad host!!"), None);
    }

    #[test]
    fn ip_in_ipset_matches_cidr() {
        let base = std::env::temp_dir().join(format!("zgui-ipset-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let f = base.join("ipset-all.txt");
        std::fs::write(&f, b"# c\n1.2.3.0/24\n80.93.214.0/24\n").unwrap();
        assert!(ip_in_ipset("1.2.3.55", &f));
        assert!(ip_in_ipset("80.93.214.205", &f));
        assert!(!ip_in_ipset("2.22.145.90", &f));
        assert!(!ip_in_ipset("not-an-ip", &f));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn decide_site_flags_collateral() {
        let ok = PhaseMeasure { ok: true, ms: 120, detail: String::new() };
        let bad = PhaseMeasure { ok: false, ms: 0, detail: "таймаут".into() };
        let (v, rec) = decide_site(&ok, &bad, true, false);
        assert!(matches!(v, Verdict::Collateral));
        assert_eq!(rec.file.as_deref(), Some("list-exclude-user.txt"));

        let (v2, _) = decide_site(&bad, &ok, true, false);
        assert!(matches!(v2, Verdict::Covered));
        let (v3, _) = decide_site(&ok, &ok, false, false);
        assert!(matches!(v3, Verdict::NoEffect));
    }

    #[test]
    fn decide_process_needs_include_when_dead_both_ways() {
        let dead = PhaseMeasure { ok: false, ms: 22000, detail: "give-up".into() };
        let (v, rec) = decide_process(&dead, &dead, false, false);
        assert!(matches!(v, Verdict::NotCovered));
        assert_eq!(rec.file.as_deref(), Some("ipset-all-user.txt"));
        assert!(rec.note.to_lowercase().contains("фильтр"));
    }
}
