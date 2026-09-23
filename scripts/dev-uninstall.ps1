# dev-uninstall.ps1 - remove every trace of a Type3arabi development install.
#
# Also cleans up the broken early dev build (profiles under 16 Arabic LANGIDs registered straight
# from target\..., 15 Arabic entries flooding the input switcher). Safe to run more than once.
#
# Run from the repository root:
#   powershell -ExecutionPolicy Bypass -File .\scripts\dev-uninstall.ps1
# Add -RemoveUserData to also delete %LOCALAPPDATA%\Type3arabi (learned words, logs).
[CmdletBinding()]
param(
    [switch]$RemoveUserData,
    [switch]$Elevated
)

$ErrorActionPreference = "Stop"
$Clsid = "{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}"
$ProfileGuid = "{90D49398-54D3-4F08-9C15-0B38D0820A87}"
$LegacyLangIds = @("0401", "0801", "0C01", "1001", "1401", "1801", "1C01", "2001", "2401", "2801",
                   "2C01", "3001", "3401", "3801", "3C01", "4001")
$InstallDir = Join-Path $env:ProgramW6432 "Type3arabi"
if (-not $env:ProgramW6432) { $InstallDir = Join-Path $env:ProgramFiles "Type3arabi" }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "Requesting administrator rights to unregister the input method..." -ForegroundColor Cyan
    $argList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"", "-Elevated")
    if ($RemoveUserData) { $argList += "-RemoveUserData" }
    $proc = Start-Process powershell.exe -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

