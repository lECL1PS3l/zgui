using System;
using System.Collections.Generic;
using System.IO;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Config
{
    /// <summary>
    /// Литерал состояния программы — порт AppState из config.rs:139-267.
    /// Хранится в data/state.json, запись атомарная (tmp + переименование),
    /// чтение толерантно к битому файлу: копия сохраняется как state.json.bad-{epoch}.
    /// </summary>
    public class State
    {
        // data в файл не пишется (config.rs:142 #[serde(skip)]): член без [JsonField].
        public string Data;

        [JsonField("roots")]
        public Roots Roots = new Roots();

        [JsonField("settings")]
        public Settings Settings = Settings.Default();

        [JsonField("profiles")]
        public List<Profile> Profiles = new List<Profile>();

        [JsonField("runtime")]
        public Runtime Runtime;

        [JsonField("updater")]
        public UpdaterCache Updater = new UpdaterCache();

        [JsonField("serviceCheckedAt")]
        public ulong ServiceCheckedAt;

        [JsonField("serviceRunning")]
        public bool? ServiceRunning;

        [JsonField("serviceStrategy")]
        public string ServiceStrategy;

        [JsonField("bootPending")]
        public bool BootPending;

        /// <summary>Обнаружен winws нашего движка, запущенный вне программы.</summary>
        [JsonField("externalWinws")]
        public bool ExternalWinws;

        [JsonField("engineVersion")]
        public string EngineVersion;

        public static State Load(string dataDir)
        {
            string path = Path.Combine(dataDir, "state.json");
            string raw = null;
            try
            {
                if (File.Exists(path))
                {
                    raw = File.ReadAllText(path);
                }
            }
            catch
            {
                raw = null;
            }

            State state;
            if (!string.IsNullOrWhiteSpace(raw) && Json.TryParse<State>(raw, out state))
            {
                // Битой копии не было — state валиден.
            }
            else
            {
                if (!string.IsNullOrEmpty(raw) && !string.IsNullOrWhiteSpace(raw))
                {
                    // Битый state.json не должен молча превращаться в «программу без
                    // настроек»: копия рядом, чтобы показать разработчику (config.rs:178).
                    string bad = Path.Combine(dataDir, "state.json.bad-" +
                        DateTimeOffset.UtcNow.ToUnixTimeSeconds());
                    try { File.WriteAllText(bad, raw); } catch { }
                }
                state = new State();
            }
            state.Data = dataDir;

            // Одноразовая миграция: старый дефолт автопроверки 6 ч → новый 72 ч
            // (config.rs:207-215). Пользовательские значения, отличные от 6, не трогаются.
            if (!state.Settings.IntervalMigrated)
            {
                if (state.Settings.UpdateIntervalHours == 6)
                {
                    state.Settings.UpdateIntervalHours = 72;
                }
                state.Settings.IntervalMigrated = true;
                state.Save();
            }
            state.EnsureDirs();
            return state;
        }

        public void Save()
        {
            Text.AtomicWrite(Path.Combine(Data, "state.json"),
                System.Text.Encoding.UTF8.GetBytes(Json.Serialize(this)));
        }

        /// <summary>Все папки data, которые нужны программе (config.rs:230-246).</summary>
        public void EnsureDirs()
        {
            string[] dirs =
            {
                Data,
                Path.Combine(Data, "catalog"),
                Path.Combine(Data, "catalog\\flowseal\\raw"),
                Path.Combine(Data, "catalog\\flowseal\\lists"),
                Path.Combine(Data, "catalog\\flowseal\\.service"),
                Path.Combine(Data, "catalog\\sources\\flowseal"),
                Path.Combine(Data, "catalog\\geoblock"),
                Path.Combine(Data, "logs"),
                Path.Combine(Data, "tmp"),
                Path.Combine(Data, "engines"),
                Path.Combine(Data, "webview"),
            };
            foreach (string d in dirs)
            {
                try { Directory.CreateDirectory(d); } catch { }
            }
        }

        public Profile Profile(string id)
        {
            return Profiles.Find(p => p.Id == id);
        }

        public string CatalogDir(string engine)
        {
            return Path.Combine(Data, "catalog", engine.ToLowerInvariant());
        }

        public string RawStrategiesDir()
        {
            return Path.Combine(Data, "catalog\\flowseal\\raw");
        }

        public string LogsDir()
        {
            return Path.Combine(Data, "logs");
        }

        public string RootFor(string engine)
        {
            return Roots.Path(engine);
        }
    }
}
