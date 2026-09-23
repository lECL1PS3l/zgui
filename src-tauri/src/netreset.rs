//! «Восстановить интернет» — безопасный сброс сетевых настроек Windows после
//! сбоев zapret/VPN/прокси. Пароли Wi-Fi, профили подключения и настройки
//! провайдера НЕ трогаются (в отличие от системного «Сброса сети»).
//!
//! Безопасный набор: winhttp/системный прокси, драйверы WinDivert, служба zapret,
//! VPN-службы (AmneziaVPN и др.), flush DNS, winsock/int ip reset.
//! `route -f` и `ipconfig /release` НЕ выполняются автоматически — они могут
//! разорвать сеть (см. комментарии ниже), поэтому вынесены за пределы кнопки.

use serde::Serialize;
use std::path::Path;

use crate::runner::{run_script_privileged, write_ps1};

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetResetResult {
    /// Выполненные шаги (человеко-читаемо).
    pub steps: Vec<String>,
    /// Требуется ли перезагрузка (после winsock/int ip reset — обязательно).
    pub reboot_required: bool,
    /// Ошибки, если были (не фатальные — продолжаем).
    pub errors: Vec<String>,
    /// Найденные виртуальные адаптеры (только для информации, не удаляются).
    pub virtual_adapters: Vec<String>,
}

