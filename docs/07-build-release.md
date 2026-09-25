# 07 — Build, sign, package, release

## 1. Toolchain
- Rust stable pinned in `rust-toolchain.toml` (1.95.0 at bootstrap; bump deliberately, one PR, full CI).
- Targets: `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`, `aarch64-pc-windows-msvc`.
- Windows builds need Visual Studio 2022 Build Tools (MSVC v143 + Windows 11 SDK ≥ 10.0.22621) for the linker,
  `rc.exe` (resources via `embed-resource`), and `signtool.exe`.
- `windows` crate pinned `=0.62.2` (ADR-0002). Features are listed explicitly in `crates/t3a-tip/Cargo.toml`;
  add features, never enable a wildcard.
- WiX Toolset **v5** (`dotnet tool install --global wix`) with `WixToolset.UI.wixext`, `WixToolset.Util.wixext`.
- Settings app: Node 20 LTS + Tauri CLI 2.x (`cargo install tauri-cli --version "^2"`), WebView2 (present on Win10 22H2/Win11).

## 2. CI (`.github/workflows/ci.yml`)
| Job | Runner | Steps |
|---|---|---|
| `portable` | ubuntu-latest | fmt, clippy (engine, data, cli), test, `cargo deny check`, bench (relative gate), mini data build + eval |
| `windows` | windows-latest | clippy + build all crates for x64, i686, aarch64; unit tests (x64); DLL size check; `regsvr32` smoke register/unregister in the runner (admin) |
| `settings` | windows-latest | Settings workspace: clippy, tests, `cargo deny` |
| `installer-smoke` | windows-latest | MSI with a seed-only test model; `scripts/validate-msi.ps1 -TestModel` |

`windows` also gates the DLL size (P7) and registers/unregisters the x64 TIP with `regsvr32` on the admin runner
(exactly one ar-SA profile, nothing left behind).

**Release workflow** (`.github/workflows/release.yml`, tag `vX.Y.Z` or manual) — the architecture chosen in the
2026-09-25 release audit (one MSI for GitHub Releases and the Microsoft Store's MSI/EXE route; MSIX does not fit a
TSF input method, whose DLL must load into other apps' processes):

| Job | Runner | Steps |
|---|---|---|
| `build` | windows-2025 (GitHub-hosted) | tag = workspace version; WiX 5.0.2 + cargo-about 0.9.2; model from `data/model.lock.toml` (`scripts/fetch-model.ps1`, SHA-256 checked); `scripts/build-installer.ps1`; `scripts/validate-msi.ps1`; upload the unsigned MSI |
| `sign` | windows-2025 | only if `vars.SIGNPATH_ENABLED == 'true'`: SignPath signing request (`signpath/github-action-submit-signing-request@v2`, artifact configuration `.signpath/artifact-configuration.xml`, manual approval in SignPath); `validate-msi.ps1 -RequireSigned` |
| `release` | ubuntu-latest | only for a `v*` tag with `vars.PUBLISH_ENABLED == 'true'` (unsigned also needs `ALLOW_UNSIGNED_RELEASE`): version-free copy, `SHA256SUMS.txt`, **draft** release; the Owner publishes it |

The model is built locally (its inputs are too large for CI and never committed) with a deterministic
`build-data`, uploaded once as release asset `model-<data_version>/type3arabi.dat`, and pinned by SHA-256 in
`data/model.lock.toml`; the MSI is then reproducible from the tagged commit plus that file.

## 3. Versioning
SemVer for the product `MAJOR.MINOR.PATCH` (pre-releases `X.Y.Z-rc.N`), same version in all crates
(workspace `version`, `apps/settings` Cargo.toml and `tauri.conf.json`), in every PE's VERSIONINFO (ProductName and
CompanyName "Type3arabi", ProductVersion/FileVersion strings = the version, numeric X.Y.Z.0, OriginalFilename,
LegalCopyright) and the Settings About page. MSI ProductVersion is numeric: `X.Y.Z-rc.N` → `X.Y.Z.N`, `X.Y.Z` → `X.Y.Z`
(Windows Installer ignores the 4th field when comparing, so RCs and the final release upgrade each other;
Settings › Apps shows which RC is installed). Data file has its own `data_version` (date-based
`YYYY.MM.DD.n`) embedded in its header and shown in About. MSI: `MajorUpgrade` with the fixed UpgradeCode
(`docs/02 §1`), per-machine scope.

## 4. Code signing (required: Windows warns loudly on unsigned IMEs; AV heuristics distrust unsigned DLLs loaded everywhere)
Sign **every** Type3arabi PE (`t3a_tip.dll` x64 + x86 (+ ARM64 when built), `t3a-hotkey.exe`, `Type3arabi Settings.exe`)
and the MSI, SHA-256, RFC 3161 timestamp. WiX's custom-action DLLs inside the MSI (`WixUiCa_X64`,
`Wix4UtilCA_X64`) are third-party and already signed by "WiX Toolset (.NET Foundation)": never re-sign them.
Planned route: SignPath Foundation deep signing in the release workflow (§2); `.signpath/artifact-configuration.xml`
is the draft configuration (to be verified with a test certificate first). The Microsoft Store's MSI/EXE route
requires the MSI and all its PE files to chain to the Microsoft Trusted Root Program.

