# dev-install.ps1 — Type3arabi local development installation script
[CmdletBinding()]
param(
    [string]$Target = "x86_64",
    [switch]$SkipBuild,
    [switch]$SetDefault
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

Write-Host "=== Type3arabi Development Installer ===" -ForegroundColor Cyan

# 1. Build if needed
if (-not $SkipBuild) {
    Write-Host "[1/5] Building release DLLs..." -ForegroundColor Yellow
    cargo build -p t3a-tip --release --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "x86_64 build failed" }
    
    cargo build -p t3a-tip --release --target i686-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "i686 build failed" }
} else {
    Write-Host "[1/5] Skipping build (using existing binaries)" -ForegroundColor Gray
}

$x64Dll = Join-Path $RepoRoot "target\x86_64-pc-windows-msvc\release\t3a_tip.dll"
$x86Dll = Join-Path $RepoRoot "target\i686-pc-windows-msvc\release\t3a_tip.dll"

if (-not (Test-Path $x64Dll)) { throw "Missing $x64Dll" }

# 2. Register COM / TSF in 64-bit view
Write-Host "[2/5] Registering 64-bit TIP DLL..." -ForegroundColor Yellow
Start-Process -FilePath "regsvr32.exe" -ArgumentList "/s `"$x64Dll`"" -Wait -NoNewWindow

# 3. Register COM / TSF in 32-bit (SysWOW64) view if present
if (Test-Path $x86Dll) {
    Write-Host "[3/5] Registering 32-bit (WOW64) TIP DLL..." -ForegroundColor Yellow
    $syswow64Regsvr = Join-Path $env:SystemRoot "SysWOW64\regsvr32.exe"
    if (Test-Path $syswow64Regsvr) {
        Start-Process -FilePath $syswow64Regsvr -ArgumentList "/s `"$x86Dll`"" -Wait -NoNewWindow
    }
}

# 4. Enable layout profile via InstallLayoutOrTip
Write-Host "[4/5] Enabling input layout (AR - Type3arabi)..." -ForegroundColor Yellow

$InputSource = @"
using System;
using System.Runtime.InteropServices;

public static class NativeInput {
    [DllImport("input.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool InstallLayoutOrTip(string psz, uint dwFlags);
}
"@

if (-not ([System.Management.Automation.PSTypeName]'NativeInput').Type) {
    Add-Type -TypeDefinition $InputSource
}

$Clsid = "{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}"
$Profile = "{90D49398-54D3-4F08-9C15-0B38D0820A87}"
$LayoutString = "0401:$Clsid$Profile"

$ILOT_INSTALL = 0x00000001
$ILOT_DEFPROFILE = 0x00000008

$flags = $ILOT_INSTALL
if ($SetDefault) {
    $flags = $flags -bor $ILOT_DEFPROFILE
}

$res = [NativeInput]::InstallLayoutOrTip($LayoutString, [uint32]$flags)
if (-not $res) {
    Write-Warning "InstallLayoutOrTip returned false; checking if already registered"
}

# 5. Set AppContainer permissions for AppData\Type3arabi
Write-Host "[5/5] Ensuring AppContainer permissions on user data folder..." -ForegroundColor Yellow
$UserDataDir = Join-Path $env:APPDATA "Type3arabi"
if (-not (Test-Path $UserDataDir)) {
    New-Item -ItemType Directory -Path $UserDataDir -Force | Out-Null
}

$appContainerSids = @(
    "S-1-15-2-1",  # ALL APPLICATION PACKAGES
    "S-1-15-2-2"   # ALL RESTRICTED APPLICATION PACKAGES
)

foreach ($sid in $appContainerSids) {
    Start-Process -FilePath "icacls.exe" -ArgumentList "`"$UserDataDir`" /grant *${sid}:(OI)(CI)(RX) /T /Q" -Wait -NoNewWindow
}

Write-Host "=== Type3arabi registered and enabled successfully! ===" -ForegroundColor Green
Write-Host "Profile: AR - Type3arabi (Saudi Arabic 0401)"
Write-Host "Switch to it with Win + Space or Alt + Shift."
