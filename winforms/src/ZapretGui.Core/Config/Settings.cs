using ZapretGui.Core.Util;

namespace ZapretGui.Core.Config
{
    /// <summary>
    /// Настройки пользователя — порт config.rs:29-81. Значения по умолчанию
    /// дословно из config.rs:64-81, имена полей camelCase как у serde.
    /// </summary>
    public class Settings
    {
        [JsonField("updateIntervalHours")]
        public int UpdateIntervalHours;

        [JsonField("gameFilter")]
        public string GameFilter;

        [JsonField("ipsetMode")]
        public string IpsetMode;

        [JsonField("autostartMode")]
        public string AutostartMode;

        /// <summary>Id профиля для автозапуска (служба или задача планировщика).</summary>
        [JsonField("autostartProfile")]
        public string AutostartProfile;

        /// <summary>Всегда перезапускать GUI от администратора.</summary>
        [JsonField("alwaysAdmin")]
        public bool AlwaysAdmin;

        /// <summary>Поднимать Telegram-мост при старте GUI.</summary>
        [JsonField("tgAutostart")]
        public bool TgAutostart;

        [JsonField("tgPort")]
        public ushort TgPort;

        /// <summary>Одноразовая миграция: старый дефолт интервала 6 ч → 72 ч.</summary>
        [JsonField("intervalMigrated")]
        public bool IntervalMigrated;

        /// <summary>Тема оформления: "grey" (графит), "dark" (космос), "light" (белая).</summary>
        [JsonField("theme")]
        public string Theme;

        /// <summary>Автозапуск GUI при входе: задача планировщика «ZapretGUI».</summary>
        [JsonField("bootApp")]
        public bool BootApp;

        /// <summary>Первый запуск: предложение «Всегда запускать от администратора» уже показано.</summary>
        [JsonField("adminOnboarded")]
        public bool AdminOnboarded;

        public static Settings Default()
        {
            return new Settings
            {
                UpdateIntervalHours = 72,
                GameFilter = "off",
                IpsetMode = "loaded",
                AutostartMode = "none",
                AutostartProfile = null,
                AlwaysAdmin = false,
                TgAutostart = false,
                TgPort = 1443,
                IntervalMigrated = false,
                Theme = "grey",
                BootApp = false,
                AdminOnboarded = false,
            };
        }
    }
}
