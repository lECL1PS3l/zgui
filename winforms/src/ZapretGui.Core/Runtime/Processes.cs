using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Text;
using System.Threading.Tasks;
using ZapretGui.Core.Log;

namespace ZapretGui.Core.Runtime
{
    /// <summary>
    /// Скрытый запуск процессов, проверка PID, перехват вывода, поиск exe
    /// (runner.rs, config.rs:397-423). Все дочерние процессы стартуют без
    /// консольного окна — CREATE_NO_WINDOW, как в Tauri-версии.
    /// </summary>
    public static class Processes
    {
        /// <summary>Каталог текущего exe (config.rs portable_data_dir / lib.rs).</summary>
        public static string CurrentExeDir
        {
            get
            {
                var loc = Assembly.GetEntryAssembly()?.Location;
                return string.IsNullOrEmpty(loc) ? AppContext.BaseDirectory : Path.GetDirectoryName(loc);
            }
        }

        /// <summary>
        /// Запускает процесс без окна, вывод пишет рядом с журналом программы
        /// (runner.rs spawn_direct).
        /// </summary>
        public static bool SpawnHidden(string exe, string[] args, string workdir, out int pid)
        {
            var dir = LogRing.Dir() ?? CurrentExeDir;
            var stamp = LogRing.NowStamp();
            return SpawnHidden(exe, args, workdir,
                Path.Combine(dir, "spawn_" + stamp + ".out.log"),
                Path.Combine(dir, "spawn_" + stamp + ".err.log"),
                out pid);
        }

        /// <summary>
        /// Запускает процесс без окна, stdout/stderr дописывает в заданные файлы
        /// (runner.rs: spawn_direct с out_log/err_log).
        /// </summary>
        public static bool SpawnHidden(string exe, string[] args, string workdir,
            string outLog, string errLog, out int pid)
        {
            pid = 0;
            try
            {
                var info = new ProcessStartInfo
                {
                    FileName = exe,
                    Arguments = BuildArgs(args),
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    WindowStyle = ProcessWindowStyle.Hidden,
                    WorkingDirectory = string.IsNullOrEmpty(workdir) ? null : workdir,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true
                };

                EnsureParent(outLog);
                EnsureParent(errLog);

                var proc = new Process { StartInfo = info };
                if (!proc.Start()) { return false; }
                pid = proc.Id;

                // ponytail: копируем сырые байты в файл — кодировка вывода
                // (UTF-8/OEM) разбирается при чтении, а не при записи.
                DrainAsync(proc.StandardOutput.BaseStream, outLog);
                DrainAsync(proc.StandardError.BaseStream, errLog);
                return true;
            }
            catch { return false; }
        }

        /// <summary>Выполняет процесс и ждёт его завершения, возвращая вывод (runner.rs run_powershell).</summary>
        public static void RunOutputHidden(string exe, string[] args, out int exitCode, out string output)
        {
            string stderr;
            RunOutputHidden(exe, args, out exitCode, out output, out stderr);
        }

        /// <summary>То же, но возвращает и stderr (run_powershell отдаёт его при ошибке).</summary>
        public static void RunOutputHidden(string exe, string[] args, out int exitCode, out string stdout, out string stderr)
        {
            var info = new ProcessStartInfo
            {
                FileName = exe,
                Arguments = BuildArgs(args),
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };

            var proc = Process.Start(info);
            // stderr читаем в отдельном потоке — иначе переполнение трубы
            // заблокирует дочерний процесс.
            var stderrTask = Task.Run(() => proc.StandardError.ReadToEnd());
            stdout = proc.StandardOutput.ReadToEnd();
            proc.WaitForExit();
            stderr = stderrTask.Result;
            exitCode = proc.ExitCode;
        }

        /// <summary>Жив ли процесс (plan Task 6): PID 0 никогда не жив.</summary>
        public static bool PidAlive(int pid)
        {
            if (pid <= 0) { return false; }
            try
            {
                var proc = Process.GetProcessById(pid);
                proc.Refresh();
                return !proc.HasExited;
            }
            catch { return false; }
        }

        /// <summary>Убивает процесс вместе с дочерними (runner.rs stop_pid).</summary>
        public static void KillTree(int pid)
        {
            if (pid <= 0) { return; }
            int code; string outText;
            RunOutputHidden("taskkill.exe", new[] { "/F", "/T", "/PID", pid.ToString() }, out code, out outText);
        }

        /// <summary>
        /// Ищет файл в каталоге (рекурсивно, не глубже 5 уровней) и возвращает
        /// относительный путь через / или null (config.rs:397-423).
        /// </summary>
        public static string FindExe(string root, string name)
        {
            if (string.IsNullOrEmpty(root) || !Directory.Exists(root)) { return null; }
            var hit = Walk(root, name, 0);
            if (hit == null) { return null; }
            return hit.Substring(root.Length).TrimStart('\\').Replace('\\', '/');
        }

        private static string Walk(string dir, string name, int depth)
        {
            if (depth > 5) { return null; }
            string[] entries;
            try { entries = Directory.GetFileSystemEntries(dir); }
            catch { return null; }

            foreach (var path in entries)
            {
                bool isFile, isDir;
                try
                {
                    isFile = File.Exists(path);
                    isDir = !isFile && Directory.Exists(path);
                }
                catch { continue; }
                if (isFile)
                {
                    if (Path.GetFileName(path).Equals(name, StringComparison.OrdinalIgnoreCase))
                    {
                        return path;
                    }
                }
                else if (isDir)
                {
                    var hit = Walk(path, name, depth + 1);
                    if (hit != null) { return hit; }
                }
            }
            return null;
        }

        // Копирует поток вывода дочернего процесса в файл (append) и закрывает файл.
        private static void DrainAsync(Stream source, string path)
        {
            var target = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read);
            source.CopyToAsync(target).ContinueWith(t => target.Dispose());
        }

        private static void EnsureParent(string path)
        {
            var dir = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir)) { Directory.CreateDirectory(dir); }
        }

        /// <summary>Собирает командную строку с экранированием по правилам CRT.</summary>
        public static string BuildArgs(string[] args)
        {
            if (args == null || args.Length == 0) { return string.Empty; }
            var sb = new StringBuilder();
            for (var i = 0; i < args.Length; i++)
            {
                if (i > 0) { sb.Append(' '); }
                sb.Append(QuoteArg(args[i]));
            }
            return sb.ToString();
        }

        private static string QuoteArg(string arg)
        {
            if (arg == null) { return "\"\""; }
            if (arg.Length == 0) { return "\"\""; }
            if (arg.IndexOfAny(new[] { ' ', '"' }) < 0) { return arg; }

            var sb = new StringBuilder(arg.Length + 2);
            sb.Append('"');
            var slashes = 0;
            foreach (var c in arg)
            {
                if (c == '\\') { slashes++; sb.Append(c); }
                else if (c == '"')
                {
                    for (var j = 0; j < slashes; j++) { sb.Append('\\'); }
                    sb.Append('\\').Append('"');
                    slashes = 0;
                }
                else { slashes = 0; sb.Append(c); }
            }
            for (var j = 0; j < slashes; j++) { sb.Append('\\'); }
            sb.Append('"');
            return sb.ToString();
        }
    }
}
