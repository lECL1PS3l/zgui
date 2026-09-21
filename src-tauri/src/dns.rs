use crate::runner::{run_elevated_script, write_ps1};
use serde::Serialize;
use std::net::ToSocketAddrs;
use std::path::Path;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DnsProvider {
    pub id: &'static str,
    pub name: &'static str,
    pub primary: &'static str,
    pub secondary: &'static str,
    pub doh_template: &'static str,
    /// Короткая подпись рядом с названием.
    pub note: &'static str,
    /// Подробное описание (показывается в раскрывающемся блоке).
    pub description: &'static str,
}

const PROVIDERS: &[DnsProvider] = &[
    DnsProvider {
        id: "google",
        name: "Google Public DNS",
        primary: "8.8.8.8",
        secondary: "8.8.4.4",
        doh_template: "https://dns.google/dns-query",
        note: "крупный Anycast",
        description: "Публичный DNS Google. Очень стабильный и быстрый, без фильтрации. \
                      Минус — принадлежит Google (логи). Хороший выбор по умолчанию.",
    },
    DnsProvider {
        id: "cloudflare",
        name: "Cloudflare",
        primary: "1.1.1.1",
        secondary: "1.0.0.1",
        doh_template: "https://cloudflare-dns.com/dns-query",
        note: "быстрый, минимум логов",
        description: "Заявлен как самый быстрый публичный DNS, логов почти не ведёт. \
                      Без фильтрации. Хорош для скорости и приватности.",
    },
    DnsProvider {
        id: "adguard",
        name: "AdGuard DNS",
        primary: "94.140.14.14",
        secondary: "94.140.15.15",
        doh_template: "https://dns.adguard-dns.com/dns-query",
        note: "блокирует рекламу и трекеры",
        description: "Блокирует рекламу, трекеры и часть вредоносных доменов на уровне DNS. \
                      Может резать и нужные сайты — если что-то не открывается, проверьте здесь.",
    },
    DnsProvider {
        id: "xbox-dns",
        name: "XBox DNS",
        primary: "111.88.96.50",
        secondary: "111.88.96.51",
        doh_template: "https://xbox-dns.ru/dns-query",
        note: "Smart DNS: Xbox Live, ИИ, игры",
        description: "Smart DNS для доступа к сервисам с региональными ограничениями: \
                      Xbox Live (ошибка 0x80a40401), игры Supercell (Brawl Stars, Clash of Clans), \
                      ChatGPT/Gemini/Claude/Copilot, JetBrains/GitHub/Notion, Twitch/Spotify/xCloud. \
                      Обычный трафик идёт напрямую, только гео-ограниченные — через шлюз. \
                      Без регистрации, заявлены DoH/DoT и Zero-Log.",
    },
    DnsProvider {
        id: "quad9",
        name: "Quad9",
        primary: "9.9.9.9",
        secondary: "149.112.112.112",
        doh_template: "https://dns.quad9.net/dns-query",
        note: "защита от вредоносных доменов",
        description: "Блокирует вредоносные и фишинговые домены (по базе угроз). \
                      Без рекламных блокировок. Некоммерческий, ориентирован на безопасность.",
    },
    DnsProvider {
        id: "opendns",
        name: "OpenDNS (Cisco)",
        primary: "208.67.222.222",
        secondary: "208.67.220.220",
        doh_template: "https://doh.opendns.com/dns-query",
        note: "стабильный, есть фильтры Cisco",
        description: "DNS от Cisco. Стабильный, с опциональной фильтрацией (по умолчанию — \
                      базовая защита от фишинга). Хорош как надёжный резерв.",
    },
    DnsProvider {
        id: "dns-sb",
        name: "DNS.SB",
        primary: "185.222.222.222",
        secondary: "45.11.45.11",
        doh_template: "https://doh.dns.sb/dns-query",
        note: "независимый, без логов",
        description: "Независимый публичный DNS, без логов и фильтрации. Хорошая альтернатива \
                      большим корпоративным резолверам.",
    },
    DnsProvider {
        id: "mullvad",
        name: "Mullvad DNS",
        primary: "194.242.2.2",
        secondary: "194.242.2.3",
        doh_template: "https://doh.mullvad.net/dns-query",
        note: "приватный, без логов",
        description: "DNS от Mullvad, известных своим отношением к приватности: \
                      без логов, без фильтрации. Доступен без VPN-подписки.",
    },
];

