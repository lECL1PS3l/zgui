using System.Collections.Generic;

namespace ZapretGui.Core.Net
{
    /// <summary>
    /// Игровой фильтр: подстановка диапазонов портов в аргументы winws
    /// (profiles.rs:151-167). Режим «off» и неизвестные режимы дают мертвый
    /// диапазон «12» — winws тогда не ломает игры.
    /// </summary>
    public static class GameFilter
    {
        /// <summary>Диапазоны портов для режима: (tcp, udp) (profiles.rs:160-166).</summary>
        public static void Ports(string mode, out string tcp, out string udp)
        {
            switch (mode)
            {
                case "all":
                    tcp = "1024-65535";
                    udp = "1024-65535";
                    return;
                case "tcp":
                    tcp = "1024-65535";
                    udp = "12";
                    return;
                case "udp":
                    tcp = "12";
                    udp = "1024-65535";
                    return;
                default:
                    tcp = "12";
                    udp = "12";
                    return;
            }
        }

        /// <summary>Заменяет плейсхолдеры %GameFilterTCP%/%GameFilterUDP% (profiles.rs:151-157).</summary>
        public static List<string> Apply(List<string> args, string tcp, string udp)
        {
            var result = new List<string>(args.Count);
            foreach (var a in args)
            {
                result.Add(a.Replace("%GameFilterTCP%", tcp).Replace("%GameFilterUDP%", udp));
            }
            return result;
        }
    }
}