$log = Join-Path $env:TEMP "type3arabi-dev-uninstall.log"
Start-Transcript -Path $log -Force | Out-Null
try {
    Write-Host "=== Type3arabi dev uninstall ===" -ForegroundColor Cyan

    # 1. Language list: remove our keyboard everywhere; drop Arabic languages left with no keyboard.
    Write-Host "[1/5] Cleaning the language list..." -ForegroundColor Yellow
    $list = Get-WinUserLanguageList
    foreach ($lang in @($list)) {
        foreach ($t in @($lang.InputMethodTips)) {
            if ($t -like "*$Clsid*") { [void]$lang.InputMethodTips.Remove($t) }
        }
        if ($lang.InputMethodTips.Count -eq 0) { [void]$list.Remove($lang) }
    }
    if ($list.Count -eq 0) { $list = New-WinUserLanguageList "en-US" }
    Set-WinUserLanguageList $list -Force

    # 2. Disable the profile for this user under every LANGID an old build used.
    Write-Host "[2/5] Disabling Type3arabi profiles..." -ForegroundColor Yellow
    if (-not ([System.Management.Automation.PSTypeName]'T3aInput').Type) {
        Add-Type -TypeDefinition @"
using System.Runtime.InteropServices;
public static class T3aInput {
    [DllImport("input.dll", CharSet = CharSet.Unicode)]
    public static extern bool InstallLayoutOrTip(string psz, uint dwFlags);
}
"@
    }
    $ILOT_UNINSTALL = 0x1
    foreach ($id in $LegacyLangIds) {
        [void][T3aInput]::InstallLayoutOrTip("${id}:$Clsid$ProfileGuid", $ILOT_UNINSTALL)
    }

    # 3. Unregister every registered copy of the DLL (old target\ builds and Program Files).
    Write-Host "[3/5] Unregistering the COM server and TSF profiles..." -ForegroundColor Yellow
    $regsvr64 = Join-Path $env:SystemRoot "System32\regsvr32.exe"
    $regsvr32 = Join-Path $env:SystemRoot "SysWOW64\regsvr32.exe"
    $dlls = @()
    foreach ($view in @(
            @("HKLM:\SOFTWARE\Classes\CLSID\$Clsid\InprocServer32", $regsvr64),
            @("HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$Clsid\InprocServer32", $regsvr32),
            @("HKCU:\SOFTWARE\Classes\CLSID\$Clsid\InprocServer32", $regsvr64))) {
        $key, $exe = $view
        $path = (Get-ItemProperty $key -ErrorAction SilentlyContinue).'(default)'
        if ($path) { $dlls += , @($exe, $path) }
    }
    $dlls += , @($regsvr64, (Join-Path $InstallDir "x64\t3a_tip.dll"))
    $dlls += , @($regsvr32, (Join-Path $InstallDir "x86\t3a_tip.dll"))
    foreach ($pair in $dlls) {
        $exe, $dll = $pair
        if ((Test-Path $exe) -and (Test-Path $dll)) {
            Write-Host "      regsvr32 /u $dll"
            Start-Process $exe -ArgumentList "/s /u `"$dll`"" -Wait -NoNewWindow
        }
    }
    # Whatever an unregister could not reach (missing DLL, crashed registration): delete directly.
    foreach ($k in @(
            "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$Clsid",
            "HKLM:\SOFTWARE\WOW6432Node\Microsoft\CTF\TIP\$Clsid",
            "HKCU:\SOFTWARE\Microsoft\CTF\TIP\$Clsid",
            "HKLM:\SOFTWARE\Classes\CLSID\$Clsid",
            "HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$Clsid",
            "HKCU:\SOFTWARE\Classes\CLSID\$Clsid")) {
        if (Test-Path $k) { Remove-Item $k -Recurse -Force; Write-Host "      removed $k" }
    }

    # 4. Keyboard list leftovers: Arabic layouts the old build left in Preload/Substitutes that are
    #    no longer backed by a language in the list.
    Write-Host "[4/5] Removing leftover Arabic keyboard entries..." -ForegroundColor Yellow
    $keep = @{}
    foreach ($lang in Get-WinUserLanguageList) {
        foreach ($t in $lang.InputMethodTips) { $keep[$t.Substring(0, 4).ToUpper()] = $true }
    }
    $preload = "HKCU:\Keyboard Layout\Preload"
    $subst = "HKCU:\Keyboard Layout\Substitutes"
    if (Test-Path $preload) {
        $vals = Get-ItemProperty $preload
        $kept = @()
        foreach ($p in $vals.PSObject.Properties | Where-Object { $_.Name -match '^\d+$' } | Sort-Object { [int]$_.Name }) {
            $lang = $p.Value.Substring(4, 4).ToUpper()
            $isArabic = $lang.EndsWith("01")
            if ($isArabic -and -not $keep.ContainsKey($lang)) { continue }
            $kept += $p.Value
        }
        if ($kept.Count -gt 0) {
            foreach ($p in $vals.PSObject.Properties | Where-Object { $_.Name -match '^\d+$' }) {
                Remove-ItemProperty $preload -Name $p.Name
            }
            for ($i = 0; $i -lt $kept.Count; $i++) {
                New-ItemProperty $preload -Name ($i + 1) -Value $kept[$i] -PropertyType String | Out-Null
            }
        }
    }
    if (Test-Path $subst) {
        foreach ($p in (Get-ItemProperty $subst).PSObject.Properties | Where-Object { $_.Name -match '^[0-9A-Fa-f]{8}$' }) {
            $lang = $p.Name.Substring(4, 4).ToUpper()
            if ($lang.EndsWith("01") -and -not $keep.ContainsKey($lang)) {
                Remove-ItemProperty $subst -Name $p.Name
            }
        }
    }

    # 5. Files.
    Write-Host "[5/5] Removing installed files..." -ForegroundColor Yellow
    if (Test-Path $InstallDir) {
        try {
            Remove-Item $InstallDir -Recurse -Force
        } catch {
            Write-Host "      Some files are still loaded by running apps; they are unregistered and" -ForegroundColor DarkYellow
            Write-Host "      harmless. Delete $InstallDir after signing out, or re-run this script." -ForegroundColor DarkYellow
        }
    }
    if ($RemoveUserData) {
        $userDir = Join-Path $env:LOCALAPPDATA "Type3arabi"
        if (Test-Path $userDir) { Remove-Item $userDir -Recurse -Force -ErrorAction SilentlyContinue }
    }

    Write-Host ""
    Write-Host "=== Type3arabi removed ===" -ForegroundColor Green
    Write-Host "Languages now:"
    Get-WinUserLanguageList | ForEach-Object { Write-Host ("  {0,-8} {1}" -f $_.LanguageTag, ($_.InputMethodTips -join ", ")) }
    Write-Host ""
    Write-Host "Sign out and back in (or restart) so every app drops the old DLL."
    Read-Host "Press Enter to close"
    exit 0
} catch {
    Write-Host "UNINSTALL FAILED: $_" -ForegroundColor Red
    Write-Host "Log: $log"
    Read-Host "Press Enter to close"
    exit 1
} finally {
    Stop-Transcript | Out-Null
}
