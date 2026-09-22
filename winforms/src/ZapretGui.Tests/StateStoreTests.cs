using System.IO;
using Xunit;
using ZapretGui.Core.Config;
using ZapretGui.Core.Util;

namespace ZapretGui.Tests
{
    public class StateStoreTests
    {
        /// <summary>Миграция 6 → 72 часа прописывается обратно в файл (config.rs:209-214).</summary>
        [Fact]
        public void MigratesOldIntervalAndPersistsFlag()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-tests-" + Path.GetRandomFileName());
            Directory.CreateDirectory(dir);
            File.WriteAllText(Path.Combine(dir, "state.json"),
                "{\"settings\":{\"updateIntervalHours\":6,\"gameFilter\":\"off\",\"ipsetMode\":\"loaded\"," +
                "\"autostartMode\":\"none\",\"tgPort\":1443,\"theme\":\"grey\"}}");

            State state = State.Load(dir);

            Assert.Equal(72, state.Settings.UpdateIntervalHours);
            Assert.True(state.Settings.IntervalMigrated);
            string after = File.ReadAllText(Path.Combine(dir, "state.json"));
            Assert.Contains("\"updateIntervalHours\":72", after);
            Assert.Contains("\"intervalMigrated\":true", after);

            // Повторная загрузка не должна трогать файл.
            State again = State.Load(dir);
            Assert.Equal(72, again.Settings.UpdateIntervalHours);
        }

        /// <summary>Битой state.json не должен молчаливо обнулять настройки (config.rs:178).</summary>
        [Fact]
        public void CorruptStateIsBackedUp()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-tests-" + Path.GetRandomFileName());
            Directory.CreateDirectory(dir);
            File.WriteAllText(Path.Combine(dir, "state.json"), "{это не json");

            State state = State.Load(dir);

            Assert.Equal(72, state.Settings.UpdateIntervalHours);
            Assert.Equal("grey", state.Settings.Theme);
            string[] bad = Directory.GetFiles(dir, "state.json.bad-*");
            Assert.Single(bad);
        }

        [Fact]
        public void RoundTripKeepsState()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-tests-" + Path.GetRandomFileName());
            Directory.CreateDirectory(dir);

            State state = new State { Data = dir };
            state.Settings.Theme = "dark";
            state.Settings.TgPort = 7777;
            state.Profiles.Add(new Profile { Id = "my", Name = "Мой", Engine = "flowseal" });
            state.Save();

            State loaded = State.Load(dir);
            Assert.Equal("dark", loaded.Settings.Theme);
            Assert.Equal((ushort)7777, loaded.Settings.TgPort);
            Assert.Single(loaded.Profiles);
            Assert.Equal("Мой", loaded.Profiles[0].Name);
        }
    }
}
