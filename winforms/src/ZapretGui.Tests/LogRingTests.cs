using System;
using System.IO;
using Xunit;
using ZapretGui.Core.Log;

namespace ZapretGui.Tests
{
    [Collection("LogRing")]
    public class LogRingTests : IDisposable
    {
        private readonly string _dir;

        public LogRingTests()
        {
            _dir = Path.Combine(Path.GetTempPath(), "zgui_log_" + Guid.NewGuid().ToString("N"));
            LogRing.Init(_dir, null);
            LogRing.Clear();
        }

        public void Dispose()
        {
            LogRing.Clear();
            try { Directory.Delete(_dir, true); } catch { }
        }

        [Fact]
        public void Stamp_IsUtcCorrect()
        {
            Assert.Equal("1970-01-01 00:00:00.000", LogRing.Stamp(0));
            Assert.Equal("2026-09-19 12:00:00.000", LogRing.Stamp(1789819200000));
        }

        [Fact]
        public void Levels_AreNormalized()
        {
            LogRing.Write("error", "t", "one");
            LogRing.Write("bogus", "t", "two");
            LogRing.Write("warn", "t", "three");
            var e = LogRing.Entries(0);
            Assert.Equal(3, e.Count);
            Assert.Equal("err", e[0].Level);
            Assert.Equal("info", e[1].Level);
            Assert.Equal("warn", e[2].Level);
        }

        [Fact]
        public void Entries_AreOrderedAndFiltered()
        {
            LogRing.Write("info", "a", "1");
            LogRing.Write("info", "b", "2");
            LogRing.Write("info", "c", "3");
            var all = LogRing.Entries(0);
            Assert.Equal(3, all.Count);
            Assert.Equal(new[] { "a", "b", "c" }, all.ConvertAll(x => x.Scope));
            // Фильтр «после seq»: пропускаем первую запись.
            var tail = LogRing.Entries(all[0].Seq);
            Assert.Equal(2, tail.Count);
            Assert.Equal(new[] { "b", "c" }, tail.ConvertAll(x => x.Scope));
        }

        [Fact]
        public void File_HasAllLines()
        {
            LogRing.Write("info", "app", "запуск");
            LogRing.Write("err", "net", "провал");
            var file = Path.Combine(_dir, "logs", "zgui.log");
            Assert.True(File.Exists(file));
            var text = File.ReadAllText(file);
            Assert.Contains("[info] app: запуск", text);
            Assert.Contains("[err] net: провал", text);
        }

        [Fact]
        public void Clear_WipesBufferAndFile()
        {
            LogRing.Write("info", "t", "x");
            Assert.Single(LogRing.Entries(0));
            LogRing.Clear();
            Assert.Empty(LogRing.Entries(0));
            Assert.False(File.Exists(Path.Combine(_dir, "logs", "zgui.log")));
        }

        [Fact]
        public void LongMessage_IsTruncated()
        {
            var msg = new string('а', 5000);
            LogRing.Write("info", "t", msg);
            var e = Assert.Single(LogRing.Entries(0));
            Assert.EndsWith("… [обрезано 1000 символов]", e.Msg);
            Assert.True(e.Msg.Length < 5000);
        }

        [Fact]
        public void Dump_JoinsEntries()
        {
            LogRing.Write("info", "a", "1");
            LogRing.Write("info", "b", "2");
            var dump = LogRing.Dump();
            Assert.Contains("[info] a: 1", dump);
            Assert.Contains("[info] b: 2", dump);
            Assert.Equal(2, dump.Split('\n').Length);
        }
    }
}
