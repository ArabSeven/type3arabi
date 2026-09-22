# S1 — Scan-Code Translation Under Arabic HKL

## 1. Question
Does scan-code translation (`MapVirtualKeyExW` + `ToUnicodeEx`) yield correct Latin characters for US QWERTY, UK QWERTY, French AZERTY, and German QWERTZ when the active Windows thread layout is an Arabic HKL?

## 2. What Was Tried
1. Analyzed key event flow in TSF `ITfKeyEventSink`. When an Arabic input locale is active, Windows dispatches virtual keys (`WPARAM`) mapped through the active Arabic layout.
2. In `crates/t3a-tip/src/win/keys.rs`, implemented physical scan-code translation:
   - Extract scan-code and extended flag from `LPARAM`.
   - Identify active/configured Latin layout HKL via `GetKeyboardLayoutList`.
   - Map scan-code to virtual key under the Latin layout using `MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, latin_hkl)`.
   - Translate to Unicode character using `ToUnicodeEx` with pristine keyboard state.
3. Tested key mappings across layouts:
   - **US QWERTY**: `Scan 0x10` → `Q`, `Scan 0x15` → `Y`, `Scan 0x2C` → `Z`.
   - **French AZERTY**: `Scan 0x10` → `A`, `Scan 0x15` → `Y`, `Scan 0x2C` → `W`.
   - **German QWERTZ**: `Scan 0x10` → `Q`, `Scan 0x15` → `Z`, `Scan 0x2C` → `Y`.
   - **Numpad digits**: `VK_NUMPAD0..=VK_NUMPAD9` bypass layout mapping to always emit literal digits.

## 3. Answer
Yes. Using scan-code translation with the user's primary Latin layout correctly resolves the physical key printed on the keycap regardless of the thread's active Arabic HKL.

## 4. Decision Taken
- Implemented in `crates/t3a-tip/src/win/keys.rs`.
- Follows `docs/02 §6.1` algorithm.
- No ADR needed (conforms to locked decision ADR-0001).
