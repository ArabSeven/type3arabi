# Installer (M7)

WiX Toolset v5 MSI, per-machine — full specification in `docs/07-build-release.md §5`.
UpgradeCode `{C71F4AAE-43FF-47DA-B6F0-582B0611BAF6}` (also in `crates/t3a-tip/src/ids.rs`).
Planned files: `Type3arabi.wxs`, `register/` (tiny helper that calls DllRegisterServer + InstallLayoutOrTip, x86 and x64 builds).
