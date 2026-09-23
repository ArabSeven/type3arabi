# build-installer.ps1 - build the Type3arabi MSI (docs/07 §5) with WiX v5.
#
# Prerequisites (docs/07 §1): dotnet tool install --global wix --version 5.0.2
#   wix extension add -g WixToolset.UI.wixext/5.0.2 ; wix extension add -g WixToolset.Util.wixext/5.0.2
# The data file must exist: target\type3arabi.dat (pipeline + build-data, docs/04).
#
# Run from the repository root (no admin needed to build):
#   powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1
# Output: target\installer\Type3arabi-<version>-x64.msi
[CmdletBinding()]
param([switch]$SkipBuild)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$Version = "0.1.0"
$BuildDir = Join-Path $RepoRoot "target\tip"
$SettingsDir = Join-Path $RepoRoot "target\settings"
$Payload = Join-Path $RepoRoot "target\installer\payload"
$Out = Join-Path $RepoRoot "target\installer\Type3arabi-$Version-x64.msi"
$env:PATH = "$env:PATH;$env:USERPROFILE\.dotnet\tools"

Push-Location $RepoRoot
try {
    if (-not $SkipBuild) {
        Write-Host "[1/3] Building release binaries..." -ForegroundColor Yellow
        cargo build -p t3a-tip --release --target x86_64-pc-windows-msvc --target-dir $BuildDir
        if ($LASTEXITCODE -ne 0) { throw "x64 TIP build failed" }
        cargo build -p t3a-tip --release --target i686-pc-windows-msvc --target-dir $BuildDir
        if ($LASTEXITCODE -ne 0) { throw "x86 TIP build failed" }
        cargo build -p t3a-hotkey --release --target x86_64-pc-windows-msvc --target-dir $BuildDir
        if ($LASTEXITCODE -ne 0) { throw "hotkey build failed" }
        Push-Location (Join-Path $RepoRoot "apps\settings")
        cargo build --release --target-dir $SettingsDir
        $code = $LASTEXITCODE
        Pop-Location
        if ($code -ne 0) { throw "settings build failed" }
    }

    Write-Host "[2/3] Assembling payload..." -ForegroundColor Yellow
    $dat = Join-Path $RepoRoot "target\type3arabi.dat"
    if (-not (Test-Path $dat)) { throw "Missing $dat (run the data pipeline and build-data first)" }
    Remove-Item $Payload -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $Payload "x64"), (Join-Path $Payload "x86") | Out-Null
    Copy-Item (Join-Path $BuildDir "x86_64-pc-windows-msvc\release\t3a_tip.dll") (Join-Path $Payload "x64\t3a_tip.dll")
    Copy-Item (Join-Path $BuildDir "i686-pc-windows-msvc\release\t3a_tip.dll") (Join-Path $Payload "x86\t3a_tip.dll")
    Copy-Item (Join-Path $BuildDir "x86_64-pc-windows-msvc\release\t3a-hotkey.exe") $Payload
    Copy-Item (Join-Path $SettingsDir "release\type3arabi-settings.exe") (Join-Path $Payload "Type3arabi Settings.exe")
    Copy-Item $dat $Payload
    Copy-Item (Join-Path $RepoRoot "NOTICE.md") $Payload
    Copy-Item (Join-Path $RepoRoot "installer\License.rtf") $Payload
    Copy-Item (Join-Path $RepoRoot "apps\settings\icons\icon.ico") $Payload

    # A data file built with `internal` sources must say so everywhere it goes (AGENTS.md R14).
    $internal = Select-String -Path $dat -Pattern '"distribution": "internal-only"' -SimpleMatch -Quiet
    $name = if ($internal) { "Type3arabi (internal build)" } else { "Type3arabi" }

    Write-Host "[3/3] Building MSI ($name)..." -ForegroundColor Yellow
    wix build (Join-Path $RepoRoot "installer\Type3arabi.wxs") -arch x64 `
        -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
        -d "Payload=$Payload" -d "Version=$Version" -d "ProductName=$name" -o $Out
    if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
    Write-Host "=== Built $Out ===" -ForegroundColor Green
} finally {
    Pop-Location
}