pub fn providers() -> &'static [DnsProvider] {
    PROVIDERS
}

pub fn provider(id: &str) -> Option<&'static DnsProvider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

// ------------------------------------------------------------- benchmark

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DnsPing {
    pub id: String,
    pub primary_ms: Option<u64>,
    pub secondary_ms: Option<u64>,
    /// Среднее по обоим адресам (для сортировки/сравнения).
    pub avg_ms: Option<u64>,
    pub error: Option<String>,
}

/// Строит минимальный DNS-запрос (A-запись) для домена.
fn build_dns_query(domain: &str) -> Vec<u8> {
    let mut q = Vec::new();
    q.extend_from_slice(&[0x12, 0x34]); // ID
    q.extend_from_slice(&[0x01, 0x00]); // flags: standard query, RD
    q.extend_from_slice(&[0x00, 0x01]); // QDCOUNT
    q.extend_from_slice(&[0x00, 0x00]); // ANCOUNT
    q.extend_from_slice(&[0x00, 0x00]); // NSCOUNT
    q.extend_from_slice(&[0x00, 0x00]); // ARCOUNT
    for label in domain.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&[0x00, 0x01]); // QTYPE A
    q.extend_from_slice(&[0x00, 0x01]); // QCLASS IN
    q
}

/// Один UDP DNS-запрос: возвращает время отклика в мс (или None).
fn query_once(server: &str, domain: &str, timeout: std::time::Duration) -> Option<u64> {
    use std::net::UdpSocket;
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(timeout)).ok()?;
    let addr = (server, 53u16).to_socket_addrs().ok()?.next()?;
    let packet = build_dns_query(domain);
    let start = std::time::Instant::now();
    sock.send_to(&packet, addr).ok()?;
    let mut buf = [0u8; 512];
    let (n, _) = sock.recv_from(&mut buf).ok()?;
    if n < 12 {
        return None;
    }
    Some(start.elapsed().as_millis() as u64)
}

/// Медиана из нескольких замеров (устойчивее среднего при выбросах).
fn median(mut v: Vec<u64>) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

fn probe_addr(server: &str) -> Option<u64> {
    let mut samples = Vec::new();
    for _ in 0..3 {
        if let Some(ms) = query_once(server, "example.com", std::time::Duration::from_millis(1500)) {
            samples.push(ms);
        }
    }
    median(samples)
}

/// Замеряет время отклика DNS-серверов по всем провайдерам.
pub fn benchmark(ids: Option<Vec<String>>) -> Vec<DnsPing> {
    let selected: Vec<&DnsProvider> = match &ids {
        Some(list) if !list.is_empty() => PROVIDERS.iter().filter(|p| list.contains(&p.id.to_string())).collect(),
        _ => PROVIDERS.iter().collect(),
    };
    selected
        .into_iter()
        .map(|p| {
            let primary_ms = probe_addr(p.primary);
            let secondary_ms = probe_addr(p.secondary);
            let avg_ms = match (primary_ms, secondary_ms) {
                (Some(a), Some(b)) => Some((a + b) / 2),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                _ => None,
            };
            DnsPing {
                id: p.id.to_string(),
                primary_ms,
                secondary_ms,
                avg_ms,
                error: if primary_ms.is_none() && secondary_ms.is_none() {
                    Some("нет ответа (UDP 53 закрыт/фильтруется)".into())
                } else {
                    None
                },
            }
        })
        .collect()
}

