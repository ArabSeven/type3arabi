# validate-msi.ps1 - static checks of a built Type3arabi MSI, no installation and no elevation needed
# (docs/07 §5-6, docs/08). Used by the release workflow and before handing an RC to testers.
#
#   powershell -ExecutionPolicy Bypass -File .\scripts\validate-msi.ps1 target\installer\Type3arabi-1.0.0-rc.2-x64.msi
#     [-ModelSha256 <sha>] [-RequireSigned] [-SignerSubject "SignPath Foundation"] [-TestModel]
#
# Checks: package identity and version, silent-install properties (no Restart Manager shutdown, no
# automatic restart), every payload file present with the right architecture and version metadata,
# custom actions (registration, hidden console tools), startup entry, the model's META, ICE validation
# and Authenticode status. Exit code = number of failed checks.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Msi,
    [string]$ModelSha256,
    [switch]$RequireSigned,
    # CI smoke builds use the tiny fixture model: skip the release-model checks.
    [switch]$TestModel,
    [string]$SignerSubject = "SignPath Foundation"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$env:PATH = "$env:PATH;$env:USERPROFILE\.dotnet\tools"
$Msi = (Resolve-Path $Msi).Path
$failed = 0
function Check($ok, $what) {
    if ($ok) { Write-Host "PASS  $what" } else { Write-Host "FAIL  $what" -ForegroundColor Red; $script:failed++ }
}

