using System.Collections.Generic;
using System.IO;
using Xunit;
using ZapretGui.Core;
using ZapretGui.Core.Config;
using ZapretGui.Core.Util;

namespace ZapretGui.Tests
{
    /// <summary>
    /// Снимок Bootstrap и предупреждения (lib.rs:112-141, 244-274) — чистая
    /// часть: собирается из литерала состояния, без обращений к движку.
    /// </summary>
    public class BootstrapTests
    {
        private static State MakeState(string dir, bool externalWinws)
        {
            var s = new State { Data = dir };
            s.Roots.Flowseal = null;
            s.ExternalWinws = externalWinws;
            s.Settings.GameFilter = "off";
            s.Profiles.Add(new Profile { Id = "p1", Name = "Стратегия", Engine = "flowseal" });
            s.Runtime = new Runtime { ProfileId = "p1", Pid = 0, Via = "app" };
            s.ServiceRunning = null;
            s.ServiceStrategy = null;
            return s;
        }

        [Fact]
        public void WarningsAlwaysMentionVpn()
        {
            string dir = IsolatedDir();
            List<string> w = AppHost.CollectWarnings(MakeState(dir, false));
            Assert.Equal("Не рекомендуется запускать Zapret вместе с VPN", w[0]);
        }

        [Fact]
        public void WarningsAddExternalWinwsNote()
        {
            string dir = IsolatedDir();
            List<string> w = AppHost.CollectWarnings(MakeState(dir, true));
            Assert.Equal(2, w.Count);
            Assert.Contains("Обход запущен вне программы", w[1]);
        }

        [Fact]
        public void WarningsDetectAuthorBatsInRoot()
        {
            string dir = IsolatedDir();
            string root = Path.Combine(dir, "engine");
            Directory.CreateDirectory(root);
            File.WriteAllText(Path.Combine(root, "general.bat"), "@echo off");
            // service.bat автора не считается: это его служебный файл.
            File.WriteAllText(Path.Combine(root, "service_install.bat"), "@echo off");

            State s = MakeState(dir, false);
            s.Roots.Flowseal = root;
            List<string> w = AppHost.CollectWarnings(s);
            Assert.Equal(2, w.Count);
            Assert.Contains("найдены оригинальные .bat/.lua автора", w[1]);
        }

        [Fact]
        public void GameFilterPortsMatchOffMode()
        {
            string dir = IsolatedDir();
            State s = MakeState(dir, false);
            var bs = new Bootstrap { Settings = s.Settings };
            GameFilterPortsFilled(bs, s.Settings);
            Assert.Equal("12", bs.GameFilterPorts[0]);
            Assert.Equal("12", bs.GameFilterPorts[1]);
        }

        // Тот же вызов, что в AppHost.Snapshot: (tcp, udp) для режима фильтра.
        private static void GameFilterPortsFilled(Bootstrap bs, Settings settings)
        {
            string tcp;
            string udp;
            ZapretGui.Core.Net.GameFilter.Ports(settings.GameFilter, out tcp, out udp);
            bs.GameFilterPorts[0] = tcp;
            bs.GameFilterPorts[1] = udp;
        }

        [Fact]
        public void RootInfoWithoutPathIsNotReady()
        {
            var roots = new Roots();
            RootInfo ri = AppHost.RootInfoFor(roots, Engines.Flowseal, Engines.WinwsExe);
            Assert.False(ri.Ready);
            Assert.Null(ri.Path);
        }

        [Fact]
        public void EngineVersionParsedFromFolderName()
        {
            Assert.Equal("1.10.2", AppHost.EngineVersionFromRoot(Path.Combine("data", "zapret-discord-youtube-1.10.2")));
            Assert.Null(AppHost.EngineVersionFromRoot(Path.Combine("data", "flowseal")));
        }

        [Fact]
        public void BootstrapSerializesCamelCase()
        {
            var bs = new Bootstrap
            {
                Owner = "none",
                DataDir = "C:\\data",
                Warnings = new List<string> { "w" },
            };
            string json = Json.Serialize(bs);
            Assert.Contains("\"gameFilterPorts\"", json);
            Assert.Contains("\"dataDir\"", json);
            Assert.Contains("\"owner\"", json);
        }

        private static string IsolatedDir()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-boot-" + System.Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(dir);
            return dir;
        }
    }
}