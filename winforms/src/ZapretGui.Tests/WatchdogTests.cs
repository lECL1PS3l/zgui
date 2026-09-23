using System;
using System.Collections.Generic;
using System.Net.Http;
using Xunit;
using ZapretGui.Core.Watchdog;

namespace ZapretGui.Tests
{
    public class WatchdogTests
    {
        private static List<DomStatus> Domains(bool yt, bool dc)
        {
            return new List<DomStatus>
            {
                new DomStatus { Label = "YouTube", Host = "www.youtube.com", Ok = yt, Ms = 10 },
                new DomStatus { Label = "Discord", Host = "discord.com", Ok = dc, Ms = 20 },
            };
        }

        [Fact]
        public void InitialStatusIsInactive()
        {
            WatchdogStatus st = Watchdog.Status();
            Assert.False(st.Active);
            Assert.Equal(0u, st.Failures);
            Assert.False(st.Alarm);
            Assert.False(Watchdog.IsRunning());
        }

        [Fact]
        public void ProbeLocalhostFails()
        {
            // localhost без HTTPS-слушателя — ожидаем неудачу; важен факт отсутствия паники.
            using (HttpClient client = Watchdog.MakeClient(TimeSpan.FromMilliseconds(800)))
            {
                bool ok;
                ulong ms;
                Watchdog.Probe(client, "localhost", out ok, out ms);
                Assert.False(ok, "на localhost нет HTTPS-сервера");
            }
        }

        [Fact]
        public void AlarmOnlyAfterThreshold()
        {
            // Первые две неудачи — тишина, третья поднимает тревогу (FAIL_THRESHOLD).
            var t1 = Watchdog.Evaluate(Domains(true, false), 0, false);
            Assert.Equal(1u, t1.Failures);
            Assert.False(t1.Alarm);
            Assert.Null(t1.Kind);

            var t2 = Watchdog.Evaluate(Domains(false, false), t1.Failures, t1.Alarm);
            Assert.Equal(2u, t2.Failures);
            Assert.False(t2.Alarm);

            var t3 = Watchdog.Evaluate(Domains(false, false), t2.Failures, t2.Alarm);
            Assert.Equal(3u, t3.Failures);
            Assert.True(t3.Alarm);
            Assert.Equal("warn", t3.Kind);
            Assert.Equal("стратегия не отвечает: не отвечают YouTube, Discord", t3.Text);
        }

        [Fact]
        public void AlarmRepeatsAreSilent()
        {
            // Тревога уже поднята — повторные неудачи не шумят.
            var t = Watchdog.Evaluate(Domains(false, true), 5, true);
            Assert.Equal(6u, t.Failures);
            Assert.True(t.Alarm);
            Assert.Null(t.Kind);
        }

        [Fact]
        public void RecoveryResetsAndNotifies()
        {
            var t = Watchdog.Evaluate(Domains(true, true), 7, true);
            Assert.Equal(0u, t.Failures);
            Assert.False(t.Alarm);
            Assert.Equal("ok", t.Kind);
            Assert.Equal("стратегия снова отвечает", t.Text);

            // Без тревоги восстановление молчит.
            var quiet = Watchdog.Evaluate(Domains(true, true), 1, false);
            Assert.Equal(0u, quiet.Failures);
            Assert.Null(quiet.Kind);
        }
    }
}