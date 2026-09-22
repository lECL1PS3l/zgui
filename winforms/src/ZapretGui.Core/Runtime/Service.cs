using System;
using System.Diagnostics;
using System.IO;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;

namespace ZapretGui.Core.Runtime
{
    /// <summary>
    /// Служба zapret: установка/удаление/состояние/стратегия — порт service.rs.
    /// Скрипты пишутся в UTF-8 с BOM и выполняются элевированным PowerShell:
    /// иначе кавычки в binPath и кириллица ломаются.
    /// </summary>
    public static class Service
    {
        /// <summary>Ключ службы в реестре (service.rs:427).</summary>
        public const string RegistryKey = "HKLM\\System\\CurrentControlSet\\Services\\" + Engines.ServiceName;

        /// <summary>Значение с идентификатором стратегии (service.rs:429).</summary>
        public const string StrategyValue = "zgui-strategy";

        /// <summary>
        /// Устанавливает службу на стратегию профиля (service.rs:384-419).
        /// Возвращает строку ошибки или null при успехе.
        /// </summary>
        public static string Install(string root, Profile profile, string[] args, string dataDir)
        {
            string cmdline = BuildCmdline(root, profile, args);
            string script = Path.Combine(dataDir, "logs",
                "svc_install_" + Process.GetCurrentProcess().Id + ".ps1");
            // ВАЖНО (почему не sc.exe): `sc` в PowerShell — алиас Set-Content, а
            // передать binPath с кавычками через нативную командную строку PS 5.1
            // надёжно нельзя. New-Service принимает готовую строку как есть.
            string body = Uac.PsHeader + "\n" +
                "Stop-Service -Name '" + Engines.ServiceName + "' -Force -ErrorAction SilentlyContinue\n" +
                "& sc.exe delete " + Engines.ServiceName + " 2>$null | Out-Null\n" +
                "Start-Sleep -Milliseconds 600\n" +
                "New-Service -Name '" + Engines.ServiceName + "' -BinaryPathName '" +
                PsSingleQuote(cmdline) + "' -StartupType Automatic " +
                "-Description 'Zapret DPI bypass software (zgui)' | Out-Null\n" +
                "Start-Service -Name '" + Engines.ServiceName + "'\n";
            try
            {
                Uac.WritePs1(script, body);
            }
            catch (Exception e)
            {
                return e.Message;
            }
            int code = Uac.RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            if (code != 0)
            {
                return "установка службы завершилась с кодом " + code;
            }
            // Запоминаем, на какую стратегию поставлена служба.
            int regCode;
            string regOut;
            Processes.RunOutputHidden("reg.exe",
                new[] { "add", RegistryKey, "/v", StrategyValue, "/t", "REG_SZ", "/d", profile.Id, "/f" },
                out regCode, out regOut);
            return null;
        }

        /// <summary>Запускает установленную службу zapret (service.rs:286-297).</summary>
        public static string StartService(string dataDir)
        {
            string script = Path.Combine(dataDir, "logs", "svc_start_" + Process.GetCurrentProcess().Id + ".ps1");
            string body = Uac.PsHeader + "\nnet start " + Engines.ServiceName + " 2>$null | Out-Null\nexit 0";
            try
            {
                Uac.WritePs1(script, body);
            }
            catch (Exception e)
            {
                return e.Message;
            }
            int code = Uac.RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            return code == 0 ? null : "запуск службы завершился с кодом " + code;
        }

        /// <summary>Удаляет службу (service.rs:421-433). Возвращает ошибку или null.</summary>
        public static string Remove(string dataDir)
        {
            string script = Path.Combine(dataDir, "logs",
                "svc_remove_" + Process.GetCurrentProcess().Id + ".ps1");
            string body = Uac.PsHeader + "\n" +
                "Stop-Service -Name '" + Engines.ServiceName + "' -Force -ErrorAction SilentlyContinue\n" +
                "& sc.exe delete " + Engines.ServiceName + " 2>$null | Out-Null\n" +
                "exit 0";
            try
            {
                Uac.WritePs1(script, body);
            }
            catch (Exception e)
            {
                return e.Message;
            }
            int code = Uac.RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            return code == 0 ? null : "ошибка удаления службы: код " + code;
        }

        /// <summary>
        /// Установлена ли служба и запущена ли (service.rs:435-443). running —
        /// null, когда служба не установлена.
        /// </summary>
        public static void State(out bool installed, out bool? running)
        {
            int code;
            string output;
            Processes.RunOutputHidden("sc.exe", new[] { "query", Engines.ServiceName },
                out code, out output);
            if (code != 0)
            {
                installed = false;
                running = null;
                return;
            }
            string txt = output.ToUpperInvariant();
            installed = true;
            running = txt.Contains("STATE") && txt.Contains("RUNNING");
        }

        /// <summary>Идентификатор стратегии, на которую поставлена служба (service.rs:449-461).</summary>
        public static string Strategy()
        {
            int code;
            string output;
            Processes.RunOutputHidden("reg.exe",
                new[] { "query", RegistryKey, "/v", StrategyValue },
                out code, out output);
            if (code != 0)
            {
                return null;
            }
            string line = null;
            foreach (var l in output.Split('\n'))
            {
                if (l.Contains(StrategyValue))
                {
                    line = l;
                    break;
                }
            }
            if (line == null)
            {
                return null;
            }
            var parts = line.Split((char[])null, StringSplitOptions.RemoveEmptyEntries);
            return parts.Length > 0 ? parts[parts.Length - 1] : null;
        }

        /// <summary>
        /// Собирает командную строку службы (service.rs:356-372): exe в кавычках,
        /// аргументы с пробелами — тоже.
        /// </summary>
        public static string BuildCmdline(string root, Profile profile, string[] args)
        {
            string rel = Processes.FindExe(root, profile.ExeName());
            string bin = string.IsNullOrEmpty(rel)
                ? Path.Combine(root, "bin", profile.ExeName())
                : Path.Combine(root, rel.Replace('/', '\\'));
            var sb = new System.Text.StringBuilder();
            sb.Append('"').Append(bin).Append('"');
            foreach (var a in args)
            {
                if (a.Contains(" "))
                {
                    sb.Append(" \"").Append(a).Append('"');
                }
                else
                {
                    sb.Append(' ').Append(a);
                }
            }
            return sb.ToString();
        }

        /// <summary>Экранирует строку для PowerShell-литерала в одинарных кавычках.</summary>
        private static string PsSingleQuote(string text)
        {
            return text.Replace("'", "''");
        }
    }
}
