using System;
using System.IO;
using System.Security.Cryptography;

namespace ZapretGui.Core.Config
{
    /// <summary>
    /// Portable-папка данных рядом с exe и хеширование — порт config.rs:270-295.
    /// </summary>
    public static class PortableData
    {
        /// <summary>
        /// Папка data рядом с exe. Бросает PortableDataException с дословным
        /// русским текстом ошибки (config.rs:276-282), если папку нельзя создать.
        /// </summary>
        public static string PortableDataDir()
        {
            string exe;
            try
            {
                exe = System.Reflection.Assembly.GetEntryAssembly().Location;
            }
            catch
            {
                exe = Environment.GetCommandLineArgs()[0];
            }
            string directory;
            try
            {
                directory = Path.GetDirectoryName(exe);
            }
            catch
            {
                throw new PortableDataException("не удалось определить папку zgui.exe");
            }
            if (string.IsNullOrEmpty(directory))
            {
                throw new PortableDataException("не удалось определить папку zgui.exe");
            }
            string data = Path.Combine(directory, "data");
            try
            {
                Directory.CreateDirectory(data);
            }
            catch (Exception e)
            {
                throw new PortableDataException(
                    "не удаётся создать portable-папку " + data + ": " + e.Message +
                    ". Поместите zgui.exe в доступную для записи папку.");
            }
            return data;
        }

        public static string Sha256Hex(byte[] data)
        {
            using (var sha = SHA256.Create())
            {
                byte[] hash = sha.ComputeHash(data);
                var sb = new System.Text.StringBuilder(hash.Length * 2);
                foreach (byte b in hash)
                {
                    sb.Append(b.ToString("x2"));
                }
                return sb.ToString();
            }
        }

        /// <summary>Хеш файла или null, если файл не читается (config.rs:293).</summary>
        public static string FileSha256(string path)
        {
            try
            {
                return Sha256Hex(File.ReadAllBytes(path));
            }
            catch
            {
                return null;
            }
        }
    }

    public class PortableDataException : Exception
    {
        public PortableDataException(string message) : base(message)
        {
        }
    }
}
