using System.Collections.Generic;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Config
{
    /// <summary>Профиль стратегии обхода — порт config.rs:83-99.</summary>
    public class Profile
    {
        [JsonField("id")]
        public string Id;

        [JsonField("name")]
        public string Name;

        [JsonField("engine")]
        public string Engine;

        [JsonField("args")]
        public List<string> Args = new List<string>();

        [JsonField("builtin")]
        public bool Builtin;

        /// <summary>Источник: "catalog" (из каталога), "bat" (импортирован из .bat) и т.п.</summary>
        [JsonField("source")]
        public string Source;

        [JsonField("updatedAt")]
        public string UpdatedAt;

        /// <summary>Имя exe движка, который запускает этот профиль (config.rs:96).</summary>
        public string ExeName()
        {
            return Engines.WinwsExe;
        }
    }

    /// <summary>Состояние запущенного winws — порт config.rs:101-109.</summary>
    public class Runtime
    {
        [JsonField("profileId")]
        public string ProfileId;

        [JsonField("pid")]
        public uint Pid;

        [JsonField("startedAt")]
        public ulong StartedAt;

        /// <summary>"app" (запущен GUI), "service" (служба), "test" (тестер).</summary>
        [JsonField("via")]
        public string Via;

        [JsonField("alive")]
        public bool Alive;
    }
}
