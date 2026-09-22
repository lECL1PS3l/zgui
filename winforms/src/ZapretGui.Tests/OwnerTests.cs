using Xunit;
using ZapretGui.Core;

namespace ZapretGui.Tests
{
    /// <summary>
    /// Таблица приоритетов владельца обхода (Review Focus плана): app запущен И
    /// служба установлена И чужой winws жив → app; тест активен → test;
    /// только чужой winws без наших PID → external.
    /// </summary>
    public class OwnerTests
    {
        [Theory]
        [InlineData(true, null, null, null, false, false, "test")]
        [InlineData(false, "profA", null, null, true, true, "app")]
        [InlineData(false, "profA", true, "svcProf", true, true, "app")]
        [InlineData(false, null, true, "svcProf", true, true, "service")]
        [InlineData(false, null, false, null, true, false, "none")]
        [InlineData(false, null, null, null, true, false, "none")]
        [InlineData(false, null, false, null, true, true, "external")]
        [InlineData(false, null, null, null, false, false, "none")]
        public void OwnerPriorityTable(bool tst, string app, bool? srv, string strat, bool any, bool own, string expected)
        {
            Assert.Equal(expected, Owner.OwnerName(Owner.WinwsOwnerOf(tst, app, srv, strat, any, own)));
        }

        [Fact]
        public void TestingWinsOverAppAndService()
        {
            Assert.Equal("test",
                Owner.OwnerName(Owner.WinwsOwnerOf(true, "prof", true, "svc", true, true)));
        }

        [Fact]
        public void ServiceRunningFalseFallsThroughToExternal()
        {
            Assert.Equal("external",
                Owner.OwnerName(Owner.WinwsOwnerOf(false, null, false, null, true, true)));
        }
    }
}
