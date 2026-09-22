using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.Reflection;
using System.Text;

namespace ZapretGui.Core.Util
{
    /// <summary>
    /// Задаёт camelCase-имя поля при сериализации/десериализации — эквивалент
    /// serde #[serde(rename_all = "camelCase")] для структур Rust (config.rs).
    /// </summary>
    [AttributeUsage(AttributeTargets.Field | AttributeTargets.Property)]
    public sealed class JsonFieldAttribute : Attribute
    {
        public string Name { get; }

        public JsonFieldAttribute(string name)
        {
            Name = name;
        }
    }

    /// <summary>
    /// Компактный JSON для DTO конфигурации/состояния/кэшей. Свой сериализатор:
    /// циклы по полям типа, без словарей и без эскейпа кириллицы (как serde_json
    /// с default settings). Без внешних зависимостей (net48).
    /// </summary>
    public static class Json
    {
        public static string Serialize(object value)
        {
            var sb = new StringBuilder();
            WriteValue(sb, value);
            return sb.ToString();
        }

        public static T Parse<T>(string text)
        {
            object parsed;
            int end;
            if (!TryParseValue(text, 0, out parsed, out end))
            {
                throw new FormatException("неверный JSON");
            }
            return (T)Bind(typeof(T), parsed);
        }

        public static bool TryParse<T>(string text, out T value)
        {
            object parsed;
            int end;
            if (string.IsNullOrEmpty(text) || !TryParseValue(text, 0, out parsed, out end))
            {
                value = default(T);
                return false;
            }
            try
            {
                value = (T)Bind(typeof(T), parsed);
                return true;
            }
            catch
            {
                value = default(T);
                return false;
            }
        }

        // ---------------- запись ----------------

        private static void WriteValue(StringBuilder sb, object value)
        {
            if (value == null)
            {
                sb.Append("null");
                return;
            }
            Type t = value.GetType();
            if (t == typeof(string))
            {
                WriteString(sb, (string)value);
                return;
            }
            if (t == typeof(bool))
            {
                sb.Append((bool)value ? "true" : "false");
                return;
            }
            if (t == typeof(char))
            {
                WriteString(sb, value.ToString());
                return;
            }
            if (t.IsEnum)
            {
                WriteString(sb, value.ToString());
                return;
            }
            if (value is IConvertible && (t.IsPrimitive || t == typeof(decimal) || t == typeof(DateTime) || t == typeof(DateTimeOffset)))
            {
                if (value is double d)
                {
                    sb.Append(d.ToString("R", CultureInfo.InvariantCulture));
                }
                else if (value is float f)
                {
                    sb.Append(f.ToString("R", CultureInfo.InvariantCulture));
                }
                else if (value is DateTime dt)
                {
                    WriteString(sb, dt.ToString("o", CultureInfo.InvariantCulture));
                }
                else if (value is DateTimeOffset dto)
                {
                    WriteString(sb, dto.ToString("o", CultureInfo.InvariantCulture));
                }
                else
                {
                    sb.Append(((IConvertible)value).ToString(CultureInfo.InvariantCulture));
                }
                return;
            }
            if (value is IDictionary dict)
            {
                sb.Append('{');
                bool first = true;
                foreach (DictionaryEntry e in dict)
                {
                    if (!first)
                    {
                        sb.Append(',');
                    }
                    first = false;
                    WriteString(sb, e.Key.ToString());
                    sb.Append(':');
                    WriteValue(sb, e.Value);
                }
                sb.Append('}');
                return;
            }
            if (value is IEnumerable seq)
            {
                sb.Append('[');
                bool first = true;
                foreach (object item in seq)
                {
                    if (!first)
                    {
                        sb.Append(',');
                    }
                    first = false;
                    WriteValue(sb, item);
                }
                sb.Append(']');
                return;
            }
            WriteObject(sb, value, t);
        }

