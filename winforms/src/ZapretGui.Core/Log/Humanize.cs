using System;
using System.Text;

namespace ZapretGui.Core.Log
{
    /// <summary>
    /// Перевод технических ошибок (OS/HTTP/Rust) на понятный русский — порт
    /// human.rs целиком. Текст дословный, порядок проверок сохранён.
    /// </summary>
    public static class Humanize
    {
        /// <summary>Максимальная длина сырой строки, которую показываем пользователю.</summary>
        private const int RawLimit = 220;

        public static string HumanError(string raw)
        {
            string s = (raw ?? string.Empty).Trim();
            if (s.Length == 0)
            {
                return "неизвестная ошибка (подробности в журнале)";
            }
            string l = s.ToLowerInvariant();

            // Уже человеческое сообщение (наше, на русском) — не трогаем.
            bool hasCyr = HasCyrillic(s);
            if (hasCyr && !LooksTechnical(s))
            {
                return s;
            }

            string hit = null;
            if (l.Contains("os error 5") || l.Contains("access is denied") ||
                l.Contains("отказано в доступе") || l.Contains("administrator") || l.Contains("admin_required"))
            {
                hit = "Windows запросит права администратора для запуска обхода — включите «Всегде запускать программу от администратора» в «Настройках»";
            }
            else if (l.Contains("os error 32") || l.Contains("being used by another process"))
            {
                hit = "файл занят другой программой — закройте её и повторите";
            }
            else if (l.Contains("os error 112") || l.Contains("not enough space"))
            {
                hit = "на диске не хватает места";
            }
            else if (l.Contains("os error 2") || l.Contains("os error 3") || l.Contains("cannot find") || l.Contains("не удается найти"))
            {
                hit = "файл или папка не найдены — возможно, движок ещё не установлен";
            }
            else if (l.Contains("error sending request") || l.Contains("error trying to connect") ||
                     l.Contains("dns error") || l.Contains("timed out") || l.Contains("connection refused") ||
                     l.Contains("connection reset") || l.Contains("network is unreachable"))
            {
                hit = "нет связи с сервером — проверьте интернет (или выключите VPN) и повторите";
            }
            else if (l.Contains("http 403") || l.Contains(" 403"))
            {
                hit = "сервер отклонил запрос (403) — возможно, исчерпан лимит обращений к GitHub, попробуйте позже";
            }
            else if (l.Contains("http 404") || l.Contains(" 404"))
            {
                hit = "на сервере нет такого файла (404) — обновите программу";
            }
            else if (l.Contains("http 5"))
            {
                hit = "сервер временно недоступен (ошибка 5xx) — попробуйте позже";
            }
            else if (l.Contains("invalid args") || l.Contains("expected u16") || l.Contains("invalid type") || l.Contains("invalid value"))
            {
                hit = "недопустимое значение поля — проверьте введённые числа";
            }
            else if (l.Contains("process exited immediately") || l.Contains("сразу завершился"))
            {
                hit = "движок сразу завершился — подробности в «Журнале»";
            }
            else if (l.Contains("launch_error") || l.Contains("не удалось запустить процесс"))
            {
                hit = "не удалось запустить процесс — возможно, запрос прав администратора отклонён";
            }
            else if (l.Contains("panic") || l.Contains("panicked"))
            {
                hit = "внутренняя ошибка программы — подробности в «Журнале»";
            }
            else if (l.Contains("no such file") || l.Contains("not found"))
            {
                hit = "файл не найден — проверьте, что движок установлен";
            }

            if (hit != null)
            {
                return hit;
            }
            if (hasCyr)
            {
                return s;
            }
            return "непредвиденная ошибка: " + Cut(s) + " (подробности в «Журнале»)";
        }

        /// <summary>Ошибка с контекстом действия: «что делали: понятный текст».</summary>
        public static string WithContext(string context, string raw)
        {
            return context + ": " + HumanError(raw);
        }

        private static string Cut(string s)
        {
            if (s.Length <= RawLimit)
            {
                return s;
            }
            return s.Substring(0, RawLimit) + "…";
        }

        private static bool HasCyrillic(string s)
        {
            foreach (char c in s)
            {
                char lower = char.ToLowerInvariant(c);
                if (lower >= 'а' && lower <= 'я')
                {
                    return true;
                }
            }
            return false;
        }

        private static bool LooksTechnical(string s)
        {
            string l = s.ToLowerInvariant();
            return l.Contains("os error") || l.Contains("error") || l.Contains("failed") ||
                   l.Contains("denied") || l.Contains("not found") || l.Contains("timed out") ||
                   l.Contains("timeout") || l.Contains("http ") || l.Contains("panic") ||
                   l.Contains("invalid") || l.Contains("unexpected") || l.Contains("cannot") ||
                   l.Contains("unable") || l.Contains("connection") || l.Contains("refused") ||
                   l.Contains("reset by peer") || l.Contains("0x") || l.Contains("exception");
        }
    }
}
