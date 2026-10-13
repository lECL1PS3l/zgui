use crate::runner::run_script_privileged;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DnsProvider {
    pub id: &'static str,
    pub name: &'static str,
    pub primary: &'static str,
    pub secondary: &'static str,
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
        note: "крупный Anycast",
        description: "Публичный DNS Google. Очень стабильный и быстрый, без фильтрации. \
                      Минус — принадлежит Google (логи). Хороший выбор по умолчанию.",
    },
    DnsProvider {
        id: "cloudflare",
        name: "Cloudflare",
        primary: "1.1.1.1",
        secondary: "1.0.0.1",
        note: "быстрый, минимум логов",
        description: "Заявлен как самый быстрый публичный DNS, логов почти не ведёт. \
                      Без фильтрации. Хорош для скорости и приватности.",
    },
    DnsProvider {
        id: "quad9",
        name: "Quad9",
        primary: "9.9.9.9",
        secondary: "149.112.112.112",
        note: "защита от вредоносных доменов",
        description: "Блокирует вредоносные и фишинговые домены (по базе угроз). \
                      Без рекламных блокировок. Некоммерческий, ориентирован на безопасность.",
    },
];

pub fn providers() -> &'static [DnsProvider] {
    PROVIDERS
}

pub fn provider(id: &str) -> Option<&'static DnsProvider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

fn adapter_expr() -> &'static str {
    "(Get-NetAdapter | Where-Object {{ $_.Status -eq 'Up' -and $_.HardwareInterface }} \
     | Select-Object -First 1 -ExpandProperty InterfaceAlias)"
}

fn apply_script(p: &DnsProvider) -> String {
    format!(
        r#"{header}
$alias = {adapter}
if (-not $alias) {{ throw 'не найдено активное сетевое подключение' }}
Set-DnsClientServerAddress -InterfaceAlias $alias -ServerAddresses @('{primary}', '{secondary}')
Clear-DnsClientCache
Write-Output ("DNS: {name}; adapter: " + $alias)
"#,
        header = crate::runner::PS_HEADER,
        adapter = adapter_expr(),
        primary = p.primary,
        secondary = p.secondary,
        name = p.name,
    )
}

fn reset_script() -> String {
    format!(
        r#"{header}
$alias = {adapter}
if (-not $alias) {{ throw 'не найдено активное сетевое подключение' }}
Set-DnsClientServerAddress -InterfaceAlias $alias -ResetServerAddresses
Clear-DnsClientCache
Write-Output ("DNS reset: " + $alias)
"#,
        header = crate::runner::PS_HEADER,
        adapter = adapter_expr(),
    )
}

pub fn apply(data: &Path, id: &str) -> Result<String, String> {
    let p = provider(id).ok_or(crate::texts::DNS_UNKNOWN_PROVIDER)?;
    let script = crate::runner::LockedScript::write(
        data.join("logs")
            .join(format!("dns_apply_{}.ps1", std::process::id())),
        &apply_script(p),
    )?;
    let code = run_script_privileged(script.path())?;
    if code != 0 {
        return Err(crate::texts::dns_apply_failed_code(code));
    }
    Ok(crate::texts::dns_applied(p.name, p.primary, p.secondary))
}

pub fn reset(data: &Path) -> Result<String, String> {
    let script = crate::runner::LockedScript::write(
        data.join("logs")
            .join(format!("dns_reset_{}.ps1", std::process::id())),
        &reset_script(),
    )?;
    let code = run_script_privileged(script.path())?;
    if code != 0 {
        return Err(crate::texts::dns_reset_failed_code(code));
    }
    Ok(crate::texts::DNS_RESET_OK.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_have_ipv4() {
        assert_eq!(providers().len(), 3);
        for p in providers() {
            assert!(p.primary.parse::<std::net::Ipv4Addr>().is_ok());
            assert!(p.secondary.parse::<std::net::Ipv4Addr>().is_ok());
            assert!(!p.description.is_empty());
            // Никаких обещаний про шифрование: провайдеры держат только адреса.
            let d = p.description.to_lowercase();
            assert!(!d.contains("шифр") && !d.contains("doh"), "текст про шифрование: {d}");
        }
    }

    #[test]
    fn finds_kept_providers() {
        assert_eq!(provider("cloudflare").unwrap().primary, "1.1.1.1");
        assert_eq!(provider("google").unwrap().primary, "8.8.8.8");
        assert_eq!(provider("quad9").unwrap().primary, "9.9.9.9");
        assert!(provider("comss").is_none());
        assert!(provider("missing").is_none());
    }

    /// Скрипты должны разбираться PowerShell: иначе «Включить DNS» упадёт
    /// только на живой машине, с админским окном и непонятным кодом.
    #[test]
    fn scripts_are_valid_powershell() {
        for (name, body) in [
            ("dns-apply", apply_script(provider("cloudflare").unwrap())),
            ("dns-reset", reset_script()),
        ] {
            let path = std::env::temp_dir().join(format!("zgui-{name}-{}.ps1", std::process::id()));
            crate::runner::write_ps1(&path, &body).unwrap();
            let check = format!(
                "$t=$null; $e=$null; [void][System.Management.Automation.Language.Parser]::ParseFile({}, [ref]$t, [ref]$e); if ($e.Count) {{ $e[0].Message; exit 1 }} else {{ exit 0 }}",
                crate::runner::ps_quote(&path.to_string_lossy())
            );
            let out = crate::runner::hidden_command("powershell.exe")
                .args(["-NoProfile", "-Command", &check])
                .output()
                .expect("powershell");
            let _ = std::fs::remove_file(&path);
            assert!(
                out.status.success(),
                "{name} не разбирается PowerShell: {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }

    /// Скрипт только назначает адреса и ничего не обещает сверх этого.
    #[test]
    fn apply_script_only_sets_addresses() {
        let s = apply_script(provider("cloudflare").unwrap());
        assert!(s.contains("Set-DnsClientServerAddress -InterfaceAlias $alias"));
        assert!(s.contains("1.1.1.1") && s.contains("1.0.0.1"));
        for gone in ["dns add encryption", "ddr=", "DohTemplate", "udpfallback"] {
            assert!(!s.contains(gone), "шифрование должно быть вырезано, но есть {gone}");
        }
        let r = reset_script();
        assert!(r.contains("-ResetServerAddresses"));
        assert!(!r.contains("encryption") && !r.contains("ddr="));
    }
}
