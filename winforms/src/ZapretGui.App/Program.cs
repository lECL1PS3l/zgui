using System;
using System.Windows.Forms;
using ZapretGui.Core;

namespace ZapretGui.App
{
    internal static class Program
    {
        [STAThread]
        private static int Main(string[] args)
        {
            // --boot: признак запуска задачей планировщика при входе (lib.rs:2863).
            bool bootPending = Array.IndexOf(args, "--boot") >= 0;

            bool elevated = Array.IndexOf(args, "--elevated") >= 0;
            if (elevated)
            {
                // Флаг только для протокола запуска: права уже в токене процесса.
            }

            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            string startupError = null;
            try
            {
                // Логгер не знает про UI — отдаём ему приёмник записей (lib.rs:2734-2741).
                AppHost.Start(bootPending, e => Bus.RaiseLog(e));
            }
            catch (Exception e)
            {
                startupError = e.Message;
            }

            if (startupError != null)
            {
                MessageBox.Show(startupError, "Zapret GUI",
                    MessageBoxButtons.OK, MessageBoxIcon.Error);
                return 1;
            }

            // Всегда перезапускаемся от админа, если так просил пользователь
            // (lib.rs:2755-2769). UAC отклонён — НЕ закрываемся, работаем без прав.
            if (AppHost.State.Settings.AlwaysAdmin && !elevated &&
                !Core.Runtime.Uac.IsElevated())
            {
                if (Core.Runtime.Uac.RelaunchAsAdmin())
                {
                    return 0;
                }
                Core.Log.LogRing.Write("warn", "app",
                    "запрос прав администратора отклонён — запуск без прав");
            }

            Core.Watchdog.Watchdog.Spawn(AppHost.State);
            AppHost.SpawnWatchers();

            Application.Run(new MainForm());
            return 0;
        }
    }
}