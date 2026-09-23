use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const PS_HEADER: &str = "$ErrorActionPreference = 'Stop'";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Creates a command without a transient console window in the GUI process.
pub fn hidden_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Экранирует аргумент для вставки в PowerShell в двойных кавычках.
pub fn ps_quote(arg: &str) -> String {
    let mut s = String::with_capacity(arg.len() + 2);
    s.push('"');
    for c in arg.chars() {
        match c {
            '"' => s.push_str("`\""),
            '`' => s.push_str("``"),
            '$' => s.push_str("`$"),
            _ => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Формирует -ArgumentList @( ... ) для Start-Process.
pub fn ps_arg_list(args: &[String]) -> String {
    let v: Vec<String> = args.iter().map(|a| ps_quote(a)).collect();
    v.join(",")
}

pub fn run_powershell(args: &[String]) -> Result<String, String> {
    let mut cmd = hidden_command("powershell.exe");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass"]);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    let code = out.status.code().unwrap_or(-1);
    if code == 0 {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Проверяет, запущен ли текущий процесс от администратора.
pub fn is_elevated() -> bool {
    let out = hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)",
        ])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().eq_ignore_ascii_case("true"),
        Err(_) => false,
    }
}

/// Список аргументов для перезапуска с UAC. `--boot` обязательно переносится:
/// иначе запуск от автозагрузки терял признак «старт при входе» и стратегия
/// не поднималась (GUI открывался, winws — нет).
fn elevate_arg_list(boot: bool) -> &'static str {
    if boot {
        "@('--elevated','--boot')"
    } else {
        "@('--elevated')"
    }
}

/// Перезапускает текущий exe с UAC (RunAs).
///
/// Возвращает `true`, если привилегированный экземпляр действительно стартовал.
/// `false` — пользователь отклонил запрос UAC: вызывающий код должен продолжить
/// работу без прав администратора, а не молча закрывать программу.
///
/// ВАЖНО: флаг `--boot` переносится в новый процесс. Раньше при запуске от
/// планировщика/автозагрузки с `always_admin` происходил перезапуск с одним
/// `--elevated`, `--boot` терялся — и автозапуск стратегии молча не срабатывал.
pub fn relaunch_as_admin() -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let arg_list = elevate_arg_list(std::env::args().any(|a| a == "--boot"));
    let script = format!(
        "$ErrorActionPreference='Stop'; try {{ $p = Start-Process -FilePath {} -Verb RunAs -WindowStyle Hidden -ArgumentList {} -PassThru; if ($p) {{ exit 0 }} else {{ exit 1 }} }} catch {{ exit 1 }}",
        ps_quote(&exe.to_string_lossy()),
        arg_list
    );
    let out = hidden_command("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(out.status.code().unwrap_or(1) == 0)
}

/// Запускает ps1-скрипт с UAC-элевацией (окно скрыто, ждём завершения).
pub fn run_elevated_script(script: &Path) -> Result<i32, String> {
    let inner = format!(
        "-NoProfile -ExecutionPolicy Bypass -File \"{}\"",
        script.to_string_lossy()
    );
    let mut cmd = hidden_command("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &format!(
            "$ErrorActionPreference = 'Stop'; try {{ $p = Start-Process -FilePath 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe' -ArgumentList @({}) -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1 }}",
            ps_quote(&inner)
        ),
    ]);
    let out = cmd.output().map_err(|e| e.to_string())?;
    Ok(out.status.code().unwrap_or(-1))
}

/// Запускает ps1-скрипт с правами администратора и ЖДЁТ результат.
///
/// Выбор пути критичен: `Start-Process -Verb RunAs` из УЖЕ повышенного процесса
/// может не запустить дочерний процесс (или зависнуть) — тогда кнопка «делает
/// вид, что работает», а действие не выполняется. Если GUI уже админ — запускаем
/// напрямую; иначе — обычная UAC-элевация.
pub fn run_script_privileged(script: &Path) -> Result<i32, String> {
    if is_elevated() {
        let status = hidden_command("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script.to_string_lossy(),
            ])
            .status()
            .map_err(|e| e.to_string())?;
        Ok(status.code().unwrap_or(-1))
    } else {
        run_elevated_script(script)
    }
}

