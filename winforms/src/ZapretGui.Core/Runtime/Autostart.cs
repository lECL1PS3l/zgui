using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;

namespace ZapretGui.Core.Runtime
{
    /// <summary>
    /// Автозапуск GUI при входе: задача планировщика «ZapretGUI» (runner.rs:174-246)
    /// и согласование с установленной службой (lib.rs:1665-1712). Задача создаётся
    /// с RunLevel Highest — стартует уже с правами администратора, без UAC.
    /// </summary>
    public static class Autostart
    {
        /// <summary>Legacy-значение автозапуска в HKCU\...\Run (версии ≤ 1.0.0).</summary>
        private const string BootRunKey = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";

        private const string LegacyValue = "zgui";

        /// <summary>Есть ли задача планировщика автозапуска (runner.rs:178-185).</summary>
        public static bool BootTaskExists()
        {
            int code;
            string output;
            Processes.RunOutputHidden("schtasks.exe",
                new[] { "/query", "/tn", Engines.BootTaskName }, out code, out output);
            return code == 0;
        }

        /// <summary>Осталась ли запись автозапуска в старом месте (runner.rs:187-194).</summary>
        public static bool LegacyBootRegistered()
        {
            int code;
            string output;
            Processes.RunOutputHidden("reg.exe",
                new[] { "query", BootRunKey, "/v", LegacyValue }, out code, out output);
            return code == 0;
        }

        /// <summary>Удаляет старую запись HKCU\...\Run (runner.rs:196-201).</summary>
        public static void RemoveLegacyBoot()
        {
            int code;
            string output;
            Processes.RunOutputHidden("reg.exe",
                new[] { "delete", BootRunKey, "/v", LegacyValue, "/f" }, out code, out output);
        }

        /// <summary>
        /// Текст ps1-скрипта для задачи планировщика (runner.rs:202-220). exe —
        /// путь к текущей программе, запуск с флагом --boot.
        /// </summary>
        public static string BootTaskScript(bool enable, string exe)
        {
            const string task = Engines.BootTaskName;
            if (!enable)
            {
                return Uac.PsHeader + "\n" +
                    "Unregister-ScheduledTask -TaskName '" + task +
                    "' -Confirm:$false -ErrorAction SilentlyContinue\nexit 0\n";
            }
            return Uac.PsHeader + "\n" +
                "$exe = " + Uac.PsQuote(exe) + "\n" +
                "$user = $env:USERDOMAIN + '\\' + $env:USERNAME\n" +
                "Unregister-ScheduledTask -TaskName '" + task +
                "' -Confirm:$false -ErrorAction SilentlyContinue\n" +
                "$action = New-ScheduledTaskAction -Execute $exe -Argument '--boot'\n" +
                "$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user\n" +
                "$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest\n" +
                "$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable\n" +
                "Register-ScheduledTask -TaskName '" + task + "' -Action $action -Trigger $trigger " +
                "-Principal $principal -Settings $settings -Force | Out-Null\nexit 0\n";
        }

        /// <summary>
        /// Создаёт/удаляет задачу планировщика (runner.rs:225-246). Создание с
        /// уровнем «наивысшие» требует администратора: без повышения уходит через
        /// один UAC-запрос. Возвращает строку ошибки или null.
        /// </summary>
        public static string ApplyBootTask(bool enable, string exe, string dataDir)
        {
            string script = Path.Combine(dataDir, "logs",
                "boot_task_" + Process.GetCurrentProcess().Id + ".ps1");
            try
            {
                Uac.WritePs1(script, BootTaskScript(enable, exe));
            }
            catch (Exception e)
            {
                return e.Message;
            }

            if (Uac.IsElevated())
            {
                int code;
                string stderr;
                Uac.RunPowerShell(new[] { "-File", script }, out code, out stderr);
                try { File.Delete(script); } catch { }
                if (code != 0)
                {
                    return stderr;
                }
            }
            else
            {
                // Ошибка кодом здесь не важна: ниже проверяем состояние фактом.
                Uac.RunElevatedScript(script);
                try { File.Delete(script); } catch { }
            }

            // Проверяем факт: молчаливая ошибка недопустима — иначе «автозапуск
            // включён», а задачи нет (из-за этого когда-то переписано).
            if (enable && !BootTaskExists())
            {
                return "задача планировщика не создана — проверьте права администратора";
            }
            if (!enable && BootTaskExists())
            {
                return "не удалось удалить задачу планировщика";
            }
            return null;
        }

        /// <summary>Нужна ли задача планировщика (lib.rs:1669-1675): служба установлена — задача не нужна.</summary>
        public static bool WantsTask(bool serviceInstalled, bool haveProfile)
        {
            return !serviceInstalled && haveProfile;
        }

        /// <summary>Действие по задаче планировщика.</summary>
        public enum BootAction
        {
            None,
            Create,
            Remove
        }

        /// <summary>План согласования автозапуска с текущим состоянием.</summary>
        public struct SyncPlan
        {
            /// <summary>Целевое состояние задачи.</summary>
            public bool WantTask;

            /// <summary>Что сделать с задачей планировщика.</summary>
            public BootAction Action;

            /// <summary>Записать ли settings.boot_app после применения.</summary>
            public bool SaveBootApp;
        }

        /// <summary>
        /// Чистое решение: какую задачу планировщика иметь (lib.rs:1677-1685).
        /// Если текущая настройка уже согласована — делать ничего не надо.
        /// </summary>
        public static SyncPlan PlanSync(bool bootApp, bool serviceInstalled, bool haveProfile)
        {
            bool want = WantsTask(serviceInstalled, haveProfile);
            var plan = new SyncPlan { WantTask = want };
            if (want == bootApp)
            {
                return plan;
            }
            plan.Action = want ? BootAction.Create : BootAction.Remove;
            plan.SaveBootApp = true;
            return plan;
        }

        /// <summary>
        /// Приводит механизм автозапуска к единственному верному состоянию
        /// (lib.rs:1688-1712). Тихо и best-effort: нехватка прав — в журнал.
        /// </summary>
        public static void Sync(State state)
        {
            bool serviceInstalled = state.ServiceRunning.HasValue;
            bool haveProfile = HaveAutostartProfile(state);
            var plan = PlanSync(state.Settings.BootApp, serviceInstalled, haveProfile);
            if (plan.Action == BootAction.None)
            {
                return;
            }
            string exe = Assembly.GetEntryAssembly()?.Location;
            if (string.IsNullOrEmpty(exe))
            {
                LogRing.Write("warn", "boot", "current_exe: путь к программе недоступен");
                return;
            }
            string error = ApplyBootTask(plan.WantTask, exe, state.Data);
            if (error != null)
            {
                LogRing.Write("warn", "boot", "не удалось согласовать автозапуск: " + error);
                return;
            }
            RemoveLegacyBoot();
            state.Settings.BootApp = plan.WantTask;
            state.Save();
            LogRing.Write("ok", "boot",
                plan.WantTask ? "автозапуск: задача планировщика создана"
                    : "автозапуск: задача планировщика снята");
        }

        /// <summary>Валиден ли выбранный профиль автозапуска (lib.rs:1665-1671).</summary>
        public static bool HaveAutostartProfile(State state)
        {
            return state.Settings.AutostartMode == "profile" &&
                !string.IsNullOrEmpty(state.Settings.AutostartProfile) &&
                state.Profiles.Exists(p => p.Id == state.Settings.AutostartProfile);
        }
    }
}
