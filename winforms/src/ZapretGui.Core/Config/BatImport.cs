using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Threading;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Config
{
    /// <summary>
    /// Разбор .bat стратегий flowseal и сохранение профилей — порт profiles.rs
    /// (decode_strategy_bytes / parse_flowseal_bat / import_bat_profiles) и
    /// save_profile из lib.rs:739-779.
    /// </summary>
    public static class BatImport
    {
        private static int _idCounter;

        /// <summary>
        /// Читает текст .bat независимо от кодировки: UTF-8 (с BOM/без), UTF-16 LE
        /// с BOM (profiles.rs:34). Авторские стратегии бывают в UTF-16, из-за чего
        /// токенизатор получал пустой набор аргументов и стратегия пропадала.
        /// </summary>
        public static string DecodeStrategyBytes(byte[] bytes)
        {
            if (bytes.Length >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF)
            {
                return Encoding.UTF8.GetString(bytes, 3, bytes.Length - 3);
            }
            if (bytes.Length >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE)
            {
                char[] chars = new char[(bytes.Length - 2) / 2];
                Buffer.BlockCopy(bytes, 2, chars, 0, chars.Length * 2);
                return new string(chars);
            }
            return Encoding.UTF8.GetString(bytes);
        }

        /// <summary>
        /// Разбирает .bat стратегию flowseal, извлекая argv для winws.exe. Возвращает
        /// токены с оставшимися плейсхолдерами %GameFilterTCP/UDP% (profiles.rs:50).
        /// </summary>
        public static List<string> ParseFlowsealBat(string content, string root)
        {
            List<string> tokens = TokenizeCmd(JoinContinuations(content));
            int pos = -1;
            for (int i = 0; i < tokens.Count; i++)
            {
                if (tokens[i].ToLowerInvariant().Contains("winws.exe"))
                {
                    pos = i;
                    break;
                }
            }
            if (pos < 0)
            {
                return new List<string>();
            }
            string bin = Path.Combine(root, "bin") + "\\";
            string lists = Path.Combine(root, "lists") + "\\";
            string rootSlash = root + "\\";
            var args = new List<string>(tokens.Count - pos - 1);
            for (int i = pos + 1; i < tokens.Count; i++)
            {
                args.Add(ExpandVars(tokens[i], rootSlash, bin, lists));
            }
            return args;
        }

        /// <summary>Читает {id}.bat из корня движка и разбирает его в argv.</summary>
        public static List<string> ParseArgsFromBat(string root, string id)
        {
            string path = Path.Combine(root, id + ".bat");
            byte[] bytes;
            try
            {
                bytes = File.ReadAllBytes(path);
            }
            catch
            {
                return new List<string>();
            }
            return ParseFlowsealBat(DecodeStrategyBytes(bytes), root);
        }

        /// <summary>
        /// Импортирует .bat из каталога raw-стратегий в профили (id = имя файла без
        /// .bat, engine flowseal) — порт profiles.rs:170-199.
        /// </summary>
        public static List<Profile> ImportBatProfiles(string root, IList<(string Name, string Content)> bats, IList<Profile> existing)
        {
            var imported = new List<Profile>();
            foreach (var bat in bats)
            {
                string id = bat.Name.EndsWith(".bat", StringComparison.OrdinalIgnoreCase)
                    ? bat.Name.Substring(0, bat.Name.Length - 4)
                    : bat.Name;
                List<string> args = ParseFlowsealBat(bat.Content, root);
                if (args.Count == 0)
                {
                    continue;
                }
                Profile prev = null;
                for (int i = 0; i < existing.Count; i++)
                {
                    if (existing[i].Id == id)
                    {
                        prev = existing[i];
                        break;
                    }
                }
                bool custom = prev != null && !prev.Builtin;
                imported.Add(new Profile
                {
                    Id = id,
                    Name = PrettyName(id),
                    Engine = Engines.Flowseal,
                    Args = args,
                    Builtin = false,
                    Source = bat.Name,
                    UpdatedAt = custom ? prev.UpdatedAt : NowEpoch().ToString()
                });
            }
            return imported;
        }

        /// <summary>
        /// Сохраняет профиль в state — порт save_profile (lib.rs:739-779). Возвращает
        /// текст ошибки (пустое название, пустые аргументы, дубль имени) или null.
        /// </summary>
        public static string Save(State state, string id, string name, string engine, List<string> args)
        {
            name = (name ?? string.Empty).Trim();
            if (name.Length == 0)
            {
                return "укажите название стратегии";
            }
            if (args == null || args.Count == 0)
            {
                return "список аргументов пуст — нечего сохранять";
            }
            // Дубли имён путают: в списке и в автозапуске две записи выглядят одинаково.
            for (int i = 0; i < state.Profiles.Count; i++)
            {
                Profile p = state.Profiles[i];
                if (p.Id != (id ?? string.Empty) && p.Name.Equals(name, StringComparison.OrdinalIgnoreCase))
                {
                    return "стратегия с названием «" + name + "» уже есть — выберите другое имя";
                }
            }
            if (string.IsNullOrEmpty(id))
            {
                state.Profiles.Add(new Profile
                {
                    Id = MakeId("custom"),
                    Name = name,
                    Engine = engine,
                    Args = args,
                    Builtin = false,
                    Source = null,
                    UpdatedAt = NowEpoch().ToString()
                });
            }
            else
            {
                Profile p = state.Profile(id);
                if (p == null)
                {
                    return "профиль не найден";
                }
                p.Name = name;
                p.Builtin = false;
                p.Args = args;
                p.UpdatedAt = NowEpoch().ToString();
            }
            state.Save();
            return null;
        }

        /// <summary>Id нового профиля: префикс + миллисекунды + счётчик (profiles.rs:14).</summary>
        public static string MakeId(string prefix)
        {
            int n = Interlocked.Increment(ref _idCounter);
            long ms = (long)DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
            return prefix + "-" + ms + "-" + n;
        }

        /// <summary>Человеческое название стратегии из имени файла (profiles.rs:201).</summary>
        public static string PrettyName(string raw)
        {
            string baseName = raw.EndsWith(".bat", StringComparison.OrdinalIgnoreCase)
                ? raw.Substring(0, raw.Length - 4)
                : raw;
            string tagged = baseName
                .Replace(" (ALT", " · ALT")
                .Replace(" (FAKE", " · FAKE")
                .Replace(" (EXP", " · EXP")
                .Replace(" (SIMPLE", " · SIMPLE");
            string s = tagged.Replace("(", string.Empty).Replace(")", string.Empty);
            if (s == "general")
            {
                s = "General";
            }
            return s;
        }

        public static long NowEpoch()
        {
            return DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        }

        // ---------------- внутреннее ----------------

        private static string JoinContinuations(string src)
        {
            var sb = new StringBuilder(src.Length);
            string[] lines = src.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None);
            bool pending = false;
            foreach (string rawLine in lines)
            {
                string line = rawLine.TrimEnd();
                if (pending)
                {
                    sb.Append(' ');
                    pending = false;
                }
                if (line.EndsWith("^"))
                {
                    sb.Append(line, 0, line.Length - 1);
                    pending = true;
                }
                else
                {
                    sb.Append(line);
                    sb.Append('\n');
                }
            }
            return sb.ToString();
        }

        /// <summary>Разбивает командную строку на токены (argv), снимая кавычки (profiles.rs:89).</summary>
        private static List<string> TokenizeCmd(string src)
        {
            var tokens = new List<string>();
            var cur = new StringBuilder();
            bool inQ = false;
            bool started = false;
            int i = 0;
            while (i < src.Length)
            {
                char c = src[i];
                if ((c == ' ' || c == '\t' || c == '\n' || c == '\r') && !inQ)
                {
                    if (started)
                    {
                        tokens.Add(cur.ToString());
                        cur.Clear();
                        started = false;
                    }
                    i++;
                    continue;
                }
                if (c == '"')
                {
                    inQ = !inQ;
                    started = true;
                    i++;
                    continue;
                }
                if (c == '^' && i + 1 < src.Length && "!\"^%&<>()".IndexOf(src[i + 1]) >= 0)
                {
                    cur.Append(src[i + 1]);
                    started = true;
                    i += 2;
                    continue;
                }
                if (c == '\\' && i + 1 < src.Length && src[i + 1] == '"')
                {
                    cur.Append('"');
                    started = true;
                    i += 2;
                    continue;
                }
                cur.Append(c);
                started = true;
                i++;
            }
            if (started)
            {
                tokens.Add(cur.ToString());
            }
            return tokens;
        }

        private static string ExpandVars(string t, string rootSlash, string bin, string lists)
        {
            return t
                .Replace("%BIN%", bin)
                .Replace("%LISTS%", lists)
                .Replace("%~dp0", rootSlash);
        }
    }
}