pub fn apply(data: &Path, id: &str, adapter: Option<&str>) -> Result<String, String> {
    let p = provider(id).ok_or("неизвестный DNS-провайдер")?;
    let script = data.join("logs").join(format!("dns_apply_{}.ps1", std::process::id()));
    let adapter_expr = match adapter.filter(|s| !s.trim().is_empty()) {
        Some(name) => crate::runner::ps_quote(name),
        None => "(Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.HardwareInterface } | Select-Object -First 1 -ExpandProperty InterfaceAlias)".into(),
    };
    let body = format!(
        r#"{header}
$alias = {adapter}
if (-not $alias) {{ throw 'не найдено активное сетевое подключение' }}
$primary = '{primary}'
$secondary = '{secondary}'
$doh = '{doh}'
& netsh.exe dns add encryption server=$primary dohtemplate=$doh autoupgrade=yes udpfallback=no 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {{
  & netsh.exe dns set encryption server=$primary dohtemplate=$doh autoupgrade=yes udpfallback=no | Out-Null
  if ($LASTEXITCODE -ne 0) {{ throw "не удалось задать профиль DoH для $primary" }}
}}
& netsh.exe dns add encryption server=$secondary dohtemplate=$doh autoupgrade=yes udpfallback=no 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {{
  & netsh.exe dns set encryption server=$secondary dohtemplate=$doh autoupgrade=yes udpfallback=no | Out-Null
  if ($LASTEXITCODE -ne 0) {{ throw "не удалось задать профиль DoH для $secondary" }}
}}
Set-DnsClientServerAddress -InterfaceAlias $alias -ServerAddresses @($primary, $secondary)
Clear-DnsClientCache
Write-Output ("DNS: {name}; adapter: " + $alias)
"#,
        header = crate::runner::PS_HEADER,
        adapter = adapter_expr,
        primary = p.primary,
        secondary = p.secondary,
        doh = p.doh_template,
        name = p.name,
    );
    write_ps1(&script, &body)?;
    let code = run_elevated_script(&script)?;
    let _ = std::fs::remove_file(&script);
    if code != 0 {
        return Err(format!("применение DNS не удалось, код {}", code));
    }
    Ok(format!("{}: {} / {} + DoH (без UDP fallback)", p.name, p.primary, p.secondary))
}

pub fn reset(data: &Path, adapter: Option<&str>) -> Result<String, String> {
    let script = data.join("logs").join(format!("dns_reset_{}.ps1", std::process::id()));
    let adapter_expr = match adapter.filter(|s| !s.trim().is_empty()) {
        Some(name) => crate::runner::ps_quote(name),
        None => "(Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $_.HardwareInterface } | Select-Object -First 1 -ExpandProperty InterfaceAlias)".into(),
    };
    let body = format!(
        r#"{header}
$alias = {adapter}
if (-not $alias) {{ throw 'не найдено активное сетевое подключение' }}
Set-DnsClientServerAddress -InterfaceAlias $alias -ResetServerAddresses
Clear-DnsClientCache
Write-Output ("DNS reset: " + $alias)
"#,
        header = crate::runner::PS_HEADER,
        adapter = adapter_expr,
    );
    write_ps1(&script, &body)?;
    let code = run_elevated_script(&script)?;
    let _ = std::fs::remove_file(&script);
    if code != 0 {
        return Err(format!("сброс DNS не удался, код {}", code));
    }
    Ok("DNS возвращён к автоматическим настройкам".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_have_ipv4_and_doh() {
        assert!(providers().len() >= 8);
        for p in providers() {
            assert!(p.primary.parse::<std::net::Ipv4Addr>().is_ok());
            assert!(p.secondary.parse::<std::net::Ipv4Addr>().is_ok());
            assert!(p.doh_template.starts_with("https://"));
            assert!(!p.description.is_empty());
        }
    }

    #[test]
    fn finds_cloudflare_and_xbox() {
        assert_eq!(provider("cloudflare").unwrap().primary, "1.1.1.1");
        assert!(provider("comss").is_none());
        let x = provider("xbox-dns").unwrap();
        assert_eq!(x.primary, "111.88.96.50");
        assert_eq!(x.secondary, "111.88.96.51");
        assert_eq!(x.doh_template, "https://xbox-dns.ru/dns-query");
        assert!(provider("missing").is_none());
    }

    #[test]
    fn builds_valid_dns_query() {
        let q = build_dns_query("example.com");
        // 12 байт заголовка + 1+7 + 1+3 + 1(TLD-конец) + 4 (QTYPE/QCLASS)
        assert_eq!(q.len(), 12 + 8 + 4 + 1 + 4);
        assert_eq!(&q[12..13], &[7u8]); // длина "example"
        assert_eq!(&q[13..20], b"example");
    }
}
