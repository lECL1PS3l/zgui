# Скачивание движков для релизной сборки Z GUI (Windows PowerShell 5.1).
# Кладёт движки в data/engines/<id>/ рядом с exe: они попадают в дистрибутив.
# Запускается при подготовке релиза, НЕ на машинах пользователей
# (там движки качает fetch_engine по кнопке в GUI).
# ВАЖНО: zapret2 НЕ качается отсюда — релизный zip bol-van сносится Defender'ом.
# Наша сборка (winws2.exe из исходников + lua/ + files/ + cygwin1.dll +
# WinDivert) уже лежит в data/engines/zapret2/ дистрибутива и обновляется
# только вручную при выходе новой версии bol-van.
#
# Пример: powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1
#         powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1 -Engines zapret2

param(
    # По умолчанию — качаемые движки; flowseal вшит в exe (embedded.rs),
    # zapret2 — собственная сборка в data/engines.
    [string[]]$Engines = @("goodbyedpi", "dpibreak")
)

$ErrorActionPreference = "Stop"

$repos = @{
    "goodbyedpi" = "ValdikSS/GoodbyeDPI"
    "dpibreak"   = "dilluti0n/dpibreak"
}
$exes = @{
    "goodbyedpi" = "goodbyedpi.exe"
    "dpibreak"   = "dpibreak.exe"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem

function Remove-TopDir([string]$zipPath, [string]$dest) {
    # Распаковка со срезкой общего верхнего каталога (как unzip в zgui).
    $zip = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
    try {
        $top = $null
        foreach ($e in $zip.Entries) {
            $t = $e.FullName.TrimEnd("/")
            if ($t -eq "") { continue }
            $first = $t.Split("/")[0]
            if ($null -eq $top) { $top = $first } elseif ($top -ne $first) { $top = $null; break }
        }
        foreach ($e in $zip.Entries) {
            $rel = $e.FullName
            if ($null -ne $top) { $rel = $rel.TrimStart("$top/") }
            if ($rel.TrimEnd("/") -eq "") { continue }
            if ($rel -match "(\.\.|\\)") { continue }
            $out = Join-Path $dest $rel.Replace("/", "\")
            if ($e.FullName.EndsWith("/")) { New-Item -ItemType Directory -Path $out -Force | Out-Null; continue }
            New-Item -ItemType Directory -Path (Split-Path $out -Parent) -Force | Out-Null
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($e, $out, $true)
        }
    } finally { $zip.Dispose() }
}

function Test-ZipHasEntry([string]$zipPath, [string]$entrySuffix) {
    $zip = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
    try {
        foreach ($e in $zip.Entries) {
            if ($e.FullName -like "*$entrySuffix") { return $true }
        }
        return $false
    } finally { $zip.Dispose() }
}

function Find-EngineExe([string]$dir, [string]$exe) {
    Get-ChildItem -Path $dir -Recurse -Filter $exe -File -ErrorAction SilentlyContinue | Select-Object -First 1
}

$root = Split-Path $PSScriptRoot -Parent
$data = Join-Path $root "data\engines"
New-Item -ItemType Directory -Path $data -Force | Out-Null
$tmp = Join-Path $root "data\tmp"
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

foreach ($id in $Engines) {
    if (-not $repos.ContainsKey($id)) { Write-Warning "неизвестный движок: $id (пропущен)"; continue }
    $repo = $repos[$id]
    $exe = $exes[$id]
    $dest = Join-Path $data $id
    if (Find-EngineExe $dest $exe) { Write-Host "OK  $id уже установлен ($dest)" -ForegroundColor Green; continue }

    Write-Host "==> $id ($repo)"
    $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers @{ "User-Agent" = "zgui-fetch" }
    Write-Host "    релиз: $($rel.tag_name)"
    $assets = @($rel.assets | Where-Object { $_.name -like "*.zip" })
    if ($assets.Count -eq 0) { throw "$id`: в последнем релизе нет zip-архива — скачайте вручную" }

    # Приоритет кандидатов: имя содержит exe → «win» в имени → остальные.
    # Сначала пробуем zip с exe в имени, затем «win», затем остальные.
    $rank = { param($n) [int]($n -like "*$exe*") * 2 + [int]($n -like "*win*") }
    $byPriority = $assets | Sort-Object { & $rank $_.name } -Descending
    $chosen = $null
    foreach ($cand in $byPriority) {
        $f = Join-Path $tmp "cand-$id-$($cand.name)"
        Invoke-WebRequest -Uri $cand.browser_download_url -OutFile $f
        if (Test-ZipHasEntry $f $exe) { $chosen = $f; break }
        Remove-Item $f -Force -ErrorAction SilentlyContinue
    }
    if (-not $chosen) { throw "$id`: ни один zip релиза не содержит $exe" }

    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    Remove-TopDir $chosen $dest
    Remove-Item $chosen -Force
    $found = Find-EngineExe $dest $exe
    if (-not $found) { throw "$id`: после распаковки $exe не найден" }
    Write-Host "OK  $id -> $($found.FullName) ($($rel.tag_name))" -ForegroundColor Green
}

Write-Host "`nГотово. Движки в $data"
