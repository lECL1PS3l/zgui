using System.Collections.Generic;
using System.IO;
using System.Text;
using Xunit;
using ZapretGui.Core.Config;

namespace ZapretGui.Tests
{
    public class BatImportTests
    {
        private const string Bat =
            "@echo off\r\n" +
            "rem это комментарий — до winws.exe\r\n" +
            "\"%~dp0bin\\winws.exe\" --wf-tcp-out=80,443 ^\r\n" +
            " --filter-tcp=80 --dpi-desync=fake,disorder2 \"--dpi-desync-cut=%GameFilterTCP%\" --new\r\n";

        [Fact]
        public void ParsesQuotedContinuedBatWithCyrillic()
        {
            List<string> args = BatImport.ParseFlowsealBat(Bat, "C:\\zapret");

            Assert.True(args.Count >= 3, "ожидалось ≥3 аргументов");
            Assert.Contains(args, a => a == "--filter-tcp=80");
            Assert.Contains(args, a => a == "--dpi-desync=fake,disorder2");
            // Кавычки сняты, плейсхолдер игрового фильтра оставлен как есть.
            Assert.Contains(args, a => a == "--dpi-desync-cut=%GameFilterTCP%");
            // Комментарий и @echo off — до winws.exe — отброшены.
            Assert.DoesNotContain(args, a => a.Contains("комментарий") || a.Contains("echo"));
            // %~dp0 раскрыт в корень движка.
            Assert.DoesNotContain(args, a => a.Contains("%~dp0"));
        }

        [Fact]
        public void NoWinwsReturnsEmpty()
        {
            Assert.Empty(BatImport.ParseFlowsealBat("echo hello\n--filter-tcp=80", "C:\\zapret"));
        }

        [Fact]
        public void DecodesUtf16AndBom()
        {
            string text = "set \"BIN=%~dp0bin\"\r\nwinws.exe --new --filter-tcp=80";
            var bytes = new List<byte> { 0xFF, 0xFE };
            foreach (char c in text)
            {
                bytes.Add((byte)(c & 0xFF));
                bytes.Add((byte)((c >> 8) & 0xFF));
            }
            string decoded = BatImport.DecodeStrategyBytes(bytes.ToArray());
            Assert.Contains("--filter-tcp=80", decoded);
            Assert.Contains("%~dp0", decoded);
            Assert.Contains("tok", BatImport.DecodeStrategyBytes(new byte[] { 0xEF, 0xBB, 0xBF, 34, 116, 111, 107, 34 }));
            Assert.Contains("plain", BatImport.DecodeStrategyBytes(Encoding.UTF8.GetBytes("plain ascii")));
        }

        [Fact]
        public void ImportSkipsEmptyAndNames()
        {
            var bats = new (string Name, string Content)[]
            {
                ("general.bat", Bat),
                ("broken.bat", "echo nothing useful here"),
            };
            List<Profile> imported = BatImport.ImportBatProfiles("C:\\zapret", bats, new List<Profile>());

            Assert.Single(imported);
            Assert.Equal("general", imported[0].Id);
            Assert.Equal("General", imported[0].Name);
            Assert.Equal("flowseal", imported[0].Engine);
            Assert.Equal("general.bat", imported[0].Source);
        }

        [Fact]
        public void SaveValidatesAndPersistsRoundTrip()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-tests-" + Path.GetRandomFileName());
            Directory.CreateDirectory(dir);
            State state = State.Load(dir);

            string err = BatImport.Save(state, null, "  Моя стратегия  ", "flowseal",
                new List<string> { "--new", "--filter-tcp=80,443", "--dpi-desync=\"fake\"" });
            Assert.Null(err);
            Assert.Single(state.Profiles);
            Assert.Equal("Моя стратегия", state.Profiles[0].Name);
            Assert.StartsWith("custom-", state.Profiles[0].Id);

            State again = State.Load(dir);
            Assert.Single(again.Profiles);
            Assert.Equal("Моя стратегия", again.Profiles[0].Name);
            Assert.Equal("--dpi-desync=\"fake\"", again.Profiles[0].Args[2]);

            Assert.Equal("укажите название стратегии",
                BatImport.Save(state, null, "   ", "flowseal", new List<string> { "--new" }));
            Assert.Equal("список аргументов пуст — нечего сохранять",
                BatImport.Save(state, null, "Ещё", "flowseal", new List<string>()));
            Assert.Equal("стратегия с названием «Моя Стратегия» уже есть — выберите другое имя",
                BatImport.Save(state, null, "Моя Стратегия", "flowseal", new List<string> { "--new" }));
        }

        [Fact]
        public void PrettyNameReplacesTags()
        {
            Assert.Equal("General", BatImport.PrettyName("general"));
            Assert.Equal("discord · ALT", BatImport.PrettyName("discord (ALT).bat"));
            Assert.Equal("Новая UTC", BatImport.PrettyName("Новая (UTC)"));
        }
    }
}
