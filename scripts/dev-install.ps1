# dev-install.ps1 - install a local development build of Type3arabi.
#
# Result: exactly ONE Arabic entry in the Windows input switcher (Win+Space):
#   "Arabic (Saudi Arabia) - Type3arabi"   (the ar-SA language with only the Type3arabi keyboard)
#
# Steps:
#   1. Build the x64 + x86 TIP DLLs as the current (non-admin) user, into target\tip so the DLLs
#      Windows keeps loaded are never the ones cargo rebuilds.
#   2. Elevate and copy DLLs + type3arabi.dat to %ProgramFiles%\Type3arabi (readable by every app,
#      including sandboxed ones), then register the COM server / TSF profile from there.
#   3. Configure the user's language list: ar-SA with only the Type3arabi keyboard.
#
# Run from the repository root:
#   powershell -ExecutionPolicy Bypass -File .\scripts\dev-install.ps1
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$Elevated
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$Clsid = "{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}"
$ProfileGuid = "{90D49398-54D3-4F08-9C15-0B38D0820A87}"
$Tip = "0401:$Clsid$ProfileGuid"
$InstallDir = Join-Path $env:ProgramW6432 "Type3arabi"
if (-not $env:ProgramW6432) { $InstallDir = Join-Path $env:ProgramFiles "Type3arabi" }
$BuildDir = Join-Path $RepoRoot "target\tip"

function Test-Admin {
    ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

# ---------------------------------------------------------------------------------------------
# Phase 1 (normal user): build, then re-launch elevated for the machine-wide steps.
if (-not $Elevated) {
    Write-Host "=== Type3arabi dev install ===" -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        if (-not $SkipBuild) {
            Write-Host "[1/4] Building TIP DLLs (x64, x86)..." -ForegroundColor Yellow
            cargo build -p t3a-tip --release --target x86_64-pc-windows-msvc --target-dir $BuildDir
            if ($LASTEXITCODE -ne 0) { throw "x64 build failed" }
            cargo build -p t3a-tip --release --target i686-pc-windows-msvc --target-dir $BuildDir
            if ($LASTEXITCODE -ne 0) { throw "x86 build failed" }
        }
        $dat = Join-Path $RepoRoot "target\type3arabi.dat"
        if (-not (Test-Path $dat)) {
            Write-Host "      Building type3arabi.dat..." -ForegroundColor Yellow
            cargo run -p t3a-cli --release -- build-data --out $dat
            if ($LASTEXITCODE -ne 0) { throw "build-data failed" }
        }
    } finally {
        Pop-Location
    }

    if (Test-Admin) {
        & $PSCommandPath -Elevated -SkipBuild
        exit $LASTEXITCODE
    }
    Write-Host "Requesting administrator rights to register the input method..." -ForegroundColor Cyan
    $argList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"", "-Elevated", "-SkipBuild")
    $proc = Start-Process powershell.exe -ArgumentList $argList -Verb RunAs -Wait -PassThru
    exit $proc.ExitCode
}