/// Создаёт точку восстановления Windows. Возвращает Ok, если точка создана
/// (или уже была создана за последние 24 часа — тогда повторно не делаем).
/// Checkpoint-Computer работает не всегда (может быть отключено политикой) —
/// вызывающая сторона решает, продолжать ли сброс.
pub fn create_restore_point(data: &Path) -> Result<String, String> {
    let stamp = data.join("logs").join("net-reset-restore.stamp");
    // Не чаще раза в сутки.
    if let Ok(meta) = std::fs::metadata(&stamp) {
        if let Ok(modified) = meta.modified() {
            if let Ok(age) = modified.elapsed() {
                if age.as_secs() < 24 * 3600 {
                    return Ok("точка восстановления уже создавалась за последние 24 ч".into());
                }
            }
        }
    }

    let script = data
        .join("logs")
        .join(format!("restore_point_{}.ps1", std::process::id()));
    let body = format!(
        r#"{header}
$ErrorActionPreference = 'Continue'
# Включаем защиту системы, если отключена (иначе Checkpoint не сработает).
try {{ Enable-ComputerRestore -Drive "$env:SystemDrive\" -ErrorAction SilentlyContinue }} catch {{}}
try {{
  Checkpoint-Computer -Description "Zapret GUI: перед сбросом сети" -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop
  exit 0
}} catch {{
  exit 7
}}
"#,
        header = crate::runner::PS_HEADER,
    );
    write_ps1(&script, &body)?;
    let code = run_script_privileged(&script);
    let _ = std::fs::remove_file(&script);
    match code {
        Ok(0) => {
            let _ = std::fs::write(&stamp, crate::profiles::now_str());
            Ok("точка восстановления создана".into())
        }
        Ok(7) => Err("не удалось создать точку восстановления (возможно, отключена Защита системы)".into()),
        Ok(c) => Err(format!("создание точки восстановления завершилось с кодом {}", c)),
        Err(e) => Err(format!("не удалось запустить создание точки (подтверждение прав отклонено?): {}", e)),
    }
}

/// Собирает список виртуальных сетевых адаптеров (VPN/туннели/TAP/Wintun).
/// Только перечисление — удаление делает пользователь вручную.
pub fn list_virtual_adapters() -> Vec<String> {
    let out = crate::runner::hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Get-NetAdapter | Where-Object { $_.InterfaceDescription -match 'Wintun|TAP-Windows|TAP|WireGuard|OpenVPN|Amnezia|Radmin|Hyper-V|Virtual|VPN' } | Select-Object -ExpandProperty InterfaceDescription",
        ])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Выполняет безопасный сброс сети (с UAC). Возвращает отчёт.
pub fn reset(data: &Path) -> Result<NetResetResult, String> {
    let script = data
        .join("logs")
        .join(format!("net_reset_{}.ps1", std::process::id()));

    let mut body = String::new();
    body.push_str(&format!("{}\n", crate::runner::PS_HEADER));
    // Не падаем на отдельных командах (некоторые могут вернуть ненулевой код).
    body.push_str("$ErrorActionPreference = 'Continue'\n\n");

    // 1. Наша/чужая служба zapret.
    body.push_str(
        "Write-Output 'STEP:остановка службы zapret'\n\
         Stop-Service -Name 'zapret' -Force -ErrorAction SilentlyContinue\n\
         sc.exe stop zapret 2>$null | Out-Null\n\
         sc.exe delete zapret 2>$null | Out-Null\n\n",
    );

    // 2. VPN-службы (включая AmneziaVPN): сначала стоп, чтобы не возрождали процессы.
    body.push_str(
        "Write-Output 'STEP:остановка VPN-служб'\n\
         $vpnServices = @('AmneziaVPN-service','AmneziaVPN','AmneziaWG','amneziawg','WireGuardTunnel','OpenVPNService','OpenVPNServiceInteractive','Happ','Nekoray','ClashVerge','sing-box')\n\
         foreach ($s in $vpnServices) {\n\
           $svc = Get-Service -Name $s -ErrorAction SilentlyContinue\n\
           if ($svc) { Stop-Service -Name $s -Force -ErrorAction SilentlyContinue; sc.exe stop $s 2>$null | Out-Null }\n\
         }\n\
         Start-Sleep -Seconds 2\n\n",
    );

    // 3. Процессы VPN и чужие winws (не наши — наш процесс к этому моменту остановлен GUI).
    body.push_str(
        "Write-Output 'STEP:завершение процессов VPN/zapret'\n\
         $names = @('winws','winws2','goodbyedpi','dpibreak','AmneziaVPN-service','AmneziaVPN','amneziawg','wg','wireguard','openvpn','openvpn-gui','sing-box','xray','v2ray','nekoray','clash','mihomo','happ','hiddify','tun2socks')\n\
         foreach ($n in $names) {\n\
           Get-Process -Name $n -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue\n\
         }\n\n",
    );

    // 4. Драйверы перехвата WinDivert (могут остаться висеть после сбоя).
    body.push_str(
        "Write-Output 'STEP:сброс драйверов WinDivert'\n\
         foreach ($d in @('WinDivert','WinDivert14','WinDivert1.4')) {\n\
           sc.exe stop $d 2>$null | Out-Null\n\
           sc.exe delete $d 2>$null | Out-Null\n\
         }\n\n",
    );

    // 5. Прокси: WinHTTP + системный (реестр).
    body.push_str(
        "Write-Output 'STEP:сброс прокси'\n\
         netsh winhttp reset proxy | Out-Null\n\
         $key = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'\n\
         Set-ItemProperty -Path $key -Name ProxyEnable -Value 0 -Type DWord -ErrorAction SilentlyContinue\n\
         Remove-ItemProperty -Path $key -Name ProxyServer -ErrorAction SilentlyContinue\n\
         Remove-ItemProperty -Path $key -Name AutoConfigURL -ErrorAction SilentlyContinue\n\
         Remove-ItemProperty -Path $key -Name AutoDetect -ErrorAction SilentlyContinue\n\n",
    );

    // 6. Кэш DNS.
    body.push_str(
        "Write-Output 'STEP:очистка кэша DNS'\n\
         ipconfig /flushdns | Out-Null\n\n",
    );

    // 7. Сброс Winsock и TCP/IP (вступает в силу ПОСЛЕ перезагрузки).
    body.push_str(
        "Write-Output 'STEP:сброс стека Winsock/TCP-IP'\n\
         netsh winsock reset | Out-Null\n\
         netsh int ip reset | Out-Null\n\n",
    );

    body.push_str("Write-Output 'DONE'\nexit 0\n");

    write_ps1(&script, &body)?;
    let code = run_script_privileged(&script);
    let _ = std::fs::remove_file(&script);
    code?;

    let virtual_adapters = list_virtual_adapters();
    Ok(NetResetResult {
        steps: vec![
            "служба zapret остановлена и удалена".into(),
            "VPN-службы остановлены (включая AmneziaVPN)".into(),
            "процессы VPN и чужие winws завершены".into(),
            "драйверы WinDivert сброшены".into(),
            "WinHTTP и системный прокси сброшены".into(),
            "кэш DNS очищен".into(),
            "Winsock и TCP/IP сброшены".into(),
        ],
        reboot_required: true,
        errors: Vec::new(),
        virtual_adapters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_default_has_no_steps() {
        let r = NetResetResult::default();
        assert!(r.steps.is_empty());
        assert!(!r.reboot_required);
    }
}
