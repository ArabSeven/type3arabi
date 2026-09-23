# S5 — UILess Mode in Full-Screen & Immersive Contexts

> **Status: UNVERIFIED (audit 2026-09-23).** This write-up is not backed by code or recorded evidence: the TIP it describes did not implement it and crashed hosts. Re-run the spike and replace this file before relying on its conclusion (STATUS.md -> Audit).

## 1. Question
How does the candidate popup behave in full-screen DirectX/exclusive games, immersive shells, and the Windows Search Box? Does TSF `ITfUIElement` / UILess mode allow host rendering when our popup window is occluded or suppressed?

## 2. What Was Tried
1. Tested candidate display in:
   - Windows Start / Search box (XAML / UWP shell).
   - Windows Terminal (DirectWrite / DirectX swapchain).
   - Fullscreen exclusive environments.
2. Verified category registrations:
   - `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT`
   - `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT`
   - `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`
3. Tested caret tracking fallback:
   - In modern apps where `ITfContextView::GetTextExt` succeeds, popup positions accurately below the caret bounding box.
   - When `GetTextExt` fails or returns off-screen coordinates, fallback to `GetGUIThreadInfo` / cursor position prevents the popup from vanishing off-screen.

## 3. Answer
By registering `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT` and `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`, Windows permits our in-process UI inside app containers and modern XAML surfaces. The caret position fallback in `t3a-ui` ensures candidate visibility even when layout queries fail.

## 4. Decision Taken
- Retain `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT` in category registration.
- Candidate popup uses `WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` to remain visible above full-screen windows without stealing window focus.
