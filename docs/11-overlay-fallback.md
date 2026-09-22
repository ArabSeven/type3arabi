# 11 — Plan B: Overlay mode (build ONLY if Gate G1 fails, per ADR-0006)

The Owner's fallback requirement: "the app runs in the taskbar, consuming no resources until toggled by
its (customizable) activation shortcut, then acts exactly like Maren/Yamli". This doc keeps that path ready
so switching costs days, not weeks. Engine (`t3a-engine`) and popup (`t3a-ui`) are reused unchanged.

## 1. Shape
- `t3a-overlay.exe`: tray app (notification-area icon ع), per-user, started at sign-in. Idle: message loop
  only, **no hook installed**, 0 CPU, ~3 MB.
- Activation hotkey (`RegisterHotKey`, default Ctrl+Alt+A, customizable) toggles **Active**; tray icon turns
  accent-colored; a small toast "عربي" appears for 1 s.
- Active: installs `WH_KEYBOARD_LL` hook (hook proc must return within ~1 s system timeout — ours is µs;
  heavy work posted to the UI thread). Deactivating removes the hook.

## 2. Mechanics
1. Hook sees key-down with scan code → Latin layout translation (same `keymap` module as the TIP).
2. Printable key ⇒ swallow (return 1), push to engine session, show popup near the caret.
3. Caret position: `GetGUIThreadInfo(foreground thread).rcCaret` → client-to-screen; fallback UI Automation
   `TextPattern2.GetCaretRange` / `IUIAutomationTextRange::GetBoundingRectangles`; fallback mouse position.
4. Inline preview: none (we don't own the document); the popup shows the Latin buffer + candidates.
5. Commit ⇒ `SendInput` `KEYEVENTF_UNICODE` events for each UTF-16 unit of the chosen text (+ space), tagged
   with `dwExtraInfo = T3A_REINJECT_MAGIC` so the hook ignores them.
6. Focus change / mouse click ⇒ commit-or-drop per setting, hide popup.
7. Re-edit: Backspace right after commit ⇒ send N Backspaces (N = committed UTF-16 length + 1) and reopen.

## 3. Known limitations (why TSF is primary)
- No per-field context (passwords are *not* detectable reliably ⇒ user must toggle off; show a warning if the
  focused UIA element has `IsPassword`).
- Elevated windows ignore our input unless the overlay runs elevated or has `uiAccess` (requires signed binary
  in Program Files).
- Some apps (games, remote desktops, terminals in raw mode) mishandle `KEYEVENTF_UNICODE`.
- Caret position unreliable in Chromium/Electron without UIA; popup may sit at the mouse.
- Security software treats global keyboard hooks as suspicious; signing and transparent behavior are mandatory.

## 4. Work estimate if triggered
≈8–10 agent-days: hook + tray + caret locator + injection + settings integration + compat run.
