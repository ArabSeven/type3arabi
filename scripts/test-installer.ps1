# test-installer.ps1 - install / upgrade / uninstall check of a Type3arabi MSI on a real machine
# (docs/08, release checklist docs/07 §6). Needs one UAC prompt (per-machine install). Nothing is typed
# anywhere and no app is closed; the report says what it found.
#
#   powershell -ExecutionPolicy Bypass -File .\scripts\test-installer.ps1 -Msi target\installer\Type3arabi-1.0.0-rc.2-x64.msi
#   ... -Uninstall      also uninstall silently, check that everything is gone, then reinstall silently
#
# What it checks (silent, like the Microsoft Store runs an MSI: msiexec /i <msi> /qn):
#   1. Apps that have the keyboard DLL loaded keep running through the upgrade (Restart Manager must
#      not close them), plus a stand-in process that holds t3a_tip.dll mapped like an app does.
#   2. No restart is started or scheduled by the installer (exit code 0 or 3010 = "restart needed").
#   3. Installed state: version in Settings > Apps, files and versions, COM + TSF registration (x64 and
#      x86), startup entry, shortcuts, and that Type3arabi is still in your keyboards.
#   4. (-Uninstall) files, registration, startup entry, shortcuts and ARP entry are gone; your
#      %LOCALAPPDATA%\Type3arabi (settings, learned words) is kept. Then it reinstalls.
# Report: %TEMP%\t3a-installer-test\report.txt (+ the msiexec logs).
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$Msi, [switch]$Uninstall)

