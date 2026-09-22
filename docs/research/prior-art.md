# Prior art — what exists, what we take, what we add

Research date: September 2026. Links were checked at that time.

## 1. Products

### Yamli Smart Arabic Keyboard (2007–present, web)
- Launched Nov 2007 by Language Analytics (Habib Haddad, Imad Jureidini). Browser-only (web page, JS
  widget, search engine front-end).
- Stated design goals: **flexible** (accept many ad-hoc spellings of the same word), **accurate** (type fast
  without constant corrections), **adaptive** (learn users' input patterns and dialects).
- Among the first to apply adaptive statistical modelling to transliteration: predicts the most likely Arabic
  word from ambiguous Latin input, improving from user interaction; shows a dropdown of options selectable by
  mouse or keyboard.
- **We take:** inline Arabic preview + dropdown, "last choice wins" adaptation, flexible spelling via a
  probabilistic channel, raw-Latin option. **We add:** system-wide (every Windows app), offline, dialect
  posterior, critical diacritics, in-popup tashkeel editor, vowel-derived harakat, re-edit.
Sources: yamli.com/about/keyboard, en.wikipedia.org/wiki/Yamli.

### Microsoft Maren (2009–2015, Windows IME)
- Built by Microsoft's Cairo Microsoft Innovation Center (CMIC); integrated system-wide as an IME
  ("works in the applications you use every day"), installed under *Arabic (Saudi Arabia)*, switched with
  Alt+Shift / Win+Space; required .NET 2.0. Later Afkar tools added Autocomplete (context-based, trained on
  Wikipedia/Quran/Bible), Morph (analysis with diacritic choices) and Multilingual (Bing translation).
  The Afkar site went down in 2015.
- Documented digit conventions: `2`→ء/أ, `3`→ع, `3'`→غ, `4`→غ, `5`→خ, `6`→ط, `6'`→ظ, `7`→ح, `7'`→خ, `8`→ق, `9`→ص.
  Abbreviation expansion: ISA→إن شاء الله, S3→سلام عليكم, LOL→هاهاهاها, THX→شكرا, PLZ→من فضلك, B4→قبل, etc.
- **We take:** IME-under-Arabic-language integration (proves the UX), digit table incl. apostrophe variants,
  abbreviation/phrase expansion, diacritic-choice idea (Morph). **We avoid:** managed runtime inside host
  processes (.NET 2.0 dependency), cloud dependencies.
Sources: learn.microsoft.com/archive/blogs/maren/world-meet-microsoft-maren; blogs.microsoft.com (Afkar, 2011);
ujca.cz/resources/ime/maren (archived installer + usage table).

### Google Input Tools / Transliteration IME (Windows desktop until May 2018)
- Arabic transliteration IME for Windows; removed in 2018; still a Chrome extension. Shows that a large
  vendor shipped exactly this product class as a Windows IME.

### Keyboards on mobile
- Gboard/SwiftKey offer Arabizi-ish transliteration on phones; Noon Keyboard (Android) is Yamli-based.
  Not relevant for Windows, but confirms user demand.

## 2. Open-source code

| Project | License | Relevance |
|---|---|---|
| ArabiziKit (`rb2625/arabizi-kit`, 2026) | MIT | Hybrid lexicon-first + context-rule engine producing ranked candidates; dialect hints (9=ق in Morocco vs ص in Egypt; doubled consonants; emphatic capitals; assimilated article); learned word table + char trigram LM + Naive Bayes dialect classifier. Reported **sentence**-exact on external sets: EGY 0.376, LEV 0.296, MAG 0.088 (hit@3 0.526/0.429/0.115) — shows rules alone are not enough and **a large LM + lexicon is the key lever**. We adopt its context-rule ideas as seed rules. |
| CAMeL Lab seq2seq-transliteration-tool | see repo | Shazal, Usman & Habash 2020 (WANLP): unified Arabizi detection + transliteration; seq2seq trained on BOLT Egyptian (LDC, non-redistributable). |
| saschanaz/ime-rs | see repo | Rust port of Microsoft SampleIME, working. Template for our COM/TSF structure. |
| mkpoli/ainuKey | see repo | Minimal working windows-rs TIP (romaji→katakana) with preedit/commit — proves our stack. |
| Microsoft SampleIME | MIT | Canonical TSF IME sample (candidate window, compartments, display attributes, registration). |
| corvusskk, rime/weasel | see repos | Mature C++ TSF IMEs; app-quirk knowledge. |

## 3. Academic results (word-level unless noted)

| Work | Dialect | Method | Result |
|---|---|---|---|
| Al-Badrashiny, Eskander, Habash, Rambow 2014 (3arrib) | EGY | char FST generates candidates → morphological analyzer filter → word 5-gram LM | 69.4% accuracy |
| Shazal, Usman, Habash 2020 | EGY | seq2seq on 287k word pairs (BOLT) | 80.6% (95% "acceptable") |
| Talafha, Abu Ammar, Al-Ayyoub 2021 (ATAR) | LEV (Jordanian) | attention LSTM, released 25k-pair corpus | 79% accuracy, BLEU 88.49 |
| Hajbi et al. 2024 | MAG | rules + weighted Levenshtein vs 294k-word Darija lexicon | — |
| Transformer Moroccan Arabizi (Expert Syst. Appl. / 2025) | MAG | transformer + semi-automatic dataset | — |
| Darwish 2014 | EGY | CRF word-level Arabizi vs English ID | 98.5% LID accuracy |
**Takeaways:** (1) Lexicon/LM-constrained generation is competitive and far cheaper at runtime than seq2seq;
(2) ~80% word accuracy is the published ceiling without user adaptation or context — our 85% target relies on
dialect-mixture LMs from FineWeb-2, context bigrams, and sticky user choices; (3) Maghrebi is the hardest
(French-influenced spelling, sparse data) — plan data collection accordingly.

## 4. Data landscape
See `data/sources.toml` for every dataset with license status. Key facts:
- **FineWeb-2** (ODC-By) gives large Arabic-script text for MSA and several dialect codes (arz, apc, ary, …)
  — our lexicon/LM backbone.
- Parallel Arabizi corpora exist (Jordanian 25k pairs, several HF sets) but most have **no stated license**
  ⇒ eval-only until authors grant permission. Hence the **golden-set protocol** (owned CC0 data).
- Diacritized text: Tashkeela (75M words, mostly classical; license conflicting), Maknuune (Palestinian,
  CC BY-SA). FineWeb-2's naturally marked tokens (e.g. شكراً) give us the critical-diacritics statistics without them.

## 5. Platform facts that shaped the architecture
- Windows blocks IMM32 IMEs in modern apps; custom IMEs must be **TSF** TIPs, loaded into each app process,
  subject to the app's AppContainer restrictions (no network, dictionaries under Program Files or ACL'd).
