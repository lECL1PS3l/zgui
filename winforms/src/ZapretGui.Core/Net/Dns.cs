using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using ZapretGui.Core;
using ZapretGui.Core.Runtime;
using ZapretGui.Core.Util;

namespace ZapretGui.Core.Net
{
    public class DnsProvider
    {
        [JsonField("id")] public string Id;
        [JsonField("name")] public string Name;
        [JsonField("primary")] public string Primary;
        [JsonField("secondary")] public string Secondary;
        [JsonField("dohTemplate")] public string DohTemplate;
        /// <summary>Короткая подпись рядом с названием.</summary>
        [JsonField("note")] public string Note;
        /// <summary>Подробное описание (показывается в раскрывающемся блоке).</summary>
        [JsonField("description")] public string Description;

        public DnsProvider(string id, string name, string primary, string secondary,
            string dohTemplate, string note, string description)
        {
            Id = id;
            Name = name;
            Primary = primary;
            Secondary = secondary;
            DohTemplate = dohTemplate;
            Note = note;
            Description = description;
        }
    }

    public class DnsPing
    {
        [JsonField("id")] public string Id;
        [JsonField("primaryMs")] public long? PrimaryMs;
        [JsonField("secondaryMs")] public long? SecondaryMs;
        /// <summary>Среднее по обоим адресам (для сортировки/сравнения).</summary>
        [JsonField("avgMs")] public long? AvgMs;
        [JsonField("error")] public string Error;
    }

    /// <summary>
    /// DNS-провайдеры, применение/сброс DoH и бенчмарк. Порт dns.rs.
    /// </summary>
    public static class Dns
    {
        public static readonly DnsProvider[] Providers =
        {
            new DnsProvider("google", "Google Public DNS", "8.8.8.8", "8.8.4.4",
                "https://dns.google/dns-query", "крупный Anycast",
                "Публичный DNS Google. Очень стабильный и быстрый, без фильтрации. " +
                "Минус — принадлежит Google (логи). Хороший выбор по умолчанию."),
            new DnsProvider("cloudflare", "Cloudflare", "1.1.1.1", "1.0.0.1",
                "https://cloudflare-dns.com/dns-query", "быстрый, минимум логов",
                "Заявлен как самый быстрый публичный DNS, логов почти не ведёт. " +
                "Без фильтрации. Хорош для скорости и приватности."),
            new DnsProvider("adguard", "AdGuard DNS", "94.140.14.14", "94.140.15.15",
                "https://dns.adguard-dns.com/dns-query", "блокирует рекламу и трекеры",
                "Блокирует рекламу, трекеры и часть вредоносных доменов на уровне DNS. " +
                "Может резать и нужные сайты — если что-то не открывается, проверьте здесь."),
            new DnsProvider("xbox-dns", "XBox DNS", "111.88.96.50", "111.88.96.51",
                "https://xbox-dns.ru/dns-query", "Smart DNS: Xbox Live, ИИ, игры",
                "Smart DNS для доступа к сервисам с региональными ограничениями: " +
                "Xbox Live (ошибка 0x80a40401), игры Supercell (Brawl Stars, Clash of Clans), " +
                "ChatGPT/Gemini/Claude/Copilot, JetBrains/GitHub/Notion, Twitch/Spotify/xCloud. " +
                "Обычный трафик идёт напрямую, только гео-ограниченные — через шлюз. " +
                "Без регистрации, заявлены DoH/DoT и Zero-Log."),
            new DnsProvider("quad9", "Quad9", "9.9.9.9", "149.112.112.112",
                "https://dns.quad9.net/dns-query", "защита от вредоносных доменов",
                "Блокирует вредоносные и фишинговые домены (по базе угроз). " +
                "Без рекламных блокировок. Некоммерческий, ориентирован на безопасность."),
            new DnsProvider("opendns", "OpenDNS (Cisco)", "208.67.222.222", "208.67.220.220",
                "https://doh.opendns.com/dns-query", "стабильный, есть фильтры Cisco",
                "DNS от Cisco. Стабильный, с опциональной фильтрацией (по умолчанию — " +
                "базовая защита от фишинга). Хорош как надёжный резерв."),
            new DnsProvider("dns-sb", "DNS.SB", "185.222.222.222", "45.11.45.11",
                "https://doh.dns.sb/dns-query", "независимый, без логов",
                "Независимый публичный DNS, без логов и фильтрации. Хорошая альтернатива " +
                "большим корпоративным резолверам."),
            new DnsProvider("mullvad", "Mullvad DNS", "194.242.2.2", "194.242.2.3",
                "https://doh.mullvad.net/dns-query", "приватный, без логов",
                "DNS от Mullvad, известных своим отношением к приватности: " +
                "без логов, без фильтрации. Доступен без VPN-подписки."),
        };

