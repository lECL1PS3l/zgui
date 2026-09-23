using System;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;
using ZapretGui.Core.Tester;
using ZapretGui.Core.Watchdog;

namespace ZapretGui.Core
{
    /// <summary>
    /// Мост Core → UI: замена tauri-эвентов (emit "zgui:*"). Ядро ничего не знает
    /// о WinForms — только публикует события, а форма подписывается.
    /// События совпадают с именами источника: log, toast, status, updates, prog,
    /// conflict, test, watchdog.
    /// </summary>
    public static class Bus
    {
        /// <summary>Новая запись журнала ("zgui:log").</summary>
        public static event Action<Entry> Log;

        /// <summary>Уведомление: (kind, text) — ok/warn/err/info ("zgui:toast").</summary>
        public static event Action<string, string> Toast;

        /// <summary>Стратегию запустили/остановили — UI перечитывает снимок ("zgui:status").</summary>
        public static event Action Status;

        /// <summary>Каталог обновлений изменился ("zgui:updates").</summary>
        public static event Action Updates;

        /// <summary>Прогресс длительной операции: id/phase/msg/pct ("zgui:prog").</summary>
        public static event Action<string, string, string, int> Prog;

        /// <summary>Найдены конфликты ("zgui:conflict").</summary>
        public static event Action Conflict;

        /// <summary>Состояние теста стратегий ("zgui:test").</summary>
        public static event Action<TestProgress> Test;

        /// <summary>Состояние наблюдения ("zgui:watchdog").</summary>
        public static event Action<WatchdogStatus> WatchdogState;

        public static void RaiseLog(Entry e) { Fire(Log, e); }
        public static void RaiseToast(string kind, string text) { Fire2(Toast, kind, text); }
        public static void RaiseStatus() { Fire0(Status); }
        public static void RaiseUpdates() { Fire0(Updates); }
        public static void RaiseProg(string id, string phase, string msg, int pct) { Fire4(Prog, id, phase, msg, pct); }
        public static void RaiseConflict() { Fire0(Conflict); }
        public static void RaiseTest(TestProgress p) { Fire(Test, p); }
        public static void RaiseWatchdog(WatchdogStatus s) { Fire(WatchdogState, s); }

        // Обработчик-исключение не должен ломать ядро: UI может быть уже закрыт.
        private static void Fire<T>(Action<T> h, T arg)
        {
            if (h == null) { return; }
            try { h(arg); } catch { }
        }

        private static void Fire2(Action<string, string> h, string a, string b)
        {
            if (h == null) { return; }
            try { h(a, b); } catch { }
        }

        private static void Fire4(Action<string, string, string, int> h, string a, string b, string c, int d)
        {
            if (h == null) { return; }
            try { h(a, b, c, d); } catch { }
        }

        private static void Fire0(Action h)
        {
            if (h == null) { return; }
            try { h(); } catch { }
        }
    }
}