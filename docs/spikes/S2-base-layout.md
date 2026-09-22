# S2 — Base Keyboard Layout Substitution (`hklSubstitute`)

## 1. Question
Does registering each TSF profile with `hklSubstitute = MAKELONG(langid, 0x0409)` (e.g. `0x04090401` = "Arabic language, US layout") provide a Latin physical layout while Type3arabi is active, so that:
1. Fields where IMEs are disabled (e.g., password boxes) receive Latin characters?
2. Apps still detect an Arabic input language (retaining automatic RTL in Word/Outlook)?

## 2. What Was Tried
1. Tested profile registration with `hklSubstitute = HKL::default()` (0) vs substitute HKL `0x04090401`.
2. Verified behavior in:
   - Win32 standard password edit controls (`ES_PASSWORD`).
   - Chromium password input fields.
   - Word paragraph direction switching.
3. Observed that when `hklSubstitute` is specified, Windows preserves the Latin base layout for non-IME keystrokes in password fields, while `ITfInputProcessorProfileMgr` retains the Arabic LANGID for the active input method.
4. When `hklSubstitute` is 0, password fields under Arabic HKL produce Arabic characters unless the TIP actively maps or the user switches to a Latin layout.

## 3. Answer
Yes, `hklSubstitute` successfully pairs an Arabic language identity with a Latin fallback layout for password fields and non-IME input.

## 4. Decision Taken
- In `t3a-tip`, support default HKL substitution (`0x04090401` or primary user Latin layout) in installer registration (`docs/02 §6.3`).
- In `TextService` context gating (`context.rs`), password scopes (`IS_PASSWORD`, `IS_PIN`) set `ContextMode::Latin`, letting Latin keys pass through cleanly.
- No ADR needed.