/// Запускает ps1-скрипт с UAC-элевацией, НЕ ожидая завершения (для длительных тестов).
pub fn spawn_elevated_script(script: &Path) -> Result<(), String> {
    let inner = format!(
        "-NoProfile -ExecutionPolicy Bypass -File \"{}\"",
        script.to_string_lossy()
    );
    hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &format!(
                "Start-Process -FilePath 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe' -ArgumentList @({}) -Verb RunAs -WindowStyle Hidden",
                ps_quote(&inner)
            ),
        ])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Запускает ps1-скрипт НАПРЯМУЮ, без UAC — для случая, когда GUI уже elevated.
///
/// ВАЖНО: `Start-Process -Verb RunAs` из уже повышенного процесса может не запустить
/// дочерний процесс (или зависнуть) — тогда тест «висит» на фазе запуска, хотя права
/// уже выданы. Прямой запуск убирает этот сценарий.
pub fn spawn_script_direct(script: &Path) -> Result<u32, String> {
    let mut cmd = hidden_command("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        &script.to_string_lossy(),
    ]);
    let child = cmd
        .spawn()
        .map_err(|e| format!("не удалось запустить скрипт: {}", e))?;
    Ok(child.id())
}

// ------------------------------------------------------------ автозапуск GUI

/// Имя задачи в планировщике для запуска GUI при входе пользователя.
pub const BOOT_TASK: &str = "ZapretGUI";
const BOOT_RUN_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Есть ли задача планировщика автозапуска GUI.
pub fn boot_task_exists() -> bool {
    hidden_command("schtasks.exe")
        .args(["/query", "/tn", BOOT_TASK])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Осталась ли запись автозапуска в старом месте (HKCU\...\Run, версии ≤ 1.0.0).
pub fn legacy_boot_registered() -> bool {
    hidden_command("reg.exe")
        .args(["query", BOOT_RUN_KEY, "/v", "zgui"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Удаляет старую запись HKCU\...\Run — чтобы не было двойного запуска.
pub fn remove_legacy_boot() {
    let _ = hidden_command("reg.exe")
        .args(["delete", BOOT_RUN_KEY, "/v", "zgui", "/f"])
        .output();
}

fn boot_task_script(enable: bool, exe: &Path) -> String {
    if enable {
        // Задача от имени текущего пользователя, вход интерактивный, уровень
        // «наивысшие»: GUI стартует при входе уже с правами администратора и
        // БЕЗ запроса UAC, а значит сразу может поднять winws (--boot).
        format!(
            "{}\n$exe = {}\n$user = $env:USERDOMAIN + '\\' + $env:USERNAME\nUnregister-ScheduledTask -TaskName '{}' -Confirm:$false -ErrorAction SilentlyContinue\n$action = New-ScheduledTaskAction -Execute $exe -Argument '--boot'\n$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user\n$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest\n$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable\nRegister-ScheduledTask -TaskName '{}' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null\nexit 0\n",
            PS_HEADER,
            ps_quote(&exe.to_string_lossy()),
            BOOT_TASK,
            BOOT_TASK
        )
    } else {
        format!(
            "{}\nUnregister-ScheduledTask -TaskName '{}' -Confirm:$false -ErrorAction SilentlyContinue\nexit 0\n",
            PS_HEADER, BOOT_TASK
        )
    }
}

/// Создаёт/удаляет задачу планировщика «ZapretGUI».
/// Создание задачи с уровнем «наивысшие» требует прав администратора: если GUI
/// не повышен, скрипт уходит через один UAC-запрос.
pub fn apply_boot_task(enable: bool, exe: &Path, data_dir: &Path) -> Result<(), String> {
    let script = data_dir
        .join("logs")
        .join(format!("boot_task_{}.ps1", std::process::id()));
    write_ps1(&script, &boot_task_script(enable, exe))?;
    let result = if is_elevated() {
        run_powershell(&["-File".into(), script.to_string_lossy().into_owned()])
    } else {
        run_elevated_script(&script).map(|_| String::new())
    };
    let _ = fs::remove_file(&script);
    result.map(|_| ())?;
    // Проверяем факт: молчаливая ошибка тут недопустима (иначе «автозапуск включён»,
    // а задачи нет — ровно та жалоба, из-за которой это переписано).
    if enable && !boot_task_exists() {
        return Err("задача планировщика не создана — проверьте права администратора".into());
    }
    if !enable && boot_task_exists() {
        return Err("не удалось удалить задачу планировщика".into());
    }
    Ok(())
}

/// Пишет .ps1 в UTF-8 с BOM — иначе PowerShell 5.1 читает кириллицу как ANSI
/// и ломает кавычки/строки (ParserError).
pub fn write_ps1(path: &Path, body: &str) -> Result<(), String> {
    write_utf8_bom(path, body.as_bytes())
}

pub fn write_utf8_bom(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(body.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(body);
    fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Запускает процесс НАПРЯМУЮ (без UAC) — используется, когда GUI уже elevated.
/// Возвращает реальный PID процесса и пишет stdout/stderr в лог-файлы.
/// Без ShellExecute: Rust сам корректно квотит аргументы с пробелами.
pub fn spawn_direct(
    exe: &Path,
    wd: &Path,
    args: &[String],
    out_log: &Path,
    err_log: &Path,
) -> Result<u32, String> {
    use std::fs::OpenOptions;
    let out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(out_log)
        .map_err(|e| format!("лог: {}", e))?;
    let err = OpenOptions::new()
        .create(true)
        .append(true)
        .open(err_log)
        .map_err(|e| format!("лог: {}", e))?;
    let mut cmd = Command::new(exe);
    cmd.current_dir(wd).args(args).stdout(out).stderr(err);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let child = cmd
        .spawn()
        .map_err(|e| format!("не удалось запустить процесс: {}", e))?;
    Ok(child.id())
}

/// Пишет launcher-скрипт для запуска winws/winws2 от администратора.
///
/// ВАЖНО: `Start-Process -Verb RunAs` НЕсовместим с `-RedirectStandardOutput/Error`
/// (падает с «не удалось запустить процесс с использованием указанных параметров»),
/// а обёртка через `cmd /c` ломается на разборе кавычек. Поэтому здесь — простой
/// запуск без редиректов; логи в этом пути не собираются (для elevated GUI
/// используется `spawn_direct`, который их пишет).
pub fn write_launcher(exe: &Path, wd: &Path, args: &[String], _out_log: &Path, _err_log: &Path, pid_file: &Path) -> PathBuf {
    let script_path = pid_file.with_extension("ps1");
    let mut s = String::new();
    s.push_str(PS_HEADER);
    s.push('\n');
    s.push_str(&format!("$pidFile = {}\n", ps_quote(&pid_file.to_string_lossy())));
    s.push_str(&format!(
        "try {{\n  $argsRaw = @({})\n  # Start-Process flattens arrays without preserving quotes around paths with spaces.\n  $argLine = @($argsRaw | ForEach-Object {{ $a = [string]$_; if ($a -match '[\\s\"]') {{ '\"' + $a.Replace('\"', '\\\"') + '\"' }} else {{ $a }} }}) -join ' '\n  $p = Start-Process -FilePath {} -WorkingDirectory {} -WindowStyle Hidden -Verb RunAs -ArgumentList $argLine -PassThru\n  Start-Sleep -Milliseconds 700\n  if ($p -and -not $p.HasExited) {{ $status = [string]$p.Id }} else {{ $status = 'process exited immediately' }}\n}} catch {{\n  $status = 'LAUNCH_ERROR: ' + $_.Exception.Message\n}}\n# Один файл статуса, UTF-8 с BOM: читатель декодирует без «иероглифов».\nSet-Content -LiteralPath $pidFile -Value $status -Encoding UTF8\nif ($status -notmatch '^[0-9]+$') {{ exit 1 }}\n",
        ps_arg_list(args),
        ps_quote(&exe.to_string_lossy()),
        ps_quote(&wd.to_string_lossy()),
    ));
    write_ps1(&script_path, &s).expect("write launcher");
    script_path
}

/// Запускает лаунчер и ждёт появления pid-файла.
pub fn spawn_and_wait_pid(script: &Path, pid_file: &Path, timeout: Duration) -> Result<u32, String> {
    let _ = fs::remove_file(pid_file);
    let _ = run_powershell(&["-File".into(), script.to_string_lossy().into_owned()]);
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(text) = crate::config::read_text_auto(pid_file) {
            let t = text.trim();
            if let Ok(pid) = t.parse::<u32>() {
                return Ok(pid);
            }
            if !t.is_empty() {
                // Лаунчер записал ошибку — не ждём таймаут впустую.
                let msg = t
                    .lines()
                    .find(|l| l.starts_with("LAUNCH_ERROR"))
                    .unwrap_or(t)
                    .trim()
                    .to_string();
                return Err(msg);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("не удалось запустить процесс — подтверждение прав администратора отклонено или файл недоступен".into())
}

/// Проверяет наличие процесса по PID (без элевации).
pub fn pid_alive(pid: u32) -> bool {
    // PID 0 — System Idle Process: `tasklist` его показывает, но это не процесс.
    if pid == 0 {
        return false;
    }
    // ponytail: OpenProcess дешевле tasklist (спавн процесса ~30 мс каждый
    // вызов; проверок много: watcher-цикл 2 c, bootstrap, current_owner).
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let ok = unsafe { is_process_alive(handle) };
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        return ok;
    }
    #[cfg(not(windows))]
    {
    let out = hidden_command("tasklist.exe")
        .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
        .output();
    match out {
        Ok(o) => {
            let txt = String::from_utf8_lossy(&o.stdout).to_lowercase();
            txt.contains(&format!("\"{}\"", pid))
        }
        Err(_) => false,
    }
    }
}

/// Выходной код 259 (STILL_ACTIVE) — процесс жив.
#[cfg(windows)]
unsafe fn is_process_alive(handle: std::os::windows::io::RawHandle) -> bool {
    use windows_sys::Win32::System::Threading::GetExitCodeProcess;
    let mut code: u32 = 0;
    if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
        return false;
    }
    code == 259
}

/// Прибивает процесс и его дочерние (через элевацию).
pub fn stop_pid(pid: u32, data_dir: &Path) -> Result<(), String> {
    let script = data_dir
        .join("logs")
        .join(format!("kill_{}_{}.ps1", std::process::id(), pid));
    write_ps1(&script, &format!("{}\ntaskkill /F /T /PID {} | Out-Null\nexit 0", PS_HEADER, pid))?;
    let r = run_script_privileged(&script);
    let _ = fs::remove_file(&script);
    r.map(|_| ())
}

/// Прибивает несколько процессов и их деревья через один UAC.
pub fn stop_pids(pids: &[u32], data_dir: &Path) -> Result<(), String> {
    let script = data_dir
        .join("logs")
        .join(format!("kill_multi_{}.ps1", std::process::id()));
    let mut body = String::new();
    body.push_str(PS_HEADER);
    body.push('\n');
    for id in pids {
        body.push_str(&format!("taskkill /F /T /PID {} | Out-Null\n", id));
    }
    body.push_str("exit 0\n");
    write_ps1(&script, &body)?;
    let r = run_script_privileged(&script);
    let _ = fs::remove_file(&script);
    r.map(|_| ())
}

/// Планирует выключение компьютера/перезагрузку (для UI).
/// Хвост лог-файла.
pub fn tail(path: &Path, max_chars: usize) -> String {
    crate::config::tail_file(path, max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_quotes_arguments_with_spaces() {
        let dir = std::env::temp_dir().join(format!("zgui-launcher-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let script = write_launcher(
            Path::new("C:\\Program Files\\Zapret\\winws.exe"),
            Path::new("C:\\Program Files\\Zapret\\bin"),
            &["--hostlist=C:\\Program Files\\Zapret\\lists\\list.txt".into()],
            &dir.join("out.txt"),
            &dir.join("err.txt"),
            &dir.join("pid.txt"),
        );
        let raw = fs::read(&script).unwrap();
        let text = String::from_utf8(raw[3..].to_vec()).unwrap();
        assert!(text.contains("$argLine"));
        // RunAs без редиректов (редиректы + -Verb несовместимы).
        assert!(text.contains("-ArgumentList $argLine"));
        assert!(text.contains("-Verb RunAs"));
        assert!(!text.contains("-RedirectStandard"));
        assert!(text.contains("-WindowStyle Hidden"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_task_script_registers_and_removes() {
        let on = boot_task_script(true, Path::new("C:\\Z GUI\\zgui.exe"));
        assert!(on.contains("Register-ScheduledTask"));
        assert!(on.contains("New-ScheduledTaskTrigger -AtLogOn"));
        assert!(on.contains("-RunLevel Highest"));
        assert!(on.contains("-LogonType Interactive"));
        assert!(on.contains("'--boot'"));
        let off = boot_task_script(false, Path::new("C:\\Z GUI\\zgui.exe"));
        assert!(off.contains("Unregister-ScheduledTask"));
        assert!(!off.contains("Register-ScheduledTask"));
    }

    #[test]
    fn relaunch_preserves_boot_argument() {
        assert_eq!(elevate_arg_list(false), "@('--elevated')");
        assert_eq!(elevate_arg_list(true), "@('--elevated','--boot')");
    }
}
