$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Definition

# 0. Build the Telegram bridge (embedded as a sub-exe).
cargo build --manifest-path "$root\..\src-tauri\Cargo.toml" -p tg-ws-proxy-rs --bin zgui-bridge --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

# 1. Build App (pulls in Core via project reference).
dotnet build "$root\src\ZapretGui.App" -c Release
if ($LASTEXITCODE -ne 0) { throw "build failed" }

$appExe  = "$root\src\ZapretGui.App\bin\Release\net48\ZapretGui.App.exe"
$coreDll = "$root\src\ZapretGui.Core\bin\Release\net48\ZapretGui.Core.dll"

# 2. Find ILRepack in the nuget cache (referenced by ZapretGui.App.csproj).
$ilrepack = Get-ChildItem "$env:USERPROFILE\.nuget\packages\ilrepack\*\tools\ILRepack.exe" -ErrorAction SilentlyContinue |
    Sort-Object -Property Name |
    Select-Object -Last 1
if (-not $ilrepack) { throw "ILRepack.exe not found in nuget cache" }

# 3. Merge Core into App -> single ZapretGui.exe. Embedded resources (engine
#    zips, tg-ws-proxy.exe) are added as EmbeddedResource in the csproj and
#    travel inside the merged exe automatically.
$artifacts = "$root\artifacts"
New-Item -ItemType Directory -Force -Path $artifacts | Out-Null
& $ilrepack.FullName "/out:$artifacts\ZapretGui.exe" $appExe $coreDll /target:winexe /wildcards
if ($LASTEXITCODE -ne 0) { throw "ILRepack failed" }

Write-Host "Built $artifacts\ZapretGui.exe"