$ErrorActionPreference = "Stop"
$Msi = (Resolve-Path $Msi).Path
$me = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $me.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    # One UAC prompt; the elevated copy writes the report and this window waits for it.
    $argList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"", "-Msi", "`"$Msi`"")
    if ($Uninstall) { $argList += "-Uninstall" }
    $p = Start-Process powershell -Verb RunAs -ArgumentList $argList -Wait -PassThru -WindowStyle Hidden
    Get-Content (Join-Path $env:TEMP "t3a-installer-test\report.txt") -ErrorAction SilentlyContinue
    exit $p.ExitCode
}

$Out = Join-Path $env:TEMP "t3a-installer-test"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$report = Join-Path $Out "report.txt"
"Type3arabi installer test  $(Get-Date -Format s)  $Msi" | Set-Content $report -Encoding UTF8
$failed = 0
function Log($s) { $s | Add-Content $report -Encoding UTF8 }
function Check($ok, $what) {
    if ($ok) { Log "PASS  $what" } else { Log "FAIL  $what"; $script:failed++ }
}

$Install = Join-Path ${env:ProgramW6432} "Type3arabi"
$Clsid = "{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}"
$ProfileGuid = "{90D49398-54D3-4F08-9C15-0B38D0820A87}"
$Upgrade = "{C71F4AAE-43FF-47DA-B6F0-582B0611BAF6}"

function Installed {
    Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -like "Type3arabi*" }
}
function Holders {
    # Processes with the keyboard DLL loaded (any architecture folder).
    @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Modules | Where-Object { $_.FileName -like "$Install\*\t3a_tip.dll" } } catch { $null }
    } | Select-Object Id, ProcessName)
}
function MsiVersionOf($path) {
    $i = New-Object -ComObject WindowsInstaller.Installer
    $db = $i.GetType().InvokeMember("OpenDatabase", "InvokeMethod", $null, $i, @($path, 0))
    $v = $db.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $db, @("SELECT Value FROM Property WHERE Property='ProductVersion'"))
    $v.GetType().InvokeMember("Execute", "InvokeMethod", $null, $v, $null)
    $r = $v.GetType().InvokeMember("Fetch", "InvokeMethod", $null, $v, $null)
    $r.GetType().InvokeMember("StringData", "GetProperty", $null, $r, 1)
}
function RestartPending {
    # A restart started by the installer (shutdown /r) shows up as a pending shutdown: "shutdown /a"
    # cancels it (and fails with 1116 when there is none), so this also protects the machine.
    & "$env:SystemRoot\System32\shutdown.exe" /a 2>$null
    $LASTEXITCODE -eq 0
}
function StandIn {
    # A process that maps t3a_tip.dll as an image (like an app that loaded the keyboard) without running
    # any of its code (DONT_RESOLVE_DLL_REFERENCES), so the upgrade finds the file in use.
    $dll = Join-Path $Install "x64\t3a_tip.dll"
    if (-not (Test-Path $dll)) { return $null }
    $code = "Add-Type -Name K -Namespace W -MemberDefinition '[DllImport(`"kernel32`", CharSet = CharSet.Unicode)] public static extern System.IntPtr LoadLibraryExW(string f, System.IntPtr h, uint fl);'; [W.K]::LoadLibraryExW('$dll', [System.IntPtr]::Zero, 1) | Out-Null; Start-Sleep -Seconds 600"
    $enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($code))
    Start-Process powershell -ArgumentList "-NoProfile", "-WindowStyle", "Hidden", "-EncodedCommand", $enc -WindowStyle Hidden -PassThru
}
function InstalledState($label) {
    Log "== Installed state ($label)"
    $arp = Installed
    $want = MsiVersionOf $Msi
    Check ($arp -and $arp.DisplayVersion -eq $want) "Settings > Apps shows Type3arabi $want (got $($arp.DisplayVersion))"
    foreach ($f in "x64\t3a_tip.dll", "x86\t3a_tip.dll", "t3a-hotkey.exe", "Type3arabi Settings.exe", "type3arabi.dat", "LICENSE.txt", "NOTICE.md", "DATASETS.md") {
        Check (Test-Path (Join-Path $Install $f)) "file $f"
    }
    Check ((Get-ItemProperty "Registry::HKEY_CLASSES_ROOT\CLSID\$Clsid\InprocServer32" -ErrorAction SilentlyContinue).'(default)' -like "*x64\t3a_tip.dll") "COM x64 InprocServer32 -> x64\t3a_tip.dll"
    Check ((Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$Clsid\InprocServer32" -ErrorAction SilentlyContinue).'(default)' -like "*x86\t3a_tip.dll") "COM x86 InprocServer32 -> x86\t3a_tip.dll"
    Check (Test-Path "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$Clsid\LanguageProfile\0x00000401\$ProfileGuid") "TSF profile registered (ar-SA only)"
    $langs = @(Get-ChildItem "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$Clsid\LanguageProfile" -ErrorAction SilentlyContinue)
    Check ($langs.Count -eq 1) "exactly one language profile (got $($langs.Count))"
    Check ((Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -ErrorAction SilentlyContinue).'Type3arabi Hotkey' -like "*t3a-hotkey.exe*") "startup entry (HKLM Run)"
    Check (Test-Path (Join-Path ([Environment]::GetFolderPath("CommonPrograms")) "Type3arabi Settings.lnk")) "Start menu shortcut"
    $keyboards = & (Join-Path $Install "t3a-hotkey.exe") --list-profiles | Out-String
    Log "      keyboards for $env:USERNAME:`n$keyboards"
}

Log "== Before"
$before = Installed
Log "      installed: $($before.DisplayName) $($before.DisplayVersion) $($before.PSChildName)"
$apps = Holders
Log "      apps with the keyboard loaded: $(($apps | ForEach-Object { "$($_.ProcessName)($($_.Id))" }) -join ', ')"
$stand = StandIn
Start-Sleep -Seconds 2
if ($stand) { Log "      stand-in holder: pid $($stand.Id)" }
$null = RestartPending  # clear any restart that was already pending, so the check below is ours

Log "== Silent install/upgrade: msiexec /i /qn"
$t0 = Get-Date
$p = Start-Process msiexec.exe -ArgumentList "/i", "`"$Msi`"", "/qn", "/l*v", "`"$Out\install.log`"" -Wait -PassThru
Log "      exit code $($p.ExitCode) after $([int]((Get-Date) - $t0).TotalSeconds) s"
Check ($p.ExitCode -in 0, 3010) "msiexec succeeded (0, or 3010 = restart needed to finish replacing files in use)"
Start-Sleep -Seconds 3
Check (-not (RestartPending)) "no restart started or scheduled by the installer"
$gone = @($apps | Where-Object { -not (Get-Process -Id $_.Id -ErrorAction SilentlyContinue) })
Check ($gone.Count -eq 0) "every app that had the keyboard loaded is still running ($($apps.Count) checked; closed: $(($gone | ForEach-Object { $_.ProcessName }) -join ', '))"
if ($stand) { Check (-not $stand.HasExited) "stand-in process holding t3a_tip.dll is still running" }
$log = Get-Content "$Out\install.log" -Raw -Encoding Unicode
Check ($log -match 'Property\(S\): MSIRESTARTMANAGERCONTROL = Disable') "log: Restart Manager disabled"
Check ($log -match 'Property\(S\): REBOOT = ReallySuppress') "log: REBOOT = ReallySuppress"
Check ($log -notmatch 'RestartManager.*shut ?down|Shutting down') "log: no application shutdown"
InstalledState "after upgrade"

if ($Uninstall) {
    Log "== Silent uninstall: msiexec /x /qn"
    $code = (Installed).PSChildName
    $p = Start-Process msiexec.exe -ArgumentList "/x", $code, "/qn", "/l*v", "`"$Out\uninstall.log`"" -Wait -PassThru
    Check ($p.ExitCode -in 0, 3010) "uninstall exit code $($p.ExitCode)"
    Check (-not (RestartPending)) "no restart started by the uninstall"
    Check (-not (Installed)) "Settings > Apps entry gone"
    Check (-not (Test-Path "Registry::HKEY_CLASSES_ROOT\CLSID\$Clsid")) "COM x64 registration gone"
    Check (-not (Test-Path "HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$Clsid")) "COM x86 registration gone"
    Check (-not (Test-Path "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$Clsid")) "TSF registration gone"
    Check (-not (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -ErrorAction SilentlyContinue).'Type3arabi Hotkey') "startup entry gone"
    Check (-not (Test-Path "HKLM:\SOFTWARE\Type3arabi")) "HKLM\Software\Type3arabi gone"
    Check (-not (Test-Path (Join-Path ([Environment]::GetFolderPath("CommonPrograms")) "Type3arabi Settings.lnk"))) "Start menu shortcut gone"
    Check (-not (Test-Path (Join-Path ([Environment]::GetFolderPath("CommonDesktopDirectory")) "Type3arabi Settings.lnk"))) "desktop shortcut gone"
    $left = @(Get-ChildItem $Install -Recurse -File -ErrorAction SilentlyContinue)
    Log "      files left in $Install (in use until the next restart): $(($left | ForEach-Object { $_.Name }) -join ', ')"
    Check (@($left | Where-Object { $_.Name -notlike "t3a_tip.dll" }).Count -eq 0) "only in-use keyboard DLLs may remain until restart"
    Check (Test-Path (Join-Path $env:LOCALAPPDATA "Type3arabi")) "your settings and learned words are kept (%LOCALAPPDATA%\Type3arabi)"
    Log "== Reinstall: msiexec /i /qn"
    $p = Start-Process msiexec.exe -ArgumentList "/i", "`"$Msi`"", "/qn", "/l*v", "`"$Out\reinstall.log`"" -Wait -PassThru
    Check ($p.ExitCode -in 0, 3010) "reinstall exit code $($p.ExitCode)"
    InstalledState "after reinstall"
}

if ($stand -and -not $stand.HasExited) { Stop-Process -Id $stand.Id -Force }
Log ""
Log $(if ($failed) { "$failed check(s) FAILED" } else { "All checks passed" })
Log "A restart finishes replacing files that were in use (exit code 3010)."
exit $failed
