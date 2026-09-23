using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using ZapretGui.Core.Config;
using ZapretGui.Core.Embedded;
using ZapretGui.Core.Log;
using ZapretGui.Core.Runtime;
using ZapretGui.Core.Updater;
using ZapretGui.Core.Util;
using Emb = ZapretGui.Core.Embedded.Embedded;
using Upd = ZapretGui.Core.Updater.Updater;

namespace ZapretGui.Core
{
    /// <summary>Корень движка для UI (lib.rs:72-94).</summary>
    public class RootInfo
    {
        [JsonField("path")] public string Path;
        [JsonField("exe")] public string Exe;
        [JsonField("ready")] public bool Ready;
    }

    /// <summary>Служба zapret для UI (lib.rs:96-102).</summary>
    public class ServiceInfo
    {
        [JsonField("installed")] public bool Installed;
        [JsonField("running")] public bool? Running;
        [JsonField("strategy")] public string Strategy;
    }

    /// <summary>Кэш обновлений для UI (lib.rs:104-109).</summary>
    public class UpdaterView
    {
        [JsonField("lastCheck")] public string LastCheck;
        [JsonField("entries")] public List<UpdEntry> Entries = new List<UpdEntry>();
    }

    /// <summary>
    /// Снимок состояния для UI (lib.rs:143-159) — единственный источник правды
    /// для поллинга раз в 4 секунды.
    /// </summary>
    public class Bootstrap
    {
        [JsonField("flowseal")] public RootInfo Flowseal;
        [JsonField("settings")] public Settings Settings;
        [JsonField("profiles")] public List<Profile> Profiles = new List<Profile>();
        [JsonField("runtime")] public Config.Runtime Runtime;
        [JsonField("service")] public ServiceInfo Service;
        [JsonField("updates")] public UpdaterView Updates;
        [JsonField("gameFilterPorts")] public string[] GameFilterPorts = new string[2];
        [JsonField("busy")] public bool Busy;
        [JsonField("elevated")] public bool Elevated;
        /// <summary>Кто держит обход: none|app|service|test|external.</summary>
        [JsonField("owner")] public string Owner;
        [JsonField("dataDir")] public string DataDir;
        [JsonField("warnings")] public List<string> Warnings = new List<string>();
    }

    /// <summary>
    /// Хост приложения: держит состояние (замена tauri Global, lib.rs:30-45),
    /// выполняет провижнинг при старте и собирает снимок Bootstrap.
    /// </summary>
    public static class AppHost
    {
        private static readonly object _lock = new object();
        private static State _state;
        private static bool _busy;
        private static bool _secondInstance;

        /// <summary>Флаг «стартовали из автозапуска» (Program --boot, lib.rs:2865).</summary>
        public static bool BootPending;

        public static State State
        {
            get { lock (_lock) { return _state; } }
        }

        public static bool Busy
        {
            get { lock (_lock) { return _busy; } }
        }

        /// <summary>Обнаружена уже запущенная копия (lib.rs:2774-2787).</summary>
        public static bool SecondInstance
        {
            get { lock (_lock) { return _secondInstance; } }
        }

        public static void SetBusy(bool busy)
        {
            lock (_lock) { _busy = busy; }
        }

        /// <summary>Проверяет-и-ставит занятость атомарно (lib.rs:421).</summary>
        public static bool TryBeginBusy()
        {
            lock (_lock)
            {
                if (_busy) { return false; }
                _busy = true;
                return true;
            }
        }

        /// <summary>
        /// Полный старт: portable-данные, журнал, каталог, движок, профили,
        /// автозапуск, починка прокси (lib.rs:2730-2800). Кидает исключение,
        /// если portable-папку создать нельзя.
        /// </summary>
        public static void Start(bool bootPending, Action<Entry> sink)
        {
            string data = PortableData.PortableDataDir();
            var state = State.Load(data);
            BootPending = bootPending;

            lock (_lock) { _state = state; }
            LogRing.Init(data, sink);
            LogRing.InstallPanicHook();
            LogRing.Write("info", "app", string.Format(
                "запуск Zapret GUI {0} (админ: {1}, портативно: {2})",
                AppInfo.Version, Uac.IsElevated() ? "да" : "нет", data));

            // Защита от двух копий: вторая копия может перетереть state.json.
            // Не блокируем запуск (замок может остаться от убитого процесса).
            string lockPath = Path.Combine(data, "zgui.lock");
            int pid = System.Diagnostics.Process.GetCurrentProcess().Id;
            try
            {
                if (File.Exists(lockPath))
                {
                    int other;
                    if (int.TryParse(File.ReadAllText(lockPath).Trim(), out other) &&
                        other != pid && Processes.PidAlive(other))
                    {
                        lock (_lock) { _secondInstance = true; }
                        LogRing.Write("warn", "app",
                            "обнаружена уже запущенная копия программы (pid " + other + ")");
                    }
                }
                File.WriteAllText(lockPath, pid.ToString());
            }
            catch { }

            Emb.SeedCatalog(data);
            ProvisionEngines(state);
            Profiles.EnsurePresets(state);
            Profiles.MigrateRemovedEngine(state);
            state.Save();
            ProvisionBoot(state);
        }