Certificate options (Owner decision, see `STATUS.md`). Since ADR-0010 the project is free and open source, so try
the open-source routes first:
0. **SignPath Foundation** (free code signing for qualifying open-source projects; signs in CI from a public
   repository) or **Certum Open Source Code Signing** (low-cost certificate for open-source developers). Check
   eligibility and whether the certificate is issued to the individual developer before paying for option 2.
   The Microsoft Store's MSI/EXE submission path needs an installer signed by a CA in the Microsoft Trusted Root
   Program, so one of these is required for the Store too.
1. **Azure Artifact Signing** (formerly Trusted Signing): ~$9.99/month, CI-friendly, no hardware token — but
   public-trust identity validation is available only to organizations in the USA, Canada, EU, UK, Australia,
   New Zealand, Japan, South Korea, Singapore, Switzerland, Norway, Israel, and to *individuals* in the USA/Canada.
   A Jordan-based individual or company does **not** qualify today.
2. **OV code-signing certificate from a public CA** (e.g. SSL.com eSigner cloud signing, Certum, Sectigo,
   DigiCert): available worldwide; since 2023 keys must live in an HSM/token or the CA's cloud signing
   service; since March 2026 maximum validity is 460 days. Prefer a **cloud-signing** product so CI can sign.
   Certum's "Open Source Code Signing" is the cheapest route if the project is released as open source.
3. EV certificates no longer grant instant SmartScreen reputation; not worth the premium.
SmartScreen reputation accrues per signing identity over downloads; keep one stable identity.

Development: a self-signed test certificate (`tools/dev/make-test-cert.ps1`) imported into the dev machine's
Trusted Root and Trusted Publishers; never used for public builds.

