using System;
using System.Collections.Generic;
using System.Net.Http;
using System.Threading;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;
using ZapretGui.Core.Runtime;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Watchdog
{
    /// <summary>Результат пробы одного домена (watchdog.rs:25-30).</summary>
    public class DomStatus
    {
        [JsonField("label")]
        public string Label;

        [JsonField("host")]
        public string Host;

        [JsonField("ok")]
        public bool Ok;

        [JsonField("ms")]
        public ulong Ms;
    }

    /// <summary>Состояние наблюдения (watchdog.rs:34-45).</summary>
    public class WatchdogStatus
    {
        /// <summary>Идёт ли наблюдение (запущена стратегия).</summary>
        [JsonField("active")]
        public bool Active;

        /// <summary>Подряд идущих неудач.</summary>
        [JsonField("failures")]
        public uint Failures;

        /// <summary>Есть ли сейчас тревога (стратегия не отвечает).</summary>
        [JsonField("alarm")]
        public bool Alarm;

        /// <summary>Последняя проверка (Unix-время, сек).</summary>
        [JsonField("checkedAt")]
        public ulong CheckedAt;

        /// <summary>Результат по доменам.</summary>
        [JsonField("domains")]
        public List<DomStatus> Domains = new List<DomStatus>();
    }

    /// <summary>
    /// Watchdog: раз в минуту проверяет доступность YouTube и Discord при запущенной
    /// стратегии и предупреждает, если она перестала отвечать. **Авто-восстановления
    /// нет** (решение владельца) — только уведомление. Порт watchdog.rs.
    /// </summary>
    public static class Watchdog
    {
        /// <summary>Домены, которые проверяет watchdog (как в FreeConnect).</summary>
        public static readonly string[,] WatchDomains =
        {
            { "YouTube", "www.youtube.com" },
            { "Discord", "discord.com" },
        };

        public const int CheckIntervalMs = 60000;
        public const int ProbeTimeoutMs = 5000;
        /// <summary>Сколько неудач подряд считаем сбоем обхода.</summary>
        public const uint FailThreshold = 3;

        private static readonly object _lock = new object();
        private static WatchdogStatus _status = new WatchdogStatus();
        private static int _running;

        /// <summary>Текущее состояние наблюдения (команда watchdog_status, lib.rs:1603).</summary>
        public static WatchdogStatus Status()
        {
            lock (_lock) { return Clone(_status); }
        }

        public static bool IsRunning()
        {
            return Interlocked.CompareExchange(ref _running, 0, 0) != 0;
        }

        /// <summary>Событие обновления состояния — GUI подписывается ("zgui:watchdog").</summary>
        public static event Action<WatchdogStatus> StatusChanged;

        /// <summary>Событие уведомления: (kind, text), kind = ok/warn.</summary>
        public static event Action<string, string> ToastReceived;

        private static void Set(WatchdogStatus s)
        {
            lock (_lock) { _status = s; }
            Action<WatchdogStatus> handler = StatusChanged;
            if (handler != null)
            {
                try { handler(Clone(s)); } catch { }
            }
        }

        private static WatchdogStatus Clone(WatchdogStatus s)
        {
            return new WatchdogStatus
            {
                Active = s.Active,
                Failures = s.Failures,
                Alarm = s.Alarm,
                CheckedAt = s.CheckedAt,
                Domains = new List<DomStatus>(s.Domains),
            };
        }

        private static void Toast(string kind, string text)
        {
            Action<string, string> handler = ToastReceived;
            if (handler != null)
            {
                try { handler(kind, text); } catch { }
            }
        }

        // ------------------------------------------------------------ проба

        /// <summary>
        /// Клиент пробы: таймаут 5 c, без редиректов и без прокси (watchdog.rs:82-87).
        /// </summary>
        public static HttpClient MakeClient(TimeSpan timeout)
        {
            var handler = new HttpClientHandler { AllowAutoRedirect = false, UseProxy = false };
            return new HttpClient(handler) { Timeout = timeout };
        }

        /// <summary>
        /// HTTPS-проба (TLS + HTTP-заголовки) с измерением времени.
        /// TCP-connect давал ложное «работает»: DPI пропускает рукопожатие TCP
        /// и режет TLS по SNI, поэтому браузер не открывает сайт, а проба «успешна».
        /// </summary>
        public static void Probe(HttpClient client, string host, out bool ok, out ulong ms)
        {
            var started = DateTime.UtcNow;
            ok = false;
            try
            {
                using (HttpResponseMessage resp = client.GetAsync("https://" + host + "/")
                    .GetAwaiter().GetResult())
                {
                    ok = true; // как в источнике: важен факт ответа, не код статуса
                }
            }
            catch
            {
                ok = false;
            }
            ms = (ulong)Math.Max(0, (DateTime.UtcNow - started).TotalMilliseconds);
        }

        // ------------------------------------------------------------ решение

        /// <summary>Итог одной итерации наблюдения (чистая логика watchdog.rs:133-163).</summary>
        public class Tick
        {
            public uint Failures;
            public bool Alarm;
            /// <summary>null — уведомления нет; иначе "ok" или "warn".</summary>
            public string Kind;
            public string Text;
        }

        /// <summary>
        /// Пересчёт неудач и тревоги по результатам доменов. Чистая функция, чтобы
        /// порог и «снова отвечает» покрывались тестами без ожидания минуты.
        /// </summary>
        public static Tick Evaluate(IList<DomStatus> domains, uint failures, bool alarm)
        {
            var t = new Tick { Failures = failures, Alarm = alarm };
            bool allOk = true; // как в источнике: пустой список — «все отвечают»
            foreach (DomStatus d in domains)
            {
                if (!d.Ok) { allOk = false; break; }
            }

            if (allOk)
            {
                t.Failures = 0;
                if (alarm)
                {
                    t.Alarm = false;
                    t.Kind = "ok";
                    t.Text = "стратегия снова отвечает";
                }
                return t;
            }

            t.Failures = failures < uint.MaxValue ? failures + 1 : uint.MaxValue;
            if (t.Failures >= FailThreshold && !alarm)
            {
                t.Alarm = true;
                var failed = new List<string>();
                foreach (DomStatus d in domains)
                {
                    if (!d.Ok) { failed.Add(d.Label); }
                }
                t.Kind = "warn";
                t.Text = "стратегия не отвечает: не отвечают " + string.Join(", ", failed.ToArray());
            }
            return t;
        }

        // ------------------------------------------------------------ владелец

        /// <summary>
        /// Текущий владелец обхода (lib.rs:221-240): тест, живой профиль GUI,
        /// служба или чужой winws.
        /// </summary>
        public static WinwsOwner CurrentOwner(State state)
        {
            bool testing = Tester.Tester.Current.Running || Tester.Tester.RunnerAlive(state.Data);
            string app = null;
            Config.Runtime rt = state.Runtime;
            if (rt != null && Processes.PidAlive(unchecked((int)rt.Pid)))
            {
                app = rt.ProfileId;
            }
            bool any = Conflicts.AnyWinwsRunning();
            bool own = any && Conflicts.OwnEnginePids(state.Data, null).Count > 0;
            return Owner.WinwsOwnerOf(testing, app, state.ServiceRunning, state.ServiceStrategy, any, own);
        }

        // ------------------------------------------------------------ запуск

        /// <summary>
        /// Запускает фоновый watchdog один раз за процесс (watchdog.rs:78).
        /// ownerProvider — источник владельца; по умолчанию CurrentOwner.
        /// </summary>
        public static void Spawn(State state, Func<WinwsOwner> ownerProvider = null)
        {
            if (Interlocked.CompareExchange(ref _running, 1, 0) != 0)
            {
                return; // уже запущен
            }

            HttpClient client;
            try
            {
                client = MakeClient(TimeSpan.FromMilliseconds(ProbeTimeoutMs));
            }
            catch (Exception e)
            {
                LogRing.Write("warn", "watchdog", "не удалось создать клиент пробы: " + e.Message);
                Interlocked.Exchange(ref _running, 0);
                return;
            }

            var thread = new Thread(() => Loop(state, ownerProvider, client))
            {
                IsBackground = true,
                Name = "zgui-watchdog",
            };
            thread.Start();
        }

        private static void Loop(State state, Func<WinwsOwner> ownerProvider, HttpClient client)
        {
            uint failures = 0;
            bool alarm = false;
            while (true)
            {
                Thread.Sleep(CheckIntervalMs);

                // Наблюдаем только когда стратегия реально запущена (программа или
                // служба) и не идёт тест — владелец обхода уже учитывает и то, и другое.
                WinwsOwner owner = ownerProvider != null ? ownerProvider() : CurrentOwner(state);
                bool active = owner == WinwsOwner.App || owner == WinwsOwner.Service;
                if (!active)
                {
                    // Стратегия не запущена — сбрасываем тревогу и не шумим.
                    failures = 0;
                    alarm = false;
                    Set(new WatchdogStatus { Active = false });
                    continue;
                }

                var domains = new List<DomStatus>();
                for (int i = 0; i < WatchDomains.GetLength(0); i++)
                {
                    string label = WatchDomains[i, 0];
                    string host = WatchDomains[i, 1];
                    bool ok;
                    ulong ms;
                    Probe(client, host, out ok, out ms);
                    domains.Add(new DomStatus { Label = label, Host = host, Ok = ok, Ms = ms });
                }

                Tick tick = Evaluate(domains, failures, alarm);
                failures = tick.Failures;
                alarm = tick.Alarm;
                if (tick.Kind == "ok")
                {
                    LogRing.Write("ok", "watchdog", tick.Text);
                }
                else if (tick.Kind == "warn")
                {
                    LogRing.Write("warn", "watchdog", tick.Text);
                }
                if (tick.Kind != null)
                {
                    Toast(tick.Kind, tick.Text);
                }

                Set(new WatchdogStatus
                {
                    Active = true,
                    Failures = failures,
                    Alarm = alarm,
                    CheckedAt = (ulong)DateTimeOffset.UtcNow.ToUnixTimeSeconds(),
                    Domains = domains,
                });
            }
        }
    }
}