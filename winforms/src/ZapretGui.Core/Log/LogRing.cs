using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Log
{
    /// <summary>
    /// Одна запись журнала (logger.rs:25-34). Поля — camelCase, как в Tauri-версии.
    /// </summary>
    public class Entry
    {
        [JsonField("seq")] public long Seq;
        [JsonField("ts")] public long Ts;
        [JsonField("level")] public string Level;
        [JsonField("scope")] public string Scope;
        [JsonField("msg")] public string Msg;
    }

    /// <summary>
    /// Журнал программы: кольцо последних записей + файл рядом с exe
    /// (logger.rs). Потокобезопасно; доставка в UI вызывается вне блокировки,
    /// иначе обработчик события мог бы сам записать лог и зайти в дедлок.
    /// </summary>
    public static class LogRing
    {
        private const int Cap = 1500;
        private const int MaxMsg = 4000;
        private const long MaxFile = 1024L * 1024L;

        private static readonly object _gate = new object();
        private static readonly LinkedList<Entry> _buf = new LinkedList<Entry>();
        private static long _seq;
        private static string _file;
        private static long _fileLen;
        private static Action<Entry> _sink;

        private static readonly DateTime UnixEpoch =
            new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc);

        private static string NormLevel(string level)
        {
            switch (level)
            {
                case "ok": return "ok";
                case "warn": return "warn";
                case "err":
                case "error": return "err";
                default: return "info";
            }
        }

        private static long NowMs()
        {
            return DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        }

        /// <summary>
        /// Подключает журнал к каталогу data (data/logs) и ставит приёмник
        /// записей для UI (logger.rs:83-92).
        /// </summary>
        public static void Init(string dataDir, Action<Entry> sink)
        {
            var dir = Path.Combine(dataDir, "logs");
            Directory.CreateDirectory(dir);
            var file = Path.Combine(dir, "zgui.log");
            long len = 0;
            try { len = new FileInfo(file).Length; } catch { }
            lock (_gate)
            {
                _file = file;
                _fileLen = len;
                _sink = sink;
            }
        }

        public static void SetSink(Action<Entry> sink)
        {
            lock (_gate) { _sink = sink; }
        }

        /// <summary>
        /// Записывает сообщение: в кольцо, в файл и в событие UI (logger.rs:101-133).
        /// </summary>
        public static void Write(string level, string scope, string msg)
        {
            level = NormLevel(level ?? string.Empty);
            var text = (msg ?? string.Empty).Trim();
            if (text.Length > MaxMsg)
            {
                text = text.Substring(0, MaxMsg) +
                    "… [обрезано " + (text.Length - MaxMsg) + " символов]";
            }

            Entry entry;
            Action<Entry> sink;
            lock (_gate)
            {
                _seq++;
                entry = new Entry
                {
                    Seq = _seq,
                    Ts = NowMs(),
                    Level = level,
                    Scope = scope ?? string.Empty,
                    Msg = text
                };
                _buf.AddLast(entry);
                while (_buf.Count > Cap) { _buf.RemoveFirst(); }
                WriteFile(entry);
                sink = _sink;
            }

            // Приёмник вызываем уже без блокировки: он может сам писать в лог.
            try { sink?.Invoke(entry); } catch { }
        }

        // Запись строки в файл журнала с ротацией на 1 МБ (logger.rs:135-156).
        private static void WriteFile(Entry entry)
        {
            if (string.IsNullOrEmpty(_file)) { return; }

            var line = "[" + Stamp(entry.Ts) + "] [" + entry.Level + "] " +
                entry.Scope + ": " + entry.Msg.Replace("\n", "\n    ") + "\n";
            var bytes = Encoding.UTF8.GetBytes(line);

            if (_fileLen + bytes.LongLength > MaxFile)
            {
                var rotated = Path.Combine(Path.GetDirectoryName(_file), "zgui.1.log");
                try { File.Delete(rotated); } catch { }
                try
                {
                    File.Move(_file, rotated);
                    _fileLen = 0;
                }
                catch { }
            }

            try
            {
                using (var f = new FileStream(_file, FileMode.Append, FileAccess.Write, FileShare.Read))
                {
                    f.Write(bytes, 0, bytes.Length);
                }
                _fileLen += bytes.LongLength;
            }
            catch { }
        }

        /// <summary>Метка для имени файла отчёта (logger.rs:159-161).</summary>
        public static string NowStamp()
        {
            return Stamp(NowMs()).Replace(':', '-').Replace(' ', '_').Replace('.', '-');
        }

        /// <summary>Формат UTC из эпохи: YYYY-MM-DD HH:MM:SS.mmm (logger.rs:164-180).</summary>
        public static string Stamp(long ms)
        {
            return UnixEpoch.AddMilliseconds(ms).ToString("yyyy-MM-dd HH:mm:ss.fff");
        }

        /// <summary>Записи после seq (0 — все из кольца) (logger.rs:197-200).</summary>
        public static List<Entry> Entries(long after)
        {
            lock (_gate)
            {
                var res = new List<Entry>();
                foreach (var e in _buf)
                {
                    if (e.Seq > after) { res.Add(e); }
                }
                return res;
            }
        }

        /// <summary>Дамп журнала для отчёта «скачать логи» (logger.rs:203-210).</summary>
        public static string Dump()
        {
            lock (_gate)
            {
                var parts = new List<string>(_buf.Count);
                foreach (var e in _buf)
                {
                    parts.Add("[" + Stamp(e.Ts) + "] [" + e.Level + "] " + e.Scope + ": " + e.Msg);
                }
                return string.Join("\n", parts);
            }
        }

        /// <summary>Стирает кольцо и файл журнала (logger.rs:212-219).</summary>
        public static void Clear()
        {
            lock (_gate)
            {
                _buf.Clear();
                if (!string.IsNullOrEmpty(_file))
                {
                    try { File.Delete(_file); } catch { }
                }
                _fileLen = 0;
            }
        }

        /// <summary>Каталог журналов (logger.rs:222-224).</summary>
        public static string Dir()
        {
            lock (_gate)
            {
                return string.IsNullOrEmpty(_file) ? null : Path.GetDirectoryName(_file);
            }
        }

        /// <summary>Текущее содержимое файла журнала целиком (для отчёта).</summary>
        public static string Read()
        {
            var file = _file;
            if (string.IsNullOrEmpty(file) || !File.Exists(file)) { return string.Empty; }
            try { return Text.ReadTextAuto(file); } catch { return string.Empty; }
        }

        /// <summary>
        /// Логирует необработанные исключения .NET — аналог panic hook
        /// (logger.rs:227-233).
        /// </summary>
        public static void InstallPanicHook()
        {
            AppDomain.CurrentDomain.UnhandledException += (sender, e) =>
            {
                try { Write("err", "panic", e.ExceptionObject == null ? "" : e.ExceptionObject.ToString()); }
                catch { }
            };
            TaskScheduler.UnobservedTaskException += (sender, e) =>
            {
                try
                {
                    Write("err", "panic", e.Exception.ToString());
                    e.SetObserved();
                }
                catch { }
            };
        }
    }
}
