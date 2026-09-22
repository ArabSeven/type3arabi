# Glossary

| Term | Meaning |
|---|---|
| Arabizi / Franco / chat alphabet | Arabic written with Latin letters and digits standing for Arabic sounds (`3`=ع, `7`=ح, `2`=ء/ق, `5`=خ, `9`=ص or ق, `6`=ط, `8`=ق/غ/ه). |
| Base form | Arabic word with all diacritics removed; used for matching and evaluation. |
| Candidate | One option in the popup. Kinds: Word, Completion, Oov, Phrase, Number, Custom, RawLatin. |
| Channel / transliteration model (TM) | Probabilities of Arabic chunks given Latin chunks, learned by EM. |
| Chunk | A short Latin substring (1–4 symbols) or Arabic substring (0–3 letters) aligned as a unit. |
| Completion | A candidate that continues beyond what has been typed (predictive). |
| Composition | The underlined, uncommitted text a TIP owns inside the app's document. |
| Dialect group | MSA, LEV (Levantine), EGY (Egyptian/Sudanese), GLF (Gulf/Yemeni/Najdi), IRQ (Iraqi), MAG (Maghrebi). |
| Dialect posterior π | Running estimate of which dialect the user writes, updated after each commit. |
| Display form | The candidate text after display transforms (sacred-name marks, tanween, hamza style). |
| Emphatic capitals | Maghrebi convention where `T S D Z H` mean ط ص ض ظ ح. |
| Gemination | A doubled consonant (`bb`) that maps to one Arabic letter with shadda. |
| Golden set | Our own native-speaker Arabizi↔Arabic data (CC0), the primary benchmark. |
| Harakat / tashkeel / tashkil | Arabic diacritics: fatha َ, damma ُ, kasra ِ, sukun ْ, shadda ّ, tanween ً ٌ ٍ, dagger alif ٰ. |
| hit@k | Fraction of words whose reference is among the top-k candidates. |
| KSR | Keystrokes per word including navigation — lower is better. |
| Lattice column | Set of search states that consumed exactly the first j Latin symbols. |
| LANGID | Windows language identifier, e.g. 0x2C01 = Arabic (Jordan). |
| Latin layout | The user's physical Latin keyboard (US, AZERTY…) used to translate keys even while Windows' input language is Arabic. |
| OOV | Out-of-vocabulary: a word not in the lexicon (names, foreign words), generated letter by letter. |
| Raw Latin | The exact text typed, offered as the last candidate and inserted by Esc. |
| Re-edit | Backspace right after inserting a word brings back its Latin buffer and the popup. |
| Safe passthrough | TIP state after an internal error: eats no keys, shows no UI. |
| Sticky choice | The user's last choice for a Latin key is ranked first next time. |
| T3A code | 1-byte internal code for each Arabic letter (docs/12 §3). |
| TIP | Text Input Processor — a TSF input method implemented as an in-proc COM DLL. |
| TSF | Text Services Framework — Windows' input-method framework (msctf). |
| UILess mode | Host (e.g. a game) draws the candidate list; the TIP only supplies data via UIElement interfaces. |
