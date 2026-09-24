# 05 — UX specification

Principles are in `docs/00 §4`. Key routing mechanics are in `docs/02 §5.2`; this doc defines what the
user sees and feels. All dimensions are in DIPs at 96 DPI (scale by DPI/96).

## 1. States

```
          type Latin char                    Tab / click ◌َ
 IDLE ───────────────────► COMPOSING ─────────────────────────► TASHKEEL
  ▲  ◄── Space/Enter/click commit ──┘  ▲  ◄──────── Esc ────────────┘
  │  ◄── Esc (commits raw Latin) ──────┘        Enter/Space/1-8 commit ──► IDLE
  └── Backspace right after a commit re-opens the word (RE-EDIT → COMPOSING)
 LATIN MODE (Ctrl+Space): keys type Latin directly, no popup. Tray icon shows "A".
```

## 2. Inline composition
- While composing, the app shows the **highlighted candidate** (Arabic) inline with a dotted underline,
  so the text appears in place, RTL, as you type (Yamli-like). Setting: inline shows Latin instead.
- In the tashkeel editor, the inline text shows the word with the marks chosen so far, solid underline.

## 3. Candidate popup

### 3.1 Anatomy (RTL; example buffer `mar7aba`, then `mar7ab` mid-word)
```
┌────────────────────────────────────────┐
│ mar7aba                        [ شامي ] │  header: Latin buffer (LTR, left) · dialect badge (right)
├────────────────────────────────────────┤
│ ◌َ                             مرحبا  ▌│  row 1 = default (highlighted, accent bar on the RIGHT edge)
│                                 مرحبه  │
│                                 مرحبة  │
│ EN                             mar7aba │  raw Latin — ALWAYS the last visible row
├────────────────────────────────────────┤

 mid-word, buffer `mar7ab`:
│ ◌َ                               مرحب ▌│
│ ⋯                               مرحبا  │  completion (⋯ marker): the word continues beyond what was typed
│ ⋯                             مرحبتين  │
│ EN                              mar7ab │
├────────────────────────────────────────┤
│ مسافة: إدراج · Tab: تشكيل · Esc: لاتيني │  footer hints (auto: shown for the first 50 words)
└────────────────────────────────────────┘
```
- Arabic text right-aligned, reading direction RTL, via DirectWrite (`DWRITE_READING_DIRECTION_RIGHT_TO_LEFT`,
  `DWRITE_TEXT_ALIGNMENT_LEADING`).
- The **harakat button** `◌َ` (28×28) appears on the left end of the highlighted/hovered row; tooltip
  "تشكيل – Add diacritics (Tab)".
- Row markers (left end, secondary color, 11 DIP): completion `⋯`, custom word `★`, raw Latin `EN`.
  Exact words, phrases, OOV: no marker.
- No row numbers (digits are letters in Arabizi).

### 3.2 Metrics (defaults)
| Element | Value |
|---|---|
| Popup width | fit content, min 180, max 420 |
| Header height / font | 22 / Segoe UI 12, secondary color |
| Row height | 34 |
| Arabic candidate font | `appearance.font_family` (default "Segoe UI"), `appearance.font_size` 18 |
| Latin raw row font | Segoe UI 14 |
| Horizontal padding | 12 |
| Highlight | accent @ 18% alpha background + 3-px accent bar at the right edge |
| Corners / shadow | Win11 `DWMWCP_ROUNDSMALL`; `CS_DROPSHADOW` |
| Footer | 20 high, 11 DIP secondary; `appearance.footer_hints = "auto"|"always"|"never"` |
| Candidates per page | `typing.candidates_per_page` = 7 (5–9); paging shows `1/3 ▾` in the footer |

### 3.3 Colors (tokens)
| Token | Light | Dark | High contrast |
|---|---|---|---|
| bg | #FFFFFF | #23262F | COLOR_WINDOW |
| border | #D6DCE8 | #3B404D | COLOR_WINDOWTEXT |
| text | #0F1F3D (Deep Navy) | #F5F8FF (Soft Cloud) | COLOR_WINDOWTEXT |
| secondary | #5B6478 | #A3AABB | COLOR_GRAYTEXT |
| accent | #1F5BFF (Primary Blue) | #4A7DFF | COLOR_HIGHLIGHT (+ COLOR_HIGHLIGHTTEXT for text) |
Brand palette (Owner brand kit, 2026-09-24; was Windows greys + the Windows accent). The website's popup replica
(`website/src/styles.css`, `--p-*`) uses the same tokens and the same blends (selection = accent at 12 % over bg).
Theme: `appearance.theme = "system"` reads `AppsUseLightTheme`; High Contrast via `SPI_GETHIGHCONTRAST` overrides.

