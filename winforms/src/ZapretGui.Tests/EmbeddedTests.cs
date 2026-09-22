using System;
using System.IO;
using System.IO.Compression;
using System.Text;
using Xunit;
using ZapretGui.Core.Embedded;

namespace ZapretGui.Tests
{
    public class EmbeddedTests
    {
        private static readonly string AssetsDir =
            Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "assets");

        private static string TempDir()
        {
            var dir = Path.Combine(Path.GetTempPath(), "zgui_emb_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(dir);
            return dir;
        }

        private static byte[] MakeZip(params string[] names)
        {
            using (var ms = new MemoryStream())
            {
                using (var zip = new ZipArchive(ms, ZipArchiveMode.Create, true))
                {
                    foreach (var name in names)
                    {
                        var entry = zip.CreateEntry(name);
                        using (var w = new StreamWriter(entry.Open(), Encoding.UTF8))
                        {
                            w.Write("content of " + name);
                        }
                    }
                }
                return ms.ToArray();
            }
        }

        [Fact]
        public void SafeRelative_RejectsTraversal()
        {
            Assert.Equal("ok/file.txt", Embedded.SafeRelative("ok/file.txt"));
            Assert.Null(Embedded.SafeRelative("../evil.txt"));
            Assert.Null(Embedded.SafeRelative("ok/../../evil.txt"));
            Assert.Null(Embedded.SafeRelative("back\\slash.txt"));
            Assert.Null(Embedded.SafeRelative("C:\\absolute.txt"));
            Assert.Null(Embedded.SafeRelative("///"));
        }

        [Fact]
        public void ExtractAll_DropsUnsafeEntries()
        {
            var zip = MakeZip("file.txt", "../../evil.txt");
            var dest = TempDir();
            try
            {
                var count = Embedded.ExtractAll(zip, dest);
                Assert.Equal(1, count);
                Assert.True(File.Exists(Path.Combine(dest, "file.txt")));
                Assert.False(File.Exists(Path.Combine(dest, "evil.txt")));
                Assert.False(File.Exists(Path.Combine(Path.GetDirectoryName(dest), "evil.txt")));
            }
            finally { Directory.Delete(dest, true); }
        }

        [Fact]
        public void ExtractAll_StripsUniformTopDir()
        {
            var zip = MakeZip("flowseal/bin/winws.exe", "flowseal/README.md");
            var dest = TempDir();
            try
            {
                var count = Embedded.ExtractAll(zip, dest);
                Assert.Equal(2, count);
                Assert.True(File.Exists(Path.Combine(dest, "bin", "winws.exe")));
                Assert.True(File.Exists(Path.Combine(dest, "README.md")));
            }
            finally { Directory.Delete(dest, true); }
        }

        [Fact]
        public void ExtractAll_KeepsRootLevelFiles()
        {
            // Верхнего каталога нет — ничего не обрезаем.
            var zip = MakeZip("README.md", "bin/winws.exe");
            var dest = TempDir();
            try
            {
                var count = Embedded.ExtractAll(zip, dest);
                Assert.Equal(2, count);
                Assert.True(File.Exists(Path.Combine(dest, "README.md")));
                Assert.True(File.Exists(Path.Combine(dest, "winws.exe")));
            }
            finally { Directory.Delete(dest, true); }
        }

        [Fact]
        public void EnsureEmbeddedEngine_ExtractsRealZip()
        {
            var zip = File.ReadAllBytes(Path.Combine(AssetsDir, "engine-flowseal.zip"));
            var data = TempDir();
            try
            {
                Embedded.EngineZip = zip;
                var root = Embedded.EnsureEmbeddedEngine(data);
                Assert.NotNull(root);
                Assert.True(Directory.Exists(Path.Combine(root, "bin")));
                Assert.True(File.Exists(Path.Combine(root, "bin", "winws.exe")));

                // Повторный вызов не переустанавливает движок.
                var again = Embedded.EnsureEmbeddedEngine(data);
                Assert.Equal(root, again);
            }
            finally
            {
                Embedded.EngineZip = null;
                Directory.Delete(data, true);
            }
        }

        [Fact]
        public void SeedCatalog_SeedsRealSnapshot()
        {
            var zip = File.ReadAllBytes(Path.Combine(AssetsDir, "flowseal-main.zip"));
            var data = TempDir();
            try
            {
                Embedded.SnapshotZip = zip;
                var written = Embedded.SeedCatalog(data);
                Assert.True(written > 0);
                Assert.True(File.Exists(Path.Combine(data, "catalog", "flowseal", "raw", "general.bat")));
                Assert.True(File.Exists(Path.Combine(data, "catalog", "flowseal", "lists", "list-general.txt")));
                Assert.True(File.Exists(Path.Combine(data, "catalog", "source-info.txt")));

                // Повторный вызов ничего не копирует — всё уже на месте.
                Assert.Equal(0, Embedded.SeedCatalog(data));
            }
            finally
            {
                Embedded.SnapshotZip = null;
                Directory.Delete(data, true);
            }
        }

        [Fact]
        public void SeedCatalog_RemovesLegacyHosts()
        {
            var zip = File.ReadAllBytes(Path.Combine(AssetsDir, "flowseal-main.zip"));
            var data = TempDir();
            try
            {
                var legacy = Path.Combine(data, "catalog", "flowseal", ".service", "hosts");
                Directory.CreateDirectory(Path.GetDirectoryName(legacy));
                File.WriteAllText(legacy, "legacy");

                Embedded.SnapshotZip = zip;
                Embedded.SeedCatalog(data);
                Assert.False(File.Exists(legacy));
            }
            finally
            {
                Embedded.SnapshotZip = null;
                Directory.Delete(data, true);
            }
        }

        [Fact]
        public void NeutralizeAuthorAutoupdate_DisablesFlag()
        {
            var data = TempDir();
            try
            {
                var utils = Path.Combine(data, "utils");
                Directory.CreateDirectory(utils);
                var flag = Path.Combine(utils, "check_updates.enabled");
                File.WriteAllText(flag, "1");

                Embedded.NeutralizeAuthorAutoupdate(data);

                Assert.False(File.Exists(flag));
                Assert.True(File.Exists(flag + ".zgui_disabled"));

                // Уже отключено — повторный вызов не падает.
                Embedded.NeutralizeAuthorAutoupdate(data);
                Assert.True(File.Exists(flag + ".zgui_disabled"));
            }
            finally { Directory.Delete(data, true); }
        }

        [Fact]
        public void Constants_MatchRust()
        {
            Assert.Equal("1.10.2", Embedded.EngineVersion);
            Assert.Equal("Official Flowseal main snapshot bundled at build time", Embedded.SnapshotInfo);
            Assert.Equal("Flowseal 1.10.2 release archive bundled in the executable", Embedded.EngineInfo);
        }
    }
}
