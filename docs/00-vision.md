# 00 — Vision & product brief

## 1. Problem
Hundreds of millions of Arabic speakers learned to type on Latin keyboards and write Arabic as
Arabizi. Writing *proper* Arabic script on Windows today means either learning the Arabic keyboard
layout or copy-pasting from Yamli.com (browser-only). Microsoft Maren (2009–2015) solved this
system-wide and was discontinued; Google Input Tools for Windows was removed in 2018. There is no
maintained, native, system-wide Arabizi input method for Windows.

## 2. Product in one sentence
A native Windows keyboard, **AR – Type3arabi**, that turns Arabizi into correct Arabic as you type,
in every app, with a smart RTL candidate popup, dialect awareness, automatic critical diacritics,
and one-keystroke access to any diacritic.

## 3. Target users (priority order)
1. **Levantine & Gulf chat-natives** (Jordan, Palestine, Lebanon, Syria, KSA, UAE, Kuwait, Qatar) who
   think in Arabizi and need Arabic script for work email, documents, government forms.
2. **Egyptian** users (largest Arabizi population; "Franco-Arab").
3. **Maghrebi** users (Morocco/Algeria/Tunisia; AZERTY keyboards, French-influenced spelling `ch`, `ou`, `9`=ق).
4. **Heritage speakers & learners** who read Arabic but can't touch-type it; they value the tashkeel editor.

## 4. Experience principles (tie-breakers for every design choice)
1. **Speed is a feature.** The popup must feel instantaneous (≤ 1 frame). Nothing we add may make typing laggy.
2. **The default is right.** Top-1 accuracy matters more than a long list. Space should "just work".
3. **Never lose the user's text.** Every state has a way back (Esc to Latin, Backspace re-edit).
4. **Invisible when idle.** No resident process is required; zero CPU when not typing.
5. **Respect the host app.** No crashes, no focus stealing, no hijacked shortcuts outside composition.
6. **Private by construction.** Offline, no telemetry, learning stored locally and wipeable.
7. **Arabic-first UI.** The popup is RTL, uses proper Arabic typography, and speaks Arabic labels
   (with English secondary where useful).

## 5. Scope v1.0
- Windows 10 22H2 and Windows 11 (x64 and ARM64); 32-bit and 64-bit host apps.
- TSF TIP registered under Arabic (all common Arabic locales), shows as **AR – Type3arabi**.
- Arabizi → Arabic word-by-word transliteration with ranked candidates, predictive completions,
  OOV (names, foreign words), raw-Latin escape, numbers, Arabic punctuation, article joining.
- Dialect groups: MSA, LEV, EGY, GLF, IRQ, MAG with auto-detection.
- Critical diacritics: sacred-name styling (**اللّه** etc.), adverbial tanween (**شكراً**, **جداً**).
- Tashkeel editor inside the popup + "harakat from your vowels".
- Learning: remembers the user's choices; custom words.
- Settings app, optional global activation hotkey companion, signed MSI installer.

## 6. Non-goals v1.0 (explicitly out)
- Machine translation, grammar correction, sentence rewriting.
- Arabic → Arabizi (reverse) and reconversion of arbitrary existing text (planned M9).
- Cloud sync of the user dictionary.
- macOS / Linux / mobile keyboards (the engine is portable; front-ends are not in scope).
- Voice input.
- Any runtime LLM or network model.

## 7. Success metrics (measured by `t3a-cli eval` and the app-compat matrix)
| Metric | v1.0 target |
|---|---|
| Word top-1 accuracy, LEV + EGY held-out | ≥ 85% |
| Word hit@5, LEV + EGY held-out | ≥ 96% |
| Word top-1, GLF / IRQ / MAG held-out | ≥ 75% |
| Keystroke-to-popup latency p99 (reference laptop) | ≤ 16 ms |
| Engine per-keystroke p99 | ≤ 3 ms |
| Private memory added per host process | ≤ 6 MB |
| Apps in compat matrix passing | 100% of Tier-1, ≥ 90% of Tier-2 |
| Crashes attributable to TIP in soak test | 0 |
