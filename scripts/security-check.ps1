# Проверка «чистоты» сборки перед релизом (Windows PowerShell 5.1).
#
# Что делает:
#   1. Сверяет SHA-256 вшитых ресурсов с src-tauri/resources/checksums.sha256
#      (защита от подмены движка/конфигов чужеродным exe).
#   2. Показывает все include_bytes! и Command::new в коде — места, куда теоретически
#      можно «зашить» чужой файл или запуск процесса (для глазами-проверки).
#   3. Ищет типовые вредоносные паттерны (eval, base64+exec, скрытые загрузки).
#   4. Запускает аудит зависимостей: cargo audit (если установлен) и npm audit.
#   5. Сканирует собранный zgui.exe антивирусом Windows (по умолчанию; -SkipDefender).
#   6. Печатает SHA-256 готового exe — его публикуем рядом с релизом.
#
# Пример: powershell -ExecutionPolicy Bypass -File scripts\security-check.ps1

param(
    [switch]$SkipDefender,
    [switch]$SkipAudit
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$fail = 0

function Section($text) {
    Write-Host ""
    Write-Host ("=== " + $text + " ===") -ForegroundColor Cyan
}

# 1. Хеши вшитых ресурсов -----------------------------------------------------
Section "Вшитые ресурсы (SHA-256)"
$resDir = Join-Path $root "src-tauri\resources"
$sumFile = Join-Path $resDir "checksums.sha256"
if (-not (Test-Path $sumFile)) {
    Write-Host "НЕТ файла checksums.sha256 — не могу проверить ресурсы" -ForegroundColor Red
    $fail++
} else {
    foreach ($line in Get-Content $sumFile) {
        $t = $line.Trim()
        if ($t -eq "" -or $t.StartsWith("#")) { continue }
        $parts = $t -split "\s+", 2
        if ($parts.Count -ne 2) { continue }
        $expected = $parts[0].ToLower()
        $name = $parts[1].Trim()
        $path = Join-Path $resDir $name
        if (-not (Test-Path $path)) {
            Write-Host ("  ПРОПАЛ: " + $name) -ForegroundColor Red
            $fail++
            continue
        }
        $actual = (Get-FileHash $path -Algorithm SHA256).Hash.ToLower()
        if ($actual -eq $expected) {
            Write-Host ("  ок: " + $name) -ForegroundColor Green
        } else {
            Write-Host ("  ПОДМЕНА: " + $name) -ForegroundColor Red
            Write-Host ("    ожидался " + $expected)
            Write-Host ("    сейчас   " + $actual)
            $fail++
        }
    }
    # Появившиеся новые архивы/файлы, которых нет в списке — тоже интересны.
    foreach ($f in Get-ChildItem $resDir -File) {
        if ($f.Name -eq "checksums.sha256") { continue }
        $known = (Get-Content $sumFile | Where-Object { $_ -match [regex]::Escape($f.Name) }).Count -gt 0
        if (-not $known) {
            Write-Host ("  НОВЫЙ файл без хеша: " + $f.Name) -ForegroundColor Yellow
            $fail++
        }
    }
}

# 2. include_bytes! и запуск процессов ---------------------------------------
Section "Где код читает файлы и запускает процессы (для глазами-проверки)"
$rsFiles = Get-ChildItem (Join-Path $root "src-tauri\src") -Recurse -Filter *.rs
Write-Host "-- include_bytes! --"
$rsFiles | Select-String -Pattern "include_bytes!" | ForEach-Object {
    Write-Host ("  " + $_.Filename + ":" + $_.LineNumber + ": " + $_.Line.Trim())
}
Write-Host "-- Command::new --"
$rsFiles | Select-String -Pattern "Command::new\(" | ForEach-Object {
    Write-Host ("  " + $_.Filename + ":" + $_.LineNumber + ": " + $_.Line.Trim())
}

# 3. Подозрительные паттерны --------------------------------------------------
Section "Подозрительные паттерны"
$patterns = @(
    "Invoke-Expression",
    "-EncodedCommand",
    "FromBase64String",
    "DownloadString",
    "DownloadFile",
    "Invoke-WebRequest",
    "\beval\(",
    "new Function\(",
    "\batob\("
)
$scanFiles = @()
$scanFiles += Get-ChildItem (Join-Path $root "src-tauri\src") -Recurse -Filter *.rs
$scanFiles += Get-ChildItem (Join-Path $root "src") -Recurse -Include *.js, *.html
$found = 0
foreach ($p in $patterns) {
    $hits = $scanFiles | Select-String -Pattern $p
    foreach ($h in $hits) {
        Write-Host ("  " + $p + " -> " + $h.Filename + ":" + $h.LineNumber + ": " + $h.Line.Trim()) -ForegroundColor Yellow
        $found++
    }
}
if ($found -eq 0) { Write-Host "  ничего подозрительного не найдено" -ForegroundColor Green }

# 4. Аудит зависимостей -------------------------------------------------------
if (-not $SkipAudit) {
    Section "Аудит зависимостей"
    if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
        Push-Location (Join-Path $root "src-tauri")
        cargo audit
        if ($LASTEXITCODE -ne 0) { $fail++ }
        Pop-Location
    } else {
        Write-Host "  cargo-audit не установлен (cargo install cargo-audit) — пропущено" -ForegroundColor Yellow
    }
    if (Test-Path (Join-Path $root "package-lock.json")) {
        Push-Location $root
        npm audit --omit=dev --audit-level=high
        # у npm-зависимостей тут только сборка (vite/tauri-cli), падать не будем
        Pop-Location
    }
}

# 5. Антивирусный скан собранного exe ----------------------------------------
$exe = Join-Path $root "src-tauri\target\release\zgui.exe"
Section "Готовый exe"
if (-not (Test-Path $exe)) {
    Write-Host "  zgui.exe не собран — сначала: npm run tauri build -- --no-bundle" -ForegroundColor Yellow
} else {
    $info = Get-Item $exe
    Write-Host ("  " + $info.FullName)
    Write-Host ("  размер: " + $info.Length + " байт, время: " + $info.LastWriteTime)
    $hash = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLower()
    Write-Host ("  SHA-256: " + $hash) -ForegroundColor Green
    if (-not $SkipDefender) {
        $mp = Join-Path ${env:ProgramFiles} "Windows Defender\MpCmdRun.exe"
        if (Test-Path $mp) {
            Write-Host "  скан Windows Defender (может занять пару минут)…"
            & $mp -Scan -ScanType 3 -File $exe
            if ($LASTEXITCODE -ne 0) {
                Write-Host "  Defender сообщил о проблеме (код " + $LASTEXITCODE + ")" -ForegroundColor Red
                $fail++
            } else {
                Write-Host "  Defender: чисто" -ForegroundColor Green
            }
        } else {
            Write-Host "  MpCmdRun.exe не найден — скан пропущен" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  скан Defender пропущен (-SkipDefender)" -ForegroundColor Yellow
    }
}

Section "Итог"
if ($fail -eq 0) {
    Write-Host "OK: проверки пройдены" -ForegroundColor Green
    exit 0
} else {
    Write-Host ("ПРОБЛЕМЫ: " + $fail) -ForegroundColor Red
    exit 1
}