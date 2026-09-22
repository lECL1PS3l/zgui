using System;
using System.Windows.Forms;

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

            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            Application.Run(new MainForm());
            return 0;
        }

        // Порт boot_pending из AppState (lib.rs:2864): выставляется до показа окна,
        // читается bootstrap-снимком, чтобы предложить применение стратегии при входе.
        public static bool BootPending { get; private set; }
    }
}
