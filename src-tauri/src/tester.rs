use crate::config::Profile;
use std::path::{Path, PathBuf};

const BUILTIN_TEST_DOMAINS: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/test-domains-russia.lst"));

#[cfg(test)]
use std::net::TcpStream;
#[cfg(test)]
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainGroup {
    pub id: &'static str,
    pub label: &'static str,
    pub critical: bool,
    pub priority: u8,
}

pub const GROUP_YOUTUBE: DomainGroup = DomainGroup { id: "youtube", label: "YouTube", critical: true, priority: 1 };
pub const GROUP_YOUTUBE_MUSIC: DomainGroup = DomainGroup { id: "youtube-music", label: "YouTube Music", critical: true, priority: 1 };
pub const GROUP_DISCORD: DomainGroup = DomainGroup { id: "discord", label: "Discord", critical: true, priority: 1 };
pub const GROUP_MICROSOFT_XBOX: DomainGroup = DomainGroup { id: "microsoft-xbox", label: "Microsoft / Xbox", critical: false, priority: 2 };
pub const GROUP_GOOGLE: DomainGroup = DomainGroup { id: "google", label: "Google", critical: false, priority: 2 };
pub const GROUP_CLOUDFLARE: DomainGroup = DomainGroup { id: "cloudflare", label: "Cloudflare", critical: false, priority: 2 };
pub const GROUP_OTHER: DomainGroup = DomainGroup { id: "other", label: "Дополнительные домены", critical: false, priority: 3 };

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
    if ["youtube.com", "www.youtube.com", "youtu.be", "youtube-nocookie.com", "youtube.googleapis.com", "youtubei.googleapis.com", "googlevideo.com", "ytimg.com", "ytimg.l.google.com", "yt3.googleusercontent.com"].contains(&h.as_str()) || h.ends_with(".youtube.com") {
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
    if h == "google.com" || h.ends_with(".google.com") || h.ends_with(".googleusercontent.com") || h.ends_with(".googleapis.com") {
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
}

/// TCP-connect с таймаутом (используется в юнит-тесте и как быстрый probe).
#[cfg(test)]
fn probe_tcp(host: &str, port: u16, timeout: Duration) -> (bool, u64, String) {
    let start = Instant::now();
    let addr = match std::net::ToSocketAddrs::to_socket_addrs(&(host, port)) {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => return (false, 0, "DNS: пусто".into()),
        },
        Err(e) => return (false, 0, format!("DNS: {}", e)),
    };
    match TcpStream::connect_timeout(&addr, timeout) {
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

/// Отсекает мусорные строки из .lst: `.ua`, IP-адреса, `*.domain`, ведущие/хвостовые точки.
fn usable_test_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.contains('*') || host.contains("..") {
        return false;
    }
    if host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    let has_alpha = host.chars().any(|c| c.is_ascii_alphabetic());
    let labels: Vec<&str> = host.split('.').collect();
    has_alpha
        && labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
}

/// Готовит список доменов из geoblock-списка (RAW .lst: по строке на домен).
/// Разбор одной строки .lst в домен (отбрасывает комментарии, пути и мусор).
/// Понимает и «человеческий» формат: `https://www.example.com/путь` → `www.example.com`
/// (иначе строка резалась по первому `/` и превращалась в `https:` — домен терялся).
fn parse_rule_line(raw: &str, seen: &mut std::collections::HashSet<String>) -> Option<(String, String)> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return None;
    }
    let without_scheme = line
        .strip_prefix("https://")
        .or_else(|| line.strip_prefix("http://"))
        .or_else(|| line.strip_prefix("//"))
        .unwrap_or(line);
    let host = without_scheme
        .split(['/', ' ', '\t', '?', '#'])
        .next()
        .unwrap_or("")
        .split(':') // порт (example.com:443)
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.')
        .to_lowercase();
    if !usable_test_host(&host) {
        return None;
    }
    if !seen.insert(host.clone()) {
        return None;
    }
    let key = host.split('.').next().unwrap_or(&host).to_string();
    Some((key, host))
}

