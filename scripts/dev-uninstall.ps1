# dev-uninstall.ps1 — Type3arabi local development uninstallation script
[CmdletBinding()]
param(
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

Write-Host "=== Type3arabi Development Uninstaller ===" -ForegroundColor Cyan

# 0. Check for Administrator privileges
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    if ($NoElevate) {
        Write-Warning "Running without Administrator privileges. Unregistering Windows TSF TIP in HKLM requires elevation."
    } else {
        Write-Host "Requesting Administrator privileges to unregister Windows TSF Text Input Processor..." -ForegroundColor Cyan
        $proc = Start-Process powershell.exe -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"") -Verb RunAs -Wait -PassThru
        exit $proc.ExitCode
    }
}

# 1. Disable layout via InstallLayoutOrTip
Write-Host "[1/3] Disabling input layout..." -ForegroundColor Yellow

$InputSource = @"
using System;
using System.Runtime.InteropServices;

public static class NativeInputUninstall {
    [DllImport("input.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool InstallLayoutOrTip(string psz, uint dwFlags);
}
"@

if (-not ([System.Management.Automation.PSTypeName]'NativeInputUninstall').Type) {
    Add-Type -TypeDefinition $InputSource
}

$Clsid = "{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}"
$Profile = "{90D49398-54D3-4F08-9C15-0B38D0820A87}"
$LayoutString = "0401:$Clsid$Profile"

$ILOT_UNINSTALL = 0x00000002

$res = [NativeInputUninstall]::InstallLayoutOrTip($LayoutString, [uint32]$ILOT_UNINSTALL)

# 2. Unregister 64-bit DLL
$x64Dll = Join-Path $RepoRoot "target\x86_64-pc-windows-msvc\release\t3a_tip.dll"
if (Test-Path $x64Dll) {
    Write-Host "[2/3] Unregistering 64-bit TIP DLL..." -ForegroundColor Yellow
    Start-Process -FilePath "regsvr32.exe" -ArgumentList "/u /s `"$x64Dll`"" -Wait -NoNewWindow
}

# 3. Unregister 32-bit (SysWOW64) DLL
$x86Dll = Join-Path $RepoRoot "target\i686-pc-windows-msvc\release\t3a_tip.dll"
$syswow64Regsvr = Join-Path $env:SystemRoot "SysWOW64\regsvr32.exe"
if ((Test-Path $x86Dll) -and (Test-Path $syswow64Regsvr)) {
    Write-Host "[3/3] Unregistering 32-bit (WOW64) TIP DLL..." -ForegroundColor Yellow
    Start-Process -FilePath $syswow64Regsvr -ArgumentList "/u /s `"$x86Dll`"" -Wait -NoNewWindow
}

Write-Host "=== Type3arabi uninstalled successfully! ===" -ForegroundColor Green
