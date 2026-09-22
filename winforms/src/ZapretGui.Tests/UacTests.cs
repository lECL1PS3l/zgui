using System.IO;
using System.Text;
using Xunit;
using ZapretGui.Core.Runtime;

namespace ZapretGui.Tests
{
    public class UacTests
    {
        [Fact]
        public void PsHeader_MatchesRust()
        {
            Assert.Equal("$ErrorActionPreference = 'Stop'", Uac.PsHeader);
        }

        [Fact]
        public void WritePs1_HasUtf8Bom()
        {
            var path = Path.Combine(Path.GetTempPath(), "zgui_test_" + System.Guid.NewGuid().ToString("N") + ".ps1");
            try
            {
                Uac.WritePs1(path, "Write-Output 'привет'");
                var bytes = File.ReadAllBytes(path);
                Assert.Equal(0xEF, bytes[0]);
                Assert.Equal(0xBB, bytes[1]);
                Assert.Equal(0xBF, bytes[2]);
                Assert.Equal("Write-Output 'привет'", Encoding.UTF8.GetString(bytes, 3, bytes.Length - 3));
            }
            finally { try { File.Delete(path); } catch { } }
        }
    }
}