/// Обязательные проверки — всегда в тесте. YouTube Music скрыт в UI,
/// но остаётся жёстким требованием к успешной стратегии.
pub const REQUIRED_DOMAINS: &[&str] = &[
    "www.youtube.com",
    "music.youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "googlevideo.com",
    "ytimg.com",
    "discord.com",
    "discord.gg",
    "discordapp.com",
    "gateway.discord.gg",
    "login.live.com",
    "account.live.com",
    "microsoft.com",
    "xbox.com",
    "xboxlive.com",
    "xboxservices.com",
    "www.google.com",
    "google.com",
    "www.cloudflare.com",
    "cloudflare.com",
];

/// Онлайн-списки геоблока — отдельный диагностический тест (лимит задаёт UI).
/// Основной набор теста: критические домены (YouTube, Discord) и вторые по
/// приоритету (Microsoft/Xbox, Google, Cloudflare — сообщаются, но для успеха
/// не обязательны). Доп. домены в стандартный тест НЕ входят: они в отдельном
/// редактируемом файле, для них отдельная кнопка (см. `load_extra_domains`).
pub fn main_domains(limit: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for host in REQUIRED_DOMAINS {
        let host = host.to_string();
        out.push((host.split('.').next().unwrap_or(&host).to_string(), host));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Путь к редактируемому списку доп. доменов (рядом с данными программы).
pub fn extra_domains_path(data: &Path) -> std::path::PathBuf {
    data.join("catalog").join("test-domains-extra.lst")
}

/// Создаёт файл доп. доменов из вшитого списка, если его ещё нет: пользователь
/// правит файл руками, вшитая таблица — только стартовое наполнение.
pub fn ensure_extra_domains_file(data: &Path) -> std::path::PathBuf {
    let p = extra_domains_path(data);
    if !p.exists() {
        let mut text = String::from(
            "# Дополнительные домены для отдельного теста «Доп. домены».\r\n\
             # Формат: один домен в строке; строки с # игнорируются.\r\n\
             # В стандартном тесте эти домены НЕ проверяются (там только YouTube/Discord\r\n\
             # и вторые по приоритету Microsoft/Xbox, Google, Cloudflare).\r\n\
             # Этот список гоняется отдельной кнопкой «Доп. домены».\r\n",
        );
        text.push_str(&String::from_utf8_lossy(BUILTIN_TEST_DOMAINS));
        let _ = crate::config::atomic_write(&p, text.as_bytes());
    }
    p
}

/// Читает список доп. доменов (при отсутствии файла — создаёт из вшитого).
pub fn load_extra_domains(data: &Path, limit: usize) -> Vec<(String, String)> {
    let p = ensure_extra_domains_file(data);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(text) = crate::config::read_text_auto(&p) {
        for raw in text.lines() {
            if let Some(d) = parse_rule_line(raw, &mut seen) {
                out.push(d);
                if out.len() >= limit {
                    break;
                }
            }
        }
    }
    out
}

pub fn load_geoblock_domains(data: &Path, limit: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let dir = data.join("catalog/geoblock");
    let files = [
        "allow-domains-russia-inside.lst",
        "allow-domains-geoblock.lst",
        "allow-domains-youtube.lst",
        "allow-domains-discord.lst",
        "allow-domains-news.lst",
        "allow-domains-telegram.lst",
        "allow-domains-twitter.lst",
        "allow-domains-meta.lst",
        "allow-domains-block.lst",
    ];
    for f in files {
        let p = dir.join(f);
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        for raw in text.lines() {
            if let Some(d) = parse_rule_line(raw, &mut seen) {
                out.push(d);
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out
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
    pub fn save(&self, data: &Path) {
        let p = data.join("tests.json");
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
}

/// Жив ли фоновый раннер теста: по маркерам `logs/test-runner.pid` и
/// `logs/test-stop.flag`. Нужен, чтобы watchdog не принял winws теста за
/// «запущенный вне программы» сразу после старта GUI.
pub fn runner_alive(data: &Path) -> bool {
    let pid = data.join("logs/test-runner.pid");
    let stop = data.join("logs/test-stop.flag");
    std::fs::read_to_string(&pid)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|p| !stop.exists() && crate::runner::pid_alive(p))
        .unwrap_or(false)
}

/// Один запуск на стратегию (используется встроенным PS-раннером).
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

/// Пишет план теста и PowerShell-раннер, который выполнит ВСЕ стратегии
/// в одном элевированном процессе (один UAC-запрос на весь тест).
pub fn write_test_runner(
    data: &Path,
    steps: &[TestStep],
    domains: &[(String, String)],
    baseline: bool,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let plan_path = data.join("logs/test-plan.json");
    let out_path = data.join("logs/test-out.json");
    let baseline_path = data.join("logs/test-baseline.json");
    let script_path = data.join("logs/test-runner.ps1");
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&baseline_path);
    // Ступ-маркеры «резкого останова»: флаг + PID раннера + PID активного winws.
    let stop_flag = data.join("logs/test-stop.flag");
    let run_pid = data.join("logs/test-runner.pid");
    let win_pid = data.join("logs/test-current.pid");
    for f in [&stop_flag, &run_pid, &win_pid] {
        let _ = std::fs::remove_file(f);
    }

    let plan = serde_json::json!({
        "steps": steps,
        "domains": domains.iter().map(|(k, h)| {
            let group = classify_domain(h);
            serde_json::json!({
                "key": k,
                "host": h,
                "group": group.id,
                "groupLabel": group.label,
                "critical": group.critical,
                "priority": group.priority,
            })
        }).collect::<Vec<_>>(),
        "out": out_path.to_string_lossy(),
        "baselineOut": baseline_path.to_string_lossy(),
        "baseline": baseline,
        "pid": run_pid.to_string_lossy(),
        "winPid": win_pid.to_string_lossy(),
        "flag": stop_flag.to_string_lossy(),
    });
    let plan_json = serde_json::to_string(&plan).map_err(|e| e.to_string())?;
    crate::runner::write_utf8_bom(&plan_path, plan_json.as_bytes())?;

    let script = format!(
        r#"{header}
$ErrorActionPreference = 'Continue'
$stopRun = $false
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$plan = Get-Content -Raw -Encoding UTF8 -LiteralPath {plan} | ConvertFrom-Json
$PID | Out-File -LiteralPath $plan.pid -Encoding ascii
$out = $plan.out
$errDir = Split-Path -Parent $out
$all = @()
$i = 0
# HTTPS probe. TCP connect is not enough: DPI lets the TCP handshake through
# and cuts TLS by SNI, so a blocked site "passes" while the browser cannot open
# it. We check the full HTTPS exchange (TLS + HTTP headers), like a browser does.
# Add-Type is required: in PS 5.1 the type System.Net.Http.HttpClientHandler is
# not resolved until the System.Net.Http assembly is loaded (checked on Win11).
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Add-Type -AssemblyName System.Net.Http
function New-ProbeClient {{
  $h = New-Object System.Net.Http.HttpClientHandler
  $h.UseProxy = $false
  $h.AllowAutoRedirect = $false
  $c = New-Object System.Net.Http.HttpClient -ArgumentList $h
  $c.Timeout = [TimeSpan]::FromSeconds(6)
  return $c
}}
function Start-Probe($client, $target) {{
  try {{
    return $client.GetAsync("https://$target/", [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead)
  }} catch {{
    return $null
  }}
}}
function Finish-Probe($task) {{
  if ($null -eq $task) {{ return [ordered]@{{ ok = $false; detail = 'request failed' }} }}
  try {{
    $resp = $task.GetAwaiter().GetResult()
    $code = [int]$resp.StatusCode
    $resp.Dispose()
    return [ordered]@{{ ok = $true; detail = "http $code" }}
  }} catch {{
    $msg = $_.Exception.Message
    if ($_.Exception.InnerException) {{ $msg = $_.Exception.InnerException.Message }}
    if (-not $msg) {{ $msg = 'failed' }}
    return [ordered]@{{ ok = $false; detail = $msg }}
  }}
}}
if ($plan.baseline) {{
  # Baseline probe WITHOUT Zapret: distinguishes "site down / not resolving"
  # from "blocked but bypassable". Written to a separate file.
  # Probes run in small batches: with many parallel handshakes the WinDivert
  # queue chokes and even healthy sites answer at ~6 s (timeout) - see below.
  $baseOut = @()
  $baseClient = New-ProbeClient
  $batchSize = 8
  $bi = 0
  for ($offset = 0; $offset -lt $plan.domains.Count; $offset += $batchSize) {{
    $end = [Math]::Min($offset + $batchSize - 1, $plan.domains.Count - 1)
    $baseChecks = @()
    foreach ($d in $plan.domains[$offset..$end]) {{
      $baseChecks += [pscustomobject]@{{ host = $d.host; task = (Start-Probe $baseClient $d.host) }}
    }}
    foreach ($c in $baseChecks) {{
      $r = Finish-Probe $c.task
      $baseOut += [ordered]@{{ host = $c.host; ok = [bool]$r.ok }}
      $bi++
    }}
    $state = [ordered]@{{ baseline = [ordered]@{{ done = $bi; total = $plan.domains.Count }}; results = @() }}
    ($state | ConvertTo-Json -Depth 4) | Out-File -LiteralPath "$out.tmp" -Encoding utf8
    Move-Item -LiteralPath "$out.tmp" -Destination $out -Force
  }}
  $baseClient.Dispose()
  ($baseOut | ConvertTo-Json -Depth 4) | Out-File -LiteralPath $plan.baselineOut -Encoding utf8
}}
function Measure-Step($step) {{
  $res = [ordered]@{{ id = $step.id; name = $step.name; engine = $step.engine; group = $step.group; started = $false; score = 0; maxScore = $plan.domains.Count; domains = @(); groups = @(); criticalOk = $false; error = $null }}
  $safe = ($step.id -replace '[^A-Za-z0-9._-]', '_')
  $errFile = Join-Path $errDir ("run-$safe.err.txt")
  $outFile = Join-Path $errDir ("run-$safe.out.txt")
  try {{
    # Start-Process joins string arrays without quoting values containing spaces.
    # Every path below can contain spaces, so create one correctly quoted command line.
    $argLine = @($step.args | ForEach-Object {{
      $a = [string]$_
      if ($a -match '[\s"]') {{ '"' + $a.Replace('"', '\"') + '"' }} else {{ $a }}
    }}) -join ' '
    $p = Start-Process -FilePath $step.exe -WorkingDirectory $step.workdir -WindowStyle Hidden -ArgumentList $argLine -PassThru -RedirectStandardError $errFile -RedirectStandardOutput $outFile
    $p.Id | Out-File -LiteralPath $plan.winPid -Encoding ascii
    Start-Sleep -Milliseconds 1800
    # One retry when the process died instantly (Driver load race, transient
    # WinDivert state). Not retried: a healthy run (probes already measured).
    if ($p -and $p.HasExited) {{
      $p = Start-Process -FilePath $step.exe -WorkingDirectory $step.workdir -WindowStyle Hidden -ArgumentList $argLine -PassThru -RedirectStandardError $errFile -RedirectStandardOutput $outFile
      $p.Id | Out-File -LiteralPath $plan.winPid -Encoding ascii
      Start-Sleep -Milliseconds 1800
    }}
    if ($p -and -not $p.HasExited) {{
      $res.started = $true
      # HTTPS probes in small parallel batches (like the author's own tester,
      # "parallel: 8"). One hundred simultaneous handshakes overload the
      # WinDivert desync path: every host then answers at ~6 s and honest
      # strategies look broken (author's harness: 92/105, ours: 19/115).
      $doms = @()
      $client = New-ProbeClient
      $batchSize = 8
      for ($offset = 0; $offset -lt $plan.domains.Count; $offset += $batchSize) {{
        $end = [Math]::Min($offset + $batchSize - 1, $plan.domains.Count - 1)
        $checks = @()
        foreach ($d in $plan.domains[$offset..$end]) {{
          $checks += [pscustomobject]@{{ key = $d.key; host = $d.host; group = $d.group; groupLabel = $d.groupLabel; started = [DateTime]::UtcNow; task = (Start-Probe $client $d.host) }}
        }}
        foreach ($c in $checks) {{
          $r = Finish-Probe $c.task
          $ms = [int]([DateTime]::UtcNow - $c.started).TotalMilliseconds
          $doms += [ordered]@{{ key = $c.key; host = $c.host; group = $c.group; groupLabel = $c.groupLabel; ok = [bool]$r.ok; ms = $ms; detail = $r.detail }}
        }}
      }}
      $client.Dispose()
      $res.domains = $doms
      $res.score = @($doms | Where-Object {{ $_.ok }}).Count
      # Critical domains are probed once more (parallel): a single timeout/RST
      # must not label a strategy as broken.
      $critKeys = @($plan.domains | Where-Object {{ $_.critical }} | ForEach-Object {{ $_.key }})
      $failedCrit = @($doms | Where-Object {{ -not $_.ok -and ($critKeys -contains $_.key) }})
      if ($failedCrit.Count -gt 0) {{
        $client2 = New-ProbeClient
        $again = @()
        foreach ($d in $failedCrit) {{
          $again += [pscustomobject]@{{ d = $d; task = (Start-Probe $client2 $d.host) }}
        }}
        foreach ($x in $again) {{
          $r2 = Finish-Probe $x.task
          if ($r2.ok) {{ $x.d.ok = $true; $x.d.detail = $r2.detail + ' (retry)' }}
        }}
        $client2.Dispose()
        $res.score = @($doms | Where-Object {{ $_.ok }}).Count
      }}
      $groupRows = @()
      foreach ($group in @($plan.domains | Group-Object group)) {{
        $items = @($doms | Where-Object {{ $_.group -eq $group.Name }})
        $passed = @($items | Where-Object {{ $_.ok }}).Count
        $first = $group.Group | Select-Object -First 1
        $isCritical = [bool]$first.critical
        $isMusic = $group.Name -eq 'youtube-music'
        $okGroup = if ($isMusic) {{ $passed -gt 0 }} else {{ $passed -gt 0 -and ($passed * 2 -ge $items.Count) }}
        $groupRows += [ordered]@{{ id = $group.Name; label = $first.groupLabel; passed = $passed; total = $items.Count; ok = $okGroup; critical = $isCritical; priority = $first.priority }}
      }}
      $res.groups = $groupRows
      $res.criticalOk = @($groupRows | Where-Object {{ $_.critical -and -not $_.ok }}).Count -eq 0
      if ($p -and -not $p.HasExited) {{ Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }}
    }} else {{
      $tail = ''
      if (Test-Path $errFile) {{ $tail = (Get-Content -LiteralPath $errFile -Raw -ErrorAction SilentlyContinue) }}
      if (-not $tail -and (Test-Path $outFile)) {{ $tail = (Get-Content -LiteralPath $outFile -Raw -ErrorAction SilentlyContinue) }}
      $code = if ($p) {{ try {{ $p.WaitForExit(); $p.ExitCode }} catch {{ '?' }} }} else {{ 'null' }}
      if (-not $isAdmin) {{
        $res.error = "ADMIN_REQUIRED: winws needs administrator rights - run the GUI as admin (exit $code) " + $tail
      }} else {{
        $res.error = "process exited immediately (exit $code) " + $tail
      }}
    }}
  }} catch {{
    $res.error = $_.Exception.Message
  }}
  return $res
}}
foreach ($step in $plan.steps) {{
  if (Test-Path -LiteralPath $plan.flag) {{ $stopRun = $true; break }}
  $i++
  $res = Measure-Step $step
  # Second chance for a near-zero result: an early run right after the test
  # starts often does not apply due to the WinDivert driver load race (the very
  # first strategy especially). Retry once and keep the better result, otherwise
  # a bad strategy and a warm-up failure look identical.
  if ($res.started -and $res.score -le 1) {{
    $r2 = Measure-Step $step
    if ($r2.started -and $r2.score -gt $res.score) {{ $res = $r2 }}
  }}
  Start-Sleep -Milliseconds 400
  $all += [pscustomobject]$res
  $state = [ordered]@{{ index = $i; total = $plan.steps.Count; currentId = $step.id; currentName = $step.name; results = $all }}
  # Atomic write: the GUI reads this file while the runner writes it; a partially
  # written 2+ MB JSON made the poll treat the test as finished (absolute 120 s
  # watchdog used to break on the first unreadable read).
  ($state | ConvertTo-Json -Depth 8) | Out-File -LiteralPath "$out.tmp" -Encoding utf8
  Move-Item -LiteralPath "$out.tmp" -Destination $out -Force
}}
$marker = if ($stopRun) {{ 'STOPPED' }} else {{ 'DONE' }}
$marker | Out-File -LiteralPath $out -Encoding utf8 -Append
"#,
        header = crate::runner::PS_HEADER,
        plan = crate::runner::ps_quote(&plan_path.to_string_lossy())
    );
    crate::runner::write_ps1(&script_path, &script)?;
    Ok((plan_path, script_path, out_path))
}

/// Читает базовую пробу (без Zapret): host → был ли доступен напрямую.
/// Сейчас не участвует в калибровке (недоступные = не прошли ни у одной стратегии),
/// но полезна для диагностики.
#[allow(dead_code)]
pub fn read_baseline(data: &Path) -> Vec<(String, bool)> {
    let p = data.join("logs/test-baseline.json");
    let Ok(text) = std::fs::read_to_string(&p) else { return Vec::new() };
    let text = text.trim_start_matches('\u{feff}').trim();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let host = e["host"].as_str()?.to_string();
                    let ok = e["ok"].as_bool().unwrap_or(false);
                    Some((host, ok))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Сохраняет результаты калибровки: обходимые Zapret домены и требующие VPN.
/// `reachable` — прошли хотя бы у одной стратегии; `vpn_only` — не прошли ни у кого,
/// но базовая проба (без Zapret) их видела (значит, сам сайт жив).
pub fn save_reachability(data: &Path, reachable: &[String], vpn_only: &[String]) {
    let dir = data.join("catalog/geoblock");
    let _ = std::fs::create_dir_all(&dir);
    let write = |name: &str, list: &[String]| {
        let mut sorted: Vec<&String> = list.iter().collect();
        sorted.sort();
        sorted.dedup();
        let body: String = sorted.iter().map(|s| format!("{}\n", s)).collect();
        let _ = std::fs::write(dir.join(name), body);
    };
    write("zapret-reachable.lst", reachable);
    write("vpn-only.lst", vpn_only);
}

/// Загружает список ранее откалиброванных «обходимых» доменов (может отсутствовать).
/// Пока не используется в тесте (фильтруем по `vpn-only`), но нужен для UI/статистики.
#[allow(dead_code)]
pub fn load_reachable(data: &Path) -> Vec<String> {
    let p = data.join("catalog/geoblock/zapret-reachable.lst");
    let Ok(text) = std::fs::read_to_string(&p) else { return Vec::new() };
    text.lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Загружает список доменов, которым нужен только VPN (недоступны через Zapret).
/// Используется, чтобы вырезать их из основного теста (не физически из .lst).
pub fn load_vpn_only(data: &Path) -> Vec<String> {
    let p = data.join("catalog/geoblock/vpn-only.lst");
    let Ok(text) = std::fs::read_to_string(&p) else { return Vec::new() };
    text.lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Читает частичный/финальный результат встроенного раннера.
pub fn read_test_progress(out_path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(out_path).ok()?;
    let text = text.trim_start_matches('\u{feff}').trim();
    // Отрезаем хвостовой маркер, если он дописан (DONE или STOPPED).
    let text = ["\nDONE", "\nSTOPPED"]
        .iter()
        .fold(text, |t, m| t.split(m).next().unwrap_or(t).trim());
    if text.is_empty() || !text.starts_with('{') {
        return None;
    }
    serde_json::from_str(text).ok()
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
        return "Результаты теста отсутствуют — прогоните «Тест стратегий».\r\n".into();
    }
    let mut sorted: Vec<&StrategyResult> = results.iter().collect();
    sorted.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let best = best_id
        .and_then(|id| results.iter().find(|r| r.id == id))
        .map(|r| format!("{} ({}/{})", r.name, r.score, r.max_score))
        .unwrap_or_else(|| "не определена".into());
    let width = sorted
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(10)
        .min(48);

    let mut out = String::new();
    out.push_str(&format!("Лучшая стратегия: {best}\r\n\r\n"));
    for r in &sorted {
        let state = if !r.started {
            let err: String = r.error.clone().unwrap_or_default().replace(['\r', '\n'], " ");
            let err: String = err.chars().take(120).collect();
            format!("не запустилась: {err}")
        } else if r.critical_ok {
            "критические домены пройдены".to_string()
        } else {
            "критические домены НЕ пройдены".to_string()
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
        out.push_str("\r\nНе ответившие критические домены:\r\n");
        out.push_str(&bad.join("\r\n"));
        out.push_str("\r\n");
    }
    out
}

/// Формирует сводку: отсортированные результаты + лучшая стратегия.
/// При равных очках выигрывает та, что прошла больше критических групп, затем —
/// с меньшей средней задержкой; имя лишь последний детерминированный tie-break.
/// (Раньше при равных очках «лучшей» становилась первая по алфавиту — случайность.)
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
        assert!(text.contains("критические домены пройдены"), "{text}");
        assert!(text.contains("критические домены НЕ пройдены"), "{text}");
        assert!(text.contains("не запустилась: process exited immediately"), "{text}");
        assert!(
            text.contains("general · ALT2 (3/115): youtube.com, discord.com"),
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
    fn load_domains_parses() {
        // Разбор списка доп. доменов из файла (он же — формат, который правит юзер).
        let tmp = std::env::temp_dir().join(format!("zgui-test-{}", std::process::id()));
        let dir = tmp.join("catalog");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("test-domains-extra.lst"),
            "# comment\nwww.youtube.com/abc\n.googlevideo.com\n.ua\n123.45.67.89\n*.wild.bad\nbad.domain.\ngooglevideo.com\nchess.com\n",
        )
        .unwrap();
        let d = load_extra_domains(&tmp, 10);
        assert!(d.iter().any(|(_, h)| h == "www.youtube.com"));
        assert!(d.iter().any(|(_, h)| h == "googlevideo.com"));
        assert!(d.iter().any(|(_, h)| h == "chess.com"));
        assert!(!d.iter().any(|(_, h)| h.starts_with(".")));
        assert!(!d.iter().any(|(_, h)| h.contains('*')));
        assert!(!d.iter().any(|(_, h)| h == "123.45.67.89"), "IP в списке хостов недопустим");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extra_domains_accept_urls_and_trailing_junk() {
        // Реальный файл пользователя: полные URL со схемой, пути, хвостовые пробелы.
        let tmp = std::env::temp_dir().join(format!("zgui-urls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let dir = tmp.join("catalog");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("test-domains-extra.lst"),
            "http://chess.com/\nhttps://www.deviantart.com/\nhttps://www.instagram.com/\nhttp://notepad-plus-plus.org/\nhttps://weather.com/\nhttp://x.com/ \nhttps://rutracker.org/\nhttps://soundcloud.com/\nhttp://linkedin.com/\nhttps://www.facebook.com/\nexample.com:8443/path\nhttps://trailing.dot./\n",
        )
        .unwrap();
        let d = load_extra_domains(&tmp, 100);
        let hosts: Vec<&str> = d.iter().map(|(_, h)| h.as_str()).collect();
        assert_eq!(hosts.len(), 12, "все строки должны распознаться: {hosts:?}");
        assert!(hosts.contains(&"chess.com"));
        assert!(hosts.contains(&"www.deviantart.com"));
        assert!(hosts.contains(&"x.com"), "хвостовой пробел не мешает: {hosts:?}");
        assert!(hosts.contains(&"example.com"), "порт отрезается: {hosts:?}");
        assert!(hosts.contains(&"trailing.dot"), "точка в конце отрезается: {hosts:?}");
        assert!(!hosts.iter().any(|h| h.contains(':') || h.contains('/') || h.ends_with('.')));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn main_domains_only_critical_and_secondary() {
        let d = main_domains(500);
        assert_eq!(d.len(), REQUIRED_DOMAINS.len(), "стандартный тест — ровно критичные и вторые");
        for (_, host) in &d {
            let group = classify_domain(host);
            assert!(
                group.critical || group.priority == 2,
                "доп. домен {host} не должен попадать в стандартный тест (группа {})",
                group.id
            );
        }
        // Доп. домены из вшитого списка — только в отдельном наборе.
        assert!(!d.iter().any(|(_, h)| h == "hdrezka.fm" || h == "chess.com"));
    }


    #[test]
    fn usable_host_rejects_junk() {
        assert!(usable_test_host("www.youtube.com"));
        assert!(usable_test_host("xn--e1afmkfd.xn--p1ai"));
        assert!(!usable_test_host(".ua"));
        assert!(!usable_test_host("ua."));
        assert!(!usable_test_host("192.168.1.1"));
        assert!(!usable_test_host("*.example.com"));
        assert!(!usable_test_host("exa mple.com"));
    }

    #[test]
    fn extra_domains_seeded_from_builtin_and_filtered() {
        // Файла нет — он создаётся из вшитого списка (стартовое наполнение),
        // который потом правит пользователь.
        let tmp = std::env::temp_dir().join(format!("zgui-builtin-domains-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let domains = load_extra_domains(&tmp, 500);
        let path = extra_domains_path(&tmp);
        assert!(path.is_file(), "файл доп. доменов должен создаться: {path:?}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# Дополнительные домены"), "в файле должна быть шапка-подсказка");
        assert!(domains.iter().any(|(_, host)| host == "hdrezka.fm"));
        assert!(domains.len() >= 100);
        // Вычищенные категории не должны вернуться (почта/СМИ/.ua).
        assert!(!domains.iter().any(|(_, host)| host == "10minutemail.com"));
        assert!(!domains.iter().any(|(_, host)| host.ends_with(".ua")));
        let _ = std::fs::remove_dir_all(&tmp);
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
    }

    #[cfg(windows)]
    #[test]
    fn runner_script_is_ascii_and_runs() {
        let tmp = std::env::temp_dir().join(format!("zgui-runner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("logs")).unwrap();
        let steps = vec![TestStep {
            id: "t".into(),
            name: "Тест".into(),
            engine: "flowseal".into(),
            group: "g".into(),
            exe: "C:\\Windows\\System32\\cmd.exe".into(),
            workdir: "C:\\Windows".into(),
            // Длинная команда с пробелами остаётся активной дольше проверки
            // процесса и подтверждает корректное quoting в $argLine.
            args: vec!["/c".into(), "ping -n 6 127.0.0.1 >nul".into()],
        }];
        let domains = vec![("y".to_string(), "127.0.0.1".to_string())];
        let (_plan, script, out) = write_test_runner(&tmp, &steps, &domains, false).unwrap();

        // Скрипт обязан быть ASCII (BOM допустим) — иначе PS 5.1 ломает кириллицу.
        let raw = std::fs::read(&script).unwrap();
        let body = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
        assert!(body.iter().all(|b| *b < 0x80), "runner должен быть ASCII-only");
        let script_text = std::str::from_utf8(body).unwrap();
        assert!(script_text.contains("$argLine"));
        assert!(script_text.contains("if ($a -match '[\\s\"]')"));
        // Проба — HTTPS (TLS+HTTP), не голый TCP-connect.
        assert!(script_text.contains("https://$target/"), "проба должна быть HTTPS");
        assert!(script_text.contains("Add-Type -AssemblyName System.Net.Http"), "нужен Add-Type для PS 5.1");
        assert!(!script_text.contains("TcpClient"), "TCP-проба не должна вернуться");

        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .status()
            .unwrap();
        assert!(status.success(), "раннер завершился с ошибкой");

        let v = read_test_progress(&out).expect("нет результата раннера");
        assert_eq!(v["total"].as_u64(), Some(1));
        let results = v["results"].as_array().unwrap();
        let r: StrategyResult = serde_json::from_value(results[0].clone()).unwrap();
        assert!(r.started, "процесс не запустился: {:?}", r.error);
        // Проба обязана отработать без исключений: 127.0.0.1 просто недоступен.
        // (Именно так ловится «Не удается найти тип HttpClientHandler» в PS 5.1.)
        assert!(r.error.is_none(), "проба упала с ошибкой: {:?}", r.error);
        // 127.0.0.1 без HTTPS-сервера — недоступен: тест герметичен (без интернета).
        assert_eq!(r.score, 0, "локальный адрес не должен считаться доступным");
        assert_eq!(r.max_score, 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
