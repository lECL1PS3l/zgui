using Xunit;
using ZapretGui.Core.Config;
using ZapretGui.Core.Runtime;

namespace ZapretGui.Tests
{
    public class AutostartTests
    {
        [Fact]
        public void LegacyBootRegistered_FalseInTestEnv()
        {
            // В тестовой среде старой записи zgui в HKCU\...\Run нет.
            Assert.False(Autostart.LegacyBootRegistered());
        }

        [Fact]
        public void BootTaskScript_On_Registers()
        {
            string on = Autostart.BootTaskScript(true, "C:\\Z GUI\\zgui.exe");
            Assert.Contains("Register-ScheduledTask", on);
            Assert.Contains("Unregister-ScheduledTask", on);
            Assert.Contains("--boot", on);
            Assert.Contains("-RunLevel Highest", on);
            Assert.Equal(Uac.PsHeader, on.Substring(0, Uac.PsHeader.Length));
        }

        [Fact]
        public void BootTaskScript_Off_OnlyUnregisters()
        {
            string off = Autostart.BootTaskScript(false, "C:\\Z GUI\\zgui.exe");
            Assert.Contains("Unregister-ScheduledTask", off);
            Assert.DoesNotContain("Register-ScheduledTask", off);
        }

        [Fact]
        public void WantsTask_ServiceBeatsProfile()
        {
            // Служба установлена — задача планировщика не нужна (механизм обхода один).
            Assert.False(Autostart.WantsTask(true, true));
            Assert.False(Autostart.WantsTask(true, false));
            Assert.True(Autostart.WantsTask(false, true));
            Assert.False(Autostart.WantsTask(false, false));
        }

        [Fact]
        public void PlanSync_ProfileWithoutService_CreatesTask()
        {
            var plan = Autostart.PlanSync(bootApp: false, serviceInstalled: false, haveProfile: true);
            Assert.True(plan.WantTask);
            Assert.Equal(Autostart.BootAction.Create, plan.Action);
            Assert.True(plan.SaveBootApp);
        }

        [Fact]
        public void PlanSync_ServiceInstalled_RemovesTask()
        {
            // Был программный автозапуск, поставили службу — задачу снимаем.
            var plan = Autostart.PlanSync(bootApp: true, serviceInstalled: true, haveProfile: true);
            Assert.False(plan.WantTask);
            Assert.Equal(Autostart.BootAction.Remove, plan.Action);
        }

        [Fact]
        public void PlanSync_AlreadySynced_Nothing()
        {
            Assert.Equal(Autostart.BootAction.None,
                Autostart.PlanSync(true, false, true).Action);
            Assert.Equal(Autostart.BootAction.None,
                Autostart.PlanSync(false, true, true).Action);
        }

        [Fact]
        public void PlanSync_NoProfileButTaskOn_Removes()
        {
            // Профиль автозапуска перестал существовать — задача больше не нужна.
            var plan = Autostart.PlanSync(bootApp: true, serviceInstalled: false, haveProfile: false);
            Assert.False(plan.WantTask);
            Assert.Equal(Autostart.BootAction.Remove, plan.Action);
        }

        [Fact]
        public void HaveAutostartProfile_RequiresExistingProfile()
        {
            var state = new State();
            // Режим есть, профиля нет.
            state.Settings.AutostartMode = "profile";
            state.Settings.AutostartProfile = "missing";
            Assert.False(Autostart.HaveAutostartProfile(state));

            state.Profiles.Add(new Profile { Id = "general", Name = "General", Engine = Engines.Flowseal });
            state.Settings.AutostartProfile = "general";
            Assert.True(Autostart.HaveAutostartProfile(state));

            // Режим не «profile» — профиль невалиден.
            state.Settings.AutostartMode = "none";
            Assert.False(Autostart.HaveAutostartProfile(state));
        }
    }
}
