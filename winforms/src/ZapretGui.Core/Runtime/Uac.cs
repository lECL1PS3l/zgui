using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

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
