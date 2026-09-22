using ZapretGui.Core.Util;

namespace ZapretGui.Core.Config
{
    public static class Engines
    {
        /// <summary>Единственный поддерживаемый движок (config.rs:6).</summary>
        public const string Flowseal = "flowseal";

        /// <summary>Имя службы Windows (config.rs:7).</summary>
        public const string ServiceName = "zapret";

        /// <summary>Имя задачи планировщика для автозапуска GUI при входе.</summary>
        public const string BootTaskName = "ZapretGUI";

        /// <summary>Имя exe движка.</summary>
        public const string WinwsExe = "winws.exe";
    }

    /// <summary>Корневые папки установленных движков — порт config.rs:9-27.</summary>
    public class Roots
    {
        [JsonField("flowseal")]
        public string Flowseal;

        /// <summary>Корневая папка движка или null, если движок не установлен.</summary>
        public string Path(string engine)
        {
            return engine == Engines.Flowseal ? Flowseal : null;
        }

        public void Set(string engine, string path)
        {
            if (engine == Engines.Flowseal)
            {
                Flowseal = path;
            }
        }
    }
}
