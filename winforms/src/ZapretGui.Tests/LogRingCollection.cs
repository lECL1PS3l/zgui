using Xunit;

namespace ZapretGui.Tests
{
    /// <summary>
    /// Тесты, пишущие в статический LogRing: выполняются последовательно,
    /// иначе общий журнал гоняет между классами.
    /// </summary>
    [CollectionDefinition("LogRing", DisableParallelization = true)]
    public class LogRingCollection
    {
    }
}
