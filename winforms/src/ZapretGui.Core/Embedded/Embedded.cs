using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Text;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;
using ZapretGui.Core.Runtime;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Embedded
{
    /// <summary>
    /// Встроенные в исполняемый файл архивы Flowseal: движок (engine-flowseal.zip)
    /// и каталог стратегий (flowseal-main.zip) (embedded.rs).
    /// </summary>
    public static class Embedded
    {
        /// <summary>Версия вшитого движка Flowseal (embedded.rs:11).</summary>
        public const string EngineVersion = "1.10.2";

        /// <summary>Описание вшитого движка для UI (embedded.rs:12).</summary>
        public const string EngineInfo = "Flowseal 1.10.2 release archive bundled in the executable";

        /// <summary>Метка источника каталога (embedded.rs:9).</summary>
        public const string SnapshotInfo = "Official Flowseal main snapshot bundled at build time";

        /// <summary>Байты engine-flowseal.zip из ресурсов программы.</summary>
        public static byte[] EngineZip;

        /// <summary>Байты flowseal-main.zip из ресурсов программы.</summary>
        public static byte[] SnapshotZip;

        /// <summary>
        /// Распаковывает встроенный релиз движка Flowseal в
        /// &lt;data&gt;/engines/flowseal, если там ещё нет exe. Возвращает путь к
        /// корню движка или null (embedded.rs:16-29).
        /// </summary>
        public static string EnsureEmbeddedEngine(string dataDir)
        {
            var root = Path.Combine(dataDir, "engines", Engines.Flowseal);
            var existing = EngineRootFor(root);
            if (existing != null) { return existing; }

            if (EngineZip == null)
            {
                LogRing.Write("err", "embedded", "встроенный архив движка не подключён");
                return null;
            }

            try { Directory.CreateDirectory(root); }
            catch (Exception e) { LogRing.Write("err", "embedded", "не создать каталог движка: " + e.Message); return null; }

            try { ExtractAll(EngineZip, root); }
            catch (Exception e) { LogRing.Write("err", "embedded", "распаковка движка не удалась: " + e.Message); return null; }

            var found = EngineRootFor(root);
            if (found == null)
            {
                LogRing.Write("err", "embedded", "во встроенном архиве flowseal не найден winws.exe");
                return null;
            }
            NeutralizeAuthorAutoupdate(found);
            return found;
        }

        /// <summary>
        /// Отключает авторскую авто-проверку обновлений Flowseal:
        /// utils/check_updates.enabled заставляет каждый *.bat вызывать
        /// service.bat check_updates, который открывает страницу релиза в
        /// браузере. Нам это не нужно — обновления ведёт GUI (embedded.rs:34-42).
        /// </summary>
        public static void NeutralizeAuthorAutoupdate(string root)
        {
            var flag = Path.Combine(root, "utils", "check_updates.enabled");
            if (!File.Exists(flag)) { return; }
            var disabled = flag + ".zgui_disabled";
            try
            {
                if (File.Exists(disabled)) { File.Delete(disabled); }
                File.Move(flag, disabled);
            }
            catch { }
            if (File.Exists(flag))
            {
                try { File.Delete(flag); } catch { }
            }
        }

        /// <summary>Нормализованный корень уже распакованного движка (embedded.rs:51-67).</summary>
        public static string EngineRootFor(string dir)
        {
            if (!Directory.Exists(dir)) { return null; }
            var rel = Processes.FindExe(dir, Engines.WinwsExe);
            if (rel == null) { return null; }

            var exePath = Path.Combine(dir, rel.Replace('/', Path.DirectorySeparatorChar));
            var cur = Path.GetDirectoryName(exePath);
            while (cur != null && IsInside(dir, cur))
            {
                if (Directory.Exists(Path.Combine(cur, "bin"))) { return cur; }
                var parent = Path.GetDirectoryName(cur);
                if (parent == null || !IsInside(dir, parent)) { break; }
                cur = parent;
            }
            return dir;
        }

        /// <summary>
        /// Полная распаковка архива с обрезкой общего верхнего каталога
        /// и защитой от zip-slip (embedded.rs:70-137). Возвращает число файлов.
        /// </summary>
        public static int ExtractAll(byte[] bytes, string dest)
        {
            Directory.CreateDirectory(dest);
            string strip;
            using (var probe = new ZipArchive(new MemoryStream(bytes), ZipArchiveMode.Read))
            {
                strip = DetectTopDir(probe);
            }

            var written = 0;
            using (var archive = new ZipArchive(new MemoryStream(bytes), ZipArchiveMode.Read))
            {
                foreach (var entry in archive.Entries)
                {
                    var name = entry.FullName;
                    var relative = name;
                    if (!string.IsNullOrEmpty(strip))
                    {
                        var prefix = strip + "/";
                        if (relative.StartsWith(prefix, StringComparison.Ordinal))
                        {
                            relative = relative.Substring(prefix.Length);
                        }
                    }
                    var trimmed = relative.TrimEnd('/');
                    if (trimmed.Length == 0) { continue; }

                    var rel = SafeRelative(trimmed);
                    if (rel == null) { continue; }
                    var outPath = Path.Combine(dest, rel);
                    if (!IsInside(dest, outPath)) { continue; }

                    var isDir = name.EndsWith("/");
                    if (isDir)
                    {
                        try { Directory.CreateDirectory(outPath); } catch { }
                        continue;
                    }

                    try
                    {
                        var parent = Path.GetDirectoryName(outPath);
                        if (!string.IsNullOrEmpty(parent)) { Directory.CreateDirectory(parent); }
                        using (var src = entry.Open())
                        using (var buf = new MemoryStream())
                        {
                            src.CopyTo(buf);
                            Text.AtomicWrite(outPath, buf.ToArray());
                        }
                        written++;
                    }
                    catch { }
                }
            }
            return written;
        }

        /// <summary>
        /// Seeds каталог стратегий из встроенного снимка Flowseal: копирует только
        /// недостающие файлы (embedded.rs:139-154 + 156-214).
        /// </summary>
        public static int SeedCatalog(string dataDir)
        {
            var written = 0;
            // Удаляем устаревший системный hosts из старых портативных данных.
            var legacyHosts = Path.Combine(dataDir, "catalog", "flowseal", ".service", "hosts");
            if (File.Exists(legacyHosts))
            {
                try { File.Delete(legacyHosts); } catch { }
            }

            if (SnapshotZip != null)
            {
                written += ExtractMissing(SnapshotZip, dataDir);
            }

            var info = Path.Combine(dataDir, "catalog", "source-info.txt");
            if (!File.Exists(info))
            {
                try
                {
                    Text.AtomicWrite(info, Encoding.UTF8.GetBytes(SnapshotInfo));
                    written++;
                }
                catch { }
            }
            return written;
        }

        // Распаковка только недостающих файлов каталога (embedded.rs:156-187).
        private static int ExtractMissing(byte[] bytes, string dataDir)
        {
            var written = 0;
            using (var archive = new ZipArchive(new MemoryStream(bytes), ZipArchiveMode.Read))
            {
                foreach (var entry in archive.Entries)
                {
                    if (entry.FullName.EndsWith("/")) { continue; }
                    var slash = entry.FullName.IndexOf('/');
                    if (slash < 0) { continue; }
                    var relative = entry.FullName.Substring(slash + 1);
                    if (relative.Length == 0) { continue; }

                    var destination = FlowsealDestination(relative);
                    if (destination == null) { continue; }
                    var target = Path.Combine(dataDir, destination);
                    if (File.Exists(target)) { continue; }

                    try
                    {
                        var parent = Path.GetDirectoryName(target);
                        if (!string.IsNullOrEmpty(parent)) { Directory.CreateDirectory(parent); }
                        using (var src = entry.Open())
                        using (var buf = new MemoryStream())
                        {
                            src.CopyTo(buf);
                            Text.AtomicWrite(target, buf.ToArray());
                        }
                        written++;
                    }
                    catch { }
                }
            }
            return written;
        }

        // Соответствие файлов снимка каталогу программы (embedded.rs:189-214).
        private static string FlowsealDestination(string relative)
        {
            switch (relative)
            {
                case "LICENSE.txt": return "catalog/sources/flowseal/LICENSE.txt";
                case "README.md": return "catalog/sources/flowseal/README.md";
                case "service.bat": return "catalog/sources/flowseal/service.bat";
            }

            string name;
            if (TryStripPrefix(relative, ".service/", out name))
            {
                if (name == "version.txt" || name == "ipset-service.txt")
                {
                    var safe = SafeRelative(name);
                    return safe == null ? null : Path.Combine("catalog/flowseal/.service", safe);
                }
            }
            if (TryStripPrefix(relative, "lists/", out name))
            {
                if (name == "list-general.txt" || name == "list-google.txt" ||
                    name == "list-exclude.txt" || name == "ipset-exclude.txt" ||
                    name == "ipset-all.txt")
                {
                    var safe = SafeRelative(name);
                    return safe == null ? null : Path.Combine("catalog/flowseal/lists", safe);
                }
            }
            if (relative.IndexOf('/') < 0 &&
                relative.EndsWith(".bat", StringComparison.OrdinalIgnoreCase) &&
                !relative.Equals("service.bat", StringComparison.OrdinalIgnoreCase))
            {
                var safe = SafeRelative(relative);
                return safe == null ? null : Path.Combine("catalog/flowseal/raw", safe);
            }
            return null;
        }

        private static bool TryStripPrefix(string text, string prefix, out string rest)
        {
            if (text.StartsWith(prefix, StringComparison.Ordinal))
            {
                rest = text.Substring(prefix.Length);
                return true;
            }
            rest = null;
            return false;
        }

        /// <summary>
        /// Безопасный относительный путь: отсекает .., абсолютные пути и
        /// разделители Windows (embedded.rs:216-228).
        /// </summary>
        public static string SafeRelative(string path)
        {
            var parts = path.Split('/');
            var outPath = new StringBuilder();
            foreach (var part in parts)
            {
                if (part.Length == 0 || part == ".") { continue; }
                if (part == ".." || part.Contains("\\") || part.Contains(":")) { return null; }
                if (outPath.Length > 0) { outPath.Append('/'); }
                outPath.Append(part);
            }
            return outPath.Length == 0 ? null : outPath.ToString();
        }

        /// <summary>Копирует файл, только если назначения ещё нет (embedded.rs:230-237).</summary>
        public static void CopyMissing(string source, string destination)
        {
            if (!File.Exists(source) || File.Exists(destination)) { return; }
            try
            {
                var parent = Path.GetDirectoryName(destination);
                if (!string.IsNullOrEmpty(parent)) { Directory.CreateDirectory(parent); }
                File.Copy(source, destination);
            }
            catch { }
        }

        /// <summary>Копирует дерево, пропуская существующие файлы (embedded.rs:239-252).</summary>
        public static void CopyTreeMissing(string source, string destination)
        {
            string[] entries;
            try { entries = Directory.GetFileSystemEntries(source); }
            catch { return; }
            Directory.CreateDirectory(destination);
            foreach (var src in entries)
            {
                var dst = Path.Combine(destination, Path.GetFileName(src));
                if (Directory.Exists(src))
                {
                    CopyTreeMissing(src, dst);
                }
                else
                {
                    CopyMissing(src, dst);
                }
            }
        }

        // Определяет общий верхний каталог архива для обрезки (embedded.rs:73-102).
        private static string DetectTopDir(ZipArchive archive)
        {
            string top = null;
            var uniform = true;
            foreach (var entry in archive.Entries)
            {
                var trimmed = entry.FullName.TrimEnd('/');
                if (trimmed.Length == 0) { continue; }
                var slash = trimmed.IndexOf('/');
                if (slash >= 0)
                {
                    var first = trimmed.Substring(0, slash);
                    if (top == null) { top = first; }
                    else if (top != first) { uniform = false; break; }
                }
                else if (top != null && top != trimmed)
                {
                    uniform = false;
                }
            }
            return uniform ? top : null;
        }

        // path находится внутри root (сравнение по компонентам, как Path::starts_with).
        private static bool IsInside(string root, string path)
        {
            try
            {
                var fullRoot = Path.GetFullPath(root).TrimEnd('\\') + "\\";
                return Path.GetFullPath(path).StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase);
            }
            catch { return false; }
        }
    }
}
