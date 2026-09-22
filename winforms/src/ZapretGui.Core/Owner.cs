namespace ZapretGui.Core
{
    /// <summary>
    /// Кто прямо сейчас держит обход (winws). Единственный источник правды для
    /// индикаторов запуска и детекта «внешнего» процесса. Порт lib.rs:172-217.
    /// </summary>
    public enum WinwsOwner
    {
        None,
        /// <summary>Профиль, запущенный самой программой (winws — дочерний процесс GUI).</summary>
        App,
        /// <summary>Служба zapret (профиль необязателен: мог не сохраниться).</summary>
        Service,
        /// <summary>Идёт прогон теста — winws управляется тестом.</summary>
        Test,
        /// <summary>winws нашего движка, поднятый вне программы (ручной .bat).</summary>
        External,
    }

    public static class Owner
    {
        /// <summary>
        /// Чистое решение без обращений к системе — чтобы покрывать логику тестами
        /// (lib.rs:186-207). Приоритет: test → app → service → external → none.
        /// </summary>
        public static WinwsOwner WinwsOwnerOf(bool testing, string appProfile, bool? serviceRunning, string serviceStrategy, bool anyWinws, bool ownWinws)
        {
            if (testing)
            {
                return WinwsOwner.Test;
            }
            if (!string.IsNullOrEmpty(appProfile))
            {
                return WinwsOwner.App;
            }
            if (serviceRunning == true)
            {
                return WinwsOwner.Service;
            }
            if (anyWinws && ownWinws)
            {
                return WinwsOwner.External;
            }
            return WinwsOwner.None;
        }

        /// <summary>Имя владельца для UI: none|app|service|test|external (lib.rs:209).</summary>
        public static string OwnerName(WinwsOwner owner)
        {
            switch (owner)
            {
                case WinwsOwner.App: return "app";
                case WinwsOwner.Service: return "service";
                case WinwsOwner.Test: return "test";
                case WinwsOwner.External: return "external";
                default: return "none";
            }
        }

    }
}
