using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using Xunit;
using ZapretGui.Core;
using ZapretGui.Core.Tester;

namespace ZapretGui.Tests
{
    public class TesterTests
    {
        private static string IsolatedDir()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-tester-" + System.Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(dir);
            return dir;
        }

        private static StrategyResult Mk(string id, string name, int score)
        {
            return new StrategyResult
            {
                Id = id,
                Name = name,
                Engine = "flowseal",
                Group = "flowseal bat",
                Started = true,
                Score = score,
                MaxScore = 5,
                CriticalOk = true,
            };
        }

        private static StrategyResult MkTie(string id, bool critGroupOk, long ms)
        {
            return new StrategyResult
            {
                Id = id,
                Name = id,
                Engine = "flowseal",
                Group = "flowseal bat",
                Started = true,
                Score = 5,
                MaxScore = 5,
                CriticalOk = true,
                Domains = new List<DomainResult>
                {
                    new DomainResult { Key = "d", Host = "d.example", Group = "youtube", GroupLabel = "YouTube", Ok = true, Ms = ms, Detail = "http 200" }
                },
                Groups = new List<GroupResult>
                {
                    new GroupResult { Id = "youtube", Label = "YouTube", Passed = 1, Total = 1, Ok = critGroupOk, Critical = true, Priority = 1 }
                },
            };
        }

        [Fact]
        public void SummarizePicksBest()
        {
            List<StrategyResult> sorted;
            string best;
            Tester.Summarize(new List<StrategyResult> { Mk("a", "A", 2), Mk("b", "B", 5), Mk("c", "C", 5) }, out sorted, out best);
            Assert.Equal("b", best);
            Assert.Equal(5, sorted[0].Score);
        }

        [Fact]
        public void SummarizeBreaksTiesByCriticalGroupsThenLatency()
        {
            List<StrategyResult> sorted;
            string best;
            Tester.Summarize(new List<StrategyResult>
            {
                MkTie("slow", true, 900),
                MkTie("fast", true, 100),
                MkTie("no-critical", false, 10),
            }, out sorted, out best);
            Assert.Equal("fast", best);
            Assert.Equal("fast", sorted[0].Id);
            Assert.Equal("no-critical", sorted[2].Id);
        }

        [Fact]
        public void UsableHostRejectsJunk()
        {
            Assert.True(Tester.UsableTestHost("www.youtube.com"));
            Assert.True(Tester.UsableTestHost("xn--e1afmkfd.xn--p1ai"));
            Assert.False(Tester.UsableTestHost(".ua"));
            Assert.False(Tester.UsableTestHost("ua."));
            Assert.False(Tester.UsableTestHost("192.168.1.1"));
            Assert.False(Tester.UsableTestHost("*.example.com"));
            Assert.False(Tester.UsableTestHost("exa mple.com"));
        }

        [Fact]
        public void LoadDomainsParses()
        {
            string dir = IsolatedDir();
            try
            {
                string sub = Path.Combine(dir, "catalog", "geoblock");
                Directory.CreateDirectory(sub);
                File.WriteAllText(Path.Combine(sub, "allow-domains-youtube.lst"),
                    "# comment\nwww.youtube.com/abc\n.googlevideo.com\n.ua\n123.45.67.89\n*.wild.bad\nbad.domain.\ngooglevideo.com\n",
                    new UTF8Encoding(false));
                List<KeyValuePair<string, string>> d = Tester.LoadDomainsFromLists(dir, 10);
                bool AnyHost(string host) { return d.Exists(x => x.Value == host); }
                Assert.True(AnyHost("www.youtube.com"));
                Assert.True(AnyHost("googlevideo.com"));
                Assert.False(d.Exists(x => x.Value.StartsWith(".")));
                Assert.False(d.Exists(x => x.Value.Contains("*")));
                Assert.False(AnyHost("123.45.67.89"));
            }
            finally
            {
                TryCleanup(dir);
            }
        }

        [Fact]
        public void BuiltinDomainsAreAvailableWithoutUpdates()
        {
            string dir = IsolatedDir();
            try
            {
                List<KeyValuePair<string, string>> domains = Tester.LoadDomainsFromLists(dir, 500);
                bool AnyHost(string host) { return domains.Exists(x => x.Value == host); }
                Assert.True(AnyHost("music.youtube.com"));
                Assert.True(AnyHost("discord.com"));
                Assert.True(AnyHost("hdrezka.fm"));
                Assert.True(domains.Count >= 100);
                Assert.False(AnyHost("10minutemail.com"));
                Assert.False(domains.Exists(x => x.Value.EndsWith(".ua")));
            }
            finally
            {
                TryCleanup(dir);
            }
        }

        [Fact]
        public void CacheRoundtrip()
        {
            string dir = IsolatedDir();
            try
            {
                TestCache c = new TestCache { BestId = "general" };
                c.Save(dir);
                TestCache back = TestCache.Load(dir);
                Assert.Equal("general", back.BestId);
            }
            finally
            {
                TryCleanup(dir);
            }
        }

        [Fact]
        public void CriticalGroupsRequireYoutubeMusic()
        {
            Assert.True(Tester.CriticalGroupOk(Tester.GroupYoutube, 3, 5));
            Assert.False(Tester.CriticalGroupOk(Tester.GroupYoutubeMusic, 0, 1));
            Assert.True(Tester.CriticalGroupOk(Tester.GroupYoutubeMusic, 1, 1));
            Assert.False(Tester.CriticalGroupOk(Tester.GroupDiscord, 1, 3));
        }

