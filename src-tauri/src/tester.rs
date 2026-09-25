use crate::config::Profile;
use std::path::{Path, PathBuf};

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
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let plan_path = data.join("logs/test-plan.json");
    let out_path = data.join("logs/test-out.json");
    let script_path = data.join("logs/test-runner.ps1");
    let _ = std::fs::remove_file(&out_path);
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
# Probe method is copied from the author's harness (utils/test zapret.ps1):
# three curl checks per host - HTTP/1.1, TLS1.2, TLS1.3 - timeout 4 s,
# connect-timeout 3 s, small parallel batches. curl.exe ships with Windows
# 10+ and matches the reference measurements 1:1.
$probeTimeout = 4
$connectTimeout = 3
$probeParallel = 8
$curlExe = 'curl.exe'
$foundCurl = Get-Command curl.exe -ErrorAction SilentlyContinue
if ($foundCurl) {{ $curlExe = $foundCurl.Source }}

function Start-HttpsProbe {{
  # One curl run for one check; output "<code> <time_total>" goes to $file.
  # NB: parameter is NOT named $host - that name is a read-only automatic
  # variable in PowerShell and assignment to it fails.
  param($target, $label, $protoArgs)
  $safe = ($target -replace '[^A-Za-z0-9._-]', '_')
  $file = Join-Path $errDir ("probe-$safe-$label.txt")
  $errFile = Join-Path $errDir ("probe-$safe-$label.err.txt")
  $ca = @('-sS', '-o', 'NUL', '-m', "$probeTimeout", '--connect-timeout', "$connectTimeout") + $protoArgs + @('-w', '"%{{http_code}} %{{time_total}}"', "https://$target/")
  $p = Start-Process -FilePath $curlExe -ArgumentList $ca -WindowStyle Hidden -PassThru -RedirectStandardOutput $file -RedirectStandardError $errFile
  return [pscustomobject]@{{ host = $target; label = $label; proc = $p; file = $file }}
}}

function Finish-HttpsProbe {{
  # ok = any HTTP code received (three digits, not 000); ms from curl.
  param($check)
  $code = '000'
  $ms = 0
  if (Test-Path -LiteralPath $check.file) {{
    $t = (Get-Content -LiteralPath $check.file -Raw -ErrorAction SilentlyContinue)
    if ($t) {{ $t = $t.Trim() }}
    if ($t -match '^([0-9]{{3}})\s+([0-9.]+)$') {{
      $code = $matches[1]
      $ms = [int]([double]$matches[2] * 1000)
    }}
  }}
  $ok = ($code -ne '000')
  return [ordered]@{{ ok = $ok; code = $code; ms = $ms }}
}}

function Invoke-HttpsMatrix {{
  # Checks for every host: HTTP1.1, TLS1.2, TLS1.3; up to $probeParallel curl
  # processes at once. Returns host -> array of check results.
  param($hosts)
  $protos = @(
    @{{ label = 'HTTP1.1'; args = @('--http1.1') }},
    @{{ label = 'TLS1.2'; args = @('--tlsv1.2', '--tls-max', '1.2') }},
    @{{ label = 'TLS1.3'; args = @('--tlsv1.3', '--tls-max', '1.3') }}
  )
  $res = @{{}}
  $queue = @()
  foreach ($h in $hosts) {{
    $res[$h] = @()
    foreach ($p in $protos) {{ $queue += [pscustomobject]@{{ host = $h; label = $p.label; args = $p.args }} }}
  }}
  $running = @()
  $qi = 0
  while (($qi -lt $queue.Count) -or ($running.Count -gt 0)) {{
    while (($qi -lt $queue.Count) -and ($running.Count -lt $probeParallel)) {{
      $q = $queue[$qi]
      $qi++
      $running += Start-HttpsProbe -target $q.host -label $q.label -protoArgs $q.args
    }}
    if ($running.Count -gt 0) {{
      $first = $running[0]
      $first.proc.WaitForExit()
      $r = Finish-HttpsProbe $first
      $r['label'] = $first.label
      $res[$first.host] += $r
      $running = @($running | Where-Object {{ $_.proc.Id -ne $first.proc.Id }})
    }}
    # Stop flag: kill the remaining curl processes and return what we have.
    if (Test-Path -LiteralPath $plan.flag) {{
      foreach ($x in $running) {{ if ($x.proc -and -not $x.proc.HasExited) {{ Stop-Process -Id $x.proc.Id -Force -ErrorAction SilentlyContinue }} }}
      return $res
    }}
  }}
  return $res
}}

function New-DomainRows {{
  # host list -> domain rows (ok = any check passed, ms = fastest check).
  param($hosts, $matrix)
  $rows = @()
  foreach ($h in $hosts) {{
    $ok = $false
    $best = 0
    $parts = @()
    foreach ($c in @($matrix[$h])) {{
      $mark = 'fail'
      if ($c.ok) {{ $mark = 'ok' }}
      $parts += ("{{0}} {{1}} {{2}}" -f $c.label, $c.code, $mark)
      if ($c.ok) {{
        $ok = $true
        if (($best -eq 0) -or ($c.ms -lt $best)) {{ $best = $c.ms }}
      }}
    }}
    $rows += [ordered]@{{ host = $h; ok = $ok; ms = $best; detail = ($parts -join '; ') }}
  }}
  return $rows
}}

