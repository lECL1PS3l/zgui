using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace ZapretGui.Core.Util
{
    /// <summary>
    /// Декодирование текстов, хвосты файлов, атомарная запись — порт
    /// config.rs:297-423 (decode_text / tail_file / find_exe / atomic_write).
    /// </summary>
    public static class Text
    {
        /// <summary>
        /// Декодирует текст, автоматически определяя кодировку. Логи и вывод
        /// PowerShell/winws приходят в UTF-16, UTF-8 (BOM/без) или OEM (CP866).
        /// </summary>
        public static string DecodeText(byte[] bytes)
        {
            if (bytes == null)
            {
                return string.Empty;
            }
            if (bytes.Length >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE)
            {
                return Encoding.Unicode.GetString(bytes, 2, bytes.Length - 2);
            }
            if (bytes.Length >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF)
            {
                return Encoding.BigEndianUnicode.GetString(bytes, 2, bytes.Length - 2);
            }
            if (bytes.Length >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF)
            {
                return Encoding.UTF8.GetString(bytes, 3, bytes.Length - 3);
            }
            // UTF-16 без BOM: каждый второй байт ноль (латиница/цифры/кириллица в BMP).
            if (bytes.Length >= 4 && bytes.Length % 2 == 0)
            {
                int zeros = 0;
                for (int i = 1; i < bytes.Length; i += 2)
                {
                    if (bytes[i] == 0)
                    {
                        zeros++;
                    }
                }
                if (zeros * 2 >= bytes.Length / 2)
                {
                    return Encoding.Unicode.GetString(bytes);
                }
            }
            if (IsValidUtf8(bytes))
            {
                return Encoding.UTF8.GetString(bytes);
            }
            // OEM (CP866) — так печатают консольные winws и cmd.
            try
            {
                return Encoding.GetEncoding(866).GetString(bytes);
            }
            catch
            {
                return Encoding.UTF8.GetString(bytes);
            }
        }

        public static string TrimBom(string text)
        {
            if (string.IsNullOrEmpty(text) || text[0] != 0xFEFF)
            {
                return text;
            }
            return text.Substring(1);
        }

        public static string ReadTextAuto(string path)
        {
            try
            {
                return DecodeText(File.ReadAllBytes(path));
            }
            catch
            {
                return null;
            }
        }

        /// <summary>Атомарная запись: tmp-файл + переименование (config.rs:364).</summary>
        public static void AtomicWrite(string path, byte[] data)
        {
            string dir = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir))
            {
                Directory.CreateDirectory(dir);
            }
            string tmp = Path.ChangeExtension(path, "zgui_tmp");
            File.WriteAllBytes(tmp, data);
            if (File.Exists(path))
            {
                File.Delete(path);
            }
            File.Move(tmp, path);
        }

        /// <summary>
        /// Хвост файла: последние строки не длиннее maxChars символов и не больше
        /// 400 строк (config.rs:374).
        /// </summary>
        public static string TailFile(string path, int maxChars)
        {
            byte[] bytes;
            try
            {
                bytes = File.ReadAllBytes(path);
            }
            catch
            {
                return string.Empty;
            }
            string s = DecodeText(bytes);
            string[] lines = s.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None);
            var tail = new Stack<string>();
            int total = 0;
            int count = 0;
            for (int i = lines.Length - 1; i >= 0; i--)
            {
                string line = lines[i];
                if (total + line.Length + 1 > maxChars || count >= 400)
                {
                    break;
                }
                tail.Push(line);
                total += line.Length + 1;
                count++;
            }
            return string.Join(Environment.NewLine, tail);
        }

        private static bool IsValidUtf8(byte[] bytes)
        {
            int i = 0;
            while (i < bytes.Length)
            {
                byte b = bytes[i];
                int need;
                int start;
                if (b <= 0x7F)
                {
                    i++;
                    continue;
                }
                if ((b & 0xE0) == 0xC0)
                {
                    need = 1;
                    start = b & 0x1F;
                }
                else if ((b & 0xF0) == 0xE0)
                {
                    need = 2;
                    start = b & 0x0F;
                }
                else if ((b & 0xF8) == 0xF0)
                {
                    need = 3;
                    start = b & 0x07;
                }
                else
                {
                    return false;
                }
                if (start == 0)
                {
                    return false; // overlong
                }
                i++;
                if (i + need > bytes.Length)
                {
                    return false;
                }
                int code = start;
                for (int k = 0; k < need; k++)
                {
                    byte c = bytes[i + k];
                    if ((c & 0xC0) != 0x80)
                    {
                        return false;
                    }
                    code = (code << 6) | (c & 0x3F);
                }
                if (code > 0x10FFFF)
                {
                    return false;
                }
                if (code >= 0xD800 && code <= 0xDFFF)
                {
                    return false;
                }
                i += need;
            }
            return true;
        }
    }
}
