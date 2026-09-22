using System;
using System.IO;
using System.Linq;
using System.Threading;
using Xunit;
using ZapretGui.Core.Runtime;

namespace ZapretGui.Tests
{
    public class ProcessesTests
    {
        [Fact]
        public void PidAlive_Zero_IsFalse()
        {
            Assert.False(Processes.PidAlive(0));
        }

        [Fact]
        public void PidAlive_AfterExit_IsFalse()
        {
            int pid;
            Assert.True(Processes.SpawnHidden("cmd.exe", new[] { "/c", "exit", "0" }, null, out pid));
            for (var i = 0; i < 50 && Processes.PidAlive(pid); i++) { Thread.Sleep(100); }
            Assert.False(Processes.PidAlive(pid));
        }

        [Fact]
        public void PidAlive_ForLiveProcess_IsTrue()
        {
            int pid;
            // cmd спит 2 секунды — процесс жив.
            Assert.True(Processes.SpawnHidden("cmd.exe", new[] { "/c", "timeout", "/t", "5", "/nobreak" }, null, out pid));
            try
            {
                Assert.True(Processes.PidAlive(pid));
            }
            finally
            {
                Processes.KillTree(pid);
            }
        }

        [Fact]
        public void RunOutputHidden_CapturesStdout()
        {
            int code; string output;
            Processes.RunOutputHidden("cmd.exe", new[] { "/c", "echo zapret-test" }, out code, out output);
            Assert.Equal(0, code);
            Assert.Contains("zapret-test", output);
        }

        [Fact]
        public void RunOutputHidden_BigStderr_NoDeadlock()
        {
            // Много текста в stderr: если его не читать параллельно — зависнем.
            int code; string output;
            Processes.RunOutputHidden("cmd.exe",
                new[] { "/c", "for", "/L", "%i", "in", "(1,1,2000)", "do", "echo", "noise-line", "1>&2" },
                out code, out output);
            Assert.Equal(0, code);
        }

        [Fact]
        public void BuildArgs_QuotesSpaces()
        {
            Assert.Equal("\"a b\" c", Processes.BuildArgs(new[] { "a b", "c" }));
            Assert.Equal("", Processes.BuildArgs(new string[0]));
        }

        [Fact]
        public void FindExe_ReturnsRelativePath()
        {
            var root = Path.Combine(Path.GetTempPath(), "zgui_test_" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path.Combine(root, "bin"));
            File.WriteAllText(Path.Combine(root, "bin", "winws.exe"), "");
            try
            {
                Assert.Equal("bin/winws.exe", Processes.FindExe(root, "winws.exe"));
                Assert.Null(Processes.FindExe(root, "missing.exe"));
            }
            finally { Directory.Delete(root, true); }
        }

        [Fact]
        public void FindExe_RespectsDepthLimit()
        {
            var root = Path.Combine(Path.GetTempPath(), "zgui_test_" + Guid.NewGuid().ToString("N"));
            var deep = root;
            for (var i = 0; i < 7; i++)
            {
                deep = Path.Combine(deep, "lvl" + i);
            }
            Directory.CreateDirectory(deep);
            File.WriteAllText(Path.Combine(deep, "winws.exe"), "");
            try
            {
                // 7 уровней вложенности — глубже лимита 5.
                Assert.Null(Processes.FindExe(root, "winws.exe"));
            }
            finally { Directory.Delete(root, true); }
        }
    }
}
