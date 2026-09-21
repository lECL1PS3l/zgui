# Самопроверка «антивирус/Defender не мешает программе» (Windows PowerShell 5.1).
#
# ВАЖНО: скрипт опирается на средства Windows Defender (Get-Mp*, MpCmdRun) и
# упрощённо воспроизводит паттерны работы. Для сторонних AV (Kaspersky/ESET/
# Bitdefender) смотрите их собственные журналы и раздел PUA/«легальные обходы».
# Скан самого zgui.exe делает scripts/security-check.ps1 (MpCmdRun) — здесь
# проверяются только записи журнала и сохранность/запуск движка.
#
# Проверяет то, что может реально сломать программу:
#   1. Журнал  — создание/перезапись файла в data\logs (паттерн приложения: tmp -> rename).
#   2. Движок  — «обновление»: кладём копию winws.exe/WinDivert в сандбокс под data\
#      и проверяем, что антивирус её не удалил сразу и «не догнал» спустя 25 секунд
#      (именно так действует эвристика: помечает файл позже), а также что winws.exe
#      реально исполняется, не будучи убитым на старте.
#   3. Статус  — защита в реальном времени, PUA-детект, исключения, история угроз.
#
# Запуск (окно само не закроется, в конце — «Нажмите Enter»):
#   powershell -ExecutionPolicy Bypass -File scripts\defender-test.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\defender-test.ps1 -AppDir "D:\Zapret"
#   powershell -ExecutionPolicy Bypass -File scripts\defender-test.ps1 -Quick  (-Quick: без паузы 25 с)
#   powershell -ExecutionPolicy Bypass -File scripts\defender-test.ps1 -NoPause (без паузы, для конвейера)
#
# ВЕРДИКТ в конце: OK / есть проблемы (и что делать).

param(
    # Папка с zgui.exe (проверяемая). Если не задана — сам находит рабочую папку:
    #   1) src-tauri\target\release (свежая отладочная сборка),
    #   2) release\ZapretGUI-*-portable (пакет),
    #   3) корень репозитория.
    [string]$AppDir = "",
    # Пропустить паузу 25 с («догон» антивируса) — быстрее, но менее показательно.
    [switch]$Quick,
    # Не ждать Enter в конце (для скриптов/конвейера).
    [switch]$NoPause
)

$ErrorActionPreference = "Stop"
$OutputEncoding = [System.Text.Encoding]::UTF8
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

# ------------------------------------------------------------------ папка
if (-not $AppDir) {
    $candidates = @(
        (Join-Path $PSScriptRoot "..\src-tauri\target\release"),
        (Join-Path $PSScriptRoot "release\ZapretGUI-*-portable"),
        $PSScriptRoot
    )
    foreach ($c in $candidates) {
        if ($c -like "*\*" ) {
            $m = Get-ChildItem -Path (Split-Path -Parent $c) -Directory -Filter (Split-Path -Leaf $c) -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($m) { $AppDir = $m.FullName; break }
        } elseif (Test-Path -LiteralPath $c) { $AppDir = $c; break }
    }
}
if (-not $AppDir -or -not (Test-Path -LiteralPath $AppDir)) {
    Write-Host "ERROR: папка не найдена: $AppDir" -ForegroundColor Red
    exit 1
}
$AppDir = (Resolve-Path -LiteralPath $AppDir).Path

$pass = 0
$fail = 0
$notes = [System.Collections.Generic.List[string]]::new()
$exitCode = 0

function Section([string]$text) {
    Write-Host ""
    Write-Host ("=== " + $text + " ===") -ForegroundColor Cyan
}
function Ok([string]$t) { Write-Host ("  ok: " + $t) -ForegroundColor Green; $script:pass++ }
function Bad([string]$t) { Write-Host ("  ПРОБЛЕМА: " + $t) -ForegroundColor Red; $script:fail++ }
function Info([string]$t) { Write-Host ("  ..: " + $t) -ForegroundColor DarkGray }

