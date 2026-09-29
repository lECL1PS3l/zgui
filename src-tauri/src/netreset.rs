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

use crate::runner::run_script_privileged;

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
    // Шаги пишутся скриптом: раньше список возвращался жёстко зашитым, и UI
    // рапортовал «VPN-службы остановлены», даже если таких служб не было.
    let steps_file = crate::runner::TempFile::new(
        data.join("logs").join(format!("net_reset_{}.steps.txt", std::process::id())),
    );

    let body = reset_script(steps_file.path());
    let script = crate::runner::LockedScript::write(
        data.join("logs").join(format!("net_reset_{}.ps1", std::process::id())),
        &body,
    )?;
    let code = run_script_privileged(script.path()).map_err(|e| crate::texts::restore_launch_failed(&e.to_string()))?;
    // Ненулевой код скрипта — честная ошибка: раньше он игнорировался и UI
    // показывал «выполнено» даже при провалившемся сбросе.
    if code != 0 {
        return Err(crate::texts::net_reset_failed_code(code));
    }

    let steps: Vec<String> = crate::config::read_text_auto(steps_file.path())
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    if steps.is_empty() {
        crate::logger::log_code("warn", "netreset", "E-NET-001", "скрипт сброса завершился без единого шага — проверьте журнал");
    }
    let virtual_adapters = list_virtual_adapters();
    Ok(NetResetResult {
        steps,
        reboot_required: true,
        errors: Vec::new(),
        virtual_adapters,
    })
}

/// Начало шага: статус по умолчанию «выполнено», провал выставляют проверки
/// внутри шага (`$zguiOk = $false`). Так список шагов честный: «попытались»
/// больше не выдаётся за «сделали».
fn step(name: &str) -> String {
    format!("$zguiOk = $true\n$zguiStep = '{name}'\n")
}

/// Конец шага: строка статуса уходит в общий список шагов.
fn step_end() -> String {
    format!(
        "if ($zguiOk) {{ $zguiSteps += \"$zguiStep{}\" }} else {{ $zguiSteps += \"$zguiStep{}\" }}\n\n",
        crate::texts::STEP_DONE_SUFFIX,
        crate::texts::STEP_FAILED_SUFFIX
    )
}

