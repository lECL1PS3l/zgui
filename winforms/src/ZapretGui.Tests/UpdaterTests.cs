using System.Collections.Generic;
using System.IO;
using System.Text;
using Xunit;
using ZapretGui.Core.Config;
using ZapretGui.Core.Updater;
using ZapretGui.Core.Util;

namespace ZapretGui.Tests
{
    public class UpdaterTests
    {
        private static Settings Mode(string ipsetMode)
        {
            return new Settings { IpsetMode = ipsetMode };
        }

        private static CatEntry Cat(string label)
        {
            return new CatEntry { Id = "g:" + label, Group = "g", Label = label };
        }

        [Fact]
        public void CanUpdateRespectsIpsetMode()
        {
            Assert.False(Updater.CanUpdate(Cat("ipset-all.txt"), Mode("none")));
            Assert.True(Updater.CanUpdate(Cat("ipset-all.txt"), Mode("loaded")));
            Assert.True(Updater.CanUpdate(Cat("list-general.txt"), Mode("none")));
        }

        [Fact]
        public void EntryIdEscapesSeparators()
        {
            Assert.Equal("g:a__b", Updater.EntryId("g", "a/b"));
            Assert.Equal("g:a__b", Updater.EntryId("g", "a\\b"));
            Assert.Equal("g:plain.txt", Updater.EntryId("g", "plain.txt"));
        }

        [Fact]
        public void WhitelistCoversFlowsealAndGeoblock()
        {
            var roots = new Roots { Flowseal = Path.Combine(Path.GetTempPath(), "engine") };
            // FetchBatNamesAsync уже отфильтровал service.bat и отсортировал
            var bats = new List<string> { "alt-youtube.bat", "general.bat" };
            List<CatEntry> entries = Updater.BuildEntries(Path.Combine(Path.GetTempPath(), "data"), roots, bats);

            Assert.Equal(4 + 1 + 2 + bats.Count + 9 + 2, entries.Count);
            Assert.Equal(new[] { "flowseal strategies:alt-youtube.bat", "flowseal strategies:general.bat" },
                new[] { entries[7].Id, entries[8].Id });
            Assert.Equal("flowseal strategies", entries[7].Group);
            Assert.True(entries[7].CatalogOnly);
            Assert.EndsWith("catalog\\flowseal\\raw\\alt-youtube.bat", entries[7].Dest);
            Assert.EndsWith("alt-youtube.bat", entries[7].Url);

            // ipset-all берётся из .service, а не из lists
            CatEntry ipset = entries[4];
            Assert.Equal("flowseal lists:ipset-all.txt", ipset.Id);
            Assert.EndsWith(".service/ipset-service.txt", ipset.Url);
            Assert.EndsWith("lists\\ipset-all.txt", ipset.Dest);
            Assert.False(ipset.CatalogOnly);

            // geoblock ip качается с ветки release
            CatEntry geoIp = entries[entries.Count - 1];
            Assert.Equal("geoblock ip:russia-blocked-community-text.lst", geoIp.Id);
            Assert.Contains("/refs/heads/release/", geoIp.Url);

            // без установленного движка списков нет, каталог остаётся
            var noRoots = new Roots();
            List<CatEntry> catalogOnly = Updater.BuildEntries("data", noRoots, new List<string>());
            Assert.Equal(2 + 0 + 9 + 2, catalogOnly.Count);
            Assert.DoesNotContain(catalogOnly, e => e.Group == "flowseal lists");
        }

        [Fact]
        public void SyncIpsetMaterializesLoadedList()
        {
            string base_ = Path.Combine(Path.GetTempPath(), "zgui-ipset-" + System.Guid.NewGuid().ToString("N"));
            string data = Path.Combine(base_, "data");
            string root = Path.Combine(base_, "engine");
            string serviceDir = Path.Combine(data, "catalog", "flowseal", ".service");
            Directory.CreateDirectory(serviceDir);
            Directory.CreateDirectory(Path.Combine(root, "lists"));
            byte[] real = new byte[4096];
            for (int i = 0; i < real.Length; i++) real[i] = (byte)'1';
            File.WriteAllBytes(Path.Combine(serviceDir, "ipset-service.txt"), real);
            string dest = Path.Combine(root, "lists", "ipset-all.txt");
            File.WriteAllText(dest, Updater.IpsetPlaceholder);

            Updater.SyncIpset(root, data, Mode("loaded"));
            Assert.Equal(real, File.ReadAllBytes(dest));

            // loaded без источника — файл не трогаем
            File.Delete(Path.Combine(serviceDir, "ipset-service.txt"));
            Updater.SyncIpset(root, data, Mode("loaded"));
            Assert.Equal(real, File.ReadAllBytes(dest));

            Updater.SyncIpset(root, data, Mode("none"));
            Assert.Equal(Updater.IpsetPlaceholder, File.ReadAllText(dest));

            Updater.SyncIpset(root, data, Mode("any"));
            Assert.Equal(0, new FileInfo(dest).Length);

            Directory.Delete(base_, true);
        }

        [Fact]
        public void ParsesVersionFromCargoToml()
        {
            string t = "[package]\nname = \"x\"\nversion = \"2.3.4-zui.2\"\nedition = \"2024\"\n";
            Assert.Equal("2.3.4-zui.2", Updater.ParseTomlVersion(t));
            Assert.Null(Updater.ParseTomlVersion("name = \"x\"\n"));
        }

        [Theory]
        [InlineData("2.4.0", "2.3.4-zui.2", true)]
        [InlineData("2.3.5", "2.3.4", true)]
        [InlineData("2.3.4", "2.3.4-zui.2", false)]
        [InlineData("2.3.4-zui.3", "2.3.4-zui.2", true)]
        [InlineData("2.3.4-zui.2", "2.3.4-zui.2", false)]
        public void VersionCompare(string candidate, string current, bool expected)
        {
            Assert.Equal(expected, Updater.VersionIsNewer(candidate, current));
        }

        [Fact]
        public void ArchiveRecordsAndPurges()
        {
            string dir = Path.Combine(Path.GetTempPath(), "zgui-archive-" + System.Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path.Combine(dir, "catalog"));
            try
            {
                UpdArchive a = UpdArchive.Load(dir);
                Assert.Null(a.Applied("flowseal lists:list-general.txt"));
                a.Record("flowseal lists:list-general.txt", "abc", "123");
                a.Record("zapret2:winws2.bat", "zzz", "123");
                a.Save(dir);

                UpdArchive b = UpdArchive.Load(dir);
                Assert.Equal("abc", b.Applied("flowseal lists:list-general.txt"));
                Assert.Equal(1, b.PurgePrefix("zapret2"));
                Assert.Null(b.Applied("zapret2:winws2.bat"));
                Assert.Equal(0, b.PurgePrefix("zapret2"));
                b.Save(dir);

                Assert.Equal("{\"flowseal lists:list-general.txt\":\"abc\"}",
                    File.ReadAllText(UpdArchive.FileFor(dir), Encoding.UTF8));
            }
            finally
            {
                Directory.Delete(dir, true);
            }
        }
    }
}
