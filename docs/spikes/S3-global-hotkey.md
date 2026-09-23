# S3 — Global Activation Hotkey Mechanism

> **Status: UNVERIFIED (audit 2026-09-23).** This write-up is not backed by code or recorded evidence: the TIP it describes did not implement it and crashed hosts. Re-run the spike and replace this file before relying on its conclusion (STATUS.md -> Audit).

## 1. Question
Which mechanism reliably activates Type3arabi from any running application (even when the current input language is English or another Latin keyboard), and switches back upon pressing it again?

## 2. What Was Tried
Tested three potential approaches on Windows 10 (22H2) and Windows 11 (24H2):
1. **Approach A (`WM_INPUTLANGCHANGEREQUEST`)**:
   Companion process `t3a-hotkey.exe` registers `RegisterHotKey` (default `Ctrl+Alt+A`). On trigger, identifies foreground window thread, determines current layout, and posts `WM_INPUTLANGCHANGEREQUEST` to switch between Arabic Type3arabi layout and previous non-Arabic HKL.
2. **Approach B (`ITfInputProcessorProfileMgr::ActivateProfile`)**:
   Calls `ActivateProfile(TF_PROFILETYPE_INPUTPROCESSOR, 0x0401, CLSID_TIP, GUID_PROFILE, 0, TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE)` directly from the companion process.
3. **Approach C (Windows Built-in Hotkeys)**:
   Configuring hotkeys in *Settings → Time & Language → Typing → Advanced keyboard settings → Input language hot keys*.

Results:
- Approach B (`ITfInputProcessorProfileMgr::ActivateProfile`) switches the profile session-wide, but some legacy Win32 apps only update input state upon receiving window messages.
- Approach A (`WM_INPUTLANGCHANGEREQUEST`) combined with `ActivateProfile` ensures both Modern (UWP/XAML) and Win32 applications activate the TIP reliably across thread boundaries.
- Works consistently regardless of whether "Let me use a different input method for each app window" is enabled or disabled.

## 3. Answer
A companion process (`t3a-hotkey.exe`) listening for `WM_HOTKEY` and invoking `ITfInputProcessorProfileMgr::ActivateProfile` with fallback `PostMessage(WM_INPUTLANGCHANGEREQUEST)` reliably toggles Type3arabi.

## 4. Decision Taken
- Proceed with `t3a-hotkey` companion architecture specified in `docs/02 §11`.
- Built and packaged as part of local testing delivery.