        [Fact]
        public void ClassifiesPriorityGroupsAndExcludesGoogleAi()
        {
            Assert.Equal("youtube-music", Tester.ClassifyDomain("music.youtube.com").Id);
            Assert.Equal("discord", Tester.ClassifyDomain("discordapp.com").Id);
            Assert.Equal("microsoft-xbox", Tester.ClassifyDomain("xboxservices.com").Id);
            Assert.Equal("google", Tester.ClassifyDomain("www.google.com").Id);
            Assert.Equal("other", Tester.ClassifyDomain("gemini.google.com").Id);
            Assert.Equal("cloudflare", Tester.ClassifyDomain("www.cloudflare.com").Id);
        }

        [Fact]
        public void WritePlanClassesDomains()
        {
            string dir = IsolatedDir();
            try
            {
                var steps = new List<TestStep>
                {
                    new TestStep { Id = "t", Name = "test", Engine = "flowseal", Group = "g", Exe = "cmd.exe", Workdir = "C:\\Windows", Args = new List<string> { "/c", "exit" } }
                };
                var domains = new List<KeyValuePair<string, string>> { new KeyValuePair<string, string>("m", "music.youtube.com") };
                Tester.RunnerFiles f = Tester.WriteTestRunner(dir, steps, domains, false);
                Assert.True(File.Exists(f.Plan));
                Assert.True(File.Exists(f.Script));
                string plan = File.ReadAllText(f.Plan, Encoding.UTF8);
                Assert.Contains("\"group\":\"youtube-music\"", plan);
                Assert.Contains("\"critical\":true", plan);
            }
            finally
            {
                TryCleanup(dir);
            }
        }

        [Fact]
        public void ReadTestProgressStripsBomAndMarker()
        {
            string dir = IsolatedDir();
            try
            {
                string outPath = Path.Combine(dir, "test-out.json");
                File.WriteAllText(outPath, "\uFEFF{\"index\": 1, \"total\": 2}\nDONE\n", new UTF8Encoding(false));
                Dictionary<string, object> v = Tester.ReadTestProgress(outPath);
                Assert.NotNull(v);
                Assert.Equal(2L, Tester.AsLong(v, "total"));
            }
            finally
            {
                TryCleanup(dir);
            }
        }

        [Fact]
        public void RunnerScriptIsAsciiAndRuns()
        {
            string dir = IsolatedDir();
            try
            {
                var steps = new List<TestStep>
                {
                    new TestStep
                    {
                        Id = "t",
                        Name = "Тест",
                        Engine = "flowseal",
                        Group = "g",
                        Exe = "C:\\Windows\\System32\\cmd.exe",
                        Workdir = "C:\\Windows",
                        // Длинная команда с пробелами остаётся активной дольше проверки
                        // процесса и подтверждает корректное quoting в $argLine.
                        Args = new List<string> { "/c", "ping -n 6 127.0.0.1 >nul" },
                    }
                };
                var domains = new List<KeyValuePair<string, string>> { new KeyValuePair<string, string>("y", "127.0.0.1") };
                Tester.RunnerFiles f = Tester.WriteTestRunner(dir, steps, domains, false);

                byte[] raw = File.ReadAllBytes(f.Script);
                byte[] body = raw.Length >= 3 && raw[0] == 0xEF && raw[1] == 0xBB && raw[2] == 0xBF
                    ? Skip(raw, 3) : raw;
                foreach (byte b in body) { Assert.True(b < 0x80, "runner должен быть ASCII-only"); }
                string scriptText = Encoding.ASCII.GetString(body);
                Assert.Contains("$argLine", scriptText);
                Assert.Contains("if ($a -match '[\\s\"]')", scriptText);
                Assert.Contains("https://$target/", scriptText);
                Assert.Contains("Add-Type -AssemblyName System.Net.Http", scriptText);
                Assert.DoesNotContain("TcpClient", scriptText);

                Process p = Process.Start(new ProcessStartInfo("powershell.exe",
                    "-NoProfile -ExecutionPolicy Bypass -File \"" + f.Script + "\"")
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                });
                Assert.True(p.WaitForExit(60000));
                Assert.Equal(0, p.ExitCode);

                Dictionary<string, object> v = Tester.ReadTestProgress(f.Out);
                Assert.NotNull(v);
                Assert.Equal(1L, Tester.AsLong(v, "total"));
                List<StrategyResult> results = Tester.ParseResults(v);
                Assert.Single(results);
                StrategyResult r = results[0];
                Assert.True(r.Started);
                Assert.Null(r.Error);
                Assert.Equal(0, r.Score);
                Assert.Equal(1, r.MaxScore);
            }
            finally
            {
                TryCleanup(dir);
            }
        }

        private static byte[] Skip(byte[] raw, int offset)
        {
            byte[] body = new byte[raw.Length - offset];
            for (int i = 0; i < body.Length; i++) { body[i] = raw[offset + i]; }
            return body;
        }

        private static void TryCleanup(string dir)
        {
            try { Directory.Delete(dir, true); } catch { }
        }
    }
}