        private static void WriteObject(StringBuilder sb, object value, Type t)
        {
            sb.Append('{');
            bool first = true;
            foreach (MemberInfo m in GetMembers(t))
            {
                object fieldValue = GetValue(m, value);
                if (!first)
                {
                    sb.Append(',');
                }
                first = false;
                WriteString(sb, GetJsonName(m));
                sb.Append(':');
                WriteValue(sb, fieldValue);
            }
            sb.Append('}');
        }

        private static void WriteString(StringBuilder sb, string s)
        {
            sb.Append('"');
            foreach (char c in s)
            {
                switch (c)
                {
                    case '"': sb.Append("\\\""); break;
                    case '\\': sb.Append("\\\\"); break;
                    case '\b': sb.Append("\\b"); break;
                    case '\f': sb.Append("\\f"); break;
                    case '\n': sb.Append("\\n"); break;
                    case '\r': sb.Append("\\r"); break;
                    case '\t': sb.Append("\\t"); break;
                    default:
                        if (c < 0x20)
                        {
                            sb.Append("\\u");
                            sb.Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else
                        {
                            sb.Append(c); // кириллицу не эскейпим
                        }
                        break;
                }
            }
            sb.Append('"');
        }

        // ---------------- чтение ----------------

        private static bool TryParseValue(string text, int start, out object value, out int end)
        {
            int i = SkipWhitespace(text, start);
            if (i >= text.Length)
            {
                value = null;
                end = i;
                return false;
            }
            char c = text[i];
            if (c == '{')
            {
                return TryParseObject(text, i, out value, out end);
            }
            if (c == '[')
            {
                return TryParseArray(text, i, out value, out end);
            }
            if (c == '"')
            {
                string s;
                int j = TryParseString(text, i, out s);
                if (j < 0)
                {
                    value = null;
                    end = i;
                    return false;
                }
                value = s;
                end = j;
                return true;
            }
            if (LiteralAt(text, i, "true"))
            {
                value = true;
                end = i + 4;
                return true;
            }
            if (LiteralAt(text, i, "false"))
            {
                value = false;
                end = i + 5;
                return true;
            }
            if (LiteralAt(text, i, "null"))
            {
                value = null;
                end = i + 4;
                return true;
            }
            return TryParseNumber(text, i, out value, out end);
        }

        private static bool LiteralAt(string text, int i, string literal)
        {
            return string.Compare(text, i, literal, 0, literal.Length, StringComparison.Ordinal) == 0;
        }

        private static bool TryParseObject(string text, int start, out object value, out int end)
        {
            var dict = new Dictionary<string, object>(StringComparer.Ordinal);
            int i = start + 1;
            i = SkipWhitespace(text, i);
            if (i < text.Length && text[i] == '}')
            {
                value = dict;
                end = i + 1;
                return true;
            }
            while (i < text.Length)
            {
                i = SkipWhitespace(text, i);
                if (i >= text.Length || text[i] != '"')
                {
                    break;
                }
                string key;
                int j = TryParseString(text, i, out key);
                if (j < 0)
                {
                    break;
                }
                i = SkipWhitespace(text, j);
                if (i >= text.Length || text[i] != ':')
                {
                    break;
                }
                i++;
                object item;
                if (!TryParseValue(text, i, out item, out j))
                {
                    break;
                }
                dict[key] = item;
                i = SkipWhitespace(text, j);
                if (i < text.Length && text[i] == ',')
                {
                    i++;
                    continue;
                }
                if (i < text.Length && text[i] == '}')
                {
                    value = dict;
                    end = i + 1;
                    return true;
                }
                break;
            }
            value = null;
            end = start;
            return false;
        }

        private static bool TryParseArray(string text, int start, out object value, out int end)
        {
            var list = new List<object>();
            int i = start + 1;
            i = SkipWhitespace(text, i);
            if (i < text.Length && text[i] == ']')
            {
                value = list;
                end = i + 1;
                return true;
            }
            while (i < text.Length)
            {
                object item;
                int j;
                if (!TryParseValue(text, i, out item, out j))
                {
                    break;
                }
                list.Add(item);
                i = SkipWhitespace(text, j);
                if (i < text.Length && text[i] == ',')
                {
                    i++;
                    continue;
                }
                if (i < text.Length && text[i] == ']')
                {
                    value = list;
                    end = i + 1;
                    return true;
                }
                break;
            }
            value = null;
            end = start;
            return false;
        }

        private static int TryParseString(string text, int start, out string value)
        {
            var sb = new StringBuilder();
            int i = start + 1;
            while (i < text.Length)
            {
                char c = text[i];
                if (c == '"')
                {
                    value = sb.ToString();
                    return i + 1;
                }
                if (c == '\\')
                {
                    i++;
                    if (i >= text.Length)
                    {
                        break;
                    }
                    char e = text[i];
                    switch (e)
                    {
                        case '"': sb.Append('"'); break;
                        case '\\': sb.Append('\\'); break;
                        case '/': sb.Append('/'); break;
                        case 'b': sb.Append('\b'); break;
                        case 'f': sb.Append('\f'); break;
                        case 'n': sb.Append('\n'); break;
                        case 'r': sb.Append('\r'); break;
                        case 't': sb.Append('\t'); break;
                        case 'u':
                            if (i + 4 < text.Length)
                            {
                                string hex = text.Substring(i + 1, 4);
                                int code;
                                if (int.TryParse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out code))
                                {
                                    sb.Append((char)code);
                                }
                                i += 4;
                            }
                            break;
                        default: sb.Append(e); break;
                    }
                    i++;
                    continue;
                }
                sb.Append(c);
                i++;
            }
            value = null;
            return -1;
        }