fn reset_script(steps_file: &Path) -> String {
    let mut body = String::new();
    body.push_str(&format!("{}\n", crate::runner::PS_HEADER));
    // Не падаем на отдельных командах (некоторые могут вернуть ненулевой код),
    // но каждый шаг сам решает, выполнена ли его работа (проверки ниже).
    body.push_str("$ErrorActionPreference = 'Continue'\n$zguiSteps = @()\n\n");

    // 1. Наша/чужая служба zapret.
    body.push_str(&step(crate::texts::STEP_SERVICE_REMOVED));
    body.push_str(
        "if (Get-Service -Name 'zapret' -ErrorAction SilentlyContinue) {\n\
           Stop-Service -Name 'zapret' -Force -ErrorAction SilentlyContinue\n\
           sc.exe stop zapret 2>$null | Out-Null\n\
           sc.exe delete zapret 2>$null | Out-Null\n\
           if (Get-Service -Name 'zapret' -ErrorAction SilentlyContinue) { $zguiOk = $false }\n\
         }\n",
    );
    body.push_str(&step_end());

    // 2. VPN-службы (включая AmneziaVPN): сначала стоп, чтобы не возрождали процессы.
    body.push_str(&step(crate::texts::STEP_VPN_SERVICES));
    body.push_str(
        "$vpnServices = @('AmneziaVPN-service','AmneziaVPN','AmneziaWG','amneziawg','WireGuardTunnel','OpenVPNService','OpenVPNServiceInteractive','Happ','Nekoray','ClashVerge','sing-box')\n\
         foreach ($s in $vpnServices) {\n\
           $svc = Get-Service -Name $s -ErrorAction SilentlyContinue\n\
           if ($svc) { Stop-Service -Name $s -Force -ErrorAction SilentlyContinue; sc.exe stop $s 2>$null | Out-Null }\n\
         }\n\
         Start-Sleep -Seconds 2\n\
         foreach ($s in $vpnServices) {\n\
           $svc = Get-Service -Name $s -ErrorAction SilentlyContinue\n\
           if ($svc -and $svc.Status -ne 'Stopped') { $zguiOk = $false }\n\
         }\n",
    );
    body.push_str(&step_end());

    // 3. Процессы VPN и чужие winws (не наши — наш процесс к этому моменту остановлен GUI).
    body.push_str(&step(crate::texts::STEP_VPN_PROCESSES));
    body.push_str(
        "$names = @('winws','winws2','goodbyedpi','dpibreak','AmneziaVPN-service','AmneziaVPN','amneziawg','wg','wireguard','openvpn','openvpn-gui','sing-box','xray','v2ray','nekoray','clash','mihomo','happ','hiddify','tun2socks')\n\
         foreach ($n in $names) {\n\
           Get-Process -Name $n -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue\n\
         }\n\
         Start-Sleep -Seconds 1\n\
         foreach ($n in $names) {\n\
           if (Get-Process -Name $n -ErrorAction SilentlyContinue) { $zguiOk = $false }\n\
         }\n",
    );
    body.push_str(&step_end());

    // 4. Драйверы перехвата WinDivert (могут остаться висеть после сбоя).
    body.push_str(&step(crate::texts::STEP_DRIVERS));
    body.push_str(
        "$drivers = @('WinDivert','WinDivert14','WinDivert1.4')\n\
         foreach ($d in $drivers) {\n\
           sc.exe stop $d 2>$null | Out-Null\n\
           sc.exe delete $d 2>$null | Out-Null\n\
         }\n\
         foreach ($d in $drivers) {\n\
           $q = & sc.exe query $d 2>$null | Out-String\n\
           if ($q -match 'RUNNING') { $zguiOk = $false }\n\
         }\n",
    );
    body.push_str(&step_end());

    // 5. Прокси: WinHTTP + системный (реестр).
    body.push_str(&step(crate::texts::STEP_PROXY));
    body.push_str(
        "netsh winhttp reset proxy | Out-Null\n\
         if ($LASTEXITCODE -ne 0) { $zguiOk = $false }\n\
         $key = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'\n\
         Set-ItemProperty -Path $key -Name ProxyEnable -Value 0 -Type DWord -ErrorAction SilentlyContinue\n\
         Remove-ItemProperty -Path $key -Name ProxyServer -ErrorAction SilentlyContinue\n\
         Remove-ItemProperty -Path $key -Name AutoConfigURL -ErrorAction SilentlyContinue\n\
         Remove-ItemProperty -Path $key -Name AutoDetect -ErrorAction SilentlyContinue\n\
         $ls = Get-ItemProperty -Path $key -ErrorAction SilentlyContinue\n\
         if ($ls -and [int]$ls.ProxyEnable -eq 1) { $zguiOk = $false }\n",
    );
    body.push_str(&step_end());

    // 6. Кэш DNS.
    body.push_str(&step(crate::texts::STEP_DNS));
    body.push_str("ipconfig /flushdns | Out-Null\nif ($LASTEXITCODE -ne 0) { $zguiOk = $false }\n");
    body.push_str(&step_end());

    // 7. Сброс Winsock и TCP/IP (вступает в силу ПОСЛЕ перезагрузки).
    body.push_str(&step(crate::texts::STEP_NETWORK));
    body.push_str(
        "netsh winsock reset | Out-Null\n\
         if ($LASTEXITCODE -ne 0) { $zguiOk = $false }\n\
         netsh int ip reset | Out-Null\n\
         if ($LASTEXITCODE -ne 0) { $zguiOk = $false }\n",
    );
    body.push_str(&step_end());

    body.push_str(&format!(
        "$zguiSteps | Set-Content -LiteralPath {} -Encoding UTF8\n\
         Write-Output 'DONE'\nexit 0\n",
        crate::runner::ps_quote(&steps_file.to_string_lossy())
    ));
    body
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

    #[test]
    fn reset_script_is_valid_powershell_and_reports_steps() {
        let steps = std::env::temp_dir().join(format!("zgui-netreset-{}.txt", std::process::id()));
        let body = reset_script(&steps);
        assert!(body.contains("$zguiSteps += "), "шаг должен попасть в список: {body}");
        assert!(body.contains(crate::texts::STEP_DNS), "текст шага должен быть в скрипте");
        assert!(body.contains("Set-Content -LiteralPath"), "список шагов должен писаться в файл");
        assert!(
            body.contains(crate::texts::STEP_DONE_SUFFIX) && body.contains(crate::texts::STEP_FAILED_SUFFIX),
            "шаги должны иметь честный статус выполнено/ошибка: {body}"
        );

        let path = std::env::temp_dir().join(format!("zgui-netreset-{}.ps1", std::process::id()));
        crate::runner::write_ps1(&path, &body).unwrap();
        let script = format!(
            "$t=$null; $e=$null; [void][System.Management.Automation.Language.Parser]::ParseFile({}, [ref]$t, [ref]$e); if ($e.Count) {{ $e[0].Message; exit 1 }} else {{ exit 0 }}",
            crate::runner::ps_quote(&path.to_string_lossy())
        );
        let out = crate::runner::hidden_command("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .expect("powershell");
        let _ = std::fs::remove_file(&path);
        assert!(
            out.status.success(),
            "скрипт сброса не разбирается PowerShell: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}
