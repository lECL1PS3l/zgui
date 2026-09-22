using System.IO;
using System.Threading.Tasks;
using Xunit;
using ZapretGui.Core;
using ZapretGui.Core.Embedded;
using ZapretGui.Core.Tele;

namespace ZapretGui.Tests
{
    public class TgBridgeTests
    {
        private static string IsolatedDir()
        {
            return Path.Combine(Path.GetTempPath(), "zgui-tg-" + System.Guid.NewGuid().ToString("N"));
        }

        [Fact]
        public void InitialStatusIsStopped()
        {
            TgBridge.Stop();
            TgStatus st = TgBridge.Status();
            Assert.False(st.Running);
            Assert.Null(st.Port);
            Assert.Null(st.Link);
        }

        [Fact]
        public void StopOnIdleIsNoop()
        {
            TgBridge.Stop();
            Assert.False(TgBridge.Status().Running);
        }

        [Fact]
        public async Task StartWithoutExeReportsMissing()
        {
            string dir = IsolatedDir();
            Directory.CreateDirectory(dir);
            Result<TgStatus> r = await TgBridge.StartAsync(dir, 1443);
            Assert.False(r.IsOk);
            Assert.Contains("tg-ws-proxy.exe", r.Error);
        }

        [Fact]
        public async Task EnsureTgBridgeExtractsExe()
        {
            string dir = IsolatedDir();
            try
            {
                Embedded.TgBridgeExe = new byte[] { 0x4d, 0x5a, 0x01 };
                Embedded.EnsureTgBridge(dir);
                Assert.True(File.Exists(TgBridge.ExePath(dir)));
                // повторный вызов не перезаписывает существующий
                Embedded.TgBridgeExe = new byte[] { 0x00 };
                Embedded.EnsureTgBridge(dir);
                Assert.Equal(3, new FileInfo(TgBridge.ExePath(dir)).Length);
            }
            finally
            {
                Embedded.TgBridgeExe = null;
                Directory.Delete(dir, true);
            }
        }

        [Fact]
        public void ParsesListenPort()
        {
            Assert.True(TgBridge.TryParsePort("0.0.0.0:27922", out ushort port));
            Assert.Equal(27922, port);
            Assert.False(TgBridge.TryParsePort("no-port", out port));
        }

        [Fact]
        public void StatsMissingIsNull()
        {
            string dir = IsolatedDir();
            try
            {
                Directory.CreateDirectory(dir);
                Assert.Null(TgBridge.Stats(dir));
            }
            finally
            {
                Directory.Delete(dir, true);
            }
        }
    }
}
