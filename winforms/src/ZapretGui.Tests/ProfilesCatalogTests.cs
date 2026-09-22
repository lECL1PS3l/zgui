using System.Collections.Generic;
using System.IO;
using System.Text;
using Xunit;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;

namespace ZapretGui.Tests
{
    public class ProfilesCatalogTests
    {
        private static string TempDir()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-tests-" + Path.GetRandomFileName());
            Directory.CreateDirectory(dir);
            return dir;
        }

        private static void WriteBat(string dir, string name, string content)
        {
            Directory.CreateDirectory(dir);
            File.WriteAllBytes(Path.Combine(dir, name), Encoding.UTF8.GetBytes(content));
        }

        [Fact]
        public void RefreshCatalogImportsRawBatsInOrder()
        {
            string dir = TempDir();
            State state = State.Load(dir);
            Assert.Empty(state.Profiles);

            string engineRoot = Path.Combine(dir, "engine");
            Directory.CreateDirectory(engineRoot);
            state.Roots.Set("flowseal", engineRoot);

            WriteBat(state.RawStrategiesDir(), "general.bat",
                "winws.exe --new --filter-tcp=80,443\r\n");
            WriteBat(state.RawStrategiesDir(), "discord (ALT).bat",
                "winws.exe --new --filter-tcp=443 --dpi-desync=fake\r\n");

            List<Profile> profiles = Profiles.RefreshCatalog(state);

            Assert.Equal(2, profiles.Count);
            // Порядок каталога сохраняется, не алфавитный (read_dir/EnumerateFiles).
            Assert.Contains(profiles, p => p.Id == "general");
            Assert.Contains(profiles, p => p.Id == "discord (ALT)");
            Assert.Equal("General", profiles.Find(p => p.Id == "general").Name);
            Assert.Equal("discord · ALT", profiles.Find(p => p.Id == "discord (ALT)").Name);
            Assert.Equal("flowseal", profiles[0].Engine);
            Assert.Equal(Profiles.GroupBat, Profiles.GroupOf(profiles[0]));

            // Повторный перечёт не плодит дубли.
            Assert.Equal(2, Profiles.RefreshCatalog(state).Count);
        }

        [Fact]
        public void RefreshKeepsCustomProfilesButDropsMissingImported()
        {
            string dir = TempDir();
            State state = State.Load(dir);
            string engineRoot = Path.Combine(dir, "engine");
            Directory.CreateDirectory(engineRoot);
            state.Roots.Set("flowseal", engineRoot);
            state.Profiles.Add(new Profile { Id = "gone", Name = "Удалённый", Engine = "flowseal", Source = "gone.bat", Args = new List<string> { "--new" } });
            state.Profiles.Add(new Profile { Id = "mine", Name = "Мой", Engine = "flowseal", Args = new List<string> { "--new" } });
            state.Save();

            WriteBat(state.RawStrategiesDir(), "general.bat", "winws.exe --new --filter-tcp=80\r\n");

            List<Profile> profiles = Profiles.RefreshCatalog(state);

            Assert.Equal(2, profiles.Count);
            Assert.Contains(profiles, p => p.Id == "mine");   // ручной остался
            Assert.Contains(profiles, p => p.Id == "general");
        }

        [Fact]
        public void DeleteProtectsAuthorProfiles()
        {
            string dir = TempDir();
            State state = State.Load(dir);
            state.Profiles.Add(new Profile { Id = "general", Name = "General", Engine = "flowseal", Source = "general.bat" });
            state.Profiles.Add(new Profile { Id = "mine", Name = "Мой", Engine = "flowseal" });
            state.Settings.AutostartProfile = "mine";
            state.Settings.AutostartMode = "service";
            state.Save();

            Assert.False(Profiles.Delete(state, "general"));
            Assert.True(Profiles.Delete(state, "mine"));
            Assert.Single(state.Profiles);
            Assert.Equal("none", state.Settings.AutostartMode);
            Assert.Null(state.Settings.AutostartProfile);
        }

        [Fact]
        public void MigrateRemovedEngineDropsZapret2()
        {
            string dir = TempDir();
            State state = State.Load(dir);
            state.Profiles.Add(new Profile { Id = "z2", Name = "winws2", Engine = "zapret2" });
            state.Updater.Entries.Add(new UpdEntry { Id = "u2", Group = "zapret2 files" });
            state.Save();

            Profiles.MigrateRemovedEngine(state);

            Assert.Empty(state.Profiles);
            Assert.Empty(state.Updater.Entries);
        }
    }

    public class HumanizeTests
    {
        [Fact]
        public void TranslatesCommonOsAndHttpErrors()
        {
            Assert.Contains("права администратора",
                Humanize.HumanError("failed to remove file: Отказано в доступе (os error 5)"));
            Assert.Contains("нет связи с сервером",
                Humanize.HumanError("error sending request for url (https://api.github.com/…): error trying to connect"));
            Assert.Contains("403", Humanize.HumanError("HTTP status client error (403 Forbidden)"));
            Assert.Contains("недопустимое значение поля",
                Humanize.HumanError("invalid args `port` for command `tg_start`: invalid type: integer 70000, expected u16"));
            Assert.Contains("права администратора",
                Humanize.HumanError("ADMIN_REQUIRED: winws needs administrator rights"));
            Assert.Contains("на диске не хватает места",
                Humanize.HumanError("os error 112: not enough space"));
            Assert.Contains("занят другой программой",
                Humanize.HumanError("os error 32: being used by another process"));
        }

        [Fact]
        public void KeepsFriendlyRussianMessagesUntouched()
        {
            string msg = "в папке не найден winws.exe — укажите корень движка";
            Assert.Equal(msg, Humanize.HumanError(msg));
        }

        [Fact]
        public void UnknownTechnicalTextIsMarked()
        {
            Assert.Contains("Журнале", Humanize.HumanError("some weird failure 0xDEADBEEF"));
            Assert.Equal("неизвестная ошибка (подробности в журнале)", Humanize.HumanError("   "));
        }

        [Fact]
        public void ContextPrefixIsAdded()
        {
            Assert.StartsWith("скачивание движка: ",
                Humanize.WithContext("скачивание движка", "os error 112: not enough space"));
        }
    }
}