        private static bool TryParseNumber(string text, int start, out object value, out int end)
        {
            int i = start;
            if (i < text.Length && (text[i] == '-' || text[i] == '+'))
            {
                i++;
            }
            int digits = 0;
            bool floating = false;
            while (i < text.Length)
            {
                char c = text[i];
                if (c >= '0' && c <= '9')
                {
                    digits++;
                }
                else if (c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-')
                {
                    floating = true;
                }
                else
                {
                    break;
                }
                i++;
            }
            if (digits == 0)
            {
                value = null;
                end = start;
                return false;
            }
            string num = text.Substring(start, i - start);
            if (floating)
            {
                double d;
                if (!double.TryParse(num, NumberStyles.Float, CultureInfo.InvariantCulture, out d))
                {
                    value = null;
                    end = start;
                    return false;
                }
                value = d;
            }
            else
            {
                long l;
                if (!long.TryParse(num, NumberStyles.Integer, CultureInfo.InvariantCulture, out l))
                {
                    value = null;
                    end = start;
                    return false;
                }
                value = l;
            }
            end = i;
            return true;
        }

        private static int SkipWhitespace(string text, int i)
        {
            while (i < text.Length)
            {
                char c = text[i];
                if (c != ' ' && c != '\t' && c != '\n' && c != '\r')
                {
                    break;
                }
                i++;
            }
            return i;
        }

        // ---------------- привязка к DTO ----------------

        private static object Bind(Type type, object parsed)
        {
            if (parsed == null)
            {
                return type.IsValueType ? Activator.CreateInstance(type) : null;
            }
            if (type == typeof(object))
            {
                return parsed;
            }
            Type parsedType = parsed.GetType();
            if (type.IsAssignableFrom(parsedType))
            {
                return parsed;
            }
            if (parsed is string s)
            {
                if (type == typeof(string))
                {
                    return s;
                }
                if (type.IsEnum)
                {
                    return Enum.Parse(type, s, true);
                }
                return Convert.ChangeType(s, Nullable.GetUnderlyingType(type) ?? type, CultureInfo.InvariantCulture);
            }
            if (parsed is bool b)
            {
                return Convert.ChangeType(b, Nullable.GetUnderlyingType(type) ?? type, CultureInfo.InvariantCulture);
            }
            if (parsed is double d)
            {
                return Convert.ChangeType(d, Nullable.GetUnderlyingType(type) ?? type, CultureInfo.InvariantCulture);
            }
            if (parsed is long l)
            {
                Type ut = Nullable.GetUnderlyingType(type) ?? type;
                if (ut == typeof(long) || ut == typeof(ulong) || ut == typeof(double) || ut == typeof(float) || ut == typeof(decimal))
                {
                    return Convert.ChangeType(l, ut, CultureInfo.InvariantCulture);
                }
                return Convert.ChangeType(l, ut, CultureInfo.InvariantCulture);
            }
            if (parsed is IList list)
            {
                if (type == typeof(string))
                {
                    return string.Join(",", list);
                }
                Type elemType = type.GetElementType();
                if (elemType == null && type.IsGenericType)
                {
                    elemType = type.GetGenericArguments()[0];
                }
                if (elemType == null)
                {
                    return parsed;
                }
                var result = (IList)Activator.CreateInstance(type);
                foreach (object item in list)
                {
                    result.Add(elemType.IsAssignableFrom(item.GetType()) ? item : Bind(elemType, item));
                }
                return result;
            }
            if (parsed is IDictionary dict)
            {
                object target = Activator.CreateInstance(type);
                foreach (MemberInfo m in GetMembers(type))
                {
                    object raw;
                    if (!dict.Contains(GetJsonName(m)))
                    {
                        continue;
                    }
                    raw = dict[GetJsonName(m)];
                    Type mt = GetMemberType(m);
                    object bound = raw == null ? GetDefault(mt) : Bind(mt, raw);
                    SetValue(m, target, bound);
                }
                return target;
            }
            return parsed;
        }

        private static object GetDefault(Type type)
        {
            return type.IsValueType ? Activator.CreateInstance(type) : null;
        }

        // ---------------- отражение членов DTO ----------------

        private static readonly Dictionary<Type, List<MemberInfo>> _members = new Dictionary<Type, List<MemberInfo>>();

        private static List<MemberInfo> GetMembers(Type type)
        {
            lock (_members)
            {
                List<MemberInfo> list;
                if (_members.TryGetValue(type, out list))
                {
                    return list;
                }
                list = new List<MemberInfo>();
                foreach (FieldInfo f in type.GetFields(BindingFlags.Public | BindingFlags.Instance))
                {
                    list.Add(f);
                }
                foreach (PropertyInfo p in type.GetProperties(BindingFlags.Public | BindingFlags.Instance))
                {
                    if (p.GetIndexParameters().Length == 0)
                    {
                        list.Add(p);
                    }
                }
                _members[type] = list;
                return list;
            }
        }

        private static string GetJsonName(MemberInfo m)
        {
            var attr = (JsonFieldAttribute)Attribute.GetCustomAttribute(m, typeof(JsonFieldAttribute));
            if (attr != null)
            {
                return attr.Name;
            }
            string name = m.Name;
            if (name.Length > 1 && char.IsUpper(name[0]) && !char.IsUpper(name[1]))
            {
                return char.ToLowerInvariant(name[0]) + name.Substring(1);
            }
            return name;
        }

        private static Type GetMemberType(MemberInfo m)
        {
            if (m is FieldInfo f)
            {
                return f.FieldType;
            }
            return ((PropertyInfo)m).PropertyType;
        }

        private static object GetValue(MemberInfo m, object target)
        {
            if (m is FieldInfo f)
            {
                return f.GetValue(target);
            }
            return ((PropertyInfo)m).GetValue(target, null);
        }

        private static void SetValue(MemberInfo m, object target, object value)
        {
            if (m is FieldInfo f)
            {
                f.SetValue(target, value);
                return;
            }
            ((PropertyInfo)m).SetValue(target, value, null);
        }
    }
}