### 3.4 Placement
Below the composition, right edges aligned (RTL), flip above near the screen bottom, avoid the touch keyboard
(`docs/02 §9`). The popup never takes focus and never covers the composition text.

### 3.5 Mouse (implemented 2026-09-23, Owner request)
- The popup never takes focus (`MA_NOACTIVATE`): clicking it keeps the app's caret and composition.
- Click a row = same as Space on that row (commit + space unless article-join). The raw-Latin row too.
- Wheel over the list = move the highlight (up = previous, down = next), like ↑/↓.
- Clicking outside = the app handles it; the composition is finalized as shown (`docs/02 §8`).
- Backlog: hover highlight; a `◌َ` button per row opening the tashkeel editor for that row.

### 3.6 Keyboard (user-facing summary; authoritative table in `docs/02 §5.2`)
| Key | Action |
|---|---|
| Space | insert highlighted + space |
| Enter | insert highlighted, no space |
| ↓ ↑ / Shift+Tab | move highlight (wraps) · PgDn/PgUp pages |
| Tab | diacritics editor for the highlighted word |
| Ctrl+Enter | insert highlighted **with harakat from your vowels** (`3allam` → عَلَّم) |
| Esc | insert exactly what you typed in Latin |
| Shift+Space | insert exactly what you typed in Latin, plus a space (no need to scroll to the Latin row) |
| Backspace | edit the Latin; right after inserting a word, Backspace brings the word back for re-choosing |
| , ; ? | insert word, then ، ؛ ؟ |
| Ctrl+Space | Arabic ⇄ Latin typing |
| Numpad digits | always numbers |

Tab, Ctrl+Enter, Shift+Space and Ctrl+Space are user-editable (`docs/13 [keys]`, `general.mode_toggle`);
the footer shows the current keys as keycaps: `Space إدراج · Tab تشكيل · Shift+Space لاتيني`.

## 4. Tashkeel (diacritics) editor — inside the same popup

### 4.1 Anatomy (example word شكراً; revised 2026-09-23 per Owner review)
```
┌────────────────────────────────────────────────────────────────────────┐
│ [✕ مسح الكل]                           [② شَكَراً]  [① شُكْراً ✦من كتابتك] │  clear-all button (left),
├────────────────────────────────────────────────────────────────────────┤  quick-pick chips (RTL)
│                              ش ك را                                    │  the word, 42 DIP; every
│                              ▔▔ ▔▔                                     │  SELECTED letter has a tinted
│                                 ‾‾                                     │  box, the FOCUSED one an
├────────────────────────────────────────────────────────────────────────┤  underline
│ ┌────┐┌────┐┌────┐┌────┐┌────┐┌────┐┌────┐┌────┐┌────┐┌────┐            │
│ │ ◌َ  ││ ◌ُ  ││ ◌ِ  ││ ◌ْ  ││ ◌ّ  ││ ◌ً  ││ ◌ٌ  ││ ◌ٍ  ││ ◌ٰ  ││ ✕  │            │  palette cell = 3 rows:
│ │فتحة││ضمة ││كسرة││سكون││شدة ││تنوين││تنوين││تنوين││ألف ││مسح │            │  mark on ◌ (30 DIP),
│ │    ││    ││    ││    ││    ││ فتح ││ ضم ││ كسر ││خنجرية│    │            │  Arabic name (wraps),
│ │ [a]││ [u]││ [i]││ [o]││ [w]││ [A]││ [U]││ [I]││ [^]││ [x]│            │  key as a keycap
│ └────┘└────┘└────┘└────┘└────┘└────┘└────┘└────┘└────┘└────┘            │  (first cell at the right)
├────────────────────────────────────────────────────────────────────────┤
│        [← →] حرف   [Shift+← →] تحديد   [Enter] إدراج   [Esc] رجوع        │  keycap hints, RTL
└────────────────────────────────────────────────────────────────────────┘
```
Letters in Arabic are joined, so letter "cells" are rectangles drawn behind each letter's shaped glyph
cluster, never separated glyphs. Implementation (GDI): the right edge of letter k is the shaped width of the
word up to and including k, measured with a trailing ZWJ so the letter keeps its joining form. Mixed
Arabic/Latin labels are laid out piece by piece (keycap + Arabic label) instead of relying on GDI bidi.

### 4.2 Navigation and selection
- The editor opens with the **first letter** (rightmost) focused and selected — visibly highlighted.
  → moves focus visually right (logically previous), ← visually left (logically next); Home/End = first/last.
- **Selection** (Owner request 2026-09-23): marks apply to *every selected letter*.
  Shift+← / Shift+→ extend the selection; mouse: click = select that letter, Ctrl+click = add/remove,
  Shift+click or drag = range. Plain arrows collapse the selection to the focused letter.
