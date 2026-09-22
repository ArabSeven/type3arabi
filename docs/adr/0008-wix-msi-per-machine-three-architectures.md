# ADR-0008: WiX MSI, per-machine, three architectures

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
TSF TIPs must be registered machine-wide (COM + TSF profiles), readable from AppContainers (Program Files), and provided for 32-bit, 64-bit and ARM64 processes with one CLSID.

## Decision
WiX Toolset v5 MSI, per-machine; registration via a helper calling DllRegisterServer (TSF APIs only) and InstallLayoutOrTip for the installing user; all PE files and the MSI signed.

## Consequences
+ Standard enterprise-deployable package; clean upgrade/uninstall; RestartManager handling.
− Requires admin to install (acceptable for an input method).

## Alternatives considered
MSIX (cannot register in-proc TSF TIPs system-wide); Inno Setup/NSIS (fine, but MSI is better for enterprise).
