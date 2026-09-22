using System;
using System.IO;
using System.Reflection;
using System.Windows.Forms;
using ZapretGui.Core.Embedded;

namespace ZapretGui.App
{
    internal static class Program
    {
        [STAThread]
        private static int Main(string[] args)
        {
            // --boot: признак запуска задачей планировщика при входе (lib.rs:2863).
            bool bootPending = Array.IndexOf(args, "--boot") >= 0;
            if (bootPending)
            {
                BootPending = true;
            }

            Embedded.EngineZip = ReadResource("ZapretGui.App.assets.engine-flowseal.zip");
            Embedded.SnapshotZip = ReadResource("ZapretGui.App.assets.flowseal-main.zip");
            Embedded.TgBridgeExe = ReadResource("ZapretGui.App.assets.tg-ws-proxy.exe");

            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            Application.Run(new MainForm());
            return 0;
        }

        private static byte[] ReadResource(string name)
        {
            var asm = Assembly.GetExecutingAssembly();
            using (var s = asm.GetManifestResourceStream(name))
            {
                if (s == null) { return null; }
                var buf = new byte[s.Length];
                var read = 0;
                while (read < buf.Length)
                {
                    var n = s.Read(buf, read, buf.Length - read);
                    if (n <= 0) { break; }
                    read += n;
                }
                return buf;
            }
        }

        // Порт boot_pending из AppState (lib.rs:2864): выставляется до показа окна,
        // читается bootstrap-снимком, чтобы предложить применение стратегии при входе.
        public static bool BootPending { get; private set; }
    }
}
