using System.Collections.Generic;
using Xunit;
using ZapretGui.Core.Net;

namespace ZapretGui.Tests
{
    public class DnsTests
    {
        [Fact]
        public void ProvidersHaveIpv4AndDoh()
        {
            Assert.True(Dns.Providers.Length >= 8);
            foreach (var p in Dns.Providers)
            {
                Assert.NotNull(System.Net.IPAddress.Parse(p.Primary));
                Assert.NotNull(System.Net.IPAddress.Parse(p.Secondary));
                Assert.StartsWith("https://", p.DohTemplate);
                Assert.False(string.IsNullOrEmpty(p.Description));
                Assert.False(string.IsNullOrEmpty(p.Note));
            }
        }

        [Fact]
        public void FindsCloudflareAndXbox()
        {
            Assert.Equal("1.1.1.1", Dns.Provider("cloudflare").Primary);
            Assert.Null(Dns.Provider("comss"));
            var x = Dns.Provider("xbox-dns");
            Assert.Equal("111.88.96.50", x.Primary);
            Assert.Equal("111.88.96.51", x.Secondary);
            Assert.Equal("https://xbox-dns.ru/dns-query", x.DohTemplate);
            Assert.Null(Dns.Provider("missing"));
        }

        [Fact]
        public void BuildsValidDnsQuery()
        {
            var q = Dns.BuildDnsQuery("example.com");
            // 12 байт заголовка + 1+7 + 1+3 + 1(TLD-конец) + 4 (QTYPE/QCLASS)
            Assert.Equal(12 + 8 + 4 + 1 + 4, q.Length);
            Assert.Equal(7, q[12]); // длина "example"
            Assert.Equal((byte)'e', q[13]);
            Assert.Equal((byte)'e', q[19]);
            Assert.Equal(3, q[20]); // длина "com"
            Assert.Equal(0, q[24]); // конец имени
            Assert.Equal(0, q[25]); Assert.Equal(1, q[26]); // QTYPE A
            Assert.Equal(0, q[27]); Assert.Equal(1, q[28]); // QCLASS IN
            Assert.Equal(0x12, q[0]); Assert.Equal(0x34, q[1]); // ID
        }

        [Fact]
        public void MedianPicksMiddle()
        {
            Assert.Equal(20, Dns.Median(new List<long> { 10, 30, 20 }));
            Assert.Equal(20, Dns.Median(new List<long> { 20 }));
            Assert.Equal(30, Dns.Median(new List<long> { 10, 30 }));
            Assert.Null(Dns.Median(new List<long>()));
            Assert.Null(Dns.Median(null));
        }

        [Fact]
        public void QueryOnceUnreachableReturnsNullFast()
        {
            // 203.0.113.0/24 — TEST-NET-1 (RFC 5737), ответа не будет.
            var sw = System.Diagnostics.Stopwatch.StartNew();
            var ms = Dns.QueryOnce("203.0.113.123", "example.com", 1000);
            sw.Stop();
            Assert.Null(ms);
            Assert.True(sw.ElapsedMilliseconds < 3500);
        }

        [Fact]
        public void BenchmarkWithUnknownIdsIsEmpty()
        {
            var rows = Dns.Benchmark(new[] { "nonexistent-proxy" });
            Assert.Equal(0, rows.Count);
        }

        [Fact]
        public void ApplyRejectsUnknownProvider()
        {
            var r = Dns.Apply(System.IO.Path.GetTempPath(), "no-such-provider", null);
            Assert.False(r.IsOk);
            Assert.Equal("неизвестный DNS-провайдер", r.Error);
        }

    }
}
