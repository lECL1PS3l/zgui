using System.Collections.Generic;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Config
{
    /// <summary>Запись каталога обновлений — порт config.rs:111-125.</summary>
    public class UpdEntry
    {
        [JsonField("id")]
        public string Id;

        [JsonField("group")]
        public string Group;

        [JsonField("label")]
        public string Label;

        [JsonField("dest")]
        public string Dest;

        [JsonField("exists")]
        public bool Exists;

        [JsonField("status")]
        public string Status;

        [JsonField("remoteHash")]
        public string RemoteHash;

        [JsonField("appliedHash")]
        public string AppliedHash;

        [JsonField("localHash")]
        public string LocalHash;

        [JsonField("size")]
        public ulong Size;

        [JsonField("error")]
        public string Error;
    }

    /// <summary>Кэш проверок обновлений — порт config.rs:127-137.</summary>
    public class UpdaterCache
    {
        [JsonField("lastCheck")]
        public string LastCheck;

        [JsonField("entries")]
        public List<UpdEntry> Entries = new List<UpdEntry>();

        [JsonField("lastAuto")]
        public string LastAuto;

        /// <summary>Кулдаун следующей автопроверки (epoch-сек).</summary>
        [JsonField("nextAuto")]
        public ulong? NextAuto;
    }
}