        /// <summary>
        /// Первый запуск без движка: распаковывает встроенный релиз Flowseal
        /// (lib.rs:586-623). Возвращает путь корня или null.
        /// </summary>
        public static string ProvisionEngines(State state)
        {
            string data = state.Data;
            string existing = state.Roots.Path(Engines.Flowseal);
            if (!string.IsNullOrEmpty(existing) && Directory.Exists(existing))
            {
                string norm = Emb.EngineRootFor(existing);
                if (norm != null)
                {
                    Emb.NeutralizeAuthorAutoupdate(norm);
                    if (norm != existing)
                    {
                        state.Roots.Set(Engines.Flowseal, norm);
                    }
                    CleanupStaleEngineDirs(norm);
                    if (string.IsNullOrEmpty(state.EngineVersion))
                    {
                        state.EngineVersion = EngineVersionFromRoot(norm) ?? Emb.EngineVersion;
                    }
                    SeedFlowsealConfigs(norm, data, state.Settings);
                    Profiles.RefreshCatalog(state);
                    return norm;
                }
            }

            string path = Emb.EnsureEmbeddedEngine(data);
            if (path == null)
            {
                return null;
            }
            state.Roots.Set(Engines.Flowseal, path);
            if (string.IsNullOrEmpty(state.EngineVersion))
            {
                state.EngineVersion = Emb.EngineVersion;
            }
            SeedFlowsealConfigs(path, data, state.Settings);
            Profiles.RefreshCatalog(state);
            return path;
        }

        /// <summary>
        /// Списки и заглушки конфигов в корне движка (lib.rs:352-370).
        /// </summary>
        public static void SeedFlowsealConfigs(string root, string dataDir, Settings settings)
        {
            string lists = Path.Combine(root, "lists");
            try { Directory.CreateDirectory(lists); } catch { }
            Emb.CopyTreeMissing(Path.Combine(dataDir, "catalog\\flowseal\\lists"), lists);

            string[,] seeds =
            {
                { "ipset-exclude-user.txt", "203.0.113.113/32\n" },
                { "list-general-user.txt", "# Never leave this file empty\ndomain.example.abc\n" },
                { "list-exclude-user.txt", "domain.example.abc\n" },
            };
            for (int i = 0; i < seeds.GetLength(0); i++)
            {
                string p = Path.Combine(lists, seeds[i, 0]);
                if (File.Exists(p)) { continue; }
                try { File.WriteAllText(p, seeds[i, 1], new UTF8Encoding(false)); } catch { }
            }
            // ipset-all.txt: в движке лежит заглушка Flowseal — материализуем реальный
            // список для режима «loaded» (иначе `--ipset=` правила мертвы).
            try { Upd.SyncIpset(root, dataDir, settings); } catch { }
        }

        /// <summary>Удаляет оставшиеся вложенные каталоги релиза (lib.rs:518-528).</summary>
        public static void CleanupStaleEngineDirs(string root)
        {
            string[] entries;
            try { entries = Directory.GetDirectories(root); } catch { return; }
            foreach (string dir in entries)
            {
                string name = Path.GetFileName(dir);
                if (name.StartsWith("zapret-discord-youtube-", StringComparison.Ordinal))
                {
                    try { Directory.Delete(dir, true); } catch { }
                    LogRing.Write("info", "engine", "удалён устаревший каталог движка: " + name);
                }
            }
        }

        /// <summary>Версия из имени каталога движка (lib.rs:1891-1896).</summary>
        public static string EngineVersionFromRoot(string root)
        {
            string name = Path.GetFileName(root);
            if (string.IsNullOrEmpty(name)) { return null; }
            int dash = name.LastIndexOf('-');
            if (dash < 0 || dash + 1 >= name.Length) { return null; }
            string v = name.Substring(dash + 1);
            return v.Length > 0 && v[0] >= '0' && v[0] <= '9' ? v : null;
        }

