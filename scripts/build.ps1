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
& cmd.exe /c "call `"$vcvars`" >nul 2>&1 && npx tauri build"
exit $LASTEXITCODE
