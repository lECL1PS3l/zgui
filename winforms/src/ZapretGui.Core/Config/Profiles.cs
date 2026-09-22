using System;
using System.Collections.Generic;
using System.IO;
using ZapretGui.Core.Updater;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Config
{
    /// <summary>
    /// Каталог стратегий, группы, удаление, миграция — порт refresh_catalog
    /// (lib.rs:693-736), ensure_presets (lib.rs:625), delete_profile (lib.rs:782),
    /// group_of (tester.rs:602), is_author_profile (lib.rs:685).
    /// </summary>
    public static class Profiles
    {
        public const string GroupPreset = "flowseal preset";
        public const string GroupBat = "flowseal bat";

        /// <summary>Группирует профили по типу стратегии для UI (tester.rs:602).</summary>
        public static string GroupOf(Profile p)
        {
            return !string.IsNullOrEmpty(p.Source) && p.Source.StartsWith("preset:", StringComparison.Ordinal)
                ? GroupPreset
                : GroupBat;
        }

        /// <summary>Профиль из каталога/пресетов — его нельзя удалять и менять (lib.rs:685).</summary>
        public static bool IsAuthorProfile(Profile p)
        {
            if (p.Builtin)
            {
                return true;
            }
            if (!string.IsNullOrEmpty(p.Source))
            {
                return p.Source.StartsWith("preset:", StringComparison.Ordinal) ||
                       p.Source.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);
            }
            return false;
        }

        /// <summary>Встроенные пресеты-шаблоны flowseal (profiles.rs:27 — пока пусто).</summary>
        public static List<Profile> BuiltinFlowsealPresets()
        {
            return new List<Profile>();
        }

        /// <summary>Добавляет пресеты, если ни одного ещё нет (lib.rs:625).</summary>
        public static void EnsurePresets(State state)
        {
            bool hasPreset = state.Profiles.Exists(p =>
                p.Engine == Engines.Flowseal &&
                !string.IsNullOrEmpty(p.Source) &&
                p.Source.StartsWith("preset:", StringComparison.Ordinal));
            if (!hasPreset)
            {
                state.Profiles.AddRange(BuiltinFlowsealPresets());
            }
        }

        /// <summary>
        /// Перечитывает .bat из каталога raw-стратегий в профили (lib.rs:693-736).
        /// Возвращает профили для UI; состояние сохраняется.
        /// </summary>
        public static List<Profile> RefreshCatalog(State state)
        {
            string root = state.Roots.Path(Engines.Flowseal);
            if (string.IsNullOrEmpty(root))
            {
                EnsurePresets(state);
                state.Save();
                return state.Profiles;
            }
            var bats = new List<(string Name, string Content)>();
            try
            {
                foreach (string path in Directory.EnumerateFiles(state.RawStrategiesDir(), "*.bat"))
                {
                    try
                    {
                        bats.Add((Path.GetFileName(path), BatImport.DecodeStrategyBytes(File.ReadAllBytes(path))));
                    }
                    catch { }
                }
            }
            catch { }

            List<Profile> imported = BatImport.ImportBatProfiles(root, bats, state.Profiles);
            var importedIds = new HashSet<string>();
            foreach (Profile p in imported)
            {
                importedIds.Add(p.Id);
            }
            // Держим ручные (custom) профили, убираем потерянные импортированные.
            state.Profiles.RemoveAll(p =>
                p.Engine == Engines.Flowseal &&
                !string.IsNullOrEmpty(p.Source) &&
                !p.Builtin &&
                !importedIds.Contains(p.Id));
            foreach (Profile p in imported)
            {
                if (p.Builtin)
                {
                    continue;
                }
                Profile ex = state.Profile(p.Id);
                if (ex != null)
                {
                    if (!ex.Builtin)
                    {
                        ex.Args = p.Args;
                        ex.UpdatedAt = p.UpdatedAt;
                    }
                }
                else
                {
                    state.Profiles.Add(p);
                }
            }
            EnsurePresets(state);
            state.Save();
            return state.Profiles;
        }

        /// <summary>
        /// Удаляет профиль; авторские (из каталога) не трогает. Если удалён автозапуск —
        /// сбрасывает ссылку (lib.rs:782-799). Возвращает false для авторского профиля.
        /// </summary>
        public static bool Delete(State state, string id)
        {
            Profile p = state.Profile(id);
            if (p != null && IsAuthorProfile(p))
            {
                return false;
            }
            state.Profiles.RemoveAll(x => x.Id == id);
            if (state.Settings.AutostartProfile == id)
            {
                state.Settings.AutostartMode = "none";
                state.Settings.AutostartProfile = null;
            }
            state.Save();
            return true;
        }

        /// <summary>
        /// Миграция после удаления движка zapret2/winws2 (lib.rs:638-683): профили и
        /// записи обновлений вырезанного движка убираются, автостарт сбрасывается.
        /// </summary>
        public static void MigrateRemovedEngine(State state)
        {
            int before = state.Profiles.Count;
            state.Profiles.RemoveAll(p => p.Engine != Engines.Flowseal);
            if (state.Profiles.Count != before && !string.IsNullOrEmpty(state.Settings.AutostartProfile))
            {
                if (state.Profile(state.Settings.AutostartProfile) == null)
                {
                    state.Settings.AutostartMode = "none";
                    state.Settings.AutostartProfile = null;
                }
            }
            // Чистим данные вырезанного движка: движки, конфиги и служебные каталоги.
            foreach (string dir in new[]
            {
                Path.Combine(state.Data, "engines\\zapret2"),
                Path.Combine(state.Data, "catalog\\zapret2"),
                Path.Combine(state.Data, "catalog\\sources\\zapret2"),
            })
            {
                try { Directory.Delete(dir, true); } catch { }
            }
            // Старый кэш обновлений: групп zapret2 больше нет, а записи продолжали
            // показываться на экране «Обновления».
            if (state.Updater != null && state.Updater.Entries != null)
            {
                state.Updater.Entries.RemoveAll(e =>
                    !string.IsNullOrEmpty(e.Group) && e.Group.StartsWith("zapret2", StringComparison.Ordinal));
            }
            // applied.json тоже чистим от записей вырезанного движка (lib.rs:677-682).
            UpdArchive archive = UpdArchive.Load(state.Data);
            if (archive.PurgePrefix("zapret2") > 0)
            {
                archive.Save(state.Data);
            }
        }
    }
}