        /// <summary>
        /// Старый автозапуск из реестра → задача планировщика; досоздаёт задачу,
        /// если автозапуск включён, а задачи нет (lib.rs:2597-2660).
        /// </summary>
        public static void ProvisionBoot(State state)
        {
            if (Autostart.LegacyBootRegistered() && !state.Settings.BootApp)
            {
                state.Settings.BootApp = true;
                LogRing.Write("info", "boot",
                    "найден автозапуск из старой версии (реестр) — переношу в планировщик");
            }
            // Файл автозапуска пропал (переустановка, удалили файл): ставить задачу
            // без GUI нельзя — поправит сам GUI после входа. Пишем причину.
            if (state.Settings.BootApp && !Autostart.HaveAutostartProfile(state))
            {
                if (Autostart.BootTaskExists())
                {
                    if (Uac.IsElevated())
                    {
                        string exe = null;
                        try { exe = System.Reflection.Assembly.GetEntryAssembly().Location; } catch { }
                        if (!string.IsNullOrEmpty(exe))
                        {
                            string e = Autostart.ApplyBootTask(false, exe, state.Data);
                            LogRing.Write(e == null ? "ok" : "warn", "boot", e == null
                                ? "задача планировщика снята: файл автозапуска удалён"
                                : "не удалось снять задачу планировщика: " + e);
                        }
                    }
                    else
                    {
                        LogRing.Write("warn", "boot",
                            "автозапуск без файла автозапуска, но задача есть — снимет GUI после следующего входа");
                    }
                }
            }
            Autostart.RemoveLegacyBoot();
        }

        /// <summary>Предупреждения «важно перед запуском» (lib.rs:112-141).</summary>
        public static List<string> CollectWarnings(State state)
        {
            var outList = new List<string>();
            outList.Add("Не рекомендуется запускать Zapret вместе с VPN");
            if (state.ExternalWinws)
            {
                outList.Add("Обход запущен вне программы (winws.exe не через наш GUI). " +
                    "Два winws конфликтуют: нажмите «Остановить» и запустите стратегию здесь.");
            }
            string root = state.Roots.Path(Engines.Flowseal);
            if (!string.IsNullOrEmpty(root) && Directory.Exists(root))
            {
                bool originalBats = false;
                try
                {
                    foreach (string f in Directory.EnumerateFiles(root))
                    {
                        string n = Path.GetFileName(f).ToLowerInvariant();
                        if (n.EndsWith(".bat", StringComparison.Ordinal) &&
                            !n.StartsWith("service", StringComparison.Ordinal))
                        {
                            originalBats = true;
                            break;
                        }
                    }
                }
                catch { }
                if (originalBats)
                {
                    outList.Add("В корне " + Engines.Flowseal +
                        " найдены оригинальные .bat/.lua автора — отключите их автозапуск " +
                        "(службу/планировщик), иначе они будут конфликтовать с нашей программой.");
                }
            }
            return outList;
        }

        /// <summary>Снимок для UI (lib.rs:244-274).</summary>
        public static Bootstrap Snapshot()
        {
            State s = State;
            if (s == null) { return null; }

            // Владельца считаем до снимка: CurrentOwner сам читает состояние.
            string owner = Owner.OwnerName(Watchdog.Watchdog.CurrentOwner(s));

            var bs = new Bootstrap
            {
                Flowseal = RootInfoFor(s.Roots, Engines.Flowseal, Engines.WinwsExe),
                Settings = s.Settings,
                Profiles = new List<Profile>(s.Profiles),
                Service = new ServiceInfo
                {
                    Installed = s.ServiceRunning.HasValue,
                    Running = s.ServiceRunning,
                    Strategy = s.ServiceStrategy,
                },
                Updates = new UpdaterView
                {
                    LastCheck = s.Updater != null ? s.Updater.LastCheck : null,
                    Entries = s.Updater != null ? new List<UpdEntry>(s.Updater.Entries) : new List<UpdEntry>(),
                },
                Busy = Busy,
                Elevated = Uac.IsElevated(),
                Owner = owner,
                DataDir = s.Data,
                Warnings = CollectWarnings(s),
            };

            string tcp;
            string udp;
            Net.GameFilter.Ports(s.Settings.GameFilter, out tcp, out udp);
            bs.GameFilterPorts[0] = tcp;
            bs.GameFilterPorts[1] = udp;

            if (s.Runtime != null)
            {
                bs.Runtime = new Config.Runtime
                {
                    ProfileId = s.Runtime.ProfileId,
                    Pid = s.Runtime.Pid,
                    StartedAt = s.Runtime.StartedAt,
                    Via = s.Runtime.Via,
                    Alive = Processes.PidAlive(unchecked((int)s.Runtime.Pid)),
                };
            }
            return bs;
        }

        /// <summary>Готов ли движок (lib.rs:80-94).</summary>
        public static RootInfo RootInfoFor(Roots roots, string engine, string exeName)
        {
            string p = roots.Path(engine);
            if (string.IsNullOrEmpty(p))
            {
                return new RootInfo { Ready = false };
            }
            string rel = Processes.FindExe(p, exeName);
            return new RootInfo
            {
                Path = p,
                Exe = rel,
                Ready = rel != null,
            };
        }