$Version = (Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if ($Version -match '^(\d+\.\d+\.\d+)(?:-[0-9A-Za-z]+\.(\d+))?$') {
    $MsiVersion = if ($Matches[2]) { "$($Matches[1]).$($Matches[2])" } else { $Matches[1] }
} else { throw "bad workspace version $Version" }

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("t3a-msi-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    wix msi decompile $Msi -x $work -o (Join-Path $work "pkg.wxs") | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "wix msi decompile failed" }
    [xml]$x = Get-Content (Join-Path $work "pkg.wxs") -Raw -Encoding UTF8
    $ns = New-Object System.Xml.XmlNamespaceManager($x.NameTable)
    $ns.AddNamespace("w", "http://wixtoolset.org/schemas/v4/wxs")
    function Nodes($xpath) { $x.SelectNodes($xpath, $ns) | ForEach-Object { $_ } }  # callers wrap in @()
    # Properties straight from the Property table (the decompiler folds some, e.g. ALLUSERS, away).
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $db = $installer.OpenDatabase($Msi, 0)
    function Prop($id) {
        $v = $db.OpenView("SELECT Value FROM Property WHERE Property='$id'")
        $v.Execute(); $r = $v.Fetch(); $v.Close()
        if ($r) { $r.StringData(1) } else { $null }
    }

    Write-Host "== Package"
    $pkg = @(Nodes "//w:Package")[0]
    Check ($pkg.UpgradeCode -eq "{C71F4AAE-43FF-47DA-B6F0-582B0611BAF6}") "UpgradeCode is the product's fixed UpgradeCode"
    Check ($pkg.Version -eq $MsiVersion) "ProductVersion $($pkg.Version) = $MsiVersion (workspace $Version)"
    Check ($pkg.Manufacturer -eq "Type3arabi") "Manufacturer Type3arabi"
    Check ((Prop "ALLUSERS") -eq "1") "per-machine package (ALLUSERS=1)"
    Check (@(Nodes "//w:MajorUpgrade").Count -eq 1 -or @(Nodes "//w:Upgrade").Count -ge 1) "major-upgrade table present"

    Write-Host "== Silent install / upgrade behavior"
    Check ((Prop "MSIRESTARTMANAGERCONTROL") -eq "Disable") "MSIRESTARTMANAGERCONTROL=Disable (Restart Manager never closes apps)"
    Check ((Prop "REBOOT") -eq "ReallySuppress") "REBOOT=ReallySuppress (never restarts on its own; returns 3010)"
    Check ((Prop "ARPURLINFOABOUT") -like "https://*") "ARP About link"
    Check ((Prop "ARPHELPLINK") -like "https://*") "ARP help link"
    $cas = @(Nodes "//w:CustomAction")
    $direct = @($cas | Where-Object { $_.ExeCommand -match 'taskkill|shutdown' })
    Check ($direct.Count -eq 0) "no console tool (taskkill/shutdown) launched directly (would flash a console)"
    foreach ($id in "StopHotkey", "RestartNow") {
        $ca = @($cas | Where-Object { $_.Id -eq $id })
        Check ($ca.Count -eq 1 -and $ca[0].DllEntry -eq "WixQuietExec64") "$id runs hidden through WixQuietExec64"
    }
    foreach ($id in "RegisterX64", "RegisterX86") {
        $ca = @($cas | Where-Object { $_.Id -eq $id })
        Check ($ca.Count -eq 1 -and $ca[0].Execute -eq "deferred" -and $ca[0].Impersonate -eq "no" -and $ca[0].ExeCommand -match 'regsvr32') "${id}: deferred, elevated regsvr32 (DllRegisterServer, TSF APIs only)"
    }
    $restart = @(Nodes "//w:Publish[@Value='RestartNow']")
    Check ($restart.Count -ge 1 -and ($restart | ForEach-Object { $_.Condition }) -match 'WIXUI_EXITDIALOGOPTIONALCHECKBOX') "restart only from the interactive finish page checkbox"
    Check (@(Nodes "//w:Custom[@Action='RestartNow']").Count -eq 0) "no restart action in the execute sequence (silent installs never restart)"

    Write-Host "== Payload"
    $files = @{}
    foreach ($f in Nodes "//w:File") { $files[$f.Id] = $f }
    $expected = @{
        TipX64Dll = @("t3a_tip.dll", "x64"); TipX86Dll = @("t3a_tip.dll", "x86"); HotkeyExe = @("t3a-hotkey.exe", "x64")
        SettingsExe = @("Type3arabi Settings.exe", "x64"); DataFile = @("type3arabi.dat", $null)
        LicenseFile = @("LICENSE.txt", $null); NoticeFile = @("NOTICE.md", $null); DatasetsFile = @("DATASETS.md", $null)
        CopyrightFile = @("COPYRIGHT.md", $null); ThirdPartyFile = @("THIRD-PARTY-LICENSES.html", $null)
    }
    foreach ($id in $expected.Keys) {
        $name, $arch = $expected[$id]
        $path = Join-Path $work "File\$id"
        Check ($files.ContainsKey($id) -and $files[$id].Name -eq $name -and (Test-Path $path)) "file $name ($id) present"
        if ($arch -and (Test-Path $path)) {
            $b = [System.IO.File]::ReadAllBytes($path)
            $pe = [BitConverter]::ToInt32($b, 0x3C)
            $machine = [BitConverter]::ToUInt16($b, $pe + 4)
            $want = if ($arch -eq "x64") { 0x8664 } else { 0x14C }
            Check ($machine -eq $want) ("{0}: machine 0x{1:X} ({2})" -f $name, $machine, $arch)
            $v = (Get-Item $path).VersionInfo
            $orig = $name
            Check ($v.ProductName -eq "Type3arabi" -and $v.CompanyName -eq "Type3arabi") "$name ($arch): ProductName/CompanyName = Type3arabi"
            Check ($v.ProductVersion -eq $Version -and $v.FileVersion -eq $Version) "$name ($arch): ProductVersion/FileVersion = $Version"
            Check ($v.OriginalFilename -eq $orig) "$name ($arch): OriginalFilename = $orig (got '$($v.OriginalFilename)')"
            Check ($v.LegalCopyright -like "*Hassan Obaida*") "$name ($arch): LegalCopyright set"
        }
    }
    $dat = Join-Path $work "File\DataFile"
    if ((Test-Path $dat) -and -not $TestModel) {
        $text = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($dat))
        $meta = [regex]::Match($text, '\{"format_version": 1, "distribution": "([^"]+)", "license": "([^"]+)"').Groups
        Check ($meta[1].Value -eq "release") "model META distribution = release (got '$($meta[1].Value)')"
        Check ($meta[2].Value -eq "CC-BY-NC-SA-4.0") "model META license = CC-BY-NC-SA-4.0"
        if ($ModelSha256) {
            Check ((Get-FileHash $dat -Algorithm SHA256).Hash -eq $ModelSha256.ToUpperInvariant()) "model SHA-256 = data/model.lock.toml"
        }
    }

    Write-Host "== Windows integration"
    $run = @(Nodes "//w:RegistryValue[@Key='Software\Microsoft\Windows\CurrentVersion\Run']")
    Check ($run.Count -eq 1 -and $run[0].Root -eq "HKLM" -and $run[0].Name -eq "Type3arabi Hotkey") "startup entry HKLM Run 'Type3arabi Hotkey' (removed with its component)"
    Check (@(Nodes "//w:RegistryKey[@Key='Software\Type3arabi'][@ForceDeleteOnUninstall='yes']").Count -eq 1) "HKLM\Software\Type3arabi removed on uninstall"
    $arpKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Type3arabi'
    $other = @(Nodes "//w:RegistryValue" | Where-Object { $_.Key -notin @('Software\Microsoft\Windows\CurrentVersion\Run', 'Software\Type3arabi', $arpKey) })
    Check ($other.Count -eq 0) "no other registry values written by the MSI (TSF/COM keys come from DllRegisterServer)"

    Write-Host "== Uninstall (Apps entry, keep or erase learned words)"
    Check ((Prop "ARPSYSTEMCOMPONENT") -eq "1") "MSI's own Apps entry hidden (ARPSYSTEMCOMPONENT=1): Windows would uninstall it without dialogs"
    $arp = @{}
    foreach ($v in Nodes "//w:RegistryValue[@Key='$arpKey']") { $arp[$v.Name] = $v }
    Check ($arp["UninstallString"].Value -eq 'MsiExec.exe /I[ProductCode]' -and $arp["UninstallString"].Root -eq "HKLM") "Apps entry: Uninstall opens the package in full UI (MsiExec /I)"
    Check ($arp["QuietUninstallString"].Value -eq 'MsiExec.exe /X[ProductCode] /qn') "Apps entry: QuietUninstallString for scripted removal"
    Check ($arp.ContainsKey("DisplayName") -and $arp.ContainsKey("DisplayVersion") -and $arp.ContainsKey("Publisher") -and $arp.ContainsKey("DisplayIcon")) "Apps entry: name, version, publisher, icon"
    Check ($arp["NoModify"].Value -eq "1" -and $arp["NoRepair"].Value -eq "1") "Apps entry: no Modify/Repair buttons"
    Check (@(Nodes "//w:RegistryKey[@Key='$arpKey'][@ForceDeleteOnUninstall='yes']").Count -eq 1) "Apps entry removed on uninstall"
    $eraseProp = @(Nodes "//w:Property[@Id='ERASEUSERDATA']")
    Check ($eraseProp.Count -eq 1 -and $eraseProp[0].Secure -eq "yes" -and -not $eraseProp[0].Value) "ERASEUSERDATA: secure, off by default"
    $box = @(Nodes "//w:Dialog[@Id='T3RemoveDlg']/w:Control[@Property='ERASEUSERDATA']")
    Check ($box.Count -eq 1 -and $box[0].Type -eq "CheckBox") "uninstall page has the erase checkbox"
    Check (@(Nodes "//w:InstallUISequence/w:Show[@Dialog='T3RemoveDlg']").Count -eq 1) "uninstall page shown in maintenance mode"
    $folders = @($x.SelectNodes("//w:CustomTable[@Id='Wix4RemoveFolderEx']/w:Row", $ns))
    Check ($folders.Count -eq 3) "3 erase folders (RemoveFolderEx) authored"
    foreach ($p in "T3ERASE_LOCAL", "T3ERASE_WEBVIEW", "T3ERASE_ROAMING") {
        $ca = @($cas | Where-Object { $_.Id -like "*$p*" -or $_.Property -eq $p })
        $seq = @(Nodes "//w:InstallExecuteSequence/w:Custom" | Where-Object { $_.Action -in ($ca | ForEach-Object { $_.Id }) })
        Check ($seq.Count -eq 1 -and $seq[0].Condition -match 'ERASEUSERDATA = 1' -and $seq[0].Condition -match 'NOT UPGRADINGPRODUCTCODE') "$p set only when ERASEUSERDATA=1 on a full removal (never an upgrade)"
    }
    # Preselected (set by T3RemoveDlg to skip MaintenanceWelcomeDlg) must be cleared before ResumeDlg and
    # ProgressDlg: both react to it (a Resume page; an "Installing" title drawn over "Removing").
    $seqView = $db.OpenView("SELECT Action, Sequence FROM InstallUISequence")
    $seqView.Execute(); $uiSeq = @{}
    while ($r = $seqView.Fetch()) { $uiSeq[$r.StringData(1)] = [int]$r.StringData(2) }
    $seqView.Close()
    $clear = $uiSeq["SetPreselected"]
    Check ($clear -and $clear -gt $uiSeq["MaintenanceWelcomeDlg"] -and $clear -lt $uiSeq["ResumeDlg"] -and $clear -lt $uiSeq["ProgressDlg"]) "uninstall: Preselected cleared after MaintenanceWelcomeDlg, before ResumeDlg/ProgressDlg (one progress title)"
    # An uninstall started from the UI (Remove event) only has REMOVE="ALL" from InstallValidate on: a
    # condition on REMOVE before it is never true there (the erase paths stayed empty, 2026-09-26).
    $execView = $db.OpenView("SELECT Action, Condition, Sequence FROM InstallExecuteSequence")
    $execView.Execute(); $early = @(); $validateAt = 0; $rows = @()
    while ($r = $execView.Fetch()) { $rows += , @($r.StringData(1), $r.StringData(2), [int]$r.StringData(3)) }
    $execView.Close()
    $validateAt = ($rows | Where-Object { $_[0] -eq "InstallValidate" })[2]
    $early = @($rows | Where-Object { $_[2] -lt $validateAt -and $_[1] -match '\bREMOVE\b' } | ForEach-Object { $_[0] })
    Check ($validateAt -gt 0 -and $early.Count -eq 0) "no action before InstallValidate depends on REMOVE (UI uninstalls set it only there)$(if ($early) { ': ' + ($early -join ', ') })"
    $exit = @(Nodes "//w:Dialog[@Id='ExitDialog']/w:Control[@Id='Title']")
    Check ($exit.Count -eq 1 -and $exit[0].Text -match '\[T3EXITTITLE\]') "finish page title follows the uninstall outcome"
    Check (@(Nodes "//w:Shortcut").Count -ge 2) "Start menu + optional desktop shortcut"
    Check (@(Nodes "//w:ServiceInstall").Count -eq 0) "no Windows service"
    Check ((Prop "DESKTOPSHORTCUT") -eq "1") "desktop shortcut on by default (DESKTOPSHORTCUT=`"`" turns it off)"

    Write-Host "== ICE validation"
    $ice = wix msi validate -sice ICE38 -sice ICE43 -sice ICE57 $Msi 2>&1
    $iceErrors = @($ice | Where-Object { $_ -match 'error' })
    Check ($iceErrors.Count -eq 0) "wix msi validate: no errors (ICE38/43/57 suppressed: per-machine desktop false positives)"
    $ice | Where-Object { $_ -match 'warning' } | ForEach-Object { Write-Host "      $_" }

    Write-Host "== Signatures"
    $ours = @("TipX64Dll", "TipX86Dll", "HotkeyExe", "SettingsExe") | ForEach-Object { Join-Path $work "File\$_" }
    foreach ($p in @($Msi) + $ours) {
        $s = Get-AuthenticodeSignature $p
        $label = Split-Path $p -Leaf
        if ($RequireSigned) {
            Check ($s.Status -eq "Valid" -and $s.SignerCertificate.Subject -like "*$SignerSubject*" -and $s.TimeStamperCertificate) "$label signed by $SignerSubject, timestamped"
        } else {
            Write-Host ("INFO  {0}: {1} {2}" -f $label, $s.Status, $s.SignerCertificate.Subject)
        }
    }
    foreach ($b in Get-ChildItem (Join-Path $work "Binary") -File | Where-Object { $_.Name -match 'Ca_' -or $_.Name -match 'UtilCA' }) {
        $s = Get-AuthenticodeSignature $b.FullName
        Check ($s.Status -eq "Valid") "third-party WiX custom-action DLL $($b.Name) keeps its own signature ($($s.SignerCertificate.Subject -replace ',.*'))"
    }
} finally {
    $db = $null; [GC]::Collect()
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host ""
if ($failed) { Write-Host "$failed check(s) failed" -ForegroundColor Red } else { Write-Host "All checks passed" -ForegroundColor Green }
exit $failed