- TIPs declare compatibility with `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT` and tray support via the input
  indicator (black/white glyph icons inside the DLL).
- Candidate windows must be owned windows (`ITfContextView::GetWnd` → fallback `GetFocus`), raise
  `EVENT_OBJECT_IME_SHOW/HIDE/CHANGE`, expose UIA with `IME_Candidate_Window`, and must not declare their own DPI awareness.
- Registration via `ITfInputProcessorProfileMgr::RegisterProfile`; immediate enablement via `InstallLayoutOrTip`;
  never write the default-keyboard registry directly. 32- and 64-bit DLLs share one CLSID.
- Third-party IMEs must be digitally signed to install without critical warnings.
- Firefox/Chromium signal private browsing to IMEs via the `IS_PRIVATE` input scope.
- Azure Artifact Signing: public-trust individual validation only in USA/Canada; organizations only in a listed
  set of countries (Jordan not included as of 2026) ⇒ OV certificate from a public CA.
Sources: learn.microsoft.com/windows/apps/develop/input/input-method-editor-requirements;
learn.microsoft.com/windows/win32/w8cookbook/third-party-input-method-editors;
learn.microsoft.com/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-registerprofile;
learn.microsoft.com/windows/win32/tsf/64-bit-platform-considerations;
bugzilla.mozilla.org/show_bug.cgi?id=1549394; learn.microsoft.com/azure/artifact-signing/quickstart;
learn.microsoft.com/windows/apps/package-and-deploy/code-signing-options.
