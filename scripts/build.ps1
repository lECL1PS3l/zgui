# Build zgui.exe (Tauri). Loads the MSVC environment from vcvars64.bat, because
# Visual Studio is not on PATH in a normal terminal (after VS updates the linker
# can be missing from the environment entirely - that is what this script fixes).
$ErrorActionPreference = 'Stop'

$vcvars = $null
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path $vswhere) {
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($vs) {
        $cand = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
        if (Test-Path $cand) { $vcvars = $cand }
    }
}
if (-not $vcvars) {
    $cand = 'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat'
    if (Test-Path $cand) { $vcvars = $cand }
}
if (-not $vcvars) {
    Write-Error 'vcvars64.bat not found - install the MSVC build tools (Visual Studio C++ workload)'
    exit 1
}
if (Get-Process zgui -ErrorAction SilentlyContinue) {
    Write-Error 'zgui.exe is running - close the application before building'
    exit 1
}

Write-Host "MSVC environment: $vcvars"
# tauri пишет служебные строки в stderr, а $ErrorActionPreference='Stop' превращает
# любой stderr от нативной команды в остановку. Приводим вывод к строке и решаем
# по коду возврата.
$ErrorActionPreference = 'Continue'
& cmd.exe /c "call `"$vcvars`" >nul 2>&1 && npx tauri build" 2>&1 |
    ForEach-Object { Write-Host $_ }
$code = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($code -ne 0) { exit $code }

# Тестовая сборка: portable-приложению нужен data\ рядом с exe, иначе движки
# не обнаруживаются. Раскладываем в .test\<дата-время>, состояние не тащим.
$root = Split-Path -Parent $PSScriptRoot
$stamp = Get-Date -Format 'yyyy-MM-dd-HHmm'

# Версия в README правится в трёх местах: бейдж «Version», подпись кнопки загрузки
# и ссылка на архив. Подставляем автоматически, чтобы под каждый релиз не искать
# их руками. Источник истины — tauri.conf.json.
$readme = Join-Path $root 'README.md'
if (Test-Path -LiteralPath $readme) {
    $conf = Get-Content -LiteralPath "$root\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json
    $ver = [string]$conf.version
    if ($ver -match '^\d+\.\d+\.\d+$') {
        $head = [System.IO.File]::ReadAllBytes($readme)
        $hasBom = $head.Length -ge 3 -and $head[0] -eq 0xEF -and $head[1] -eq 0xBB -and $head[2] -eq 0xBF
        $text = [System.IO.File]::ReadAllText($readme, (New-Object System.Text.UTF8Encoding($false)))
        $num = '\d+\.\d+\.\d+'
        # Имя архива короче тега: тег v1.3.0, файл Z-GUI.Stable.1.3.zip.
        $fileVer = $ver -replace '\.0$', ''
        $before = $text
        $text = [regex]::Replace($text, "badge/version-$num-success", "badge/version-$ver-success")
        # Подпись кнопки — «Скачать»; в README она встречается дважды: текстом
        # и в URL-кодировке shields.io. Ищем оба вида.
        $text = [regex]::Replace($text, "Скачать Z-GUI $num", "Скачать Z-GUI $ver")
        $text = [regex]::Replace($text, "Скачать-Z--GUI%20$num", "Скачать-Z--GUI%20$ver")
        $text = [regex]::Replace($text, "releases/download/v$num/Z-GUI\.Stable\.[0-9.]+\.zip", "releases/download/v$ver/Z-GUI.Stable.$fileVer.zip")
        if ($text -ne $before) {
            [System.IO.File]::WriteAllText($readme, $text, (New-Object System.Text.UTF8Encoding($hasBom)))
            Write-Host "README: версия обновлена на $ver"
        } else {
            Write-Host "README: версия уже $ver"
        }
    } else {
        Write-Host "README: версия в tauri.conf.json не распознана ('$ver') — пропущено"
    }
}

$dest = "E:\Base\opencode\.test\$stamp"
New-Item -ItemType Directory -Path $dest -Force | Out-Null
Copy-Item -LiteralPath "$root\src-tauri\target\release\zgui.exe" -Destination $dest -Force
New-Item -ItemType Directory -Path (Join-Path $dest 'data') -Force | Out-Null
foreach ($part in 'engines', 'catalog') {
    $from = Join-Path $root "data\$part"
    if (Test-Path -LiteralPath $from) {
        Copy-Item -LiteralPath $from -Destination (Join-Path $dest 'data') -Recurse -Force
    }
}

Write-Host "`nТестовая сборка: $dest"
exit 0
