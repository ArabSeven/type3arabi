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
| `installer` (tags + manual) | windows-latest | build release DLLs ×3, hotkey, settings; sign (§4); build MSI; sign MSI; upload artifacts |

## 3. Versioning
SemVer for the product `MAJOR.MINOR.PATCH`, same version in all crates (workspace `version`), in the DLL
VERSIONINFO, MSI ProductVersion and Settings About page. Data file has its own `data_version` (date-based
`YYYY.MM.DD.n`) embedded in its header and shown in About. MSI: `MajorUpgrade` with the fixed UpgradeCode
(`docs/02 §1`), per-machine scope.

## 4. Code signing (required: Windows warns loudly on unsigned IMEs; AV heuristics distrust unsigned DLLs loaded everywhere)
Sign **every** PE (`t3a_tip.dll` ×3, `t3a-hotkey.exe`, `Type3arabi Settings.exe`, custom-action helper) and
the MSI, SHA-256, RFC 3161 timestamp (`/tr http://timestamp.digicert.com /td sha256 /fd sha256`).

Certificate options (Owner decision, see `STATUS.md`):
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
- Registration (implemented): deferred elevated custom actions run `System32egsvr32.exe` on the x64 DLL and
  `SysWOW64egsvr32.exe` on the x86 DLL, i.e. our `DllRegisterServer` / `DllUnregisterServer` (TSF APIs only,
  R6). No separate helper binary (Agent decision D9).
- Enable for the installing user: `t3a-hotkey.exe --enable-profile` (immediate, impersonated, after
  InstallFinalize) calls `InstallLayoutOrTip("0401:{CLSID}{PROFILE}", 0)` — one entry, ar-SA (ADR-0009).
  Uninstall: `--disable-profile` (`ILOT_UNINSTALL`). Other users: Windows language settings (backlog: Settings button).
- **Restart** (Owner request 2026-09-23): the finish page has "Restart now (recommended)", checked by default
  (`shutdown /r /t 20`). Finishing with it unchecked runs `t3a-hotkey.exe --restart-warning`, a bilingual
  message explaining that already-open apps may not show or may keep an older Type3arabi until a restart.
- Build: `scripts/build-installer.ps1` → `target\installer\Type3arabi-<ver>-x64.msi`; validate with
  `wix msi validate`. Internal-data builds are named "Type3arabi (internal build)".
- `HKLM\...\Run` value `Type3arabi Hotkey` → `t3a-hotkey.exe` (exits if disabled in the user's config).
- Start menu: "Type3arabi Settings".
- Uninstall: unregister DLLs, `InstallLayoutOrTip(... ILOT_UNINSTALL)` for the current user, remove Run value,
  **leave** `%LOCALAPPDATA%\Type3arabi` unless the user ticks "Remove my words and settings".
- Files in use: TSF DLLs are loaded in running apps; use WiX `RestartManager`-friendly behavior and schedule
  replacement on reboot when needed (standard MSI `FilesInUse`); the new version works in newly started apps.

## 6. Release checklist
1. `main` green on all CI jobs; `STATUS.md` milestone evidence complete.
2. Eval report for the release data build attached; no golden-test regression.
3. App-compat matrix (`docs/08 §5`) run on Win10 22H2 x64, Win11 24H2 x64, Win11 ARM64 — all Tier-1 pass.
4. Soak: 8 h scripted typing across 5 apps, 0 crashes, memory flat.
5. Signed artifacts verified with `signtool verify /pa /v`.
6. `NOTICE.md` lists every shipped dependency (cargo-about output) and data attribution (FineWeb-2 ODC-By…).
7. No source with status `internal` in the release data build: each is cleared to `approved` with written permission or an explicit license, or removed and the data rebuilt with `--mode release`. CC BY-SA sources require the data file to be distributed under CC BY-SA.
8. Tag `vX.Y.Z`, publish MSI + SHA-256 + release notes.