- With one letter selected, a vowel, sukun or tanween **auto-advances** to the next letter; shadda and dagger
  alif stay (so `w` then `a` gives shadda+fatha on the same letter). With several selected, focus stays.
- **مسح الكل** (clear all) at the top removes every mark from every letter; `x` clears the selected letters.
- Mouse: click a palette cell = press its key; click a chip = that quick pick.

### 4.3 Mark keys (mnemonics shown in the palette)
| Key | Mark | Why |
|---|---|---|
| `a` | fatha َ | the vowel you type |
| `u` | damma ُ | |
| `i` | kasra ِ | |
| `o` | sukun ْ | sukun looks like a small **o** |
| `w` | shadda ّ (toggle) | shadda looks like a small **w** |
| `A` / `U` / `I` (Shift) | tanween ً ٌ ٍ | "double vowel" |
| `^` | dagger alif ٰ | |
| `x`, Delete, Backspace | clear marks on the focused letter (Backspace on a bare letter = back to the list) | |
| `1`–`8` | insert quick pick N | |
| ↑ ↓ | move through quick picks, Enter inserts | |
| Enter / Space | insert the edited word (Space adds a space) | |
| Esc | back to the candidate list (edits discarded) | |
Validity: marks allowed on every letter except ا and ى (ا accepts only fathatan, rendered per tanween style);
one vowel/sukun/tanween per letter (new replaces old); shadda combinable with a vowel/tanween.
Invalid key ⇒ brief palette flash, no change.

### 4.4 Quick picks
Order: ① vowel-derived harakat (badge "من كتابتك / from your typing") when it differs from the others,
then corpus variants by probability (`docs/03 §10.5`). Max 8.

## 5. Critical diacritics by default (no user action)
- **اللّه** and its family (والله، بالله، لله، اللهم…) styled per Settings → Diacritics (default shadda). Typing
  `allah` shows: اللّه, الله, (other styles), …
- **Adverbial tanween**: شكراً، جداً، أهلاً، طبعاً، عفواً، أبداً، مثلاً (data-driven list).
- Nothing else gets marks automatically; everything else is one Tab away.

## 6. Tray / language indicator
- Windows input indicator shows the brand icon (ع in a box) and our mode icon: **ع** (Arabic) / **A** (Latin).
  Click toggles. Right-click menu: عربي/Arabic · Latin · Settings… · Help.

## 7. Settings app (Tauri 2, bilingual; Arabic UI when Windows display language is Arabic)
| Page | Controls |
|---|---|
| General | Arabic⇄Latin toggle key (capture field); Global activation hotkey on/off + capture (shows conflicts); start companion at sign-in |
| Typing | Latin keyboard layout (Auto/list); inline preview Arabic/Latin; article joining; Arabic punctuation; numerals Western/Eastern; predictive completions; candidates per page; Backspace re-edit; on focus loss keep preview/Latin; Latin in URL & e-mail fields |
| Dialect | Auto (live bars of the detected mix) or fixed: شامي Levantine · مصري Egyptian · خليجي Gulf · عراقي Iraqi · مغربي Maghrebi · فصحى MSA |
| Diacritics & style | Allah form (4 radios with live preview); adverbial tanween on/off + style; hamza standard/relaxed; harakat from vowels light/full |
| My words | custom words table (Arabic, Latin spellings, dialect) with add/edit/delete; import/export CSV; learned choices (search, delete); **Export / Import learning** (`.t3learn`, optional settings; merge or replace; docs/03 §9.5); **Forget everything** |
| Appearance | theme; font family & size (with preview of the popup); footer hints; dialect badge |
| Apps | excluded apps (pick running exe); RTL assist on/off + window classes |
| Practice | a text box with 10 guided prompts ("type: sba7 el 5er") and live feedback |
| About | version, data version, licenses (NOTICE), privacy statement |
Settings writes `config.toml` atomically; changes apply on the next word in every app.

## 8. First run
Installer finish page: ☑ Open Type3arabi Settings (Practice page). The Practice page explains in 3 lines:
Win+Space (or the hotkey) to switch, type as you chat, Space to accept, Tab for harakat.

## 9. Accessibility
UIA provider per `docs/02 §9`; Narrator announces each highlighted candidate; high contrast honored; all
features reachable by keyboard; minimum text 11 DIP; hit targets ≥ 24×24.

## 10. Localization
UI strings in `apps/settings/src/i18n/{ar,en}.json` and `crates/t3a-ui/src/strings.rs` (Arabic + English;
popup uses Arabic labels, tooltips bilingual). Dialect names always shown in Arabic with English secondary.
