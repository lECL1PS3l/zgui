namespace ZapretGui.Core
{
    /// <summary>
    /// Результат команды, которая может завершиться ошибкой: аналог Rust
    /// Result&lt;T, String&gt;. Error == null — успех.
    /// </summary>
    public struct Result<T>
    {
        public T Value;
        public string Error;

        public bool IsOk
        {
            get { return Error == null; }
        }

        public static Result<T> Ok(T value)
        {
            return new Result<T> { Value = value };
        }

        public static Result<T> Err(string error)
        {
            return new Result<T> { Error = error };
        }
    }
}