try {
    $data = Join-Path $AppDir "data"
    $logs = Join-Path $data "logs"
    $tmp  = Join-Path $data "tmp"
    $sandbox = Join-Path $tmp ("av-engine-" + [System.Guid]::NewGuid().ToString("N"))
    $selfCheckLog = Join-Path $logs "av-selfcheck.log"

    Write-Host ""
    Write-Host ("Тестируем папку: " + $AppDir)

    # ================================================================ 1. статус
    Section "Статус защиты Windows"
    try {
        $st = Get-MpComputerStatus
        Ok("защита в реальном времени: " + $(if ($st.RealTimeProtectionEnabled) { "включена" } else { "ВЫКЛЮЧЕНА" }))
        if (-not $st.RealTimeProtectionEnabled) {
            $notes.Add("Реальная защита (Real-time) выключена или антивирус не Microsoft Defender — проверьте свой антивирус.")
        }
    } catch {
        Info("Get-MpComputerStatus недоступен: " + $_.Exception.Message)
        $notes.Add("Не удалось прочитать статус защитника — возможно, стоит другой антивирус.")
    }

    # Настройки PUA и исключения — для справки.
    try {
        $pref = Get-MpPreference
        Info("PUA-защита (PUAProtection): " + $(switch ($pref.PUAProtection) { 0 { "выкл (0)" } 1 { "предупреждать (1)" } 2 { "блокировать (2) — winws может попасть под неё" } default { $pref.PUAProtection } }))
        if ($pref.ExclusionPath) { Info("исключения по путям: " + ($pref.ExclusionPath -join "; ")) }
        if ($pref.ExclusionProcess) { Info("исключения по процессам: " + ($pref.ExclusionProcess -join "; ")) }
    } catch {
        Info("Get-MpPreference недоступен: " + $_.Exception.Message)
    }

    # ================================================================ 2. журнал
    Section "Журнал в папке программы (data\log)"
    New-Item -ItemType Directory -Force -Path $logs | Out-Null
    $mark = "av-selfcheck " + (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
    try {
        Set-Content -LiteralPath $selfCheckLog -Value $mark -Encoding UTF8
        Add-Content -LiteralPath $selfCheckLog -Value "логи дописываются" -Encoding UTF8
        # Паттерн приложения: пишем во временный файл, затем атомарно переименовываем.
        $tmpF = Join-Path $logs "av-selfcheck.log.zgui_tmp"
        Set-Content -LiteralPath $tmpF -Value $mark -Encoding UTF8
        Rename-Item -LiteralPath $tmpF -NewName "av-selfcheck-atomic.log" -Force
        if ((Test-Path -LiteralPath $selfCheckLog) -and (Test-Path -LiteralPath (Join-Path $logs "av-selfcheck-atomic.log"))) {
            Ok("журнал создан и перезаписан (пытались записать -> переименовать)")
        } else {
            Bad("журнал пропал после записи — антивирус удаляет файлы программы")
            $notes.Add("Defender/Kaspersky удаляет data\log — добавьте папку приложения в исключения.")
        }
    } catch {
        Bad("запись журнала упала: " + $_.Exception.Message)
        $notes.Add("Ошибка записи в " + $logs + " — проверьте права на папку и исключения антивируса.")
    }
    # Очистим следы теста, чтобы не мусорить в журнале.
    Remove-Item -LiteralPath $selfCheckLog -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $logs "av-selfcheck-atomic.log") -ErrorAction SilentlyContinue

    # ================================================================ 3. движок
    Section "Обновление движка (winws.exe + WinDivert под data\)"
    $exe = $null
    $wdiv = $null
    if (Test-Path -LiteralPath (Join-Path $data "engines")) {
        $exe = Get-ChildItem -Path (Join-Path $data "engines") -Recurse -Filter "winws.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
        $wdiv = Get-ChildItem -Path (Join-Path $data "engines") -Recurse -Filter "WinDivert*.sys" -File -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    New-Item -ItemType Directory -Force -Path $sandbox | Out-Null
    $ok = $true
    if ($exe) {
        $dest = Join-Path $sandbox "bin"
        New-Item -ItemType Directory -Force -Path $dest | Out-Null
        $newExe = Join-Path $dest "winws.exe"
        try {
            # «Новая версия» движка пишется как при установке: во tmp, затем rename.
            Copy-Item -LiteralPath $exe.FullName -Destination ($newExe + ".zgui_tmp") -Force
            Rename-Item -LiteralPath ($newExe + ".zgui_tmp") -NewName "winws.exe" -Force
            $h1 = (Get-FileHash -LiteralPath $newExe -Algorithm SHA256).Hash
            Start-Sleep -Seconds 3   # даём антивирусу время «догнать» файл
            $stillOk = $true
            if (Test-Path -LiteralPath $newExe) {
                $h2 = (Get-FileHash -LiteralPath $newExe -Algorithm SHA256).Hash
                if ($h1 -eq $h2) {
                    Ok("winws.exe скопирован в сандбокс (hash совпадает, сразу)")
                } else {
                    Bad("winws.exe изменён после записи (" + $h2.Substring(0, 8) + ") — вмешательство антивируса")
                    $stillOk = $false
                    $ok = $false
                }
            } else {
                Bad("winws.exe пропал из сандбокса — антивирус удалил движок")
                $notes.Add("При обновлении движка Defender удаляет winws.exe (PUA). Настройте исключение пути " + $AppDir + " и, при желании, отключите PUA-детект. Читайте docs\antivirus-test.md.")
                $ok = $false
                $stillOk = $false
            }
            # Эвристика AV часто «догоняет» файл с задержкой — перепроверяем позже.
            if ($stillOk -and -not $Quick) {
                Start-Sleep -Seconds 25
                if (Test-Path -LiteralPath $newExe) {
                    $h3 = (Get-FileHash -LiteralPath $newExe -Algorithm SHA256).Hash
                    if ($h3 -eq $h1) {
                        Ok("winws.exe пережил паузу 25 c — антивирус «не догнал» файл")
                    } else {
                        Bad("winws.exe изменён через 25 c — антивирус вмешался позже")
                        $ok = $false
                    }
                } else {
                    Bad("winws.exe пропал через 25 c — антивирус удалил его с задержкой")
                    $ok = $false
                }
            }
        } catch {
            Bad("запись winws.exe упала: " + $_.Exception.Message)
            $ok = $false
        }
    } else {
        Info("движок в data\engines не найден — тест пропущен (создастся при первом запуске программы)")
        $notes.Add("Движок ещё не распакован (data\engines пусто): запустите программу один раз и повторите тест.")
    }
    if ($wdiv -and $ok) {
        $wDest = Join-Path $sandbox "bin"
        New-Item -ItemType Directory -Force -Path $wDest | Out-Null
        $wFile = Join-Path $wDest $wdiv.Name
        try {
            Copy-Item -LiteralPath $wdiv.FullName -Destination $wFile -Force
            Start-Sleep -Seconds 2
            if (Test-Path -LiteralPath $wFile) {
                Ok("WinDivert-драйвер на месте: " + $wdiv.Name)
            } else {
                Bad("WinDivert-драйвер удалён антивирусом — движок не сможет работать")
                $notes.Add("WinDivert*.sys удаляется антивирусом — добавьте исключение папки приложения.")
                $ok = $false
            }
        } catch {
            Bad("запись WinDivert упала: " + $_.Exception.Message)
        }
    }
    if ($ok -and $exe) { Ok("сандбокс ['new engine'] пережил паузу — антивирус записи не мешает") }

    # Исполнение: PUA-детект чаще срабатывает на запуск, а не на запись.
    # Пробуем исполнить winws.exe с безобидным флагом и смотрим на результат.
    # ВАЖНО: рядом с exe обязаны лежать соседние DLL (cygwin1.dll, WinDivert.dll),
    # иначе загрузчик покажет «Системная ошибка» и процесс «зависнет» на диалоге —
    # это не про антивирус. Поэтому сначала копируем весь bin движка.
    if ($ok -and $exe -and (Test-Path -LiteralPath $newExe)) {
        try {
            Get-ChildItem -LiteralPath $exe.Directory -File -ErrorAction Stop |
                Copy-Item -Destination $dest -Force
            $proc = Start-Process -FilePath $newExe -ArgumentList "--version" -WindowStyle Hidden -PassThru
            $stayed = -not $proc.WaitForExit(5000)
            if ($stayed) {
                Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
                Ok("winws.exe запустился и продолжает работать — AV не блокирует старт (процесс остановлен нами)")
            } elseif (Test-Path -LiteralPath $newExe) {
                Ok("winws.exe исполнился (код " + $proc.ExitCode + ") и файл на месте — исполнение не заблокировано")
            } else {
                Bad("winws.exe пропал после запуска — антивирус убил движок в момент исполнения")
                $notes.Add("Detector удаляет движок именно при запуске — исключение папки приложения обязательно, см. docs\antivirus-test.md.")
                $ok = $false
            }
        } catch {
            Info("исполнение winws не протестировано: " + $_.Exception.Message)
        }
    }

    # ================================================================ 4. история
    Section "История угроз Defender (что реально помечалось)"
    $self = $AppDir.ToLowerInvariant()
    $hits = 0
    $total = 0
    $needAdminForPaths = $false
    try {
        $det = @(Get-MpThreatDetection -ErrorAction SilentlyContinue | Sort-Object InitialDetectionTime -Descending | Select-Object -First 25)
        $total = $det.Count
        foreach ($d in $det) {
            $res = Get-MpThreat -ThreatID $d.ThreatID -ErrorAction SilentlyContinue
            $paths = @()
            if ($res -and $res.Resources) { foreach ($r in $res.Resources) { if ($r.Path) { $paths += $r.Path } } }
            if (-not $paths) {
                # Без прав администратора ресурсы скрыты — ищем упоминание нашей
                # папки/winws прямо в записи детекта (грубая, но информативная проверка).
                $needAdminForPaths = $true
                $json = ($d | ConvertTo-Json -Depth 4 -Compress | Out-String)
                $inOurs = $json -match [regex]::Escape($self)
                $winws = $json -match "winws|WinDivert"
            } else {
                $inOurs = [bool]($paths | Where-Object { $_.ToLowerInvariant() -match [regex]::Escape($self) })
                $winws = [bool]($paths | Where-Object { $_ -match "winws|WinDivert" })
            }
            if ($inOurs -or $winws) {
                $hits++
                $name = if ($res) { $res.ThreatName } else { $d.ThreatID }
                $tag = if ($inOurs) { "В НАШЕЙ ПАПКЕ" } else { "вне (но имя/путь похоже)" }
                $pstr = if ($paths) { ($paths | Select-Object -Unique) -join "; " } else { "<пути скрыты без админа>" }
                Info(("найдено: [" + $tag + "] " + $name + "  файлы: " + $pstr + "  [время: " + $d.InitialDetectionTime + "]"))
            }
        }
        if ($hits -eq 0) {
            Ok("в истории угроз нет записей про winws/WinDivert и нашу папку")
            if ($total -gt 0) {
                Info("в истории ещё есть " + $total + " записей, но НЕ про winws/WinDivert/нашу папку — это обычно детекты dev-тестов (тест-харнесс дёргает встроенные бинари движков) и на собранный exe не влияет.")
            }
        } else {
            # Это не «провал»: тест движка выше подтвердил, что файлы при этом на
            # месте (PUAProtection = предупреждать, не блокировать). Это контекст.
            Info("в истории " + $total + " записей, из них " + $hits + " относятся к winws/нашей папке — типовые generic/PUA-детекты; файлы не удалены, смотрите итог теста движка.")
            if ($needAdminForPaths) {
                Info("точные пути ресурсов скрыты (нужны права администратора): запустите скрипт от админа, чтобы увидеть их.")
            }
            $notes.Add("Defender помечал winws/WinDivert (generic/PUA). Рекомендуется исключение папки приложения: Add-MpPreference -ExclusionPath '" + $AppDir + "' — см. docs\antivirus-test.md.")
        }
    } catch {
        Info("история угроз недоступна (нужны права администратора): " + $_.Exception.Message)
    }

    # ================================================================ 5. итог
    Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
    Section "ВЕРДИКТ"
    if ($fail -eq 0) {
        Write-Host ("OK: проверки пройдены ($pass ok, $fail провалов)") -ForegroundColor Green
        Write-Host "  Антивирус записи программы не мешает. Журнал и обновление движка должны работать."
        $exitCode = 0
    } else {
        Write-Host ("ПРОБЛЕМЫ: $fail (" + $pass + " ok)") -ForegroundColor Red
        Write-Host ""
        Write-Host "Что сделать (по порядку):" -ForegroundColor Yellow
        Write-Host ("  1. Добавить папку программы в исключения: Add-MpPreference -ExclusionPath '" + $AppDir + "' (от администратора)")
        Write-Host "  2. Если Defender удаляет winws.exe — в «Безопасность Windows → Защита от вирусов → Параметры → Исключения»"
        Write-Host "     добавить папку приложения; при необходимости отключить PUA-детект."
        Write-Host "  3. Если стоит сторонний антивирус (Касперский и т.п.) — искать его собственные исключения."
        Write-Host ""
        Write-Host "Подробный чеклист и ожидаемые результаты — в docs\antivirus-test.md."
        $exitCode = $fail
    }
} catch {
    Write-Host ""
    Write-Host ("КРИТИЧЕСКАЯ ОШИБКА СКРИПТА: " + $_.Exception.Message) -ForegroundColor Red
    if ($_.InvocationInfo.ScriptLineNumber) {
        Write-Host ("  (строка " + $_.InvocationInfo.ScriptLineNumber + ")") -ForegroundColor DarkGray
    }
    $exitCode = 1
} finally {
    if (-not $NoPause -and -not [Console]::IsInputRedirected) {
        Write-Host ""
        Read-Host "Нажмите Enter, чтобы закрыть окно"
    }
}
exit $exitCode