## 5. Installer (WiX, `installer/Type3arabi.wxs`)
- Per-machine, `InstallScope="perMachine"`, `%ProgramFiles%\Type3arabi\`:
  `x64\t3a_tip.dll`, `x86\t3a_tip.dll` (same install folder; registered in the 32-bit registry view),
  `arm64\t3a_tip.dll` (only on ARM64 OS), `type3arabi.dat`, `t3a-hotkey.exe`,
  `Type3arabi Settings.exe`, `NOTICE.md`, `LICENSE`.
- **Silent install / upgrade** (`msiexec /i … /qn`, which is how the Microsoft Store runs an MSI): the keyboard DLL is
  loaded by every app the user types in, so upgrades always find it in use. The package sets
  `MSIRESTARTMANAGERCONTROL=Disable` (Restart Manager never closes apps) and `REBOOT=ReallySuppress` (Windows Installer
  never restarts the PC; in-use files are replaced at the next restart and msiexec returns 3010). Interactive
  installs offer the restart on the finish page; the FilesInUse dialog says nothing needs closing ("Ignore").
  Console tools (`taskkill`, `shutdown`) run through WiX's QuietExec (no console window).
- Disclosure: the page after the license, "What Type3arabi adds", lists every system change (keyboard registration,
  the sign-in helper and its Arabic 101 tidy-up, Start menu/desktop shortcuts, local data path, no network) and what
  uninstalling removes. Same list in README.md.
- Registration (implemented): deferred elevated custom actions run `System32\regsvr32.exe` on the x64 DLL and
  `SysWOW64\regsvr32.exe` on the x86 DLL, i.e. our `DllRegisterServer` / `DllUnregisterServer` (TSF APIs only,
  R6). No separate helper binary (Agent decision D9).
- Enable for the installing user: `t3a-hotkey.exe --enable-profile` (immediate, impersonated, after
  InstallFinalize) calls `InstallLayoutOrTip("0401:{CLSID}{PROFILE}", 0)` — one entry, ar-SA (ADR-0009).
  Uninstall: `--disable-profile` (`ILOT_UNINSTALL`). Other users: Windows language settings (backlog: Settings button).
- **Restart** (Owner request 2026-09-23): the finish page has "Restart now (recommended)", checked by default
  (`shutdown /r /t 20`). Finishing with it unchecked runs `t3a-hotkey.exe --restart-warning`, a bilingual
  message explaining that already-open apps may not show or may keep an older Type3arabi until a restart.
- Build: `scripts/build-installer.ps1` → `target\installer\Type3arabi-<ver>-x64.msi` plus the version-free copy
  `Type3arabi-x64.msi` (ADR-0010: the website links to `…/releases/latest/download/Type3arabi-x64.msi`); validate
  with `wix msi validate`. Internal-data builds are named "Type3arabi (internal build)" and are never published.
- Files also installed: `LICENSE.txt` (Apache-2.0), `COPYRIGHT.md` (code vs. model licensing), `NOTICE.md`, `DATASETS.md` (model license and provenance),
  `THIRD-PARTY-LICENSES.html` (cargo about, both workspaces; generated by `build-installer.ps1`, config `about.toml`).
- Public builds pass the release-mode model: `build-installer.ps1 -Data target\type3arabi-release.dat`; the version
  comes from the workspace `Cargo.toml` (an RC such as 1.0.0-rc.1 is MSI 1.0.0; same-version upgrades are allowed).
- `HKLM\...\Run` value `Type3arabi Hotkey` → `t3a-hotkey.exe`: sign-in tidy-up of stray Arabic layouts (docs/02 §2),
  then the hotkey loop (exits after the tidy-up if the hotkey is disabled in the user's config).
- Start menu: "Type3arabi Settings". Desktop: the same shortcut on the Public desktop, on by default; an Options page
  after the license page ("Create a desktop shortcut", checked) can turn it off; silent installs: `DESKTOPSHORTCUT=""`.
  Validate with `wix msi validate -sice ICE38 -sice ICE43 -sice ICE57 <msi>`: those three ICEs assume the Desktop folder
  is per-user, which is false for a per-machine package (ALLUSERS=1 => Public desktop). ICE61 (same-version upgrades)
  and ICE69 (shortcut in its own component) are expected warnings.
- Uninstall: unregister DLLs, `InstallLayoutOrTip(... ILOT_UNINSTALL)` for the current user, remove Run value,
  shortcuts and `HKLM\Software\Type3arabi`. Settings and learned words (`%LOCALAPPDATA%\Type3arabi`, the Settings
  app's `%LOCALAPPDATA%\com.type3arabi.settings`, `%APPDATA%\Type3arabi`) are **kept** unless `ERASEUSERDATA=1`
  (util:RemoveFolderEx for the uninstalling user; never on an upgrade). Windows removes an MSI from Settings › Apps
  without dialogs, so the MSI's own entry is hidden (`ARPSYSTEMCOMPONENT=1`) and the package writes the Apps entry
  itself (`Uninstall\Type3arabi`, UninstallString `MsiExec.exe /I{ProductCode}`): Uninstall opens the package in
  maintenance mode, where `T3RemoveDlg` offers the erase checkbox (off by default) and the finish page says
  "Type3arabi is successfully uninstalled", plus that the learned words are still saved when they were kept
  (Owner, 2026-09-26). `QuietUninstallString` = `/X … /qn` for scripts. Other accounts that enabled the keyboard keep a dangling list entry, which Windows
  ignores once the TIP is unregistered.
- Other accounts on a shared PC: the installer enables the keyboard for the installing user only; Settings › General
  shows "Add" for any other account.
- Files in use: TSF DLLs are loaded in running apps; the installer never closes them (above) and replaces the DLLs
  at the next restart (standard MSI in-use handling); until then running apps keep the old version.
- Validation: `scripts/validate-msi.ps1 <msi>` (static, no install: identity, version, silent properties, payload
  architecture + version metadata, custom actions, registry, model META/SHA-256, ICE, signatures) and, on a test PC,
  `scripts/test-installer.ps1 -Msi <msi> [-Uninstall]` (one UAC prompt: silent upgrade with apps holding the DLL, no
  restart, installed state, clean uninstall, reinstall).

## 6. Release checklist
1. `main` green on all CI jobs; `STATUS.md` milestone evidence complete.
2. Eval report for the release data build attached; no golden-test regression.
3. App-compat matrix (`docs/08 §5`) run on Win10 22H2 x64, Win11 24H2 x64, Win11 ARM64 — all Tier-1 pass.
4. Soak: 8 h scripted typing across 5 apps, 0 crashes, memory flat.
5. Signed artifacts verified: `scripts/validate-msi.ps1 <msi> -RequireSigned` (MSI + every Type3arabi PE inside,
   timestamped; WiX DLLs keep their own signature).
6. `NOTICE.md` lists every shipped dependency (cargo-about output) and data attribution (FineWeb-2 ODC-By…).
7. The release data is built with `--mode release`: no `internal` source (each is cleared to `approved` with written
   permission or an explicit license, or left out). META `license` is `CC-BY-NC-SA-4.0` (or `CC-BY-4.0` if no NC
   source was used); `DATASETS.md` is current (`uv run t3ap datasets-md`; a pipeline test enforces it).
8. Model asset `model-<data_version>` uploaded and `data/model.lock.toml` current; tag `vX.Y.Z`; the release workflow
   drafts the GitHub Release (versioned MSI, `Type3arabi-x64.msi` (same file), `SHA256SUMS.txt`, notes from
   `docs/releases/`); the Owner publishes it. No other download host (ADR-0010).
9. Microsoft Store: submit the signed MSI (MSI/EXE submission) by its **versioned** release URL (never the version-free
   copy; the file behind the URL must never change). The Store runs it with `/qn` and does not update existing
   installs (a new version is a new submission). The listing says "free and open source (Apache-2.0);
   language model CC BY-NC-SA 4.0" and links to the repository, `DATASETS.md` and the privacy statement (offline,
   no data collected).
