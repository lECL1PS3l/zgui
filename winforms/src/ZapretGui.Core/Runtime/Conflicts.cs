using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Net.Sockets;
using System.Text;
using ZapretGui.Core.Config;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Runtime
{
    public class ConflictProcess
    {
        [JsonField("pid")] public uint Pid;
        [JsonField("name")] public string Name;
        [JsonField("note")] public string Note;

        public ConflictProcess() { }

        public ConflictProcess(uint pid, string name, string note)
        {
            Pid = pid;
            Name = name;
            Note = note;
        }
    }

    public class ConflictReport
    {
        [JsonField("processes")] public List<ConflictProcess> Processes = new List<ConflictProcess>();
        [JsonField("vpn")] public List<ConflictProcess> Vpn = new List<ConflictProcess>();
        [JsonField("foreignService")] public bool ForeignService;
        [JsonField("ownService")] public bool OwnService;
        [JsonField("message")] public string Message = "";

        public bool HasConflicts()
        {
            return Processes.Count > 0 || Vpn.Count > 0 || ForeignService;
        }
    }

    /// <summary>
    /// Детект и снятие конфликтующего ПО: чужие winws, чужая служба zapret, VPN/прокси.
    /// Порт service.rs (+ heal_orphan_proxy из lib.rs).
    /// </summary>
    public static class Conflicts
    {
        // Известные VPN/прокси-клиенты (без .exe). service.rs:32
        private static readonly string[] VpnProcesses =
        {
            "happ", "happd", "amneziawg", "amneziavpn", "amnezia", "awg",
            "wireguard", "openvpn", "openvpn-gui", "openconnect",
            "outline", "outline-client", "tunnelblick",
            "sing-box", "singbox", "xray", "v2ray", "v2raya",
            "nekoray", "nekobox", "clash", "clash-verge", "clash-meta",
            "mihomo", "clashx", "shadowsocks", "shadowsocks-rust",
            "ss-local", "sslocal", "trojan", "trojan-go",
            "hysteria", "hysteria2", "tun2socks", "wintun", "proxifier",
            "windscribe", "nordvpn", "expressvpn", "surfshark", "protonvpn",
            "mullvad", "privatevpn", "hiddify", "v2rayng", "tunsafe",
            "vpngate", "softether",
        };

        // Службы-туннели VPN. service.rs:84
        private static readonly string[] VpnServices =
        {
            "WireGuardTunnel", "OpenVPNService", "OpenVPNServiceInteractive",
            "AmneziaWG", "amneziawg", "AmneziaVPN", "AmneziaVPN-service", "Happ",
        };

        private static readonly string[] WinwsNames = { "winws.exe", "winws2.exe" };

        /// <summary>
        /// Имя процесса — известный VPN/прокси-клиент (с учётом суффиксов вроде
        /// «clash-verge»). service.rs:117
        /// </summary>
        public static bool IsVpnProcess(string name)
        {
            if (string.IsNullOrEmpty(name)) return false;
            var n = name.ToLowerInvariant();
            var baseName = n.EndsWith(".exe") ? n.Substring(0, n.Length - 4) : n;
            foreach (var v in VpnProcesses)
            {
                if (baseName == v) return true;
                if (baseName.StartsWith(v + "-")) return true;
            }
            return false;
        }

        /// <summary>
        /// Разбор вывода tasklist /FO CSV /NH в пары (имя, pid). service.rs:96
        /// </summary>
        public static List<KeyValuePair<string, uint>> ParseTaskListCsv(string text)
        {
            var found = new List<KeyValuePair<string, uint>>();
            if (string.IsNullOrEmpty(text)) return found;
            var lines = text.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None);
            foreach (var line in lines)
            {
                var cols = line.Split(new[] { "\",\"" }, StringSplitOptions.None);
                if (cols.Length < 2) continue;
                var name = cols[0].Trim('"');
                if (!uint.TryParse(cols[1].Trim('"'), out var pid)) continue;
                found.Add(new KeyValuePair<string, uint>(name, pid));
            }
            return found;
        }

        /// <summary>
        /// Перечисляет процессы из tasklist, отфильтрованные по предикату имени. service.rs:96
        /// </summary>
        public static List<KeyValuePair<string, uint>> ListProcesses(Func<string, bool> want)
        {
            Processes.RunOutputHidden("tasklist.exe", new[] { "/FO", "CSV", "/NH" },
                out _, out var stdout, out _);
            return ParseTaskListCsv(stdout).FindAll(kv => want(kv.Key));
        }

        /// <summary>Находит запущенные VPN/прокси-клиенты. service.rs:124</summary>
        public static List<ConflictProcess> DetectVpn()
        {
            var outList = new List<ConflictProcess>();
            foreach (var kv in ListProcesses(IsVpnProcess))
            {
                outList.Add(new ConflictProcess(kv.Value, kv.Key, "VPN/прокси — конфликтует с zapret"));
            }

            foreach (var svc in VpnServices)
            {
                Processes.RunOutputHidden("sc.exe", new[] { "query", svc },
                    out var code, out var stdout, out _);
                if (code == 0 && stdout.ToUpperInvariant().Contains("RUNNING"))
                {
                    outList.Add(new ConflictProcess(0, "service:" + svc, "VPN-служба запущена"));
                }
            }
            return outList;
        }

        /// <summary>
        /// Пути процессов по именам (pid → полный путь к exe) через WMI. service.rs:220
        /// '|' в имени файла Windows запрещён — безопасный разделитель значений.
        /// </summary>
        public static Dictionary<uint, string> ProcessImagePaths(string[] names)
        {
            var filter = string.Join(" or ", names.Select(n => "Name='" + n + "'"));
            var script =
                "Get-CimInstance Win32_Process -Filter \"" + filter +
                "\" -Property ProcessId,ExecutablePath " +
                "| Where-Object { $_.ExecutablePath } " +
                "| ForEach-Object { '{0}|{1}' -f $_.ProcessId, $_.ExecutablePath }";
            Processes.RunOutputHidden("powershell.exe",
                new[] { "-NoProfile", "-NonInteractive", "-Command", script },
                out _, out var stdout, out _);

            var map = new Dictionary<uint, string>();
            foreach (var line in stdout.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None))
            {
                var sep = line.IndexOf('|');
                if (sep < 0) continue;
                if (!uint.TryParse(line.Substring(0, sep).Trim(), out var pid)) continue;
                var path = line.Substring(sep + 1).Trim();
                if (Path.IsPathRooted(path)) map[pid] = path;
            }
            return map;
        }

        /// <summary>
        /// Свой ли движок: exe лежит под папкой данных программы (data/engines/...).
        /// Сравнение регистронезависимое; префикс с разделителем, чтобы data
        /// не совпало с чужим data2. service.rs:252
        /// </summary>
        public static bool IsOwnEngine(string path, string dataDir)
        {
            if (string.IsNullOrEmpty(path) || string.IsNullOrEmpty(dataDir)) return false;
            var p = path.ToLowerInvariant();
            var d = dataDir.ToLowerInvariant();
            var prefix = d.EndsWith("\\") ? d : d + "\\";
            return p.StartsWith(prefix, StringComparison.Ordinal);
        }

        /// <summary>
        /// Ищет чужие процессы winws.exe/winws2.exe (не наши), чужую службу zapret и VPN.
        /// service.rs:151
        /// </summary>
        public static ConflictReport Check(string dataDir, uint? ourPid)
        {
            var report = new ConflictReport();

            Service.State(out var installed, out var running);
            var strategy = Service.Strategy();
            report.OwnService = strategy != null;
            report.ForeignService = installed && strategy == null;
            if (report.ForeignService)
            {
                report.Processes.Add(new ConflictProcess(0, "service:" + Engines.ServiceName,
                    running == true
                        ? "служба zapret (не от нашего GUI) запущена"
                        : "служба zapret (не от нашего GUI) установлена"));
            }

            // Пути процессов winws/winws2 через WMI (tasklist не отдаёт путь к exe).
            var imagePaths = ProcessImagePaths(WinwsNames);

            foreach (var exe in WinwsNames)
            {
                Processes.RunOutputHidden("tasklist.exe",
                    new[] { "/FI", "IMAGENAME eq " + exe, "/FO", "CSV", "/NH" },
                    out _, out var stdout, out _);
                foreach (var kv in ParseTaskListCsv(stdout))
                {
                    if (ourPid.HasValue && kv.Value == ourPid.Value) continue;
                    var ownPath = imagePaths.TryGetValue(kv.Value, out var img)
                        ? IsOwnEngine(img, dataDir)
                        : false;
                    // Путь не смогли узнать (нет прав на WMI), но наша служба установлена
                    // и запущена — почти наверняка это winws службы, трогать его нельзя.
                    var assumedService = report.OwnService && running == true && !imagePaths.ContainsKey(kv.Value);
                    if (ownPath || assumedService) continue;
                    report.Processes.Add(new ConflictProcess(kv.Value, kv.Key,
                        "чужой процесс zapret (не запущен нашим GUI)"));
                }
            }

            report.Vpn = DetectVpn();

            if (report.Processes.Count > 0 || report.Vpn.Count > 0)
            {
                report.Message = "Обнаружено конфликтующее ПО";
            }
            return report;
        }

        /// <summary>Только VPN-отчёт (команда vpn_check). lib.rs:185</summary>
        public static ConflictReport VpnCheck()
        {
            var report = new ConflictReport { Vpn = DetectVpn() };
            if (report.Vpn.Count > 0) report.Message = "Обнаружено конфликтующее ПО";
            return report;
        }

        /// <summary>
        /// PID-ы winws/winws2, запущенных из наших данных. service.rs:266
        /// </summary>
        public static List<uint> OwnEnginePids(string dataDir, uint? exclude)
        {
            var pids = new List<uint>();
            foreach (var kv in ProcessImagePaths(WinwsNames))
            {
                if (exclude.HasValue && kv.Key == exclude.Value) continue;
                if (IsOwnEngine(kv.Value, dataDir)) pids.Add(kv.Key);
            }
            pids.Sort();
            var distinct = new List<uint>();
            foreach (var pid in pids) if (distinct.Count == 0 || distinct[distinct.Count - 1] != pid) distinct.Add(pid);
            return distinct;
        }

        /// <summary>Дешёвая проверка: запущен ли хоть один winws/winws2 (без WMI). service.rs:278</summary>
        public static bool AnyWinwsRunning()
        {
            return ListProcesses(n =>
                n.Equals("winws.exe", StringComparison.OrdinalIgnoreCase) ||
                n.Equals("winws2.exe", StringComparison.OrdinalIgnoreCase)).Count > 0;
        }

        /// <summary>
        /// Гасит чужие процессы и VPN (taskkill от админа). Свои pid-ы не трогает.
        /// Возвращает строку ошибки или null. service.rs:300
        /// </summary>
        public static string KillConflicts(ConflictReport report, uint? ourPid, string dataDir)
        {
            var pids = new List<uint>();
            foreach (var p in report.Processes.Concat(report.Vpn))
            {
                if (p.Pid > 0 && (!ourPid.HasValue || p.Pid != ourPid.Value) && !pids.Contains(p.Pid))
                    pids.Add(p.Pid);
            }
            pids.Sort();

            var foreignService = report.ForeignService;
            // VPN-службы (pid = 0, имя вида service:<name>) останавливаем отдельно.
            var vpnServices = new List<string>();
            foreach (var p in report.Vpn)
            {
                if (p.Pid == 0 && p.Name.StartsWith("service:", StringComparison.Ordinal))
                    vpnServices.Add(p.Name.Substring("service:".Length));
            }

            if (pids.Count == 0 && !foreignService && vpnServices.Count == 0) return null;

            var sb = new StringBuilder();
            sb.AppendLine(Uac.PsHeader);
            // Сначала ОСТАНАВЛИВАЕМ службы и ждём — иначе служба перезапустит свой процесс.
            foreach (var svc in vpnServices)
            {
                sb.Append("Stop-Service -Name '").Append(svc).Append("' -Force -ErrorAction SilentlyContinue\n");
                sb.Append("sc.exe stop '").Append(svc).Append("' 2>$null | Out-Null\n");
            }
            if (vpnServices.Count > 0) sb.Append("Start-Sleep -Seconds 2\n");
            if (foreignService)
            {
                // ВАЖНО: `sc` в PowerShell 5.1 — это алиас Set-Content, а не sc.exe.
                sb.Append("net stop ").Append(Engines.ServiceName).Append(" 2>$null | Out-Null\n");
                sb.Append("& sc.exe delete ").Append(Engines.ServiceName).Append(" 2>$null | Out-Null\n");
            }
            foreach (var pid in pids)
            {
                sb.Append("taskkill /F /T /PID ").Append(pid).Append(" 2>$null | Out-Null\n");
            }
            sb.Append("exit 0\n");

            var script = Path.Combine(Path.Combine(dataDir, "logs"),
                "conflict_kill_" + Process.GetCurrentProcess().Id + ".ps1");
            try
            {
                Uac.WritePs1(script, sb.ToString());
            }
            catch (Exception e)
            {
                return e.Message;
            }
            var code = Uac.RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            return code == 0 ? null : "не удалось выгрузить: код " + code;
        }

        /// <summary>
        /// Извлекает локальный (127.*/localhost) порт из значения ProxyServer.
        /// Возвращает 0 для внешних/корпоративных прокси — их нельзя трогать.
        /// lib.rs:286
        /// </summary>
        public static int LocalProxyPort(string server)
        {
            if (string.IsNullOrEmpty(server)) return 0;
            var addr = server;
            foreach (var part in server.Split(';'))
            {
                var eq = part.IndexOf('=');
                addr = (eq >= 0 ? part.Substring(eq + 1) : part).Trim();
                break;
            }
            var colon = addr.LastIndexOf(':');
            if (colon < 0) return 0;
            var host = addr.Substring(0, colon);
            if (host.StartsWith("127.") || host.Equals("localhost", StringComparison.OrdinalIgnoreCase))
            {
                return int.TryParse(addr.Substring(colon + 1).Trim(), out var port) ? port : 0;
            }
            return 0;
        }

        private static string RegRead(string key, string name)
        {
            Processes.RunOutputHidden("reg", new[] { "query", key, "/v", name },
                out _, out var stdout, out _);
            foreach (var line in stdout.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None))
            {
                if (line.Contains(name))
                {
                    var parts = line.Split(new[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
                    return parts.Length > 2 ? parts[2] : null;
                }
            }
            return null;
        }

        /// <summary>
        /// Чинит «осиротевший» системный прокси (остался от выгруженного VPN):
        /// если ProxyEnable=1, прокси локальный и порт уже никем не слушается —
        /// сбрасывает на прямое подключение. Возвращает сброшенный прокси или null.
        /// lib.rs:300
        /// </summary>
        public static string HealOrphanProxy()
        {
            const string key = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";
            var enabledRaw = RegRead(key, "ProxyEnable");
            var enabled = enabledRaw != null &&
                (enabledRaw.Trim() == "0x1" || enabledRaw.Trim() == "1");
            if (!enabled) return null;

            var server = RegRead(key, "ProxyServer");
            if (server == null) return null;
            server = server.Trim();
            var port = LocalProxyPort(server);
            if (port == 0) return null;

            // Кто-то реально слушает порт (живой VPN/прокси) — не вмешиваемся.
            try
            {
                using (var client = new TcpClient())
                {
                    var ar = client.BeginConnect("127.0.0.1", port, null, null);
                    if (!ar.AsyncWaitHandle.WaitOne(250)) return null;
                    client.EndConnect(ar);
                }
            }
            catch { return null; }

            Processes.RunOutputHidden("reg",
                new[] { "add", key, "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f" },
                out _, out _, out _);
            Processes.RunOutputHidden("reg",
                new[] { "delete", key, "/v", "ProxyServer", "/f" },
                out _, out _, out _);
            return server;
        }
    }
}