function Invoke-PingPass {{
  # One ICMP packet per host, all in parallel; true when ping.exe exit code 0.
  param($hosts)
  $info = @{{}}
  $procs = @()
  foreach ($h in $hosts) {{
    $pf = Join-Path $errDir ("ping-" + ($h -replace '[^A-Za-z0-9._-]', '_') + ".txt")
    $pp = Start-Process -FilePath 'ping.exe' -ArgumentList @('-n', '1', '-w', '1200', $h) -WindowStyle Hidden -PassThru -RedirectStandardOutput $pf -RedirectStandardError ($pf + '.err')
    $procs += [pscustomobject]@{{ host = $h; file = $pf; proc = $pp }}
  }}
  foreach ($pp in $procs) {{
    $pp.proc.WaitForExit()
    # ExitCode is not populated for Start-Process -PassThru with redirects;
    # "TTL=" in the reply is locale-independent proof of a successful ping.
    $txt = Get-Content -LiteralPath $pp.file -Raw -ErrorAction SilentlyContinue
    $info[$pp.host] = [bool]($txt -match 'TTL=')
  }}
  return $info
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
      # Curl matrix (same method as the author's harness): HTTP1.1 + TLS1.2 +
      # TLS1.3 per host in small parallel batches. Small batches keep the
      # WinDivert desync path from choking (author's harness: 92/105, our old
      # 115-at-once probe: 19/115).
      $allHosts = @($plan.domains | ForEach-Object {{ $_.host }})
      $matrix = Invoke-HttpsMatrix -hosts $allHosts
      $rows = New-DomainRows -hosts $allHosts -matrix $matrix
      # Every failed host gets one more chance: a browser-like retry, so a
      # single dropped connection/RST cannot mark a strategy as broken.
      $failedHosts = @($rows | Where-Object {{ -not $_.ok }} | ForEach-Object {{ $_.host }})
      if ($failedHosts.Count -gt 0) {{
        $matrix2 = Invoke-HttpsMatrix -hosts $failedHosts
        $rows2 = New-DomainRows -hosts $failedHosts -matrix $matrix2
        foreach ($r2 in $rows2) {{
          if ($r2.ok) {{
            $r = $rows | Where-Object {{ $_.host -eq $r2.host }} | Select-Object -First 1
            $r.ok = $true
            $r.ms = $r2.ms
            $r.detail = $r.detail + ' | retry: ' + $r2.detail
          }}
        }}
      }}
      # ICMP ping for every host (informational only, like the author's harness).
      $pingOk = Invoke-PingPass -hosts $allHosts
      $doms = @()
      foreach ($d in $plan.domains) {{
        $r = $rows | Where-Object {{ $_.host -eq $d.host }} | Select-Object -First 1
        $detail = $r.detail
        $pingMark = 'ping fail'
        if ($pingOk[$d.host]) {{ $pingMark = 'ping ok' }}
        $detail = $detail + '; ' + $pingMark
        $doms += [ordered]@{{ key = $d.key; host = $d.host; group = $d.group; groupLabel = $d.groupLabel; ok = [bool]$r.ok; ms = [int]$r.ms; detail = $detail }}
      }}
      # Stop flag: the user pressed "stop" (or the GUI is closing) - kill the
      # engine right away; the step loop writes STOPPED itself, so the GUI
      # never waits minutes for the current step.
      if (Test-Path -LiteralPath $plan.flag) {{
        if ($p -and -not $p.HasExited) {{ Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }}
      }}
      $res.domains = $doms
      $res.score = @($doms | Where-Object {{ $_.ok }}).Count
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
        // Оба хоста без сервера: curl-матрица обязана корректно завершиться,
        // ничего не пройдёт. Тест герметичен (без сети).
        let domains = vec![
            ("y".to_string(), "127.0.0.1".to_string()),
            ("x".to_string(), "example.invalid".to_string()),
        ];
        let (_plan, script, out) = write_test_runner(&tmp, &steps, &domains).unwrap();

        // Скрипт обязан быть ASCII (BOM допустим) — иначе PS 5.1 ломает кириллицу.
        let raw = std::fs::read(&script).unwrap();
        let body = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
        assert!(body.iter().all(|b| *b < 0x80), "runner должен быть ASCII-only");
        let script_text = std::str::from_utf8(body).unwrap();
        assert!(script_text.contains("$argLine"));
        assert!(script_text.contains("if ($a -match '[\\s\"]')"));
        // Проба — методика автора: curl HTTP/1.1 + TLS1.2 + TLS1.3 на домен.
        assert!(script_text.contains("https://$target/"), "проба должна быть HTTPS");
        assert!(script_text.contains("--http1.1"), "нужна HTTP/1.1-проверка");
        assert!(script_text.contains("--tlsv1.2"), "нужна TLS1.2-проверка");
        assert!(script_text.contains("--tlsv1.3"), "нужна TLS1.3-проверка");
        assert!(script_text.contains("--connect-timeout"), "connect-timeout как у автора");
        assert!(script_text.contains("Invoke-HttpsMatrix"), "матрица проб должна быть в скрипте");
        assert!(script_text.contains("retry: "), "повтор для всех упавших доменов");
        assert!(script_text.contains("ping ok"), "ICMP-пинг хостов остаётся информационным");
        assert!(!script_text.contains("ping-only:"), "DNS-пинг убран из теста");
        assert!(!script_text.contains("TcpClient"), "TCP-проба не должна вернуться");
        assert!(!script_text.contains("System.Net.Http"), "HttpClient-проба больше не используется");

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
        // Проба обязана отработать без исключений (curl-запуск и ping).
        assert!(r.error.is_none(), "проба упала с ошибкой: {:?}", r.error);
        // Ни одного HTTPS-сервера нет — очков быть не должно.
        assert_eq!(r.score, 0, "без сервера домены не проходят");
        assert_eq!(r.max_score, 2);

        let _ = std::fs::remove_dir_all(&tmp);
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
