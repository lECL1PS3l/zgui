using System;
using System.Collections.Generic;
using System.IO;
using Xunit;
using ZapretGui.Core;
using ZapretGui.Core.Config;
using ZapretGui.Core.Log;
using ZapretGui.Core.Runtime;

namespace ZapretGui.Tests
{
    /// <summary>
    /// Процессную часть старта (launcher, UAC, реальный winws) проверяем вручную:
    /// тесты не должны вызывать диалог UAC. Здесь покрыты детерминированные ветки.
    /// </summary>
    [Collection("LogRing")]
    public class ProfileRunnerTests : IDisposable
    {
        private readonly string _dir;

        public ProfileRunnerTests()
        {
            _dir = Path.Combine(Path.GetTempPath(), "zgui_pr_" + System.Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_dir);
            LogRing.Init(_dir, null);
        }

        public void Dispose()
        {
            LogRing.Clear();
            try { Directory.Delete(_dir, true); } catch { }
        }

        [Fact]
        public void Start_MissingExe_ReturnsError()
        {
            var profile = new Profile
            {
                Id = "general",
                Name = "General",
                Engine = Engines.Flowseal,
                Args = new List<string> { "--new" }
            };
            var settings = Settings.Default();

            var r = ProfileRunner.Start(profile, Path.Combine(_dir, "nope"), settings, _dir);

            Assert.False(r.IsOk);
            Assert.Contains("winws.exe", r.Error);
            // Сообщение уже человеческое — humanize его не исказил.
            Assert.Contains("корне движка", r.Error);
        }

        [Fact]
        public void CurrentStatus_ReflectsState()
        {
            var state = new State { Data = _dir };
            Assert.Null(ProfileRunner.CurrentStatus(state));

            state.Runtime = new Runtime { ProfileId = "x", Pid = 0, Via = "app" };
            Runtime rt = ProfileRunner.CurrentStatus(state);
            Assert.Equal("x", rt.ProfileId);
            // PID 0 никогда не жив (runner.rs pid_alive).
            Assert.False(rt.Alive);
        }

        [Fact]
        public void Stop_ClearsRuntimeAndLogs()
        {
            var state = new State { Data = _dir };
            state.Runtime = new Runtime { ProfileId = "x", Pid = 0, Via = "app" };
            state.Profiles.Add(new Profile { Id = "x", Name = "X", Engine = Engines.Flowseal });

            Assert.True(ProfileRunner.Stop(state, true));
            Assert.Null(state.Runtime);

            // Повторная остановка ничего не делает.
            Assert.False(ProfileRunner.Stop(state, true));

            // Журнал фиксирует остановку.
            Assert.Contains("остановлено: x", LogRing.Dump());
        }
    }
}
