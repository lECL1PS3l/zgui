using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Runtime
{
    /// <summary>
    /// PowerShell-обёртки и elevation из runner.rs. .ps1 пишется в UTF-8 с BOM —
    /// иначе PowerShell 5.1 читает кириллицу как ANSI и ломает кавычки.
    /// </summary>
    public static class Uac
    {
        /// <summary>$ErrorActionPreference = 'Stop' (runner.rs:8).</summary>
        public const string PsHeader = "$ErrorActionPreference = 'Stop'";

        /// <summary>Пишет текст в UTF-8 с BOM (runner.rs:248-258).</summary>
        public static void WritePs1(string path, string body)
        {
            File.WriteAllText(path, body, new UTF8Encoding(true));
        }

        /// <summary>
        /// Запущен ли процесс от администратора (runner.rs:59-71). Проверка через
        /// членство в группе Administrators — как IsInRole в PowerShell.
        /// </summary>
        public static bool IsElevated()
        {
            IntPtr token;
            if (!OpenProcessToken(GetCurrentProcess(), TokenQuery, out token))
            {
                return false;
            }
            try
            {
                uint cb = 0;
                CreateWellKnownSid(BuiltinAdministratorsSid, IntPtr.Zero, IntPtr.Zero, ref cb);
                if (cb == 0) { return false; }
                IntPtr sid = Marshal.AllocHGlobal((int)cb);
                try
                {
                    if (!CreateWellKnownSid(BuiltinAdministratorsSid, IntPtr.Zero, sid, ref cb))
                    {
                        return false;
                    }
                    bool isMember;
                    return CheckTokenMembership(token, sid, out isMember) && isMember;
                }
                finally { Marshal.FreeHGlobal(sid); }
            }
            finally { CloseHandle(token); }
        }

        /// <summary>Экранирует аргумент для PowerShell: обрамляет кавычками (runner.rs:31-44).</summary>
        public static string PsQuote(string arg)
        {
            var sb = new StringBuilder((arg ?? string.Empty).Length + 2);
            sb.Append('"');
            foreach (var c in arg ?? string.Empty)
            {
                if (c == '"') { sb.Append("`\""); }
                else if (c == '`') { sb.Append("``"); }
                else if (c == '$') { sb.Append("`$"); }
                else { sb.Append(c); }
            }
            sb.Append('"');
            return sb.ToString();
        }

        /// <summary>Формирует -ArgumentList @('a','b') для Start-Process (runner.rs:47-51).</summary>
        public static string PsArgList(IList<string> args)
        {
            var parts = new string[args.Count];
            for (var i = 0; i < args.Count; i++)
            {
                parts[i] = PsQuote(args[i]);
            }
            return string.Join(",", parts);
        }

        /// <summary>
        /// Запускает PowerShell и ждёт завершения: при успехе возвращает stdout,
        /// при ошибке — код и stderr (runner.rs:54-68).
        /// </summary>
        public static string RunPowerShell(string[] args, out int exitCode, out string stderr)
        {
            var full = new string[args.Length + 3];
            full[0] = "-NoProfile";
            full[1] = "-ExecutionPolicy";
            full[2] = "Bypass";
            args.CopyTo(full, 3);
            string stdout;
            Processes.RunOutputHidden("powershell.exe", full, out exitCode, out stdout, out stderr);
            return stdout.Trim();
        }

        /// <summary>
        /// Запускает ps1-скрипт с UAC-элевацией и ждёт его завершения
        /// (runner.rs:97-118). Возвращает код завершения.
        /// </summary>
        public static int RunElevatedScript(string script)
        {
            string inner = "-NoProfile -ExecutionPolicy Bypass -File \"" + script + "\"";
            string cmd = "$ErrorActionPreference = 'Stop'; try { $p = Start-Process -FilePath " +
                "'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe' -ArgumentList @(" +
                PsQuote(inner) + ") -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode } " +
                "catch { exit 1 }";
            string stderr;
            RunPowerShell(new[] { "-Command", cmd }, out int code, out stderr);
            return code;
        }

        /// <summary>
        /// Пишет launcher-скрипт: поднимает winws через Start-Process -Verb RunAs
        /// и оставляет pid-файл (runner.rs:120-144). Каталог для скрипта — рядом
        /// с pid-файлом, расширение .ps1.
        /// </summary>
        public static string WriteLauncher(string exe, string wd, string[] args, string pidFile)
        {
            string scriptPath = Path.ChangeExtension(pidFile, ".ps1");
            string body = PsHeader + "\n" +
                "$pidFile = " + PsQuote(pidFile) + "\n" +
                "try {\n" +
                "  $argsRaw = @(" + PsArgList(args) + ")\n" +
                "  # Start-Process flattens arrays without preserving quotes around paths with spaces.\n" +
                "  $argLine = @($argsRaw | ForEach-Object { $a = [string]$_; if ($a -match '[\\s\"]') { '\"' + $a.Replace('\"', '\\\"') + '\"' } else { $a } }) -join ' '\n" +
                "  $p = Start-Process -FilePath " + PsQuote(exe) + " -WorkingDirectory " + PsQuote(wd) +
                " -WindowStyle Hidden -Verb RunAs -ArgumentList $argLine -PassThru\n" +
                "  Start-Sleep -Milliseconds 700\n" +
                "  if ($p -and -not $p.HasExited) { $status = [string]$p.Id } else { $status = 'process exited immediately' }\n" +
                "} catch {\n" +
                "  $status = 'LAUNCH_ERROR: ' + $_.Exception.Message\n" +
                "}\n" +
                "# Один файл статуса, UTF-8 с BOM: читатель декодирует без «иероглифов».\n" +
                "Set-Content -LiteralPath $pidFile -Value $status -Encoding UTF8\n" +
                "if ($status -notmatch '^[0-9]+$') { exit 1 }\n";
            WritePs1(scriptPath, body);
            return scriptPath;
        }

        /// <summary>
        /// Запускает launcher и ждёт появления pid-файла (runner.rs:146-172).
        /// Возвращает null при успехе (pid — в out), либо ошибку launcher-а.
        /// </summary>
        public static string SpawnAndWaitPid(string script, string pidFile, int timeoutSeconds, out uint pid)
        {
            pid = 0;
            try { File.Delete(pidFile); } catch { }
            string stderr;
            RunPowerShell(new[] { "-File", script }, out int code, out stderr);
            var deadline = DateTime.UtcNow.AddSeconds(timeoutSeconds);
            while (DateTime.UtcNow < deadline)
            {
                string text = Text.ReadTextAuto(pidFile);
                if (text != null)
                {
                    string t = text.Trim();
                    if (uint.TryParse(t, out pid))
                    {
                        return null;
                    }
                    if (t.Length > 0)
                    {
                        // Launcher записал ошибку — не ждём таймаут впустую.
                        string line = null;
                        foreach (var l in t.Split('\n'))
                        {
                            string trimmed = l.Trim();
                            if (trimmed.StartsWith("LAUNCH_ERROR", StringComparison.Ordinal))
                            {
                                line = trimmed;
                                break;
                            }
                        }
                        return line ?? t;
                    }
                }
                Thread.Sleep(200);
            }
            return "не удалось запустить процесс — подтверждение прав администратора отклонено или файл недоступен";
        }

        /// <summary>Прибивает несколько процессов одним UAC-запросом (runner.rs:372-387).</summary>
        public static bool StopPids(IList<uint> pids, string dataDir)
        {
            if (pids == null || pids.Count == 0) { return true; }
            string script = Path.Combine(dataDir, "logs", "kill_multi_" + Process.GetCurrentProcess().Id + ".ps1");
            var sb = new StringBuilder();
            sb.AppendLine(PsHeader);
            foreach (uint id in pids)
            {
                sb.Append("taskkill /F /T /PID ").Append(id).Append(" | Out-Null\n");
            }
            sb.Append("exit 0\n");
            try { WritePs1(script, sb.ToString()); }
            catch { return false; }
            int code = RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            return code == 0;
        }

        /// <summary>
        /// Запускает ps1 с UAC-элевацией, НЕ ожидая завершения — для длительного
        /// теста стратегий (runner.rs:130-149). Возвращает null при успехе, иначе ошибку.
        /// </summary>
        public static string SpawnElevatedScript(string script)
        {
            string inner = "-NoProfile -ExecutionPolicy Bypass -File \"" + script + "\"";
            string cmd = "Start-Process -FilePath 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe' -ArgumentList @(" +
                PsQuote(inner) + ") -Verb RunAs -WindowStyle Hidden";
            try
            {
                var info = new ProcessStartInfo("powershell.exe",
                    Processes.BuildArgs(new[] { "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", cmd }))
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    WindowStyle = ProcessWindowStyle.Hidden
                };
                using (Process.Start(info)) { }
                return null;
            }
            catch (Exception e) { return e.Message; }
        }

        /// <summary>
        /// Запускает ps1 напрямую, без UAC — для случая, когда GUI уже повышен
        /// (runner.rs:156-169). Start-Process -Verb RunAs из повышенного процесса
        /// может не запустить дочерний процесс, и тест «зависнет» на старте.
        /// </summary>
        public static string SpawnScriptDirect(string script)
        {
            try
            {
                var info = new ProcessStartInfo("powershell.exe",
                    Processes.BuildArgs(new[] { "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script }))
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    WindowStyle = ProcessWindowStyle.Hidden
                };
                using (Process.Start(info)) { }
                return null;
            }
            catch (Exception e) { return "не удалось запустить скрипт: " + e.Message; }
        }

        /// <summary>Прибивает процесс и его дочерние через элевированный taskkill (runner.rs:185-196).</summary>
        public static bool StopPid(int pid, string dataDir)
        {
            string script = Path.Combine(dataDir, "logs",
                "kill_" + Process.GetCurrentProcess().Id + "_" + pid + ".ps1");
            try
            {
                WritePs1(script, PsHeader + "\ntaskkill /F /T /PID " + pid + " | Out-Null\nexit 0");
            }
            catch
            {
                return false;
            }
            int code = RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            return code == 0;
        }

        /// <summary>
        /// Перезапускает текущий exe с UAC (RunAs). Флаг --boot обязательно
        /// переносится: иначе запуск от автозагрузки терял признак «старт при
        /// входе» и стратегия не поднималась (runner.rs:73-95).
        /// Возвращает true, если привилегированный экземпляр стартовал;
        /// false — пользователь отклонил запрос UAC.
        /// </summary>
        public static bool RelaunchAsAdmin()
        {
            var exe = System.Reflection.Assembly.GetEntryAssembly().Location;
            var boot = Array.IndexOf(Environment.GetCommandLineArgs(), "--boot") >= 0;
            var args = boot ? "--elevated --boot" : "--elevated";
            var handle = ShellExecuteW(IntPtr.Zero, "runas", exe, args, null, SwHide);
            // ShellExecuteW возвращает значение > 32 при успехе.
            return handle.ToInt64() > 32;
        }

        private const uint TokenQuery = 0x0008;
        private const int BuiltinAdministratorsSid = 26;
        private const int SwHide = 0;

        [DllImport("kernel32.dll")]
        private static extern IntPtr GetCurrentProcess();

        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool OpenProcessToken(IntPtr processHandle, uint desiredAccess, out IntPtr tokenHandle);

        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool CreateWellKnownSid(int sidType, IntPtr domainSid, IntPtr sid, ref uint cbSid);

        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool CheckTokenMembership(IntPtr tokenHandle, IntPtr sidToCheck, out bool isMember);

        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);

        [DllImport("shell32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        private static extern IntPtr ShellExecuteW(IntPtr hwnd, string lpOperation, string lpFile,
            string lpParameters, string lpDirectory, int nShowCmd);
    }
}
