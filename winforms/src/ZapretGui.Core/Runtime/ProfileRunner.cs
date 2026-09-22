using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;
using ZapretGui.Core.Net;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Runtime
{
    /// <summary>
    /// Запуск/остановка/статус стратегии обхода — порт lib.rs:901-1120
    /// (do_start, do_stop, current_status). Процессная часть: winws.exe
    /// поднимается либо напрямую (GUI уже от администратора), либо через
    /// элевированный launcher с pid-файлом.
    /// </summary>
    public static class ProfileRunner
    {
        /// <summary>Стратегия запущена процессом программы (config.rs:106).</summary>
        public const string ViaApp = "app";

        /// <summary>
        /// Запускает профиль: ищет winws.exe, подставляет порты игрового фильтра,
        /// поднимает процесс и пишет runtime (lib.rs:901-975).
        /// </summary>
        public static Result<Config.Runtime> Start(Profile profile, string rootPath, Settings settings, string dataDir)
        {
            string rel = Processes.FindExe(rootPath, profile.ExeName());
            if (string.IsNullOrEmpty(rel))
            {
                string e = "не найден " + profile.ExeName() + " в корне движка";
                LogRing.Write("err", "start", profile.Name + ": " + e);
                return Result<Config.Runtime>.Err(Humanize.HumanError(e));
            }
            string exe = Path.Combine(rootPath, rel.Replace('/', '\\'));

            GameFilter.Ports(settings.GameFilter, out string tcp, out string udp);
            string[] args = GameFilter.Apply(profile.Args, tcp, udp).ToArray();

            string logs = Path.Combine(dataDir, "logs");
            try { Directory.CreateDirectory(logs); } catch { }
            string outLog = Path.Combine(logs, "stdout-" + profile.Id + ".txt");
            string errLog = Path.Combine(logs, "stderr-" + profile.Id + ".txt");
            string pidFile = Path.Combine(logs, "pid-" + profile.Id + ".txt");
            string wd = Path.Combine(rootPath, "bin");

            // Чистим старые логи: иначе при мгновенном выходе процесса в ошибку
            // попадёт содержимое прошлого запуска (в т.ч. в другой кодировке).
            try { File.Delete(outLog); } catch { }
            try { File.Delete(errLog); } catch { }

            uint pid;
            if (Uac.IsElevated())
            {
                // GUI уже от администратора: winws стартует напрямую — мгновенно,
                // с логами и корректным квотингом, без UAC и launcher-скриптов.
                int started;
                Processes.SpawnHidden(exe, args, wd, outLog, errLog, out started);
                Thread.Sleep(900);
                pid = (uint)started;
            }
            else
            {
                // Иначе — элевированный launcher (один UAC); логи в этом пути
                // не собираются.
                string launcher = Uac.WriteLauncher(exe, wd, args, pidFile);
                string launchError = Uac.SpawnAndWaitPid(launcher, pidFile, 60, out pid);
                try { File.Delete(launcher); } catch { }
                if (launchError != null)
                {
                    return Result<Config.Runtime>.Err(launchError);
                }
            }

            if (!Processes.PidAlive((int)pid))
            {
                // Показываем и stderr, и stdout: winws пишет диагностику в оба потока.
                string errTail = Text.TailFile(errLog, 2000);
                string outTail = Text.TailFile(outLog, 2000);
                var parts = new List<string>(2);
                if (!string.IsNullOrWhiteSpace(errTail)) { parts.Add(errTail); }
                if (!string.IsNullOrWhiteSpace(outTail)) { parts.Add(outTail); }
                string msg = string.Join("\n", parts);
                LogRing.Write("err", "start",
                    string.Format("«{0}» сразу завершился: {1}", profile.Name, msg.Trim()));

                string hint = Uac.IsElevated()
                    ? "стратегия сразу завершилась — подробности в «Журнале»"
                    : "для запуска обхода нужен запрос прав администратора — включите «Всегда запускать программу от администратора» в «Настройках»";
                return Result<Config.Runtime>.Err(string.IsNullOrWhiteSpace(msg)
                    ? hint + " (движок не выдал ни одной строки вывода)"
                    : hint + ". Последние строки движка:\n" + msg.Trim());
            }

            var runtime = new Config.Runtime
            {
                ProfileId = profile.Id,
                Pid = pid,
                StartedAt = (ulong)DateTimeOffset.UtcNow.ToUnixTimeSeconds(),
                Via = ViaApp,
                Alive = true
            };
            LogRing.Write("ok", "start",
                string.Format("запущена стратегия «{0}» ({1}, pid {2})", profile.Name, profile.Engine, pid));
            return Result<Config.Runtime>.Ok(runtime);
        }

        /// <summary>
        /// Останавливает профиль: сбрасывает runtime, глушит дерево процесса
        /// (lib.rs:831-870 do_stop — ветка runtime). Возвращает true, если
        /// что-то было запущено.
        /// </summary>
        public static bool Stop(State state, bool silent)
        {
            Config.Runtime rt = state.Runtime;
            if (rt == null)
            {
                return false;
            }
            state.Runtime = null;
            state.Save();
            if (rt.Via == "service")
            {
                StopService(state.Data);
            }
            else if (Processes.PidAlive((int)rt.Pid))
            {
                Uac.StopPid((int)rt.Pid, state.Data);
            }
            LogRing.Write("info", "stop",
                string.Format("остановлено: {0} (pid {1}, через {2})", rt.ProfileId, rt.Pid, rt.Via));
            return true;
        }

        /// <summary>Состояние запуска с проверкой живости процесса (lib.rs:1089-1095).</summary>
        public static Config.Runtime CurrentStatus(State state)
        {
            Config.Runtime rt = state.Runtime;
            if (rt == null)
            {
                return null;
            }
            rt.Alive = Processes.PidAlive((int)rt.Pid);
            return rt;
        }

        /// <summary>
        /// Останавливает службу zapret: net stop через элевированный PowerShell
        /// (lib.rs:794-802). Служба остаётся установленной, но не работает.
        /// </summary>
        internal static void StopService(string dataDir)
        {
            string script = Path.Combine(dataDir, "logs",
                "svc_stop_" + System.Diagnostics.Process.GetCurrentProcess().Id + ".ps1");
            try
            {
                Uac.WritePs1(script, Uac.PsHeader + "\nnet stop " + Engines.ServiceName +
                    " 2>$null | Out-Null\nexit 0");
            }
            catch
            {
                return;
            }
            Uac.RunElevatedScript(script);
            try { File.Delete(script); } catch { }
        }
    }
}
