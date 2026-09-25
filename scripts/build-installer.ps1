# build-installer.ps1 - build the Type3arabi MSI (docs/07 §5) with WiX v5.
#
# Prerequisites (docs/07 §1): dotnet tool install --global wix --version 5.0.2
#   wix extension add -g WixToolset.UI.wixext/5.0.2 ; wix extension add -g WixToolset.Util.wixext/5.0.2
# The data file must exist (pipeline + build-data, docs/04). Public releases use a release-mode model
# (AGENTS.md R14): -Data target\type3arabi-release.dat. The internal default yields an "(internal build)".
#
# Run from the repository root (no admin needed to build):
#   powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1 [-Data target\type3arabi-release.dat]
# Output: target\installer\Type3arabi-<version>-x64.msi (+ the version-free Type3arabi-x64.msi)
[CmdletBinding()]
param([switch]$SkipBuild, [string]$Data = "target\type3arabi.dat")

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
# One version for everything (docs/07 §3): the workspace version in Cargo.toml, e.g. 1.0.0-rc.1.
$Version = (Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
# MSI ProductVersion is numeric only: 1.0.0-rc.1 -> 1.0.0 (the .wxs allows same-version upgrades).
$MsiVersion = ($Version -split '-')[0]
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
    $dat = if ([System.IO.Path]::IsPathRooted($Data)) { $Data } else { Join-Path $RepoRoot $Data }
    if (-not (Test-Path $dat)) { throw "Missing $dat (run the data pipeline and build-data first)" }
    Remove-Item $Payload -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $Payload "x64"), (Join-Path $Payload "x86") | Out-Null
    Copy-Item (Join-Path $BuildDir "x86_64-pc-windows-msvc\release\t3a_tip.dll") (Join-Path $Payload "x64\t3a_tip.dll")
    Copy-Item (Join-Path $BuildDir "i686-pc-windows-msvc\release\t3a_tip.dll") (Join-Path $Payload "x86\t3a_tip.dll")
    Copy-Item (Join-Path $BuildDir "x86_64-pc-windows-msvc\release\t3a-hotkey.exe") $Payload
    Copy-Item (Join-Path $SettingsDir "release\type3arabi-settings.exe") (Join-Path $Payload "Type3arabi Settings.exe")
    Copy-Item $dat (Join-Path $Payload "type3arabi.dat")
    Copy-Item (Join-Path $RepoRoot "NOTICE.md") $Payload
    Copy-Item (Join-Path $RepoRoot "DATASETS.md") $Payload
    Copy-Item (Join-Path $RepoRoot "LICENSE") $Payload
    Copy-Item (Join-Path $RepoRoot "installer\License.rtf") $Payload
    Copy-Item (Join-Path $RepoRoot "apps\settings\icons\icon.ico") $Payload
    # Third-party license notices of every compiled crate (docs/07 §6.6): cargo about, both workspaces.
    $core = Join-Path $Payload "licenses-core.html"; $settings = Join-Path $Payload "licenses-settings.html"
    cargo about generate about.hbs -o $core
    if ($LASTEXITCODE -ne 0) { throw "cargo about (core) failed - cargo install cargo-about --features cli" }
    cargo about generate -c about.toml --manifest-path apps\settings\Cargo.toml about.hbs -o $settings
    if ($LASTEXITCODE -ne 0) { throw "cargo about (settings) failed" }
    $body = { param($f) [regex]::Match((Get-Content $f -Raw -Encoding UTF8), '(?s)<body>(.*)</body>').Groups[1].Value }
    $head = [regex]::Match((Get-Content $core -Raw -Encoding UTF8), '(?s)^(.*<body>)').Groups[1].Value
    $html = $head + "`n<h1>Keyboard DLL and hotkey companion</h1>" + (& $body $core) + `
        "`n<hr><h1>Type3arabi Settings app</h1>" + (& $body $settings) + "`n</body>`n</html>`n"
    Set-Content -Path (Join-Path $Payload "THIRD-PARTY-LICENSES.html") -Value $html -Encoding UTF8
    Remove-Item $core, $settings
    Copy-Item (Join-Path $RepoRoot "installer\WixUIDialog.bmp") $Payload
    Copy-Item (Join-Path $RepoRoot "installer\WixUIBanner.bmp") $Payload

    # A data file built with `internal` sources must say so everywhere it goes (AGENTS.md R14).
    $internal = Select-String -Path $dat -Pattern '"distribution": "internal-only"' -SimpleMatch -Quiet
    $name = if ($internal) { "Type3arabi (internal build)" } else { "Type3arabi" }

    Write-Host "[3/3] Building MSI ($name)..." -ForegroundColor Yellow
    wix build (Join-Path $RepoRoot "installer\Type3arabi.wxs") -arch x64 `
        -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
        -loc (Join-Path $RepoRoot "installer\Type3arabi.wxl") -culture en-US `
        -d "Payload=$Payload" -d "Version=$MsiVersion" -d "ProductName=$name" -o $Out
    if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
    # Version-free copy for the website's ".../releases/latest/download/Type3arabi-x64.msi" link (ADR-0010).
    Copy-Item $Out (Join-Path (Split-Path $Out) "Type3arabi-x64.msi") -Force
    Write-Host "=== Built $Out (+ Type3arabi-x64.msi) ===" -ForegroundColor Green
} finally {
    Pop-Location
}
