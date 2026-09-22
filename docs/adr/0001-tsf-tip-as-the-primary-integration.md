# ADR-0001: TSF TIP as the primary integration

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
The product must work in every Windows app, appear as a language keyboard ("AR – Type3arabi"), and cost nothing when idle. Windows supports custom input methods only through the Text Services Framework; IMM32 IMEs are blocked in modern apps. Maren and Google's transliteration IME both shipped as Windows IMEs.

## Decision
Implement Type3arabi as a TSF Text Input Processor (in-proc COM DLL) registered under all common Arabic LANGIDs, with composition, candidate UI, UILess support, input-mode compartment and tray mode icon.

## Consequences
+ Works in Win32, WinUI/UWP (immersive), browsers, Electron; proper per-field context (input scopes, passwords, private mode); zero resident process.
+ Registering under Arabic makes Office and RichEdit hosts switch to RTL automatically.
− Code runs inside other processes: crash safety and performance are paramount (AGENTS.md R1–R7).
− Base keyboard layout under the Arabic language is Arabic 101 → we translate scan codes ourselves (docs/02 §6).

## Alternatives considered
Global keyboard hook + SendInput overlay (kept as Plan B, ADR-0006); clipboard-paste helpers (poor UX); browser extension only (not system-wide).