        /// <summary>
        /// Фоновые наблюдатели: состояние процесса и службы раз в 2 c, детект
        /// «winws вне программы» и автопроверка конфигов (lib.rs:2475-2592).
        /// </summary>
        public static void SpawnWatchers()
        {
            var t = new System.Threading.Thread(WatcherLoop)
            {
                IsBackground = true,
                Name = "zgui-watchers",
            };
            t.Start();
        }

        private static void WatcherLoop()
        {
            long lastSvc = 0;
            bool startupCheck = true;
            while (true)
            {
                System.Threading.Thread.Sleep(2000);
                State s = State;
                if (s == null) { continue; }

                bool changed = false;
                Config.Runtime rt = s.Runtime;
                if (rt != null && rt.Via == "app" && !Processes.PidAlive(unchecked((int)rt.Pid)))
                {
                    s.Runtime = null;
                    changed = true;
                    Bus.RaiseStatus();
                    Bus.RaiseToast("warn", "процесс запрета завершился — смотрите журнал");
                }

                long now = BatImport.NowEpoch();
                if (now - lastSvc > 10)
                {
                    lastSvc = now;
                    bool installed;
                    bool? running;
                    Service.State(out installed, out running);
                    bool? serviceRunning = installed ? running : (bool?)null;
                    string strat = installed ? Service.Strategy() : null;
                    if (s.ServiceRunning != serviceRunning || s.ServiceStrategy != strat)
                    {
                        s.ServiceRunning = serviceRunning;
                        s.ServiceStrategy = strat;
                        changed = true;
                    }
                }
                if (changed) { s.Save(); }

                // «Запущено вне программы»: winws нашего движка без записи runtime.
                bool external = Watchdog.Watchdog.CurrentOwner(s) == WinwsOwner.External;
                if (s.ExternalWinws != external)
                {
                    s.ExternalWinws = external;
                    s.Save();
                }

                AutoCheckUpdates(s, ref startupCheck);
            }
        }

        // Автопроверка конфигов по расписанию (lib.rs:2528-2589).
        private static void AutoCheckUpdates(State s, ref bool startupCheck)
        {
            int interval = s.Settings.UpdateIntervalHours;
            if (interval <= 0 || Busy) { return; }

            UpdaterCache cache = s.Updater;
            long now = BatImport.NowEpoch();
            bool cooled = !cache.NextAuto.HasValue || (long)cache.NextAuto.Value <= now;
            long lastAuto = 0;
            if (!string.IsNullOrEmpty(cache.LastAuto))
            {
                long.TryParse(cache.LastAuto, out lastAuto);
            }
            bool plannedDue = lastAuto == 0 || lastAuto + (long)interval * 3600 <= now;
            // Пустой каталог: игнорируем расписание и ждём короткий кулдаун, чтобы
            // каталог заполнился сам при старте.
            bool empty = cache.Entries.Count == 0;
            bool due = startupCheck || (empty ? cooled : (cooled && plannedDue));
            if (!due) { return; }

            startupCheck = false;
            if (!TryBeginBusy()) { return; }
            long retry = empty ? 60 : 15 * 60;
            cache.NextAuto = (ulong)(now + retry);
            s.Save();

            var thread = new System.Threading.Thread(() =>
            {
                try
                {
                    var res = Upd.CheckAllAsync(s.Data, s.Roots, s.Settings)
                        .ConfigureAwait(false).GetAwaiter().GetResult();
                    if (res.IsOk)
                    {
                        var fresh = State;
                        fresh.Updater.Entries = res.Value;
                        fresh.Updater.LastCheck = BatImport.NowEpoch().ToString();
                        fresh.Updater.LastAuto = BatImport.NowEpoch().ToString();
                        fresh.Save();
                        Bus.RaiseUpdates();
                        Bus.RaiseToast("info", "автопроверка обновлений конфигов завершена");
                    }
                    else
                    {
                        LogRing.Write("warn", "updates", "автопроверка не удалась: " + res.Error);
                    }
                }
                catch (Exception e)
                {
                    LogRing.Write("warn", "updates", "автопроверка не удалась: " + e.Message);
                }
                finally
                {
                    SetBusy(false);
                }
            })
            { IsBackground = true, Name = "zgui-autocheck" };
            thread.Start();
        }
    }

    /// <summary>Сведения о программе для «О программе» (lib.rs:2291-2300).</summary>
    public static class AppInfo
    {
        public const string Name = "Zapret GUI";
        public const string Version = "1.2.0";
        public const bool Portable = true;

        public static string Info()
        {
            return Util.Json.Serialize(new Dictionary<string, object>
            {
                { "name", Name },
                { "version", Version },
                { "portable", Portable },
                { "elevated", Uac.IsElevated() },
                { "bundledConfigs", Emb.SnapshotInfo },
                { "bundledEngines", Emb.EngineInfo },
            });
        }
    }
}