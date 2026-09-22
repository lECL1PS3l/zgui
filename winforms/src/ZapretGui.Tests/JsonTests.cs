using Xunit;
using ZapretGui.Core.Config;
using ZapretGui.Core.Util;

namespace ZapretGui.Tests
{
    public class JsonTests
    {
        [Fact]
        public void SettingsJson_UsesCamelCase_and_ObeysDefaults()
        {
            var s = Settings.Default();
            var j = Json.Serialize(s);
            Assert.Contains("\"updateIntervalHours\":72", j);
            Assert.Contains("\"autostartMode\":\"none\"", j);
            Assert.DoesNotContain("\\u0", j); // кириллица не эскейпится
        }

        [Fact]
        public void DefaultSettings_HaveProdDefaults()
        {
            var s = Settings.Default();
            Assert.Equal(72, s.UpdateIntervalHours);
            Assert.Equal("none", s.AutostartMode);
            Assert.False(s.AlwaysAdmin);
            Assert.Equal(1443, s.TgPort);
            Assert.Equal("grey", s.Theme);
        }
    }
}