        private const string DefaultAdapterExpr =
            "(Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.HardwareInterface } | " +
            "Select-Object -First 1 -ExpandProperty InterfaceAlias)";

        public static DnsProvider Provider(string id)
        {
            foreach (var p in Providers) if (p.Id == id) return p;
            return null;
        }

        /// <summary>Строит минимальный DNS-запрос (A-запись) для домена. dns.rs:128</summary>
        public static byte[] BuildDnsQuery(string domain)
        {
            var q = new List<byte>(32);
            q.Add(0x12); q.Add(0x34); // ID
            q.Add(0x01); q.Add(0x00); // flags: standard query, RD
            q.Add(0x00); q.Add(0x01); // QDCOUNT
            q.Add(0x00); q.Add(0x00); // ANCOUNT
            q.Add(0x00); q.Add(0x00); // NSCOUNT
            q.Add(0x00); q.Add(0x00); // ARCOUNT
            foreach (var label in domain.Split('.'))
            {
                var bytes = Encoding.ASCII.GetBytes(label);
                q.Add((byte)bytes.Length);
                q.AddRange(bytes);
            }
            q.Add(0);
            q.Add(0x00); q.Add(0x01); // QTYPE A
            q.Add(0x00); q.Add(0x01); // QCLASS IN
            return q.ToArray();
        }

        /// <summary>Один UDP DNS-запрос: возвращает время отклика в мс (или null). dns.rs:147</summary>
        public static long? QueryOnce(string server, string domain, int timeoutMs)
        {
            try
            {
                var addresses = System.Net.Dns.GetHostAddresses(server);
                if (addresses.Length == 0) return null;
                var endPoint = new IPEndPoint(addresses[0], 53);
                var packet = BuildDnsQuery(domain);
                using (var sock = new UdpClient(new IPEndPoint(IPAddress.Any, 0)))
                {
                    sock.Client.ReceiveTimeout = timeoutMs;
                    var sw = Stopwatch.StartNew();
                    sock.Send(packet, packet.Length, endPoint);
                    var remoteEP = endPoint;
                    var buf = sock.Receive(ref remoteEP);
                    sw.Stop();
                    if (buf.Length < 12) return null;
                    return sw.ElapsedMilliseconds;
                }
            }
            catch { return null; }
        }

        /// <summary>Медиана из нескольких замеров (устойчивее среднего при выбросах). dns.rs:164</summary>
        public static long? Median(List<long> values)
        {
            if (values == null || values.Count == 0) return null;
            values.Sort();
            return values[values.Count / 2];
        }

        private static long? ProbeAddr(string server)
        {
            var samples = new List<long>();
            for (var i = 0; i < 3; i++)
            {
                var ms = QueryOnce(server, "example.com", 1500);
                if (ms.HasValue) samples.Add(ms.Value);
            }
            return Median(samples);
        }

        /// <summary>Замеряет время отклика DNS-серверов по выбранным провайдерам. dns.rs:183</summary>
        public static List<DnsPing> Benchmark(IList<string> ids)
        {
            var selected = new List<DnsProvider>();
            if (ids != null && ids.Count > 0)
            {
                foreach (var p in Providers) if (ids.Contains(p.Id)) selected.Add(p);
            }
            else
            {
                selected.AddRange(Providers);
            }

            var outList = new List<DnsPing>(selected.Count);
            foreach (var p in selected)
            {
                var primaryMs = ProbeAddr(p.Primary);
                var secondaryMs = ProbeAddr(p.Secondary);
                long? avgMs;
                if (primaryMs.HasValue && secondaryMs.HasValue) avgMs = (primaryMs.Value + secondaryMs.Value) / 2;
                else if (primaryMs.HasValue) avgMs = primaryMs.Value;
                else if (secondaryMs.HasValue) avgMs = secondaryMs.Value;
                else avgMs = null;

                outList.Add(new DnsPing
                {
                    Id = p.Id,
                    PrimaryMs = primaryMs,
                    SecondaryMs = secondaryMs,
                    AvgMs = avgMs,
                    Error = (!primaryMs.HasValue && !secondaryMs.HasValue)
                        ? "нет ответа (UDP 53 закрыт/фильтруется)"
                        : null,
                });
            }
            return outList;
        }

