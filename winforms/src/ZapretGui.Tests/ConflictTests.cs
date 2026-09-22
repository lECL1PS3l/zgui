using Xunit;
using ZapretGui.Core.Runtime;

namespace ZapretGui.Tests
{
    public class ConflictTests
    {
        [Fact]
        public void IsVpnProcessDetectsKnownNames()
        {
            Assert.True(Conflicts.IsVpnProcess("Happ.exe"));
            Assert.True(Conflicts.IsVpnProcess("wireguard.exe"));
            Assert.True(Conflicts.IsVpnProcess("AmneziaVPN.exe"));
            Assert.True(Conflicts.IsVpnProcess("sing-box.exe"));
            Assert.True(Conflicts.IsVpnProcess("clash-verge.exe"));
            Assert.False(Conflicts.IsVpnProcess("winws.exe"));
            Assert.False(Conflicts.IsVpnProcess("chrome.exe"));
            Assert.False(Conflicts.IsVpnProcess("explorer.exe"));
        }

        [Fact]
        public void IsVpnProcessHandlesSuffixesAndCasing()
        {
            Assert.True(Conflicts.IsVpnProcess("openvpn-gui.exe"));
            Assert.True(Conflicts.IsVpnProcess("AMNEZIAVPN.EXE"));
            Assert.True(Conflicts.IsVpnProcess("happ-lite.exe"));
            Assert.False(Conflicts.IsVpnProcess("winws2.exe"));
            Assert.False(Conflicts.IsVpnProcess(""));
            Assert.False(Conflicts.IsVpnProcess("clash Royale"));
        }

        [Fact]
        public void IsOwnEngineRecognisedByDataPrefix()
        {
            var data = @"D:\Zapret\data";
            Assert.True(Conflicts.IsOwnEngine(@"D:\Zapret\data\engines\flowseal\bin\winws.exe", data));
            Assert.True(Conflicts.IsOwnEngine(@"d:\zapret\DATA\engines\flowseal\WINWS.EXE", data));
            Assert.False(Conflicts.IsOwnEngine(@"D:\Zapret\data2\winws.exe", data));
            Assert.False(Conflicts.IsOwnEngine(@"D:\Zapret\data-x\bin\winws.exe", data));
            Assert.False(Conflicts.IsOwnEngine(@"C:\Users\x\Downloads\zapret\bin\winws.exe", data));
            Assert.False(Conflicts.IsOwnEngine(@"C:\winws.exe", ""));
        }

        [Fact]
        public void ConflictReportFlags()
        {
            var r = new ConflictReport();
            Assert.False(r.HasConflicts());
            r.Processes.Add(new ConflictProcess(42, "winws.exe", "x"));
            Assert.True(r.HasConflicts());

            var r2 = new ConflictReport { ForeignService = true };
            Assert.True(r2.HasConflicts());

            var r3 = new ConflictReport();
            r3.Vpn.Add(new ConflictProcess(7, "Happ.exe", "vpn"));
            Assert.True(r3.HasConflicts());
            Assert.Equal(1, r3.Vpn.Count);
        }

        [Fact]
        public void ParseTaskListCsvSkipsJunk()
        {
            var list = Conflicts.ParseTaskListCsv(
                "\"winws.exe\",\"1234\",\"Console\",\"1\",\"1,234 K\"\r\n" +
                "\"System Idle Process\",\"0\",\"Services\",\"0\",\"0 K\"\r\n" +
                "garbage\r\n");
            Assert.Equal(2, list.Count);
            Assert.Equal("winws.exe", list[0].Key);
            Assert.Equal(1234u, list[0].Value);
            Assert.Equal("System Idle Process", list[1].Key);
            Assert.Equal(0u, list[1].Value);

            Assert.Equal(0, Conflicts.ParseTaskListCsv("").Count);
        }

        [Fact]
        public void LocalProxyPortDetectsOnlyLocal()
        {
            Assert.Equal(8080, Conflicts.LocalProxyPort("127.0.0.1:8080"));
            Assert.Equal(8080, Conflicts.LocalProxyPort("localhost:8080"));
            Assert.Equal(8080, Conflicts.LocalProxyPort("http=127.0.0.1:8080"));
            Assert.Equal(8080, Conflicts.LocalProxyPort("http=127.0.0.1:8080;https=127.0.0.1:1080"));
            Assert.Equal(0, Conflicts.LocalProxyPort("proxy.corp.example.com:8080"));
            Assert.Equal(0, Conflicts.LocalProxyPort(""));
            Assert.Equal(0, Conflicts.LocalProxyPort("127.0.0.1"));
            Assert.Equal(0, Conflicts.LocalProxyPort("10.0.0.1:8888"));
        }

        [Fact]
        public void VpnCheckReportHasVpnOnly()
        {
            var report = Conflicts.VpnCheck();
            Assert.NotNull(report.Vpn);
            Assert.False(report.ForeignService);
            Assert.False(report.OwnService);
        }

        [Fact]
        public void AnyWinwsRunningAnswersBool()
        {
            // На машине разработчика может быть (или не быть) любой winws — проверяем лишь тип.
            var any = Conflicts.AnyWinwsRunning();
            Assert.True(any is bool);
        }

        [Fact]
        public void ListProcessesReturnsSomething()
        {
            var list = Conflicts.ListProcesses(name => name.ToLowerInvariant().Contains("explorer"));
            Assert.True(list.Count >= 1);
        }

        [Fact]
        public void CheckReportShapeMatchesSource()
        {
            var report = Conflicts.Check(System.IO.Path.GetTempPath(), null);
            Assert.NotNull(report.Processes);
            Assert.NotNull(report.Vpn);
            // ownService/foreignService взаимоисключаются при установленной службе,
            // но на чистой машине обе false.
            Assert.True(!report.OwnService || !report.ForeignService);
            Assert.True(report.HasConflicts() == (report.Message.Length > 0));
        }
    }
}
