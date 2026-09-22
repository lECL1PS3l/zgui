using System.Collections.Generic;
using System.IO;
using Xunit;
using ZapretGui.Core.Config;
using ZapretGui.Core.Runtime;

namespace ZapretGui.Tests
{
    /// <summary>
    /// Установку/удаление службы (UAC, sc.exe, New-Service) проверяем вручную.
    /// Здесь — детерминированные части: cmdline, чтение состояния, реестр.
    /// </summary>
    public class ServiceTests
    {
        [Fact]
        public void State_DoesNotThrow()
        {
            // На машине разработчика может стоять настоящая служба zapret —
            // поэтому проверяем инвариант, а не конкретные значения: running
            // задан только когда служба установлена.
            Service.State(out bool installed, out bool? running);
            Assert.Equal(installed, running.HasValue);
        }

        [Fact]
        public void Strategy_NoService_Null()
        {
            Assert.Null(Service.Strategy());
        }

        [Fact]
        public void BuildCmdline_QuotesPathAndSpacedArgs()
        {
            // Изолированный каталог: winws.exe в нём нет — используется fallback.
            var dir = Path.Combine(Path.GetTempPath(), "zgui_svc_" + System.Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(dir);
            try
            {
                var profile = new Profile
                {
                    Id = "general",
                    Name = "General",
                    Engine = Engines.Flowseal
                };
                var args = new List<string> { "--new", "--filter-tcp=80,443", "--wf-tcp=out" };

                string cmd = Service.BuildCmdline(dir, profile, args.ToArray());

                string exe = Path.Combine(dir, "bin", Engines.WinwsExe);
                Assert.Equal("\"" + exe + "\" --new --filter-tcp=80,443 --wf-tcp=out", cmd);
            }
            finally
            {
                try { Directory.Delete(dir, true); } catch { }
            }
        }

        [Fact]
        public void BuildCmdline_QuotedWhenExeFound()
        {
            var dir = Path.Combine(Path.GetTempPath(), "zgui_svc_" + System.Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path.Combine(dir, "bin"));
            string fakeExe = Path.Combine(dir, "bin", Engines.WinwsExe);
            File.WriteAllText(fakeExe, "");

            var profile = new Profile { Id = "p", Name = "P", Engine = Engines.Flowseal };
            string cmd = Service.BuildCmdline(dir, profile, new[] { "--new" });

            // Путь нашёлся реально — берём именно его.
            Assert.Equal("\"" + fakeExe + "\" --new", cmd);
            try { Directory.Delete(dir, true); } catch { }
        }
    }
}