        /// <summary>Применяет DNS-провайдера (DoH через netsh). dns.rs:214</summary>
        public static Result<string> Apply(string dataDir, string id, string adapter)
        {
            var p = Provider(id);
            if (p == null) return Result<string>.Err("неизвестный DNS-провайдер");

            var script = Path.Combine(Path.Combine(dataDir, "logs"),
                "dns_apply_" + Process.GetCurrentProcess().Id + ".ps1");
            var adapterExpr = !string.IsNullOrWhiteSpace(adapter)
                ? Uac.PsQuote(adapter)
                : DefaultAdapterExpr;
            var body = Uac.PsHeader + "\n" +
                "$alias = " + adapterExpr + "\n" +
                "if (-not $alias) { throw 'не найдено активное сетевое подключение' }\n" +
                "$primary = '" + p.Primary + "'\n" +
                "$secondary = '" + p.Secondary + "'\n" +
                "$doh = '" + p.DohTemplate + "'\n" +
                "& netsh.exe dns add encryption server=$primary dohtemplate=$doh autoupgrade=yes udpfallback=no 2>$null | Out-Null\n" +
                "if ($LASTEXITCODE -ne 0) {\n" +
                "  & netsh.exe dns set encryption server=$primary dohtemplate=$doh autoupgrade=yes udpfallback=no | Out-Null\n" +
                "  if ($LASTEXITCODE -ne 0) { throw \"не удалось задать профиль DoH для $primary\" }\n" +
                "}\n" +
                "& netsh.exe dns add encryption server=$secondary dohtemplate=$doh autoupgrade=yes udpfallback=no 2>$null | Out-Null\n" +
                "if ($LASTEXITCODE -ne 0) {\n" +
                "  & netsh.exe dns set encryption server=$secondary dohtemplate=$doh autoupgrade=yes udpfallback=no | Out-Null\n" +
                "  if ($LASTEXITCODE -ne 0) { throw \"не удалось задать профиль DoH для $secondary\" }\n" +
                "}\n" +
                "Set-DnsClientServerAddress -InterfaceAlias $alias -ServerAddresses @($primary, $secondary)\n" +
                "Clear-DnsClientCache\n" +
                "Write-Output (\"DNS: " + p.Name + "; adapter: \" + $alias)\n";

            try { Uac.WritePs1(script, body); }
            catch (Exception e) { return Result<string>.Err(e.Message); }
            var code = Uac.RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            if (code != 0) return Result<string>.Err("применение DNS не удалось, код " + code);
            return Result<string>.Ok(p.Name + ": " + p.Primary + " / " + p.Secondary + " + DoH (без UDP fallback)");
        }

        /// <summary>Сброс DNS к автоматическим настройкам. dns.rs:258</summary>
        public static Result<string> Reset(string dataDir, string adapter)
        {
            var script = Path.Combine(Path.Combine(dataDir, "logs"),
                "dns_reset_" + Process.GetCurrentProcess().Id + ".ps1");
            var adapterExpr = !string.IsNullOrWhiteSpace(adapter)
                ? Uac.PsQuote(adapter)
                : DefaultAdapterExpr;
            var body = Uac.PsHeader + "\n" +
                "$alias = " + adapterExpr + "\n" +
                "if (-not $alias) { throw 'не найдено активное сетевое подключение' }\n" +
                "Set-DnsClientServerAddress -InterfaceAlias $alias -ResetServerAddresses\n" +
                "Clear-DnsClientCache\n" +
                "Write-Output (\"DNS reset: \" + $alias)\n";

            try { Uac.WritePs1(script, body); }
            catch (Exception e) { return Result<string>.Err(e.Message); }
            var code = Uac.RunElevatedScript(script);
            try { File.Delete(script); } catch { }
            if (code != 0) return Result<string>.Err("сброс DNS не удался, код " + code);
            return Result<string>.Ok("DNS возвращён к автоматическим настройкам");
        }
    }
}
