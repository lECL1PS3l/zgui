using System.Collections.Generic;
using Xunit;
using ZapretGui.Core.Net;

namespace ZapretGui.Tests
{
    public class GameFilterTests
    {
        [Theory]
        [InlineData("off", "12", "12")]
        [InlineData("all", "1024-65535", "1024-65535")]
        [InlineData("tcp", "1024-65535", "12")]
        [InlineData("udp", "12", "1024-65535")]
        [InlineData("games", "12", "12")]
        [InlineData("", "12", "12")]
        public void Ports_Modes(string mode, string tcp, string udp)
        {
            GameFilter.Ports(mode, out string actualTcp, out string actualUdp);
            Assert.Equal(tcp, actualTcp);
            Assert.Equal(udp, actualUdp);
        }

        [Fact]
        public void Ports_GamesIsNotEmpty()
        {
            GameFilter.Ports("games", out string tcp, out string udp);
            Assert.False(string.IsNullOrEmpty(tcp));
            Assert.False(string.IsNullOrEmpty(udp));
        }

        [Fact]
        public void Apply_ReplacesPlaceholders()
        {
            var args = new List<string> { "--wf-tcp=%GameFilterTCP%", "--wf-udp=%GameFilterUDP%" };
            var applied = GameFilter.Apply(args, "443,80", "500-1000");
            Assert.Equal("--wf-tcp=443,80", applied[0]);
            Assert.Equal("--wf-udp=500-1000", applied[1]);
        }

        [Fact]
        public void Apply_NoPlaceholders_PreservesArgs()
        {
            var args = new List<string> { "--new", "--filter-tcp=80" };
            var applied = GameFilter.Apply(args, "12", "12");
            Assert.Equal(args, applied);
        }
    }
}
