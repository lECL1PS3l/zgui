using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Text;
using System.Threading.Tasks;
using ZapretGui.Core.Config;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Updater
{
    /// <summary>Каталог обновляемого конфига (updater.rs:7-15).</summary>
    public class CatEntry
    {
        public string Id;
        public string Group;
        public string Label;
        public string Url;
        public string Dest;
        public bool CatalogOnly;
    }

    /// <summary>
    /// Обновление конфигов движка из белого списка + синхронизация ipset —
    /// порт updater.rs. Сеть — через Httpx, в юнит-тестах не дёргается.
    /// </summary>
    public static class Updater
    {
        /// <summary>Заглушка Flowseal для режима ipset «none» (updater.rs:346).</summary>
        public const string IpsetPlaceholder = "203.0.113.113/32\n";

        /// <summary>Версия встроенного моста tg-ws-proxy-rs (CARGO_PKG_VERSION, Task 13).</summary>
        public const string TgBridgeVersion = "2.3.4-zui.2";

        private const string FlowsealRepo = "Flowseal/zapret-discord-youtube";

        // Сигнал 1 для check_tg_bridge: апстрим-версия крейта в ZUI.
        private const string ZuiCargoUrl =
            "https://raw.githubusercontent.com/AmantesNihilo/zapret-universal-interface/main/crates/tg-ws-proxy-rs/Cargo.toml";

        private static string Raw(string repo, string branch, string path)
        {
            return "https://raw.githubusercontent.com/" + repo + "/refs/heads/" + branch + "/" + path;
        }

        private static string Api(string repo, string path)
        {
            return "https://api.github.com/repos/" + repo + "/" + path;
        }

        /// <summary>Id записи: группа + метка, '/' и '\' → "__" (updater.rs:44).</summary>
        public static string EntryId(string group, string label)
        {
            return group + ":" + label.Replace("/", "__").Replace("\\", "__");
        }

        /// <summary>
        /// Каталог обновляемых конфигов: нужен движок/сеть только для списка .bat.
        /// Белый список — бинарники и user-файлы не трогаются (updater.rs:39-137).
        /// </summary>
        public static async Task<Result<List<CatEntry>>> CollectEntriesAsync(string dataDir, Roots roots)
        {
            Result<List<string>> bats = await FetchBatNamesAsync(Httpx.Client()).ConfigureAwait(false);
            if (!bats.IsOk)
            {
                return Result<List<CatEntry>>.Err(bats.Error);
            }
            return Result<List<CatEntry>>.Ok(BuildEntries(dataDir, roots, bats.Value));
        }

        /// <summary>Чистая часть каталога — для тестов без сети.</summary>
        public static List<CatEntry> BuildEntries(string dataDir, Roots roots, List<string> bats)
        {
            var entries = new List<CatEntry>();
            // Списки качаются в live-корень движка, только если он установлен.
            if (!string.IsNullOrEmpty(roots?.Flowseal))
            {
                string root = roots.Flowseal;
                string lists = Path.Combine(root, "lists");
                Push(entries, "flowseal lists", "list-general.txt",
                    Raw(FlowsealRepo, "main", "lists/list-general.txt"),
                    Path.Combine(lists, "list-general.txt"), false);
                Push(entries, "flowseal lists", "list-google.txt",
                    Raw(FlowsealRepo, "main", "lists/list-google.txt"),
                    Path.Combine(lists, "list-google.txt"), false);
                Push(entries, "flowseal lists", "list-exclude.txt",
                    Raw(FlowsealRepo, "main", "lists/list-exclude.txt"),
                    Path.Combine(lists, "list-exclude.txt"), false);
                Push(entries, "flowseal lists", "ipset-exclude.txt",
                    Raw(FlowsealRepo, "main", "lists/ipset-exclude.txt"),
                    Path.Combine(lists, "ipset-exclude.txt"), false);
                // ipset-all.txt: в GitHub лежит заглушка, реальный список — в
                // .service/ipset-service.txt (его же качает service.bat).
                Push(entries, "flowseal lists", "ipset-all.txt",
                    Raw(FlowsealRepo, "main", ".service/ipset-service.txt"),
                    Path.Combine(lists, "ipset-all.txt"), false);
            }

            foreach (string f in new[] { "version.txt", "ipset-service.txt" })
            {
                Push(entries, "flowseal service", f,
                    Raw(FlowsealRepo, "main", ".service/" + f),
                    Path.Combine(dataDir, "catalog", "flowseal", ".service", f), true);
            }
            foreach (string name in bats)
            {
                Push(entries, "flowseal strategies", name,
                    Raw(FlowsealRepo, "main", name),
                    Path.Combine(dataDir, "catalog", "flowseal", "raw", name), true);
            }

            string geo = Path.Combine(dataDir, "catalog", "geoblock");
            string[][] geoLists =
            {
                new[] { "allow-domains-russia-inside.lst", "Russia/inside-raw.lst" },
                new[] { "allow-domains-geoblock.lst", "Categories/geoblock.lst" },
                new[] { "allow-domains-block.lst", "Categories/block.lst" },
                new[] { "allow-domains-news.lst", "Categories/news.lst" },
                new[] { "allow-domains-youtube.lst", "Services/youtube.lst" },
                new[] { "allow-domains-discord.lst", "Services/discord.lst" },
                new[] { "allow-domains-telegram.lst", "Services/telegram.lst" },
                new[] { "allow-domains-twitter.lst", "Services/twitter.lst" },
                new[] { "allow-domains-meta.lst", "Services/meta.lst" },
            };
            foreach (string[] pair in geoLists)
            {
                Push(entries, "geoblock domains", pair[0],
                    Raw("itdoginfo/allow-domains", "main", pair[1]),
                    Path.Combine(geo, pair[0]), true);
            }
            foreach (string[] pair in new[]
            {
                new[] { "russia-blocked-text.lst", "text/ru-blocked.txt" },
                new[] { "russia-blocked-community-text.lst", "text/ru-blocked-community.txt" },
            })
            {
                Push(entries, "geoblock ip", pair[0],
                    Raw("runetfreedom/russia-blocked-geoip", "release", pair[1]),
                    Path.Combine(geo, pair[0]), true);
            }
            return entries;
        }

        private static void Push(List<CatEntry> entries, string group, string label, string url, string dest, bool catalogOnly)
        {
            entries.Add(new CatEntry
            {
                Id = EntryId(group, label),
                Group = group,
                Label = label,
                Url = url,
                Dest = dest,
                CatalogOnly = catalogOnly,
            });
        }

        /// <summary>Список .bat репозитория через GitHub API (updater.rs:139-164).</summary>
        public static async Task<Result<List<string>>> FetchBatNamesAsync(HttpClient cli)
        {
            Result<string> r = await Httpx.GetJsonAsync(cli, Api(FlowsealRepo, "contents/?ref=main")).ConfigureAwait(false);
            if (!r.IsOk)
            {
                return Result<List<string>>.Err(r.Error);
            }
            List<GhFile> arr;
            try
            {
                arr = Json.Parse<List<GhFile>>(r.Value);
            }
            catch (Exception e)
            {
                return Result<List<string>>.Err(e.Message);
            }
            var names = arr
                .Where(f => f.Name != null)
                .Where(f => f.Name.ToLowerInvariant().EndsWith(".bat", StringComparison.Ordinal))
                .Where(f => !f.Name.ToLowerInvariant().StartsWith("service", StringComparison.Ordinal))
                .Select(f => f.Name)
                .ToList();
            names.Sort(StringComparer.Ordinal);
            return Result<List<string>>.Ok(names);
        }

        /// <summary>Тег последнего релиза движка без ведущей «v» (updater.rs:167-188).</summary>
        public static async Task<Result<string>> CheckEngineLatestAsync()
        {
            Result<string> r = await Httpx.GetJsonAsync(Httpx.Client(), Api(FlowsealRepo, "releases/latest")).ConfigureAwait(false);
            if (!r.IsOk)
            {
                return Result<string>.Err(r.Error);
            }
            GhRelease rel;
            try
            {
                rel = Json.Parse<GhRelease>(r.Value);
            }
            catch (Exception e)
            {
                return Result<string>.Err(e.Message);
            }
            string tag = (rel?.TagName ?? string.Empty).Trim().TrimStart('v');
            if (tag.Length == 0)
            {
                return Result<string>.Err("в релизе нет tag_name");
            }
            return Result<string>.Ok(tag);
        }

        /// <summary>ipset-all.txt обновляется только в режиме «loaded» (updater.rs:219-223).</summary>
        public static bool CanUpdate(CatEntry e, Settings settings)
        {
            return !(e.Label == "ipset-all.txt" && settings.IpsetMode != "loaded");
        }

        /// <summary>Проверка одной записи каталога (updater.rs:204-256).</summary>
        public static async Task<UpdEntry> CheckEntryAsync(HttpClient cli, CatEntry e, Settings settings, UpdArchive archive)
        {
            UpdEntry u = new UpdEntry
            {
                Id = e.Id,
                Group = e.Group,
                Label = e.Label,
                Dest = e.Dest,
                Exists = false,
                Status = "unknown",
                Size = 0,
            };
            if (!CanUpdate(e, settings))
            {
                u.Status = "skip-user";
                return u;
            }
            Result<byte[]> fetched = await Httpx.FetchBytesAsync(cli, e.Url).ConfigureAwait(false);
            if (!fetched.IsOk)
            {
                u.Status = "err";
                u.Error = fetched.Error;
                return u;
            }
            byte[] bytes = fetched.Value;
            u.Size = (ulong)bytes.Length;
            string remote = PortableData.Sha256Hex(bytes);
            u.RemoteHash = remote;
            string applied = archive.Applied(e.Id);
            u.AppliedHash = applied;
            if (File.Exists(e.Dest))
            {
                u.Exists = true;
                string local = PortableData.FileSha256(e.Dest);
                u.LocalHash = local;
                if (local == remote)
                {
                    u.Status = "ok";
                }
                else if (applied == local || applied == null)
                {
                    // Файл отличается от удалённого и мы его не применяли (или
                    // применяли ровно то, что лежит) — можно обновить.
                    u.Status = "avail";
                }
                else
                {
                    u.Status = "modified";
                }
            }
            else
            {
                u.Status = "new";
            }
            return u;
        }

        /// <summary>
        /// Проверка всех записей параллельно, порядок сохранён (updater.rs:260-281).
        /// </summary>
        public static async Task<Result<List<UpdEntry>>> CheckAllAsync(string dataDir, Roots roots, Settings settings)
        {
            Result<List<CatEntry>> entries = await CollectEntriesAsync(dataDir, roots).ConfigureAwait(false);
            if (!entries.IsOk)
            {
                return Result<List<UpdEntry>>.Err(entries.Error);
            }
            HttpClient cli = Httpx.Client();
            UpdArchive archive = UpdArchive.Load(dataDir);
            Task<UpdEntry>[] tasks = entries.Value
                .Select(e => CheckEntryAsync(cli, e, settings, archive))
                .ToArray();
            UpdEntry[] results = await Task.WhenAll(tasks).ConfigureAwait(false);
            return Result<List<UpdEntry>>.Ok(results.ToList());
        }

        /// <summary>Применяет выбранные обновления (updater.rs:284-343).</summary>
        public static async Task<Result<List<UpdEntry>>> ApplyAsync(string dataDir, Roots roots, Settings settings, List<string> ids)
        {
            Result<List<CatEntry>> entries = await CollectEntriesAsync(dataDir, roots).ConfigureAwait(false);
            if (!entries.IsOk)
            {
                return Result<List<UpdEntry>>.Err(entries.Error);
            }
            HttpClient cli = Httpx.Client();
            UpdArchive archive = UpdArchive.Load(dataDir);
            string ts = BatImport.NowEpoch().ToString(CultureInfo.InvariantCulture);
            var applied = new List<UpdEntry>();
            foreach (CatEntry e in entries.Value)
            {
                UpdEntry u0 = await CheckEntryAsync(cli, e, settings, archive).ConfigureAwait(false);
                bool selected = ids == null || ids.Count == 0
                    ? u0.Status == "avail" || u0.Status == "new"
                    : ids.Contains(e.Id);
                if (!selected || u0.Status == "err" || u0.Status == "skip-user")
                {
                    applied.Add(u0);
                    continue;
                }
                Result<byte[]> fetched = await Httpx.FetchBytesAsync(cli, e.Url).ConfigureAwait(false);
                if (!fetched.IsOk)
                {
                    u0.Status = "err";
                    u0.Error = fetched.Error;
                    applied.Add(u0);
                    continue;
                }
                string remote = PortableData.Sha256Hex(fetched.Value);
                if (File.Exists(e.Dest))
                {
                    Backup(e.Dest, ts, dataDir, e.CatalogOnly);
                }
                try
                {
                    Text.AtomicWrite(e.Dest, fetched.Value);
                }
                catch
                {
                    // Баг-особенность источника: запись в archive всё равно идёт.
                    u0.Status = "err";
                    u0.Error = "не удалось записать файл";
                    archive.Record(e.Id, remote, ts);
                    applied.Add(u0);
                    continue;
                }
                archive.Record(e.Id, remote, ts);
                u0.Status = "ok";
                u0.AppliedHash = remote;
                u0.LocalHash = PortableData.FileSha256(e.Dest);
                u0.Exists = true;
                u0.Error = null;
                applied.Add(u0);
            }
            archive.Save(dataDir);
            return Result<List<UpdEntry>>.Ok(applied);
        }

        /// <summary>
        /// Приводит lists/ipset-all.txt движка к режиму ipset (updater.rs:356-373):
        /// loaded — реальный список (.service/ipset-service.txt, фолбэк .backup);
        /// none — заглушка; any — пустой файл; без источника файл не трогаем.
        /// </summary>
        public static void SyncIpset(string root, string dataDir, Settings settings)
        {
            string dest = Path.Combine(root, "lists", "ipset-all.txt");
            byte[] body;
            switch (settings.IpsetMode)
            {
                case "any":
                    body = new byte[0];
                    break;
                case "none":
                    body = Encoding.UTF8.GetBytes(IpsetPlaceholder);
                    break;
                default:
                    string service = Path.Combine(dataDir, "catalog", "flowseal", ".service", "ipset-service.txt");
                    string backup = Path.Combine(root, "lists", "ipset-all.txt.backup");
                    body = ReadBytesOrNull(service) ?? ReadBytesOrNull(backup);
                    if (body == null)
                    {
                        return;
                    }
                    break;
            }
            Text.AtomicWrite(dest, body);
        }

        private static byte[] ReadBytesOrNull(string path)
        {
            try
            {
                return File.ReadAllBytes(path);
            }
            catch
            {
                return null;
            }
        }

        private static void Backup(string src, string ts, string dataDir, bool catalogOnly)
        {
            string name = Path.GetFileName(src);
            string dir;
            if (catalogOnly)
            {
                dir = Path.Combine(dataDir, "catalog", ".backups", ts);
            }
            else
            {
                string parent = Path.GetDirectoryName(src);
                if (parent == null)
                {
                    return;
                }
                dir = Path.Combine(parent, ".backups", ts);
            }
            try
            {
                Directory.CreateDirectory(dir);
                File.Copy(src, Path.Combine(dir, name), true);
            }
            catch
            {
                // резервная копия не критична (updater.rs:417-419)
            }
        }

        // ------------------------------------------------- telegram bridge

        /// <summary>Состояние обновления Telegram-моста (updater.rs:423-436).</summary>
        public static async Task<TgBridgeInfo> CheckTgBridgeAsync()
        {
            string local = TgBridgeVersion;
            TgBridgeInfo info = new TgBridgeInfo { LocalVersion = local };
            HttpClient cli = Httpx.Client();

            // Сигнал 1: версия крейта в апстриме ZUI.
            Result<byte[]> zui = await Httpx.FetchBytesAsync(cli, ZuiCargoUrl).ConfigureAwait(false);
            if (zui.IsOk)
            {
                string upstream = ParseTomlVersion(Encoding.UTF8.GetString(zui.Value));
                if (upstream != null)
                {
                    info.UpstreamVersion = upstream;
                    info.UpdateAvailable = VersionIsNewer(upstream, local);
                }
            }
            else
            {
                info.Note = "не удалось узнать версию ZUI: " + zui.Error;
            }

            // Сигнал 2: последний коммит Flowseal/tg-ws-proxy (информационно).
            Result<string> commits = await Httpx.GetJsonAsync(cli, Api("Flowseal/tg-ws-proxy", "commits?per_page=1")).ConfigureAwait(false);
            if (commits.IsOk)
            {
                try
                {
                    List<GhCommit> arr = Json.Parse<List<GhCommit>>(commits.Value);
                    GhCommit first = arr.Count > 0 ? arr[0] : null;
                    string date = first?.Commit?.Committer?.Date;
                    if (!string.IsNullOrEmpty(date))
                    {
                        string msg = (first.Commit.Message ?? string.Empty);
                        int nl = msg.IndexOf('\n');
                        info.UpstreamCommit = date + " — " + (nl < 0 ? msg : msg.Substring(0, nl));
                    }
                }
                catch
                {
                    // информационный сигнал, ошибка не важна
                }
            }
            return info;
        }

        /// <summary>Извлекает version = "x.y.z" из Cargo.toml (updater.rs:439-451).</summary>
        public static string ParseTomlVersion(string text)
        {
            if (text == null)
            {
                return null;
            }
            foreach (string raw in text.Split('\n'))
            {
                string t = raw.Trim();
                if (!t.StartsWith("version", StringComparison.Ordinal))
                {
                    continue;
                }
                string rest = t
                    .Substring("version".Length)
                    .TrimStart()
                    .TrimStart('=')
                    .Trim()
                    .Trim('"')
                    .Trim();
                if (rest.Length > 0)
                {
                    return rest;
                }
            }
            return null;
        }

        /// <summary>Сравнение версий вида 2.3.4-zui.2 (updater.rs:507-522).</summary>
        public static bool VersionIsNewer(string candidate, string current)
        {
            SplitVersion(candidate, out List<ulong> a, out ulong aSuffix);
            SplitVersion(current, out List<ulong> b, out ulong bSuffix);
            int cmp = CompareNumbers(a, b);
            if (cmp != 0)
            {
                return cmp > 0;
            }
            return aSuffix > bSuffix;
        }

        private static void SplitVersion(string s, out List<ulong> numbers, out ulong suffix)
        {
            int dash = s.IndexOf('-');
            string base_ = dash < 0 ? s : s.Substring(0, dash);
            string suffixPart = dash < 0 ? string.Empty : s.Substring(dash + 1);
            numbers = new List<ulong>();
            foreach (string part in base_.Split('.'))
            {
                ulong n;
                ulong.TryParse(part, NumberStyles.None, CultureInfo.InvariantCulture, out n);
                numbers.Add(n);
            }
            suffix = 0;
            int dot = suffixPart.LastIndexOf('.');
            string last = dot < 0 ? suffixPart : suffixPart.Substring(dot + 1);
            ulong.TryParse(last, NumberStyles.None, CultureInfo.InvariantCulture, out suffix);
        }

        private static int CompareNumbers(List<ulong> a, List<ulong> b)
        {
            int n = Math.Min(a.Count, b.Count);
            for (int i = 0; i < n; i++)
            {
                int cmp = a[i].CompareTo(b[i]);
                if (cmp != 0)
                {
                    return cmp;
                }
            }
            return a.Count.CompareTo(b.Count);
        }
    }

    /// <summary>Реестр применённых хэшей: data/catalog/applied.json (updater.rs:376-407).</summary>
    public class UpdArchive
    {
        private readonly Dictionary<string, string> _map = new Dictionary<string, string>(StringComparer.Ordinal);

        public static string FileFor(string dataDir)
        {
            return Path.Combine(dataDir, "catalog", "applied.json");
        }

        public static UpdArchive Load(string dataDir)
        {
            var archive = new UpdArchive();
            string text;
            try
            {
                text = File.ReadAllText(FileFor(dataDir));
            }
            catch
            {
                return archive;
            }
            Dictionary<string, object> m;
            try
            {
                m = Json.Parse<Dictionary<string, object>>(text);
            }
            catch
            {
                return archive;
            }
            foreach (KeyValuePair<string, object> kv in m)
            {
                if (kv.Value == null)
                {
                    continue;
                }
                archive._map[kv.Key] = kv.Value is string s
                    ? s
                    : Convert.ToString(kv.Value, CultureInfo.InvariantCulture);
            }
            return archive;
        }

        public string Applied(string id)
        {
            string hash;
            return _map.TryGetValue(id, out hash) ? hash : null;
        }

        /// <summary>Убирает записи с указанным префиксом группы (lib.rs:678).</summary>
        public int PurgePrefix(string prefix)
        {
            List<string> stale = _map.Keys.Where(k => k.StartsWith(prefix, StringComparison.Ordinal)).ToList();
            foreach (string k in stale)
            {
                _map.Remove(k);
            }
            return stale.Count;
        }

        public void Record(string id, string hash, string ts)
        {
            _map[id] = hash;
        }

        public void Save(string dataDir)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(FileFor(dataDir)));
                File.WriteAllText(FileFor(dataDir), Json.Serialize(_map));
            }
            catch
            {
                // Apply уже мог записать сами файлы; реестр не критичен.
            }
        }
    }

    public class TgBridgeInfo
    {
        [JsonField("localVersion")]
        public string LocalVersion;

        [JsonField("upstreamVersion")]
        public string UpstreamVersion;

        [JsonField("upstreamCommit")]
        public string UpstreamCommit;

        [JsonField("updateAvailable")]
        public bool UpdateAvailable;

        [JsonField("note")]
        public string Note;
    }

    // Минимальные DTO для ответов GitHub API.

    internal class GhFile
    {
        [JsonField("name")]
        public string Name;
    }

    internal class GhRelease
    {
        [JsonField("tagName")]
        public string TagName;
    }

    internal class GhCommit
    {
        [JsonField("commit")]
        public GhCommitDetail Commit;
    }

    internal class GhCommitDetail
    {
        [JsonField("committer")]
        public GhCommitter Committer;

        [JsonField("message")]
        public string Message;
    }

    internal class GhCommitter
    {
        [JsonField("date")]
        public string Date;
    }
}
