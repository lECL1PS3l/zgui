using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text;
using System.Threading;
using ZapretGui.Core;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;
using ZapretGui.Core.Net;
using ZapretGui.Core.Runtime;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Tester
{
    /// <summary>
    /// Группа доменов для оценки стратегии (tester.rs:11-25).
    /// </summary>
    public class DomainGroup
    {
        public readonly string Id;
        public readonly string Label;
        public readonly bool Critical;
        public readonly int Priority;

        public DomainGroup(string id, string label, bool critical, int priority)
        {
            Id = id;
            Label = label;
            Critical = critical;
            Priority = priority;
        }
    }

    /// <summary>Сводка по одной группе доменов в результате стратегии (tester.rs:27-37).</summary>
    public class GroupResult
    {
        [JsonField("id")] public string Id;
        [JsonField("label")] public string Label;
        [JsonField("passed")] public int Passed;
        [JsonField("total")] public int Total;
        [JsonField("ok")] public bool Ok;
        [JsonField("critical")] public bool Critical;
        [JsonField("priority")] public int Priority;
    }

    /// <summary>Результат пробы одного домена (tester.rs:74-86).</summary>
    public class DomainResult
    {
        [JsonField("key")] public string Key;
        [JsonField("host")] public string Host;
        [JsonField("group")] public string Group;
        [JsonField("groupLabel")] public string GroupLabel;
        [JsonField("ok")] public bool Ok;
        [JsonField("ms")] public long Ms;
        [JsonField("detail")] public string Detail;
    }

    /// <summary>Результат пробы всех доменов для одной стратегии (tester.rs:88-104).</summary>
    public class StrategyResult
    {
        [JsonField("id")] public string Id;
        [JsonField("name")] public string Name;
        [JsonField("engine")] public string Engine;
        [JsonField("group")] public string Group;
        [JsonField("started")] public bool Started;
        [JsonField("score")] public int Score;
        [JsonField("maxScore")] public int MaxScore;
        [JsonField("domains")] public List<DomainResult> Domains = new List<DomainResult>();
        [JsonField("error")] public string Error;
        [JsonField("groups")] public List<GroupResult> Groups = new List<GroupResult>();
        [JsonField("criticalOk")] public bool CriticalOk;
    }

    /// <summary>Прогресс теста для UI (tester.rs:106-121).</summary>
    public class TestProgress
    {
        [JsonField("running")] public bool Running;
        [JsonField("phase")] public string Phase = "";
        [JsonField("currentId")] public string CurrentId;
        [JsonField("currentName")] public string CurrentName;
        [JsonField("index")] public int Index;
        [JsonField("total")] public int Total;
        [JsonField("pct")] public int Pct;
        [JsonField("msg")] public string Msg = "";
        [JsonField("results")] public List<StrategyResult> Results = new List<StrategyResult>();
        [JsonField("bestId")] public string BestId;
        [JsonField("bestName")] public string BestName;
        [JsonField("done")] public bool Done;
    }

    /// <summary>Кэш прошедших тестов: data/tests.json (tester.rs:277-299).</summary>
    public class TestCache
    {
        [JsonField("testedAt")] public string TestedAt;
        [JsonField("bestId")] public string BestId;
        [JsonField("results")] public List<StrategyResult> Results = new List<StrategyResult>();

        public static TestCache Load(string dataDir)
        {
            string path = Path.Combine(dataDir, "tests.json");
            string raw;
            try { raw = File.ReadAllText(path); } catch { return new TestCache(); }
            TestCache c;
            return Json.TryParse(raw, out c) ? c : new TestCache();
        }

        public void Save(string dataDir)
        {
            try
            {
                Text.AtomicWrite(Path.Combine(dataDir, "tests.json"),
                    Encoding.UTF8.GetBytes(Json.Serialize(this)));
            }
            catch { }
        }
    }

    /// <summary>Один запуск на стратегию во встроенном PS-раннере (tester.rs:315-325).</summary>
    public class TestStep
    {
        [JsonField("id")] public string Id;
        [JsonField("name")] public string Name;
        [JsonField("engine")] public string Engine;
        [JsonField("group")] public string Group;
        [JsonField("exe")] public string Exe;
        [JsonField("workdir")] public string Workdir;
        [JsonField("args")] public List<string> Args = new List<string>();
    }

    /// <summary>
    /// Тестер стратегий: группы доменов, списки проб, встроенный PowerShell-раннер
    /// (один UAC на все стратегии), кэш результатов, выбор лучшей стратегии —
    /// порт tester.rs + lib.rs:1075-1660 (test_strategies/cancel_test/apply_best_strategy).
    /// </summary>
    public static class Tester
    {
        public static readonly DomainGroup GroupYoutube = new DomainGroup("youtube", "YouTube", true, 1);
        public static readonly DomainGroup GroupYoutubeMusic = new DomainGroup("youtube-music", "YouTube Music", true, 1);
        public static readonly DomainGroup GroupDiscord = new DomainGroup("discord", "Discord", true, 1);
        public static readonly DomainGroup GroupMicrosoftXbox = new DomainGroup("microsoft-xbox", "Microsoft / Xbox", false, 2);
        public static readonly DomainGroup GroupGoogle = new DomainGroup("google", "Google", false, 2);
        public static readonly DomainGroup GroupCloudflare = new DomainGroup("cloudflare", "Cloudflare", false, 2);
        public static readonly DomainGroup GroupOther = new DomainGroup("other", "Дополнительные домены", false, 3);

        /// <summary>
        /// Обязательные проверки — всегда в тесте. YouTube Music скрыт в UI,
        /// но остаётся жёстким требованием к успешной стратегии (tester.rs:194-215).
        /// </summary>
        public static readonly string[] RequiredDomains =
        {
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
        };

        // Файлы теста в data/logs (lib.rs:1075-1081).
        private const string PlanFile = "test-plan.json";
        private const string ScriptFile = "test-runner.ps1";
        private const string OutFile = "test-out.json";
        private const string BaselineFile = "test-baseline.json";
        private const string StopFlagFile = "test-stop.flag";
        private const string RunnerPidFile = "test-runner.pid";
        private const string WinPidFile = "test-current.pid";

        private static readonly object _lock = new object();
        private static TestProgress _current = new TestProgress();

        /// <summary>Текущее состояние теста; UI опрашивает его (lib.rs set_test).</summary>
        public static TestProgress Current
        {
            get { lock (_lock) { return _current; } }
        }

        /// <summary>Событие прогресса теста — GUI подписывается на обновления экрана.</summary>
        public static event Action<TestProgress> ProgressChanged;

        /// <summary>Событие уведомления: (kind, text), kind = ok/warn/info.</summary>
        public static event Action<string, string> ToastReceived;

        // ------------------------------------------------------------ группы

        private static readonly string[] YoutubeHosts =
        {
            "youtube.com", "www.youtube.com", "youtu.be", "youtube-nocookie.com",
            "youtube.googleapis.com", "youtubei.googleapis.com", "googlevideo.com",
            "ytimg.com", "ytimg.l.google.com", "yt3.googleusercontent.com",
        };

        private static readonly string[] DiscordHosts =
        {
            "discord.com", "discord.gg", "discord.media", "discordapp.com", "discordapp.net",
            "discordapp.io", "discordapp.org", "discordstatus.com", "discord.status",
            "gateway.discord.gg", "dl.discordapp.net", "images.discordapp.net", "status.discordapp.com",
        };

        private static readonly string[] XboxHosts =
        {
            "login.live.com", "account.live.com", "microsoft.com", "www.microsoft.com",
            "xbox.com", "www.xbox.com", "xboxlive.com", "xboxservices.com",
        };

        private static bool HostIn(string host, string[] list)
        {
            return Array.IndexOf(list, host) >= 0;
        }

        /// <summary>Классифицирует домен по группе важности (tester.rs:39-64).</summary>
        public static DomainGroup ClassifyDomain(string host)
        {
            string h = host.ToLowerInvariant();
            if (h == "music.youtube.com") { return GroupYoutubeMusic; }
            if (HostIn(h, YoutubeHosts) || h.EndsWith(".youtube.com", StringComparison.Ordinal)) { return GroupYoutube; }
            if (HostIn(h, DiscordHosts) || h.EndsWith(".discord.com", StringComparison.Ordinal) || h.EndsWith(".discordapp.com", StringComparison.Ordinal)) { return GroupDiscord; }
            if (HostIn(h, XboxHosts) || h.EndsWith(".xbox.com", StringComparison.Ordinal) || h.EndsWith(".xboxlive.com", StringComparison.Ordinal) || h.EndsWith(".xboxservices.com", StringComparison.Ordinal)) { return GroupMicrosoftXbox; }
            // Google AI projects are intentionally not part of the Google secondary group.
            if (h.Contains("gemini") || h.Contains("aistudio") || h.Contains("notebooklm") ||
                h.EndsWith(".ai.google", StringComparison.Ordinal) || h.Contains("labs.google"))
            {
                return GroupOther;
            }
            if (h == "google.com" || h.EndsWith(".google.com", StringComparison.Ordinal) ||
                h.EndsWith(".googleusercontent.com", StringComparison.Ordinal) || h.EndsWith(".googleapis.com", StringComparison.Ordinal))
            {
                return GroupGoogle;
            }
            if (h == "cloudflare.com" || h.EndsWith(".cloudflare.com", StringComparison.Ordinal) ||
                h.EndsWith(".cloudflare.net", StringComparison.Ordinal) || h == "cloudflare-dns.com" || h == "one.one.one.one")
            {
                return GroupCloudflare;
            }
            return GroupOther;
        }

        /// <summary>
        /// Группа критических доменов пройдена (tester.rs:66-72): youtube-music
        /// требует хотя бы одного успеха, остальные критические — не менее половины.
        /// </summary>
        public static bool CriticalGroupOk(DomainGroup group, int passed, int total)
        {
            if (group.Id == GroupYoutubeMusic.Id)
            {
                return total > 0 && passed > 0;
            }
            return !group.Critical || (total > 0 && passed * 2 >= total);
        }

        // ------------------------------------------------------------ списки доменов

        /// <summary>
        /// Отсекает мусорные строки из .lst: `.ua`, IP-адреса, `*.domain`,
        /// ведущие/хвостовые точки (tester.rs:153-167).
        /// </summary>
        public static bool UsableTestHost(string host)
        {
            if (string.IsNullOrEmpty(host) || host.Length > 253) { return false; }
            if (host.IndexOf('*') >= 0 || host.Contains("..")) { return false; }
            if (host[0] == '.' || host[host.Length - 1] == '.') { return false; }
            bool hasAlpha = false;
            foreach (char c in host)
            {
                if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')) { hasAlpha = true; break; }
            }
            if (!hasAlpha) { return false; }
            string[] labels = host.Split('.');
            if (labels.Length < 2) { return false; }
            foreach (string l in labels)
            {
                if (l.Length == 0) { return false; }
                foreach (char c in l)
                {
                    bool ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                              (c >= '0' && c <= '9') || c == '-' || c == '_';
                    if (!ok) { return false; }
                }
            }
            return true;
        }

        /// <summary>
        /// Разбор одной строки .lst в домен: отбрасывает комментарии, пути и мусор
        /// (tester.rs:171-190).
        /// </summary>
        public static KeyValuePair<string, string>? ParseRuleLine(string raw, HashSet<string> seen)
        {
            string line = raw.Trim();
            if (line.Length == 0 || line[0] == '#' || line.StartsWith("//", StringComparison.Ordinal))
            {
                return null;
            }
            string host = line.Split(new[] { '/', ' ', '\t' })[0].Trim().ToLowerInvariant();
            if (!UsableTestHost(host)) { return null; }
            if (!seen.Add(host)) { return null; }
            string key = host.Split('.')[0];
            return new KeyValuePair<string, string>(key, host);
        }

        private static void Truncate(List<KeyValuePair<string, string>> list, int limit)
        {
            if (list.Count > limit) { list.RemoveRange(limit, list.Count - limit); }
        }

        /// <summary>
        /// Основной тест: обязательные + ручной вшитый список (tester.rs:219-243).
        /// Онлайн-геоблок сюда не попадает — для него есть load_geoblock_domains.
        /// </summary>
        public static List<KeyValuePair<string, string>> LoadDomainsFromLists(string dataDir, int limit)
        {
            var outList = new List<KeyValuePair<string, string>>();
            var seen = new HashSet<string>(StringComparer.Ordinal);

            foreach (string host in RequiredDomains)
            {
                seen.Add(host);
                outList.Add(new KeyValuePair<string, string>(host.Split('.')[0], host));
            }
            if (outList.Count >= limit)
            {
                Truncate(outList, limit);
                return outList;
            }

            foreach (string raw in BuiltinDomainLines())
            {
                KeyValuePair<string, string>? d = ParseRuleLine(raw, seen);
                if (d.HasValue)
                {
                    outList.Add(d.Value);
                    if (outList.Count >= limit) { break; }
                }
            }
            return outList;
        }

        /// <summary>Онлайн-списки геоблока — отдельный диагностический тест (tester.rs:246-274).</summary>
        public static List<KeyValuePair<string, string>> LoadGeoblockDomains(string dataDir, int limit)
        {
            var outList = new List<KeyValuePair<string, string>>();
            var seen = new HashSet<string>(StringComparer.Ordinal);
            string dir = Path.Combine(dataDir, "catalog\\geoblock");
            string[] files =
            {
                "allow-domains-russia-inside.lst",
                "allow-domains-geoblock.lst",
                "allow-domains-youtube.lst",
                "allow-domains-discord.lst",
                "allow-domains-news.lst",
                "allow-domains-telegram.lst",
                "allow-domains-twitter.lst",
                "allow-domains-meta.lst",
                "allow-domains-block.lst",
            };
            foreach (string f in files)
            {
                string text;
                try { text = File.ReadAllText(Path.Combine(dir, f)); }
                catch { continue; }
                foreach (string raw in text.Split('\n'))
                {
                    KeyValuePair<string, string>? d = ParseRuleLine(raw, seen);
                    if (d.HasValue)
                    {
                        outList.Add(d.Value);
                        if (outList.Count >= limit) { return outList; }
                    }
                }
            }
            return outList;
        }

        /// <summary>
        /// Вшитый ручной список доменов из ресурсов программы
        /// (tester.rs:4 include_bytes resources/test-domains-russia.lst).
        /// </summary>
        private static string[] _builtinLines;

        private static string[] BuiltinDomainLines()
        {
            if (_builtinLines != null) { return _builtinLines; }
            byte[] bytes = ReadBuiltinDomains();
            _builtinLines = bytes == null
                ? new string[0]
                : Encoding.UTF8.GetString(bytes).Split('\n');
            return _builtinLines;
        }

        private static byte[] ReadBuiltinDomains()
        {
            try
            {
                Assembly asm = Assembly.GetExecutingAssembly();
                foreach (string name in asm.GetManifestResourceNames())
                {
                    if (name.EndsWith("test-domains-russia.lst", StringComparison.Ordinal))
                    {
                        using (Stream s = asm.GetManifestResourceStream(name))
                        {
                            var buf = new byte[s.Length];
                            int read = 0;
                            while (read < buf.Length)
                            {
                                int n = s.Read(buf, read, buf.Length - read);
                                if (n <= 0) { break; }
                                read += n;
                            }
                            return buf;
                        }
                    }
                }
            }
            catch { }
            return null;
        }

        /// <summary>
        /// Жив ли фоновый раннер теста: по маркерам logs/test-runner.pid и
        /// logs/test-stop.flag (tester.rs:304-312).
        /// </summary>
        public static bool RunnerAlive(string dataDir)
        {
            string pidFile = Path.Combine(dataDir, "logs", RunnerPidFile);
            string flag = Path.Combine(dataDir, "logs", StopFlagFile);
            try
            {
                if (!File.Exists(pidFile)) { return false; }
                uint pid;
                if (!uint.TryParse(File.ReadAllText(pidFile).Trim(), out pid)) { return false; }
                return !File.Exists(flag) && Processes.PidAlive((int)pid);
            }
            catch { return false; }
        }

        // ------------------------------------------------------------ кэш/списки калибровки

        /// <summary>Читает базовую пробу (без Zapret): host → был ли доступен напрямую (tester.rs:529-545).</summary>
        public static List<KeyValuePair<string, bool>> ReadBaseline(string dataDir)
        {
            var outList = new List<KeyValuePair<string, bool>>();
            string text;
            try { text = File.ReadAllText(Path.Combine(dataDir, "logs", BaselineFile)); }
            catch { return outList; }
            text = text.TrimStart('\uFEFF').Trim();
            List<object> arr;
            if (!Json.TryParse(text, out arr)) { return outList; }
            foreach (object item in arr)
            {
                var d = item as Dictionary<string, object>;
                if (d == null) { continue; }
                object hostObj;
                if (!d.TryGetValue("host", out hostObj)) { continue; }
                object okObj;
                d.TryGetValue("ok", out okObj);
                outList.Add(new KeyValuePair<string, bool>(hostObj as string, okObj is bool && (bool)okObj));
            }
            return outList;
        }

        /// <summary>
        /// Сохраняет результаты калибровки: обходимые Zapret домены и требующие VPN
        /// (tester.rs:550-562).
        /// </summary>
        public static void SaveReachability(string dataDir, IList<string> reachable, IList<string> vpnOnly)
        {
            string dir = Path.Combine(dataDir, "catalog\\geoblock");
            try { Directory.CreateDirectory(dir); } catch { }
            WriteSortedList(dir, "zapret-reachable.lst", reachable);
            WriteSortedList(dir, "vpn-only.lst", vpnOnly);
        }

        private static void WriteSortedList(string dir, string name, IList<string> list)
        {
            var sorted = new List<string>(list);
            sorted.Sort(StringComparer.Ordinal);
            var sb = new StringBuilder();
            string last = null;
            foreach (string s in sorted)
            {
                if (last == s) { continue; }
                last = s;
                sb.Append(s).Append('\n');
            }
            try { File.WriteAllText(Path.Combine(dir, name), sb.ToString()); } catch { }
        }

        /// <summary>Список ранее откалиброванных «обходимых» доменов (tester.rs:567-574).</summary>
        public static List<string> LoadReachable(string dataDir)
        {
            return ReadDomainList(Path.Combine(dataDir, "catalog\\geoblock\\zapret-reachable.lst"));
        }

        /// <summary>Список доменов, которым нужен только VPN (tester.rs:578-585).</summary>
        public static List<string> LoadVpnOnly(string dataDir)
        {
            return ReadDomainList(Path.Combine(dataDir, "catalog\\geoblock\\vpn-only.lst"));
        }

        private static List<string> ReadDomainList(string path)
        {
            var outList = new List<string>();
            string text;
            try { text = File.ReadAllText(path); } catch { return outList; }
            foreach (string l in text.Split('\n'))
            {
                string t = l.Trim().ToLowerInvariant();
                if (t.Length > 0 && t[0] != '#') { outList.Add(t); }
            }
            return outList;
        }

        // ------------------------------------------------------------ раннер

        private class PlanDomain
        {
            [JsonField("key")] public string Key;
            [JsonField("host")] public string Host;
            [JsonField("group")] public string Group;
            [JsonField("groupLabel")] public string GroupLabel;
            [JsonField("critical")] public bool Critical;
            [JsonField("priority")] public int Priority;
        }

        private class TestPlanFile
        {
            [JsonField("steps")] public List<TestStep> Steps;
            [JsonField("domains")] public List<PlanDomain> Domains;
            [JsonField("out")] public string Out;
            [JsonField("baselineOut")] public string BaselineOut;
            [JsonField("baseline")] public bool Baseline;
            [JsonField("pid")] public string Pid;
            [JsonField("winPid")] public string WinPid;
            [JsonField("flag")] public string Flag;
        }

        /// <summary>
        /// PowerShell-раннер: выполняет ВСЕ стратегии в одном элевированном процессе
        /// (один UAC-запрос на весь тест). Пишется в ASCII (BOM допустим) — иначе
        /// PS 5.1 ломает кириллицу; пути квотируются отдельно от Start-Process.
        /// </summary>
        private const string RunnerScript = @"
$ErrorActionPreference = 'Continue'
$stopRun = $false
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$plan = Get-Content -Raw -Encoding UTF8 -LiteralPath @@PLAN@@ | ConvertFrom-Json
$PID | Out-File -LiteralPath $plan.pid -Encoding ascii
$out = $plan.out
$errDir = Split-Path -Parent $out
$all = @()
$i = 0
# HTTPS probe. TCP connect is not enough: DPI lets the TCP handshake through
# and cuts TLS by SNI, so a blocked site ""passes"" while the browser cannot open
# it. We check the full HTTPS exchange (TLS + HTTP headers), like a browser does.
# Add-Type is required: in PS 5.1 the type System.Net.Http.HttpClientHandler is
# not resolved until the System.Net.Http assembly is loaded (checked on Win11).
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Add-Type -AssemblyName System.Net.Http
function New-ProbeClient {
  $h = New-Object System.Net.Http.HttpClientHandler
  $h.UseProxy = $false
  $h.AllowAutoRedirect = $false
  $c = New-Object System.Net.Http.HttpClient -ArgumentList $h
  $c.Timeout = [TimeSpan]::FromSeconds(6)
  return $c
}
function Start-Probe($client, $target) {
  try {
    return $client.GetAsync(""https://$target/"", [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead)
  } catch {
    return $null
  }
}
function Finish-Probe($task) {
  if ($null -eq $task) { return [ordered]@{ ok = $false; detail = 'request failed' } }
  try {
    $resp = $task.GetAwaiter().GetResult()
    $code = [int]$resp.StatusCode
    $resp.Dispose()
    return [ordered]@{ ok = $true; detail = ""http $code"" }
  } catch {
    $msg = $_.Exception.Message
    if ($_.Exception.InnerException) { $msg = $_.Exception.InnerException.Message }
    if (-not $msg) { $msg = 'failed' }
    return [ordered]@{ ok = $false; detail = $msg }
  }
}
if ($plan.baseline) {
  # Baseline probe WITHOUT Zapret: distinguishes ""site down / not resolving""
  # from ""blocked but bypassable"". Written to a separate file.
  # Probes run in batches: thousands of geoblock domains must not open all at once.
  $baseOut = @()
  $baseClient = New-ProbeClient
  $batchSize = 300
  $bi = 0
  for ($offset = 0; $offset -lt $plan.domains.Count; $offset += $batchSize) {
    $end = [Math]::Min($offset + $batchSize - 1, $plan.domains.Count - 1)
    $baseChecks = @()
    foreach ($d in $plan.domains[$offset..$end]) {
      $baseChecks += [pscustomobject]@{ host = $d.host; task = (Start-Probe $baseClient $d.host) }
    }
    foreach ($c in $baseChecks) {
      $r = Finish-Probe $c.task
      $baseOut += [ordered]@{ host = $c.host; ok = [bool]$r.ok }
      $bi++
    }
    $state = [ordered]@{ baseline = [ordered]@{ done = $bi; total = $plan.domains.Count }; results = @() }
    ($state | ConvertTo-Json -Depth 4) | Out-File -LiteralPath $out -Encoding utf8
  }
  $baseClient.Dispose()
  ($baseOut | ConvertTo-Json -Depth 4) | Out-File -LiteralPath $plan.baselineOut -Encoding utf8
}
foreach ($step in $plan.steps) {
  if (Test-Path -LiteralPath $plan.flag) { $stopRun = $true; break }
  $i++
  $res = [ordered]@{ id = $step.id; name = $step.name; engine = $step.engine; group = $step.group; started = $false; score = 0; maxScore = $plan.domains.Count; domains = @(); groups = @(); criticalOk = $false; error = $null }
  $safe = ($step.id -replace '[^A-Za-z0-9._-]', '_')
  $errFile = Join-Path $errDir (""run-$safe.err.txt"")
  $outFile = Join-Path $errDir (""run-$safe.out.txt"")
  try {
    # Start-Process joins string arrays without quoting values containing spaces.
    # Every path below can contain spaces, so create one correctly quoted command line.
    $argLine = @($step.args | ForEach-Object {
      $a = [string]$_
      if ($a -match '[\s""]') { '""' + $a.Replace('""', '\""') + '""' } else { $a }
    }) -join ' '
    $p = Start-Process -FilePath $step.exe -WorkingDirectory $step.workdir -WindowStyle Hidden -ArgumentList $argLine -PassThru -RedirectStandardError $errFile -RedirectStandardOutput $outFile
      $p.Id | Out-File -LiteralPath $plan.winPid -Encoding ascii
    Start-Sleep -Milliseconds 1800
    if ($p -and -not $p.HasExited) {
      $res.started = $true
      # HTTPS probes in batches: connections inside a batch run in parallel,
      # timeouts do not add up (100+ domains cost ~one timeout, not a hundred).
      $doms = @()
      $client = New-ProbeClient
      $batchSize = 300
      for ($offset = 0; $offset -lt $plan.domains.Count; $offset += $batchSize) {
        $end = [Math]::Min($offset + $batchSize - 1, $plan.domains.Count - 1)
        $checks = @()
        foreach ($d in $plan.domains[$offset..$end]) {
          $checks += [pscustomobject]@{ key = $d.key; host = $d.host; group = $d.group; groupLabel = $d.groupLabel; started = [DateTime]::UtcNow; task = (Start-Probe $client $d.host) }
        }
        foreach ($c in $checks) {
          $r = Finish-Probe $c.task
          $ms = [int]([DateTime]::UtcNow - $c.started).TotalMilliseconds
          $doms += [ordered]@{ key = $c.key; host = $c.host; group = $c.group; groupLabel = $c.groupLabel; ok = [bool]$r.ok; ms = $ms; detail = $r.detail }
        }
      }
      $client.Dispose()
      $res.domains = $doms
      $res.score = @($doms | Where-Object { $_.ok }).Count
      $groupRows = @()
      foreach ($group in @($plan.domains | Group-Object group)) {
        $items = @($doms | Where-Object { $_.group -eq $group.Name })
        $passed = @($items | Where-Object { $_.ok }).Count
        $first = $group.Group | Select-Object -First 1
        $isCritical = [bool]$first.critical
        $isMusic = $group.Name -eq 'youtube-music'
        $okGroup = if ($isMusic) { $passed -gt 0 } else { $passed -gt 0 -and ($passed * 2 -ge $items.Count) }
        $groupRows += [ordered]@{ id = $group.Name; label = $first.groupLabel; passed = $passed; total = $items.Count; ok = $okGroup; critical = $isCritical; priority = $first.priority }
      }
      $res.groups = $groupRows
      $res.criticalOk = @($groupRows | Where-Object { $_.critical -and -not $_.ok }).Count -eq 0
      if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    } else {
      $tail = ''
      if (Test-Path $errFile) { $tail = (Get-Content -LiteralPath $errFile -Raw -ErrorAction SilentlyContinue) }
      if (-not $tail -and (Test-Path $outFile)) { $tail = (Get-Content -LiteralPath $outFile -Raw -ErrorAction SilentlyContinue) }
      $code = if ($p) { try { $p.WaitForExit(); $p.ExitCode } catch { '?' } } else { 'null' }
      if (-not $isAdmin) {
        $res.error = ""ADMIN_REQUIRED: winws needs administrator rights - run the GUI as admin (exit $code) "" + $tail
      } else {
        $res.error = ""process exited immediately (exit $code) "" + $tail
      }
    }
  } catch {
    $res.error = $_.Exception.Message
  }
  Start-Sleep -Milliseconds 400
  $all += [pscustomobject]$res
  $state = [ordered]@{ index = $i; total = $plan.steps.Count; currentId = $step.id; currentName = $step.name; results = $all }
  ($state | ConvertTo-Json -Depth 8) | Out-File -LiteralPath $out -Encoding utf8
}
$marker = if ($stopRun) { 'STOPPED' } else { 'DONE' }
$marker | Out-File -LiteralPath $out -Encoding utf8 -Append
";

        /// <summary>Пути файлов теста, написанных write_test_runner.</summary>
        public class RunnerFiles
        {
            public readonly string Plan;
            public readonly string Script;
            public readonly string Out;

            public RunnerFiles(string plan, string script, string outPath)
            {
                Plan = plan;
                Script = script;
                Out = outPath;
            }
        }

        /// <summary>
        /// Пишет план теста и PowerShell-раннер (tester.rs:329-523). Возвращает
        /// пути (план, скрипт, вывод прогресса).
        /// </summary>
        public static RunnerFiles WriteTestRunner(
            string dataDir, IList<TestStep> steps, IList<KeyValuePair<string, string>> domains, bool baseline)
        {
            string logs = Path.Combine(dataDir, "logs");
            try { Directory.CreateDirectory(logs); } catch { }
            string planPath = Path.Combine(logs, PlanFile);
            string outPath = Path.Combine(logs, OutFile);
            string baselinePath = Path.Combine(logs, BaselineFile);
            string scriptPath = Path.Combine(logs, ScriptFile);
            string stopFlag = Path.Combine(logs, StopFlagFile);
            string runPid = Path.Combine(logs, RunnerPidFile);
            string winPid = Path.Combine(logs, WinPidFile);

            TryDelete(outPath);
            TryDelete(baselinePath);
            // Ступ-маркеры «резкого останова»: флаг + PID раннера + PID активного winws.
            TryDelete(stopFlag);
            TryDelete(runPid);
            TryDelete(winPid);

            var planDomains = new List<PlanDomain>(domains.Count);
            foreach (KeyValuePair<string, string> d in domains)
            {
                DomainGroup g = ClassifyDomain(d.Value);
                planDomains.Add(new PlanDomain
                {
                    Key = d.Key,
                    Host = d.Value,
                    Group = g.Id,
                    GroupLabel = g.Label,
                    Critical = g.Critical,
                    Priority = g.Priority,
                });
            }

            var plan = new TestPlanFile
            {
                Steps = new List<TestStep>(steps),
                Domains = planDomains,
                Out = outPath,
                BaselineOut = baselinePath,
                Baseline = baseline,
                Pid = runPid,
                WinPid = winPid,
                Flag = stopFlag,
            };
            File.WriteAllText(planPath, Json.Serialize(plan), new UTF8Encoding(true));

            string script = Uac.PsHeader + "\n" +
                RunnerScript.Replace("@@PLAN@@", Uac.PsQuote(planPath));
            Uac.WritePs1(scriptPath, script);
            return new RunnerFiles(planPath, scriptPath, outPath);
        }

        private static void TryDelete(string path)
        {
            try { File.Delete(path); } catch { }
        }

        /// <summary>Читает частичный/финальный результат встроенного раннера (tester.rs:588-599).</summary>
        public static Dictionary<string, object> ReadTestProgress(string outPath)
        {
            string text;
            try { text = File.ReadAllText(outPath); }
            catch { return null; }
            // BOM допустим в любом месте файла: PS 5.1 Out-File -Append пишет его снова.
            text = text.Replace("\uFEFF", string.Empty).Trim();
            int i = text.IndexOf("\nDONE", StringComparison.Ordinal);
            if (i >= 0) { text = text.Substring(0, i); }
            i = text.IndexOf("\nSTOPPED", StringComparison.Ordinal);
            if (i >= 0) { text = text.Substring(0, i); }
            text = text.Trim();
            if (text.Length == 0 || text[0] != '{') { return null; }
            Dictionary<string, object> v;
            return Json.TryParse(text, out v) ? v : null;
        }

        // ------------------------------------------------------------ сводка

        /// <summary>
        /// Формирует сводку: отсортированные результаты + лучшая стратегия
        /// (tester.rs:614-635). При равных очках выигрывает та, что прошла больше
        /// критических групп, затем — с меньшей средней задержкой; имя лишь
        /// последний детерминированный tie-break.
        /// </summary>
        public static void Summarize(List<StrategyResult> results, out List<StrategyResult> sorted, out string bestId)
        {
            sorted = new List<StrategyResult>(results);
            sorted.Sort(CompareResults);
            bestId = null;
            // первая в отсортированном списке, прошедшая критические группы — лучшая
            foreach (StrategyResult r in sorted)
            {
                if (r.Started && r.CriticalOk) { bestId = r.Id; break; }
            }
        }

        private static int CompareResults(StrategyResult a, StrategyResult b)
        {
            int c = b.Score.CompareTo(a.Score);
            if (c != 0) { return c; }
            c = CriticalGroupsPassed(b).CompareTo(CriticalGroupsPassed(a));
            if (c != 0) { return c; }
            c = AvgMs(a).CompareTo(AvgMs(b));
            if (c != 0) { return c; }
            return string.Compare(a.Name == null ? string.Empty : a.Name.ToLowerInvariant(),
                b.Name == null ? string.Empty : b.Name.ToLowerInvariant(), StringComparison.Ordinal);
        }

        private static int CriticalGroupsPassed(StrategyResult r)
        {
            int n = 0;
            if (r.Groups == null) { return n; }
            foreach (GroupResult g in r.Groups)
            {
                if (g.Critical && g.Ok) { n++; }
            }
            return n;
        }

        private static long AvgMs(StrategyResult r)
        {
            if (r.Domains == null || r.Domains.Count == 0) { return long.MaxValue; }
            long sum = 0;
            foreach (DomainResult d in r.Domains) { sum += d.Ms; }
            return sum / r.Domains.Count;
        }

        // ------------------------------------------------------------ состояние

        /// <summary>Текущий прогресс с подхватом теста, оставленного прошлой сессией (lib.rs:1088-1120).</summary>
        public static TestProgress Status(string dataDir)
        {
            TestProgress cur = Current;
            if (cur.Running) { return cur; }
            if (RunnerAlive(dataDir))
            {
                var resume = new TestProgress
                {
                    Running = true,
                    Phase = "resume",
                    Msg = "тест продолжается в фоне — его можно остановить",
                };
                Set(resume);
                return resume;
            }
            return cur;
        }

        private static void Set(TestProgress p)
        {
            lock (_lock) { _current = p; }
            Action<TestProgress> handler = ProgressChanged;
            if (handler != null)
            {
                try { handler(p); } catch { }
            }
        }

        private static void Toast(string kind, string text)
        {
            Action<string, string> handler = ToastReceived;
            if (handler != null)
            {
                try { handler(kind, text); } catch { }
            }
        }

        // ------------------------------------------------------------ запуск теста

        private class WorkerCtx
        {
            public State State;
            public string Data;
            public string Script;
            public string OutPath;
            public List<TestStep> Steps;
            public List<KeyValuePair<string, string>> Domains;
            public bool Elevated;
            public bool Geoblock;
            public Config.Runtime HadRuntime;
            public bool HadService;
        }

        /// <summary>
        /// Запускает каждую выбранную стратегию по очереди, проверяет контрольные
        /// домены и определяет лучшую (lib.rs:1126-1553). mode: "geoblock" —
        /// диагностический тест онлайн-списков, иначе ручной список.
        /// </summary>
        public static Result<bool> Run(State state, IList<string> ids, string mode)
        {
            bool geoblock = mode == "geoblock";
            if (Current.Running)
            {
                return Result<bool>.Err("тест уже выполняется");
            }

            var profiles = new List<Profile>();
            if (ids == null || ids.Count == 0)
            {
                foreach (Profile p in state.Profiles)
                {
                    if (p.Engine == Engines.Flowseal) { profiles.Add(p); }
                }
            }
            else
            {
                foreach (Profile p in state.Profiles)
                {
                    if (IdsContains(ids, p.Id)) { profiles.Add(p); }
                }
            }
            if (profiles.Count == 0)
            {
                return Result<bool>.Err("нет стратегий для теста");
            }
            // VPN мешает тесту — просим выгрузить (фронт показывает окно и вызывает kill_conflicts).
            List<ConflictProcess> vpn = Conflicts.DetectVpn();
            if (vpn.Count > 0)
            {
                return Result<bool>.Err("VPN_RUNNING:" + vpn.Count);
            }
            foreach (Profile p in profiles)
            {
                if (string.IsNullOrEmpty(state.Roots.Path(p.Engine)))
                {
                    return Result<bool>.Err("для «" + p.Name + "» не задан корень движка");
                }
            }

            // Основной тест идёт ровно по ручному списку (обязательные + вшитые) —
            // без геоблока. Если есть калибровка geoblock-теста, недоступные домены
            // (не обходятся Zapret) отсекаем, но обязательные критические группы не
            // трогаем. Диагностический геоблок-тест берёт ВЕСЬ онлайн-список + базовую пробу.
            List<KeyValuePair<string, string>> custom;
            if (geoblock)
            {
                custom = LoadGeoblockDomains(state.Data, int.MaxValue);
            }
            else
            {
                custom = LoadDomainsFromLists(state.Data, int.MaxValue);
                // Вырезаем ТОЛЬКО те домены, что калибровка отметила как «требует VPN».
                // Reachable-список НЕ используется как белый: иначе домены ручного
                // списка, которых нет в онлайн-геоблоке, выпадали бы.
                List<string> vpnOnly = LoadVpnOnly(state.Data);
                if (vpnOnly.Count > 0)
                {
                    var req = new HashSet<string>(RequiredDomains);
                    custom.RemoveAll(d => !vpnOnly.Contains(d.Value) || req.Contains(d.Value));
                }
            }
            if (custom.Count == 0)
            {
                return Result<bool>.Err("нет доменов для теста (списки пусты)");
            }

            // Готовим шаги: exe, рабочий каталог, аргументы с game-filter.
            GameFilter.Ports(state.Settings.GameFilter, out string tcp, out string udp);
            var steps = new List<TestStep>(profiles.Count);
            foreach (Profile p in profiles)
            {
                string root = state.Roots.Path(p.Engine);
                if (string.IsNullOrEmpty(root))
                {
                    return Result<bool>.Err("для «" + p.Name + "» не задан корень движка");
                }
                string exe = LocateExe(root, p.ExeName());
                if (exe == null)
                {
                    return Result<bool>.Err("не найден " + p.ExeName() + " в корне движка");
                }
                steps.Add(new TestStep
                {
                    Id = p.Id,
                    Name = p.Name,
                    Engine = p.Engine,
                    Group = Profiles.GroupOf(p),
                    Exe = exe,
                    Workdir = Path.Combine(root, "bin"),
                    Args = GameFilter.Apply(p.Args, tcp, udp),
                });
            }

            RunnerFiles files;
            try
            {
                files = WriteTestRunner(state.Data, steps, custom, geoblock);
            }
            catch (Exception e)
            {
                return Result<bool>.Err("не удалось написать раннер теста: " + e.Message);
            }
            int total = steps.Count;
            // Тест поднимает свои winws: текущий обход (профиль, служба, ручной .bat)
            // обязан быть остановлен. Иначе winws выходит сразу с «A copy of winws is
            // already running with the same filter», а проба проходит «чужим» обходом.
            Config.Runtime hadRuntime = state.Runtime;
            Service.State(out bool serviceInstalled, out bool? serviceRunning);
            bool hadService = serviceInstalled && serviceRunning == true;
            ProfileRunner.StopAllOwn(state);
            // Если winws всё ещё жив (чужой движок из другой папки или отказ UAC при
            // остановке) — тест даст мусор. Лучше честная ошибка.
            if (Conflicts.AnyWinwsRunning())
            {
                LogRing.Write("err", "test", "winws всё ещё запущен — тест отменён до остановки");
                return Result<bool>.Err("winws всё ещё запущен (обход вне программы или чужой процесс) — остановите его и повторите тест");
            }
            LogRing.Write("info", "test",
                "старт теста: " + total + " стратегий, " + custom.Count + " домен(ов)" +
                (geoblock ? " (диагностика геоблока)" : string.Empty));

            // Если GUI уже запущен от администратора — не гоняем UAC-обёртку: она может
            // не стартовать дочерний процесс, и тест «зависает» на фазе запуска.
            bool elevated = Uac.IsElevated();

            Set(new TestProgress
            {
                Running = true,
                Phase = "launch",
                Total = total,
                Pct = 0,
                Msg = elevated ? "запускаю тест" : "запускаю тест (подтвердите права администратора один раз)",
            });

            var ctx = new WorkerCtx
            {
                State = state,
                Data = state.Data,
                Script = files.Script,
                OutPath = files.Out,
                Steps = steps,
                Domains = custom,
                Elevated = elevated,
                Geoblock = geoblock,
                HadRuntime = hadRuntime,
                HadService = hadService,
            };
            var thread = new Thread(Worker)
            {
                IsBackground = true,
                Name = "zgui-test",
            };
            thread.Start(ctx);
            return Result<bool>.Ok(true);
        }

        private static bool IdsContains(IList<string> ids, string id)
        {
            foreach (string s in ids)
            {
                if (s == id) { return true; }
            }
            return false;
        }

        /// <summary>Полный путь к exe движка в корне, иначе null (lib.rs:803-807).</summary>
        private static string LocateExe(string root, string exeName)
        {
            string rel = Processes.FindExe(root, exeName);
            if (string.IsNullOrEmpty(rel)) { return null; }
            return Path.Combine(root, rel.Replace('/', '\\'));
        }

        private static void Worker(object arg)
        {
            var ctx = (WorkerCtx)arg;
            List<TestStep> steps = ctx.Steps;
            int total = steps.Count;
            List<KeyValuePair<string, string>> custom = ctx.Domains;
            string data = ctx.Data;
            string outPath = ctx.OutPath;

            // Один UAC на весь тест: скрипт выполняет все стратегии внутри.
            // Если GUI уже админ — запускаем напрямую (без Start-Process -Verb RunAs).
            string launchError = ctx.Elevated
                ? Uac.SpawnScriptDirect(ctx.Script)
                : Uac.SpawnElevatedScript(ctx.Script);
            if (launchError != null)
            {
                LogRing.Write("err", "test", "раннер теста не стартовал: " + launchError);
                Set(new TestProgress
                {
                    Running = false,
                    Phase = "done",
                    Msg = "не удалось запустить тест: " + launchError,
                    Total = total,
                    Done = true,
                });
                return;
            }

            // Поллим промежуточный JSON, пока раннер пишет результаты.
            var results = new List<StrategyResult>();
            var started = DateTime.UtcNow;
            while (true)
            {
                if (File.Exists(Path.Combine(data, "logs", StopFlagFile))) { break; }
                Dictionary<string, object> v = ReadTestProgress(outPath);
                if (v != null)
                {
                    // Фаза базовой пробы геоблок-теста (без Zapret): показываем прогресс,
                    // иначе UI выглядит «замершим» до первого winws.
                    object baseline;
                    if (v.TryGetValue("baseline", out baseline))
                    {
                        var bd = baseline as Dictionary<string, object>;
                        long bDone = bd != null ? AsLong(bd, "done") : 0;
                        long btotal = bd != null ? AsLong(bd, "total") : 0;
                        Set(new TestProgress
                        {
                            Running = true,
                            Phase = "baseline",
                            Total = total,
                            Msg = "базовая проба (без Zapret): " + bDone + "/" + btotal,
                        });
                        Thread.Sleep(700);
                        continue;
                    }
                    long idx = AsLong(v, "index");
                    string curId = AsString(v, "currentId");
                    string curName = AsString(v, "currentName");
                    results = ParseResults(v);
                    bool done = idx >= total && results.Count >= total;
                    Set(new TestProgress
                    {
                        Running = !done,
                        Phase = done ? "done" : "probe",
                        CurrentId = curId,
                        CurrentName = curName,
                        Index = (int)idx,
                        Total = total,
                        Pct = (int)((idx / (double)Math.Max(1, total)) * 100.0),
                        Msg = curName != null
                            ? "тестирую «" + curName + "»"
                            : "тест завершён",
                        Results = results,
                        Done = done,
                    });
                    if (done) { break; }
                }
                else
                {
                    // Если PID раннера так и не появился — не висим 120 с, а выходим
                    // с понятной ошибкой. PID есть, но нет вывода — ждём дольше.
                    bool pidSeen = File.Exists(Path.Combine(data, "logs", RunnerPidFile));
                    int limitSeconds = pidSeen ? 120 : 20;
                    if ((DateTime.UtcNow - started).TotalSeconds > limitSeconds) { break; }
                }
                Thread.Sleep(700);
            }
            bool stopped = File.Exists(Path.Combine(data, "logs", StopFlagFile));
            TryDelete(outPath);

            // Если раннер не оставил результатов — вероятнее всего UAC отклонён.
            if (results.Count == 0)
            {
                foreach (TestStep s in steps)
                {
                    results.Add(new StrategyResult
                    {
                        Id = s.Id,
                        Name = s.Name,
                        Engine = s.Engine,
                        Group = s.Group,
                        Started = false,
                        Score = 0,
                        MaxScore = custom.Count,
                        Error = ctx.Elevated
                            ? "тест не запустился — раннер не стартовал (см. logs/test-runner.ps1 и run-*.err.txt)"
                            : "тест не запустился — подтверждение прав администратора отклонено или раннер не стартовал",
                    });
                }
            }

            // Диагностика «стратегия не запустилась»: причины уже собраны раннером,
            // но в журнале их не было — при разборе жалоб не хватало фактов.
            foreach (StrategyResult r in results)
            {
                if (!r.Started)
                {
                    string err = r.Error ?? string.Empty;
                    if (err.Length > 300) { err = err.Substring(0, 300); }
                    LogRing.Write("err", "test", "«" + r.Name + "» не запустилась: " + err);
                }
            }

            // Возвращаем обход, который остановили перед тестом: иначе пользователь
            // остаётся без защиты, а служба — в остановленном состоянии.
            if (ctx.HadRuntime != null)
            {
                LogRing.Write("info", "test", "возвращаю прежнюю стратегию «" + ctx.HadRuntime.ProfileId + "»");
                Result<Config.Runtime> back = DoStart(ctx.State, ctx.HadRuntime.ProfileId);
                if (!back.IsOk)
                {
                    LogRing.Write("err", "test", "не удалось вернуть прежнюю стратегию: " + back.Error);
                    Toast("warn", "прежняя стратегия не вернулась: " + back.Error);
                }
            }
            else if (ctx.HadService)
            {
                string e = Service.StartService(data);
                if (e != null)
                {
                    LogRing.Write("err", "test", "не удалось вернуть службу zapret: " + e);
                }
            }

            // Калибровка геоблока: какие домены обходятся Zapret, а какие недоступны.
            if (ctx.Geoblock && !stopped)
            {
                var passed = new HashSet<string>();
                foreach (StrategyResult r in results)
                {
                    if (r.Domains == null) { continue; }
                    foreach (DomainResult d in r.Domains)
                    {
                        if (d.Ok) { passed.Add(d.Host); }
                    }
                }
                var allHosts = new List<string>(custom.Count);
                foreach (KeyValuePair<string, string> d in custom) { allHosts.Add(d.Value); }
                var reachable = new List<string>(passed);
                // «Недоступные» — не прошли НИ У ОДНОЙ стратегии. Если прогон не дал
                // ни одного успеха (тест не отработал) — ничего не помечаем, иначе
                // при сбое сети весь список стал бы «недоступным».
                List<string> vpnOnly;
                if (passed.Count == 0)
                {
                    vpnOnly = new List<string>();
                }
                else
                {
                    vpnOnly = new List<string>(allHosts.Count);
                    foreach (string h in allHosts)
                    {
                        if (!passed.Contains(h)) { vpnOnly.Add(h); }
                    }
                }
                SaveReachability(data, reachable, vpnOnly);
                string msg = "калибровка: обходится " + reachable.Count +
                    " доменов, недоступны (только VPN) — " + vpnOnly.Count;
                Toast("ok", msg);
            }

            List<StrategyResult> sorted;
            string bestId;
            Summarize(results, out sorted, out bestId);
            string bestName = null;
            if (bestId != null)
            {
                foreach (StrategyResult r in results)
                {
                    if (r.Id == bestId) { bestName = r.Name; break; }
                }
            }
            // Диагностический геоблок-тест не влияет на «лучшую стратегию»/автозапуск.
            if (ctx.Geoblock)
            {
                bestId = null;
                bestName = null;
            }
            if (stopped)
            {
                LogRing.Write("warn", "test", "тест остановлен пользователем");
                // Пользователь резко остановил тест: частичные результаты не считаем итоговыми.
                string msg = "тест остановлен пользователем";
                Set(new TestProgress
                {
                    Running = false,
                    Phase = "done",
                    Total = total,
                    Msg = msg,
                    Done = true,
                });
                Toast("info", msg);
                return;
            }
            if (!ctx.Geoblock)
            {
                var cache = new TestCache
                {
                    TestedAt = BatImport.NowEpoch().ToString(),
                    BestId = bestId,
                    Results = sorted,
                };
                cache.Save(data);
            }
            string final;
            if (ctx.Geoblock)
            {
                final = "диагностика геоблок-списков завершена";
            }
            else if (bestName != null)
            {
                final = "лучшая стратегия: «" + bestName + "»";
            }
            else
            {
                final = "ни одна стратегия не набрала очков";
            }
            LogRing.Write(bestName != null || ctx.Geoblock ? "ok" : "warn", "test",
                "тест завершён: " + final + " (стратегий: " + total + ")");
            Set(new TestProgress
            {
                Running = false,
                Phase = "done",
                Index = total,
                Total = total,
                Pct = 100,
                Msg = final,
                Results = sorted,
                BestId = bestId,
                BestName = bestName,
                Done = true,
            });
            Toast("ok", final);
        }

        public static List<StrategyResult> ParseResults(Dictionary<string, object> v)
        {
            var results = new List<StrategyResult>();
            object boxed;
            if (!v.TryGetValue("results", out boxed) || !(boxed is List<object> arr)) { return results; }
            foreach (object item in arr)
            {
                StrategyResult r;
                if (Json.TryParse(Json.Serialize(item), out r)) { results.Add(r); }
            }
            return results;
        }

        public static long AsLong(Dictionary<string, object> d, string key)
        {
            object v;
            if (!d.TryGetValue(key, out v) || v == null) { return 0; }
            if (v is long) { return (long)v; }
            if (v is double) { return (long)(double)v; }
            if (v is bool) { return (bool)v ? 1 : 0; }
            long n;
            long.TryParse(v.ToString(), out n);
            return n;
        }

        private static string AsString(Dictionary<string, object> d, string key)
        {
            object v;
            if (!d.TryGetValue(key, out v)) { return null; }
            return v as string;
        }

        // ------------------------------------------------------------ стоп / применение

        /// <summary>
        /// Резко останавливает тест стратегий: стоп-флаг для раннера + мгновенный
        /// kill деревьев (elevated раннер и активный winws) через один UAC
        /// (lib.rs:1610-1629). Возвращает ошибку или null.
        /// </summary>
        public static string Cancel(string dataDir)
        {
            string flag = Path.Combine(dataDir, "logs", StopFlagFile);
            string runPid = Path.Combine(dataDir, "logs", RunnerPidFile);
            string winPid = Path.Combine(dataDir, "logs", WinPidFile);
            // Сначала маркер — поллер/раннер корректно завершат рабочий цикл сами.
            try { File.WriteAllText(flag, BatImport.NowEpoch().ToString()); }
            catch (Exception e) { return e.Message; }
            var pids = new List<uint>(2);
            foreach (string p in new[] { runPid, winPid })
            {
                try
                {
                    if (uint.TryParse(File.ReadAllText(p).Trim(), out uint pid) && !pids.Contains(pid))
                    {
                        pids.Add(pid);
                    }
                }
                catch { }
            }
            if (pids.Count > 0)
            {
                Uac.StopPids(pids, dataDir);
            }
            return null;
        }

        /// <summary>
        /// Одно действие «применить лучшую стратегию»: включить автозапуск И
        /// запустить сейчас (lib.rs:1634-1660). Возвращает ошибку или null.
        /// </summary>
        public static string ApplyBestStrategy(State state, string id)
        {
            if (state.Profile(id) == null)
            {
                return "профиль не найден";
            }
            state.Settings.AutostartMode = "profile";
            state.Settings.AutostartProfile = id;
            state.Save();
            TestCache cache = TestCache.Load(state.Data);
            cache.BestId = id;
            cache.Save(state.Data);
            Autostart.Sync(state);
            Result<Config.Runtime> r = StartOrSwitch(state, id);
            if (!r.IsOk)
            {
                return r.Error;
            }
            Toast("ok", "Лучшая стратегия: автозапуск включён и запущена сейчас");
            return null;
        }

        /// <summary>
        /// Запускает профиль без тупиков на настройках: если установлена служба
        /// zapret — переводит её на нужную стратегию, иначе поднимает winws как
        /// процесс программы (lib.rs:999-1049).
        /// </summary>
        private static Result<Config.Runtime> StartOrSwitch(State state, string id)
        {
            // Во время прогона теста запуск запрещён: do_start/stop_all_own убили бы
            // winws теста.
            if (Current.Running || RunnerAlive(state.Data))
            {
                return Result<Config.Runtime>.Err("идёт тест стратегий — дождитесь окончания");
            }
            // Решаем по факту, а не по записи в state.json: службу могли создать или
            // удалить извне, и устаревшее состояние повело бы по неверной ветке.
            Service.State(out bool installed, out bool? running);
            if (!installed)
            {
                return DoStart(state, id);
            }
            Profile p = state.Profile(id);
            if (p == null)
            {
                return Result<Config.Runtime>.Err("профиль не найден");
            }
            string root = state.Roots.Path(p.Engine);
            if (root == null)
            {
                return Result<Config.Runtime>.Err("корень «" + p.Engine + "» не задан — нажмите «Скачать движок»");
            }
            GameFilter.Ports(state.Settings.GameFilter, out string tcp, out string udp);
            string[] args = GameFilter.Apply(p.Args, tcp, udp).ToArray();
            // Один живой winws: снимаем процесс программы и старую службу перед пересозданием.
            ProfileRunner.StopAllOwn(state);
            string e = Service.Install(root, p, args, state.Data);
            if (e != null)
            {
                LogRing.Write("err", "service", "переключение службы не удалось: " + e);
                return Result<Config.Runtime>.Err(Humanize.WithContext("не удалось переключить службу на эту стратегию", e));
            }
            LogRing.Write("ok", "service", "служба zapret переключена на «" + p.Name + "»");
            state.ServiceRunning = true;
            state.ServiceStrategy = p.Id;
            state.Runtime = null;
            state.Save();
            return Result<Config.Runtime>.Ok(new Config.Runtime
            {
                ProfileId = p.Id,
                Pid = 0,
                StartedAt = (ulong)BatImport.NowEpoch(),
                Via = "service",
                Alive = true,
            });
        }

        /// <summary>Запуск стратегии как процесса программы (lib.rs:901-994).</summary>
        private static Result<Config.Runtime> DoStart(State state, string id)
        {
            Profile p = state.Profile(id);
            if (p == null)
            {
                LogRing.Write("err", "start", "профиль " + id + " не найден");
                return Result<Config.Runtime>.Err("профиль не найден — возможно, он был удалён в другой копии программы");
            }
            string root = state.Roots.Path(p.Engine);
            if (root == null)
            {
                LogRing.Write("warn", "start", "корень движка «" + p.Engine + "» не задан");
                return Result<Config.Runtime>.Err("корень «" + p.Engine + "» не задан — нажмите «Скачать движок» на вкладке «Стратегии»");
            }
            ProfileRunner.StopAllOwn(state);
            return ProfileRunner.Start(p, root, state.Settings, state.Data);
        }
    }
}