# ---------------------------------------------------------------------------------------------
# Phase 2 (elevated)
$log = Join-Path $env:TEMP "type3arabi-dev-install.log"
Start-Transcript -Path $log -Force | Out-Null
try {
    $srcX64 = Join-Path $BuildDir "x86_64-pc-windows-msvc\release\t3a_tip.dll"
    $srcX86 = Join-Path $BuildDir "i686-pc-windows-msvc\release\t3a_tip.dll"
    $srcDat = Join-Path $RepoRoot "target\type3arabi.dat"
    foreach ($f in @($srcX64, $srcX86, $srcDat)) {
        if (-not (Test-Path $f)) { throw "Missing $f - run without -SkipBuild first." }
    }

    Write-Host "[2/4] Copying files to $InstallDir ..." -ForegroundColor Yellow
    $stamp = Get-Date -Format "yyyyMMddHHmmss"
    function Copy-Replacing($src, $dst) {
        New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
        if (Test-Path $dst) {
            # A loaded DLL cannot be overwritten but can be renamed; the old copy is removed later.
            try { Remove-Item $dst -Force } catch { Rename-Item $dst "$dst.old-$stamp" -Force }
        }
        Copy-Item $src $dst -Force
    }
    $dstX64 = Join-Path $InstallDir "x64\t3a_tip.dll"
    $dstX86 = Join-Path $InstallDir "x86\t3a_tip.dll"
    Copy-Replacing $srcX64 $dstX64
    Copy-Replacing $srcX86 $dstX86
    Copy-Replacing $srcDat (Join-Path $InstallDir "type3arabi.dat")
    Get-ChildItem $InstallDir -Recurse -Filter "*.old-*" | ForEach-Object {
        try { Remove-Item $_.FullName -Force } catch { }
    }

    Write-Host "[3/4] Registering the input method (one Arabic profile)..." -ForegroundColor Yellow
    $regsvr64 = Join-Path $env:SystemRoot "System32\regsvr32.exe"
    $regsvr32 = Join-Path $env:SystemRoot "SysWOW64\regsvr32.exe"
    # Unregister first: the new DllUnregisterServer also removes profiles older builds created
    # under 16 Arabic LANGIDs.
    foreach ($pair in @(@($regsvr64, $dstX64), @($regsvr32, $dstX86))) {
        $exe, $dll = $pair
        if (-not (Test-Path $exe)) { continue }
        Start-Process $exe -ArgumentList "/s /u `"$dll`"" -Wait -NoNewWindow
        $p = Start-Process $exe -ArgumentList "/s `"$dll`"" -Wait -NoNewWindow -PassThru
        if ($p.ExitCode -ne 0) { throw "regsvr32 failed for $dll (exit $($p.ExitCode))" }
    }

    Write-Host "[4/4] Configuring your language list..." -ForegroundColor Yellow
    $list = Get-WinUserLanguageList
    foreach ($lang in @($list)) {
        # Drop our keyboard from any other Arabic variant (left over from older builds).
        if ($lang.LanguageTag -ne "ar-SA") {
            foreach ($t in @($lang.InputMethodTips)) {
                if ($t -like "*$Clsid*") { [void]$lang.InputMethodTips.Remove($t) }
            }
        }
    }
    foreach ($lang in @($list)) {
        if ($lang.LanguageTag -like "ar*" -and $lang.LanguageTag -ne "ar-SA" -and $lang.InputMethodTips.Count -eq 0) {
            [void]$list.Remove($lang)
        }
    }
    $ar = $list | Where-Object { $_.LanguageTag -eq "ar-SA" } | Select-Object -First 1
    if (-not $ar) {
        $list.Add("ar-SA")
        $ar = $list | Where-Object { $_.LanguageTag -eq "ar-SA" } | Select-Object -First 1
    }
    $ar.InputMethodTips.Clear()
    $ar.InputMethodTips.Add($Tip)
    Set-WinUserLanguageList $list -Force

    $check = Get-WinUserLanguageList | Where-Object { $_.LanguageTag -eq "ar-SA" }
    if (-not ($check.InputMethodTips -contains $Tip)) {
        throw "Windows did not accept the Type3arabi keyboard in the language list."
    }

    # Per-user data folder readable by sandboxed (AppContainer) apps.
    $userDir = Join-Path $env:LOCALAPPDATA "Type3arabi"
    New-Item -ItemType Directory -Force -Path $userDir | Out-Null
    foreach ($sid in @("S-1-15-2-1", "S-1-15-2-2")) {
        & icacls.exe $userDir /grant "*${sid}:(OI)(CI)(RX)" /T /Q | Out-Null
    }

    Write-Host ""
    Write-Host "=== Type3arabi installed ===" -ForegroundColor Green
    Write-Host "Languages now:"
    Get-WinUserLanguageList | ForEach-Object { Write-Host ("  {0,-8} {1}" -f $_.LanguageTag, ($_.InputMethodTips -join ", ")) }
    Write-Host ""
    Write-Host "Switch with Win+Space to 'Arabic (Saudi Arabia) - Type3arabi', then type in Notepad."
    Write-Host "Apps that were already open load the new DLL after they are restarted."
    Read-Host "Press Enter to close"
    exit 0
} catch {
    Write-Host "INSTALL FAILED: $_" -ForegroundColor Red
    Write-Host "Log: $log"
    Read-Host "Press Enter to close"
    exit 1
} finally {
    Stop-Transcript | Out-Null
}
