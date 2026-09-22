using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Threading.Tasks;
using ZapretGui.Core.Config;
using ZapretGui.Core.Embedded;
using ZapretGui.Core.Log;
using ZapretGui.Core.Runtime;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Tele
{
    /// <summary>Состояние Telegram-моста — порт telegram::TgStatus (camelCase).</summary>
    public class TgStatus
    {
        [JsonField("running")]
        public bool Running;

        [JsonField("port")]
        public ushort? Port;

        [JsonField("link")]
        public string Link;

        [JsonField("error")]
        public string Error;
    }

    /// <summary>
    /// Telegram MTProto-мост как отдельный скрытый процесс — порт telegram.rs.
    /// В Tauri accept loop крутлся в процессе GUI, здесь это дочерний
    /// tg-ws-proxy.exe, который сообщает бинд через stdout.
    /// </summary>
    public static class TgBridge
    {
        public const string ExeName = "tg-ws-proxy.exe";

        private const string ListenLine = "ZGUI_LISTEN";
        private const string ConfigErrLine = "ZGUI_CONFIG_ERR";
        private const string RunErrLine = "ZGUI_RUN_ERR";

        /// <summary>Ждём бинда сокета, как telegram.rs (20 секунд).</summary>
        private static readonly TimeSpan BindTimeout = TimeSpan.FromSeconds(20);

        private static readonly object _gate = new object();

        private static string _dataDir;
        private static Process _process;
        private static int? _pid;
        private static ushort? _port;
        private static string _link;
        private static string _error;

        public static string Dir(string dataDir)
        {
            return Path.Combine(dataDir, "telegram");
        }

        public static string ExePath(string dataDir)
        {
            return Path.Combine(Dir(dataDir), ExeName);
        }

        /// <summary>Файл статистики, который мост обновляет сам (STATS.summary).</summary>
        public static string StatsPath(string dataDir)
        {
            return Path.Combine(Dir(dataDir), "stats.txt");
        }

        public static TgStatus Status()
        {
            lock (_gate)
            {
                bool running = _pid.HasValue && Processes.PidAlive(_pid.Value);
                if (_pid.HasValue && !running)
                {
                    // процесс умер сам — забыем его, статус не врёт
                    _process = null;
                    _pid = null;
                    _port = null;
                    _link = null;
                }
                return new TgStatus
                {
                    Running = running,
                    Port = running ? _port : (ushort?)null,
                    Link = running ? _link : null,
                    Error = _error,
                };
            }
        }

        /// <summary>
        /// Запускает мост на порту (настройка tgPort, 1443 по умолчанию) и ждёт
        /// фактического бинда сокета (telegram.rs:55-126).
        /// </summary>
        public static async Task<Result<TgStatus>> StartAsync(string dataDir, ushort port)
        {
            string exe = ExePath(dataDir);
            if (!File.Exists(exe))
            {
                return Result<TgStatus>.Err("не найден " + ExeName + " — переустановите программу");
            }
            lock (_gate)
            {
                if (_pid.HasValue && Processes.PidAlive(_pid.Value))
                {
                    return Result<TgStatus>.Err("Telegram-прокси уже запущен");
                }
                _dataDir = dataDir;
                _process = null;
                _pid = null;
                _port = null;
                _link = null;
                _error = null;
            }

            string dir = Dir(dataDir);
            string stats = StatsPath(dataDir);
            try
            {
                Directory.CreateDirectory(dir);
            }
            catch
            {
            }

            Process proc;
            try
            {
                var info = new ProcessStartInfo
                {
                    FileName = exe,
                    Arguments = Processes.BuildArgs(new[]
                    {
                        "--port", port.ToString(CultureInfo.InvariantCulture),
                        "--default-domains",
                        "--stats-file", stats,
                    }),
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    WindowStyle = ProcessWindowStyle.Hidden,
                    WorkingDirectory = dir,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                };
                proc = new Process { StartInfo = info };
                proc.Start();
            }
            catch (Exception e)
            {
                string msg = "не удалось запустить " + ExeName + ": " + e.Message;
                SetError(msg);
                return Result<TgStatus>.Err(msg);
            }

            string errLog = Path.Combine(LogRing.Dir() ?? dir, "tg-bridge_" + LogRing.NowStamp() + ".log");
            DrainTo(proc, errLog);

            string line = await ReadProtocolLine(proc).ConfigureAwait(false);
            if (line == null)
            {
                Processes.KillTree(proc.Id);
                string tail = TailError(errLog);
                string msg = "Telegram-прокси неожиданно завершился" + (string.IsNullOrEmpty(tail) ? "" : ": " + tail);
                SetError(msg);
                return Result<TgStatus>.Err(msg);
            }

            string[] parts = line.Split('\t');
            if (parts[0] == ConfigErrLine || parts[0] == RunErrLine)
            {
                Processes.KillTree(proc.Id);
                string msg = parts.Length > 1 ? parts[1] : "ошибка моста";
                SetError(msg);
                return Result<TgStatus>.Err(msg);
            }
            if (parts[0] != ListenLine || parts.Length < 3 || !TryParsePort(parts[1], out ushort bound))
            {
                Processes.KillTree(proc.Id);
                string msg = "мост сообщил неизвестный формат ответа";
                SetError(msg);
                return Result<TgStatus>.Err(msg);
            }

            lock (_gate)
            {
                _process = proc;
                _pid = proc.Id;
                _port = bound;
                _link = parts[2];
                _error = null;
            }
            Log("ok", "прокси запущен на порту " + bound);
            return Result<TgStatus>.Ok(Status());
        }

        /// <summary>Достаёт порт из адреса вида 127.0.0.1:1443.</summary>
        public static bool TryParsePort(string addr, out ushort port)
        {
            port = 0;
            int colon = addr.LastIndexOf(':');
            if (colon < 0)
            {
                return false;
            }
            return ushort.TryParse(addr.Substring(colon + 1), NumberStyles.None, CultureInfo.InvariantCulture, out port);
        }

        /// <summary>Останавливает мост (telegram.rs:44-50).</summary>
        public static void Stop()
        {
            int? pid;
            string stats;
            lock (_gate)
            {
                pid = _pid;
                stats = _dataDir == null ? null : StatsPath(_dataDir);
                _process = null;
                _pid = null;
                _port = null;
                _link = null;
            }
            if (pid.HasValue)
            {
                Processes.KillTree(pid.Value);
            }
            if (stats != null)
            {
                try { File.Delete(stats); } catch { }
            }
            Log("info", "прокси остановлен");
        }

        /// <summary>Сводная статистика моста или null, если мост не отчитывался.</summary>
        public static string Stats(string dataDir)
        {
            try
            {
                string text = File.ReadAllText(StatsPath(dataDir));
                return string.IsNullOrEmpty(text) ? null : text;
            }
            catch
            {
                return null;
            }
        }

        private static async Task<string> ReadProtocolLine(Process proc)
        {
            var read = Task.Run(() =>
            {
                try
                {
                    string line;
                    while ((line = proc.StandardOutput.ReadLine()) != null)
                    {
                        if (line.StartsWith("ZGUI_", StringComparison.Ordinal))
                        {
                            return line;
                        }
                    }
                }
                catch
                {
                }
                return (string)null;
            });
            Task winner = await Task.WhenAny(read, Task.Delay(BindTimeout)).ConfigureAwait(false);
            if (winner != read)
            {
                return null;
            }
            return await read.ConfigureAwait(false);
        }

        private static void SetError(string msg)
        {
            lock (_gate)
            {
                _error = msg;
            }
            Log("err", "не удалось запустить прокси: " + msg);
        }

        // Копирует stderr моста (там tracing) в файл журнала.
        private static void DrainTo(Process proc, string path)
        {
            Task.Run(() =>
            {
                try
                {
                    Directory.CreateDirectory(Path.GetDirectoryName(path));
                    using (var file = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read))
                    using (var source = proc.StandardError.BaseStream)
                    {
                        source.CopyTo(file);
                    }
                }
                catch
                {
                }
            });
        }

        private static string TailError(string errLog)
        {
            try
            {
                if (!File.Exists(errLog))
                {
                    return string.Empty;
                }
                return Text.TailFile(errLog, 500);
            }
            catch
            {
                return string.Empty;
            }
        }

        private static void Log(string level, string msg)
        {
            if (LogRing.Dir() == null)
            {
                return;
            }
            LogRing.Write(level, "telegram", msg);
        }
    }
}
