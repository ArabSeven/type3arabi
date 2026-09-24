# 03 — The prediction engine (`t3a-engine`)

This is the product's brain. It is specified so that an implementer makes **no** modelling decisions:
every formula, data structure, default and tie-break is here. Tunable numbers live in `EngineParams`
(§12) and are finally set by the tuning stage of the pipeline (`docs/04 §7`).

## 1. Overview

```
 keystroke ─► normalize (§3) ─► Latin buffer L ─► incremental lattice search (§5)
                                                     ├─ lexicon-constrained beam over the Arabic trie ─► exact words + completions
                                                     ├─ unconstrained beam + Arabic char-LM (OOV)      ─► names / foreign words
                                                     └─ special generators (§6): phrases, numbers, raw Latin
                                        score each candidate with the log-linear model (§7)
                                        assemble & rank the list (§8) ─► display transforms (§10) ─► CandidateList
 commit ─► Commit{text, trailing} + UserEvent ─► user model update (§9) + dialect posterior update (§7.2)
```

Design lineage (see `docs/research/prior-art.md`): the **noisy-channel / lexicon + LM** approach of Yamli,
Maren and Google Input Tools (fast, predictable, adaptive), the **FST-generate + LM-rerank** pipeline of
Al-Badrashiny et al. 2014, ArabiziKit's **context rules + dialect hints**, and Yamli's **adaptive
last-choice memory**. We add: incremental lattice reuse, dialect-mixture LMs with an online posterior,
vowel-derived tashkeel, data-driven critical diacritics, and surrounding-text context.

## 2. Notation
- `L = l₁…lₙ` normalized Latin buffer (symbols, §3). `L[i..j]` a chunk.
- `A` Arabic alphabet in **T3A codes** (`docs/12 §3`, 42 letters, 1 byte each). Marks are *not* in `A`.
- `V` lexicon: undiacritized Arabic word forms (sequences over `A`), each with a record (§4 of `docs/12`).
- `D = {MSA, LEV, EGY, GLF, IRQ, MAG}` dialect groups, index 0..5 in that order.
- `π ∈ Δ(D)` current dialect posterior (sums to 1).
- All log-probabilities are natural logs (`ln`). Quantized on disk as `u8 q` with `lp = −q / 8.0`; `q = 255` means "absent".

## 3. Input normalization (`normalize.rs`)

Input arrives as `InputChar { ch: char, literal_digit: bool }`. The session keeps both the **raw buffer**
(exactly what was typed, for the raw-Latin candidate and Esc) and the **normalized buffer** `L`.

Per character, in order:
1. **Accented Latin → base**: `à â ä → a`, `é è ê ë → e`, `î ï → i`, `ô ö → o`, `ù û ü → u`, `ç → s`, `ñ → n`. Others unchanged.
2. **Case**: letters are stored lowercase, but uppercase `T S D Z H` become the *emphatic symbols*
   `Ŧ Ş Đ Ẓ Ḥ` (internal code points, never shown) **unless** (a) it is the first letter of the token,
   or (b) the token is entirely uppercase. Case (a)/(b) are decided lazily: when the 2nd character arrives,
   if the whole token so far is uppercase, re-normalize it to lowercase; the first char is always lowercase.
   Emphatic symbols have their own rules (`T→ط 0.8`), and the seed file maps them; lowercase fallbacks exist.
3. **Literal digits** (`literal_digit = true`, from the numpad) are kept as digits but flagged; any flagged
   digit forces the token to be a number (§6.1).
4. **Apostrophe variants** `’ ‘ ʼ ´ \`` → `'`.
5. **Elongation**: a run of ≥ 3 identical characters collapses to 2 in `L` (`7abibiiiii → 7abibii`).
   Exception: laughter tokens (§6.3) are detected on the raw buffer first.
6. **Allowed symbols in `L`**: `a–z`, the 5 emphatic symbols, digits `0–9`, `'`. Hyphen is a *joiner*
   only directly after an article (§6.4); any other character ends the token (handled by the TIP as
   punctuation/commit).

## 4. Transliteration model (TM)

### 4.1 Rules
A rule `r = (ℓ, α, pos, lp[D], flags)`:
- `ℓ` Latin chunk, 1–4 symbols (`"sh"`, `"3'"`, `"kh"`, `"ou"`, `"ess"`),
- `α` Arabic chunk, **0–3** letters (0 = the Latin symbols are vowels written as a short vowel, i.e. nothing),
- `pos ⊆ {I, M, F}` (initial / medial / final; `IF` = whole-word), `*` = any,
- `lp[d]` = `ln P(α | ℓ, pos, d)` per dialect; a missing dialect value falls back to the `*` row,
- `flags`: `GEM` (α's letter is geminated → shadda candidate), `TANWEEN` (α is `ا` standing for `اً`),
  `VOWEL(kind)` (the chunk is a vowel: `a|e|i|o|u` family, used by §10.6), `ARTICLE` (α contains the article).

Position of a chunk `L[j..j+k]`: `I` if `j = 0`; `F` if `j + k = n` **and the word is being finalized**
(see §5.5); else `M`. A rule with `pos = I` applies only at `j = 0`; `F` only in final expansion.

The model is **log-linear, direct** (`P(α|ℓ)`), not a generative channel `P(ℓ|α)`: it is estimated by
counting aligned chunk pairs (EM, `docs/04 §6`) and combined with the LM using tuned weights.

### 4.2 Effective rule table
When `π` changes (at most once per word, on session reset), build `EffectiveRules`: for every rule,
`lp_eff = ln Σ_d π_d · exp(lp[d])`. Stored in a pre-allocated table indexed by chunk id, so a transition
costs one array read. Chunks are indexed by a tiny hash map `chunk → [rule ids]` built at load time
(≤ 2 000 chunks).

### 4.3 Gemination (generic, not in the seed file)
For a chunk `XX` where `X` is a single consonant symbol or digit (`bb`, `ll`, `77`, `ss`…), add
virtual rules: for every rule `(X → α, |α| = 1)`, a rule `(XX → α)` with `lp = lp(X → α) + ln p_gem`
and flag `GEM` (`p_gem = params.p_gem`, default 0.75). The two-letter reading (`ll → لل`, needed for
`الله`) arises naturally from two single transitions. Digraph doubles (`shsh`, `khkh`, `thth`) get the
same treatment.
Conversely, when two consecutive transitions consume the **same doubled consonant chunk** and emit the
**same letter** (the two-letter reading `bb → بب`), that path pays `ln(1 − p_gem)`, so gemination is the
default reading of doubled consonants; the lexicon still picks `لل` where a word needs it (`الله`).

### 4.4 Arabic insertions (Arabic letter with no Latin)
Only one, applied in final expansion: after a word-final `و` produced by a vowel rule, optionally
append `ا` (wāw al-jamāʿa: `katabou → كتبوا`) with `lp = ln params.p_waw_alif` (default 0.25).

### 4.5 Seed
`data/seed/mappings.tsv` is the hand-authored prior (human-reviewed). The pipeline's EM stage uses it
as the Dirichlet prior and initial alignment model, then emits the trained table into the data file.
The engine never reads the TSV at runtime except in `t3a-cli --seed-only` mode (bootstrap before M2).

## 5. Search (`search.rs`)

### 5.1 Lattice, incrementally
The lattice has one **column per prefix length** `0..=n`. Column `j` holds up to `B` states that have
consumed exactly `L[0..j]` using **non-final** rules. Columns depend only on `L[0..j]`, so:
- `push(ch)`: compute column `n+1` from columns `n+1−k` (`k = 1..=4`, chunk `L[n+1−k..n+1]`) and then
  run final expansion (§5.5) for column `n+1` into a scratch area.
- `pop()`: drop column `n` and recompute final expansion for column `n−1` (cached from before if unchanged).
Cost per keystroke: `O(4 · B · R)` transitions, `R` ≈ rules per chunk (≤ 8).

### 5.2 State
```rust
struct State {            // 24 bytes, Copy, lives in a pre-allocated column Vec
    node: u32,            // trie node index (lexicon) — or OOV_ROOT_SENTINEL for OOV lattice
    g: f32,               // accumulated λ_tm · TM
    h: f32,               // heuristic: λ_lm · maxlp(node)   (§5.4)
    back: u32,            // index into the backpointer arena: (prev column, prev state idx, rule id)
    last_letter: u8,      // T3A code of the last emitted letter (0 = none)
    flags: u8,            // GEM on last letter, VOWEL_PENDING, ARTICLE_SEEN
    depth: u8,            // letters emitted
    _pad: u8,
}
```

### 5.3 Transition
For state `s` in column `j` and each rule `r` with `ℓ = L[j..j+k]`, `pos` compatible:
walk the trie from `s.node` along `α`'s letters (0–3 child lookups; children are sorted by code →
binary search, ≤ 42 entries). If every step exists → new state in column `j+k` with
`g' = s.g + λ_tm · lp_eff(r)`, `h' = λ_lm · maxlp(node')`. For `α = ε` the node is unchanged.
**Recombination**: two states in the same column with the same `(node, last_letter, flags & GEM)` keep only
the higher `g` (Viterbi). Use a small open-addressing map per column (capacity `2B`).

### 5.4 Pruning
Priority `f = g + h`. Keep the best `B = params.beam` (default 96) states per column, and drop any state
with `f < f_best − params.prune_delta` (default 12.0). `maxlp(node)` is the best dialect-mixture unigram
log-prob of any word in the node's subtree, stored per node as a `u8` (global mixture at build time;
the per-session dialect mixture is only applied at scoring — admissible enough in practice).

### 5.5 Final expansion
From column `n` (and columns `n−k` via final rules with `ℓ = L[n−k..n]`): apply `F`/`IF` rules and
the waw-alif insertion. Also carry over every column-`n` state unchanged (words that end on a
medial-rule path). Results go to a scratch set (not stored as a column).

### 5.6 Candidate generation from the lattice
1. **Exact words**: every final-expansion state whose node is **terminal** → word id `w`.
   Keep the best `g` per `w`. Take the top `params.k_exact` (12) by `g + λ_lm·LM(w)` for full scoring.
2. **Completions** (if `config.predictive_completions` and `n ≥ params.min_completion_len` (3)):
   for the top `params.m_completion_seeds` (6) non-final states by `f`, best-first search in the subtree
   (priority `g + λ_lm·maxlp(child) − params.gamma_completion · extra_depth`), max
   `params.completion_node_budget` (256) node pops in total; emit up to `params.k_completion` (2)
   words with `extra_depth ∈ 1..=6`, score gets `−γ·extra_depth` (`γ` default 0.9).
3. **OOV**: a parallel lattice without the trie: state carries the emitted letters (≤ 24, inline array)
   and a char-LM context (last 4 codes). Transition score adds `λ_chr · ln P_chr(letter | ctx)`.
   Beam `params.beam_oov` (32). At the end add the end-of-word char-LM term. Emit the top
   `params.k_oov` (2) strings not already present as exact words.
4. **User trie**: custom words (§9.5) live in an in-memory trie with the same node API; searched in the
   same pass (a second root), candidates tagged `Custom`.

### 5.7 Budgets
Engine time per keystroke p99 ≤ 3 ms on the reference laptop (`docs/06`). No heap allocation after
warm-up (columns, arenas, maps, candidate arena are reused; capacity fixed by params).

## 6. Special tokens & rules

### 6.1 Numbers
Token = only digits (optionally containing `.`/`,` between digits, handled by the TIP as part of number
mode) **or** contains a literal (numpad) digit ⇒ `Number` candidate first: digits in
`style.numerals` (`"western"` 0-9 default, `"eastern"` ٠-٩ U+0660–U+0669). If `n ≥ 2` digits-only,
no Arabizi candidates at all; if a single top-row digit, Arabizi candidates follow the number.

### 6.2 Phrases & abbreviations (`data/seed/phrases.tsv` → `PHRS` section)
Exact match of the normalized key (lowercase, emphatics lowered, apostrophes removed, elongation
already collapsed to ≤ 2 repeats) against the phrase keys, which are stored in the same form. Output may be multi-word Arabic. Field `default=1` ⇒ phrase goes to rank 1 (unless a sticky
user choice exists); `default=0` ⇒ inserted at rank 2. Dialect-tagged rows are included only if
`π_d ≥ 0.15` for one of their dialects.

### 6.3 Laughter & elongation
Raw token (lowercased) that starts with `h`, contains only `h`, `a`, `e`, and has ≥ 3 `h` ⇒ candidate
`ه × min(count_h, 8)` at rank 1 (`hhhhh → ههههه`, `hahaha → ههه`). `lol` is a phrase entry.

### 6.4 Article joining & joiners
If the committed token is exactly `el | al | il | l | 'l` (article) or `w | wa | we` (conjunction),
and `config.typing.article_joining = true`, the commit's trailing is `None` (no space), so the next word
attaches: `el` + `yom` ⇒ `اليوم`. Output for the article token: `ال`; for `w`: `و`. The hyphen typed right
after an article (`el-yom`) is swallowed. Sun-letter assimilation is handled inside single tokens by
seed rules (`ess → الس`, `enn → الن`, …).

### 6.5 Clitics
v1: the lexicon contains cliticized forms observed in corpora (`wbi7ebbha → وبيحبها` if seen).
M9: a clitic lattice (prefix set × stem × suffix set) for unseen combinations.

### 6.6 Punctuation mapping (applied by the TIP, table here)
With `style.arabic_punctuation = true`: `,` → `،` (U+060C), `;` → `؛` (U+061B), `?` → `؟` (U+061F).
`%` → `٪` only if numerals are eastern. Everything else unchanged. Punctuation always ends a token.

## 7. Scoring

### 7.1 Formula
For a candidate `c` from buffer `L` with previous context words `p₁ p₂`:

```
score(c) = λ_tm · TM(c)                      // best alignment log-prob (g of its state, un-weighted then weighted)
         + λ_lm · LM(c)                      // ln Σ_d π_d P(w|d)
         + λ_ctx · CTX(c | p₁)               // clamp(ln P_bigram(w|p₁) − ln P(w), −2, +4); 0 if no bigram
         + λ_usr · USR(L, c)                 // §9.2
         + KIND(c)                           // Exact 0, Completion −γ·extra_depth, Oov −params.oov_penalty (4.0),
                                             // Custom +params.custom_bonus (1.0)
```
OOV candidates use `λ_chr · CHR(c)` (char-LM log-prob) instead of `λ_lm · LM`.
Default weights (before tuning): `λ_tm = 1.0, λ_lm = 0.8, λ_ctx = 0.6, λ_usr = 1.5, λ_chr = 0.35`.
Ties (|Δ| < 1e-4): higher `LM`, then shorter display string, then lexicographic T3A order.

### 7.2 Dialect mixture and online posterior
- Each lexicon word stores `lp[d]` for the 6 groups (`u8`, 255 = unseen in that dialect; unseen is
  treated as `params.unseen_dialect_lp` = −18.0).
- **Prior**: `config.dialect.profile`: `"auto"` ⇒ prior from region (`GetUserGeoID(GEOCLASS_NATION)`
  → ISO country → `data/seed/region_priors.tsv`, then uniform with MSA 0.25 — the TIP's LANGID is
  always ar-SA (docs/02 §2.1) and is never used as a dialect signal); a fixed profile `"LEV"` etc. ⇒ `π = 0.8` on it, `0.15` MSA, rest spread.
- **Update** after each commit of a lexicon/custom word `w` (not phrases, numbers, raw):
  `π_d ← normalize( π_d^(1−η) · P(w|d)^η )` (computed in log space), `η = params.dialect_eta` (0.35),
  then floor every `π_d` at `params.dialect_floor` (0.02) and MSA at 0.10; renormalize.
  Only in `"auto"`. `π` is persisted in the user snapshot (§9.4) when learning is allowed.
  Measured with `t3a-cli adapt` (2026-09-24, LEV + MAG test sets): after a switch the new dialect leads within
  a median of 3–5 words (max 22), and top-1 on a stream alternating dialects every 25 words is 55.2%, within
  ~1 point of knowing the dialect in advance. η = 0.08 needed 6–17 words (and 76+ before the floor fix).
- The `EffectiveRules` table (§4.2) is rebuilt when `π` moved by more than 0.02 (L1) since the last build.

### 7.3 Context bigram
`BIGR` section: for each previous word id, a sorted array of `(next_id: u32, q: u8)` (top ≤ 64 next words
by KN-smoothed probability, `count ≥ 3`). Lookup = binary search. `p₁` comes from surrounding text
(`docs/02 §12.1`) mapped to a word id by exact trie lookup (after stripping marks); unknown ⇒ no context.

### 7.4 Char LM (OOV)
5-gram over T3A codes + `^`/`$` boundaries, stupid backoff (α = 0.4), stored in `CHLM` as an
open-addressing table of `(fingerprint: u32, q: u8, order: u8)`; `≤ 600k` entries.

## 8. List assembly (`rank.rs`)
1. Gather candidates: phrase(s), exact words (≤ 12), completions (≤ 2), custom words, OOV (≤ 2), number.
2. Score (§7). Map each to its **display string** (§10). Deduplicate by display string, keeping the max score
   (and the "best" kind: Exact > Custom > Phrase > Completion > Oov).
3. Sort by score, descending.
4. **Sticky last choice**: if learning is readable and the user model has `last_choice(L) = w` with
   `count ≥ 1` and `neg(L,w) == 0` after that choice → move `w` to rank 1.
5. Phrase `default=1` → rank 1 (after sticky), `default=0` → rank 2.
6. Number (digits-only token) → rank 1, overriding all.
7. Truncate to `params.max_candidates` (21 = 3 pages of 7).
8. **Raw Latin** candidate = the raw buffer; not scored; the UI always shows it as the **last row of the
   visible page** (`docs/05 §3`). If sticky says the user chose raw Latin for this `L`, it is rank 1 instead.

## 9. User model (`user.rs`)

### 9.1 Stored facts
- `choices: map LatinKey → small list of (WordRef, count: u16, last_ts: u32)` (≤ 4 per key)
- `neg: map (LatinKey, WordRef) → u16`
- `word_counts: map WordRef → (count: u32, last_ts: u32)`
- `custom: list of CustomWord { arabic, latin_hints[], dialect_mask, added_ts }`
- `pi: [f32; 6]`
`LatinKey` = normalized buffer, lowercase ASCII, ≤ 32 bytes (longer ⇒ not learned).
`WordRef` = the **undiacritized Arabic string** (not a lexicon id — ids change between data builds).
Resolved to a lexicon id lazily and cached.

### 9.2 Features
`decay(t) = 0.5 ^ (age_days / params.user_half_life_days)` (90).
`USR(L, w) = ln(1 + c(L,w)·decay) + 0.3 · ln(1 + c(w)·decay) − 0.7 · ln(1 + neg(L,w))`.

### 9.3 Updates (only if the context allows learning, `docs/02 §7`)
On commit of candidate `w` at displayed rank `r` for key `L`:
- `c(L,w) += 1`, `last_ts = now`; `c(w) += 1`; `neg(L,w) = 0` (an explicit choice clears earlier negative evidence).
- If `r > 0` **and** the user navigated (arrow/mouse), then for the rank-1 candidate `t ≠ w`: `neg(L,t) += 1`.
- If `w` is OOV and has now been committed ≥ 2 times ⇒ auto-add as `CustomWord` with `latin_hints = [L]`.
- Esc (raw Latin commit) ⇒ `c(L, RAW) += 1` (so frequent English words become sticky Latin).
No update on: implicit finalization (focus loss, app-terminated composition), re-edit restore, numbers.

### 9.4 Persistence and multi-process consistency
Files in `%LOCALAPPDATA%\Type3arabi\user\`:
- `journal.t3j`: append-only **128-byte records**:
  `magic u16 = 0x5433 | ver u8 = 1 | kind u8 | ts u32 | latin_len u8 | arabic_len u8 | latin [32] | arabic [84] (UTF-8) | crc16 u16`.
  Kinds: `1 Choose`, `2 Negative`, `3 AddWord`, `4 DeleteWord`, `5 Dialect(pi as 6×f16 in arabic field)`, `6 Wipe`.
  Written by the per-process writer thread with one `WriteFile` on a handle opened with `FILE_APPEND_DATA`
  and `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`. Bad CRC ⇒ record skipped.
- `snapshot.t3u`: compacted model (versioned, little-endian, length-prefixed strings), replaced atomically
  (`write temp → MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`).
- **Tailing**: at composition start each TIP compares the journal size with its read offset; grows ⇒
  read and apply new records; shrinks ⇒ reload snapshot and read the journal from 0.
- **Compaction**: when the journal exceeds 256 KB, the writer thread try-locks the named mutex
  `Local\Type3arabi.UserStore` (per session); on success merges snapshot + journal, evicts to caps
  (50 000 keys LRU by `last_ts`, 5 000 custom words), writes the snapshot, truncates the journal.
- AppContainer / secure desktop / IS_PRIVATE: read-only (snapshot + journal readable through the ACL),
  never write.

### 9.5 Custom words
Added via Settings (Arabic + optional Latin spellings + dialect) or auto-learned (§9.3). They form an
in-memory **user trie** searched with the main one; LM score = `params.custom_lm` (−9.0, a mid-frequency
word) plus `KIND` bonus. Latin hints additionally act as exact phrase keys (rank 1).

## 10. Display transforms & diacritics (`display.rs`, `tashkeel.rs`)

### 10.1 Unicode output rules
- Letters U+0621–U+064A plus the extra letters in `docs/12 §3`. Never output tatweel (U+0640),
  presentation forms (U+FB50–U+FEFF), ZWJ/ZWNJ, or bidi control characters.
- **Mark order per letter**: base letter → shadda (U+0651) → at most one of {fatha U+064E, damma U+064F,
  kasra U+0650, sukun U+0652, fathatan U+064B, dammatan U+064C, kasratan U+064D} → superscript alef (U+0670).
  (Typing order, as produced by Arabic keyboards; deliberately *not* NFC, which would put the vowel before shadda.)
- Comparison/dedup and eval always operate on the **base form** (marks stripped).

### 10.2 Sacred names (`style.allah_form`)
Word set `SACRED = {الله, والله, بالله, تالله, فالله, لله, ولله, فلله, اللهم}` (exact base-form match),
including inside phrase outputs (word by word). Transform: find the last `ل` that is followed by `ه`, insert:
| style | inserted after that ل | example |
|---|---|---|
| `plain` | — | الله |
| `shadda` (**default**) | U+0651 | اللّه |
| `shadda_fatha` | U+0651 U+064E | اللَّه |
| `shadda_dagger` | U+0651 U+0670 | اللّٰه |
When style ≠ `plain`, the plain form is also offered as the next candidate.

### 10.3 Adverbial tanween (`style.adverbial_tanween = true` default)
Lexicon words flagged `TANWEEN_FATH` (pipeline: ends in `ا`, and `count(word + U+064B) / count(word) ≥ 0.25`
with ≥ 50 occurrences in approved corpora — e.g. شكراً، جداً، أهلاً، طبعاً، عفواً، أبداً، مثلاً، تقريباً، حالاً)
get fathatan per `style.tanween_style`: `"on_alif"` (default: `شكراً` = … ر ا U+064B), `"before_alif"`
(`شكرًا` = … ر U+064B ا), `"off"`. The un-tanweened form is **not** added as a separate candidate
(same base form); the user can remove marks in the tashkeel editor.

### 10.4 Hamza style
`style.hamza = "standard"` (default, lexicon spelling) | `"relaxed"` (map أ إ آ → ا in output; ؤ ئ ء unchanged).

### 10.5 Vocalized variants (quick picks)
`DIAC` section: per word id up to 8 fully/partially vocalized spellings with `q` (from approved
diacritized corpora, `docs/04 §5`). The tashkeel editor shows: (1) vowel-derived harakat (§10.6) if it
differs from all variants, then (2) variants by probability, dialect-filtered.

### 10.6 Vowel-derived harakat ("harakat from your vowels")
Using the winning alignment (backpointers) of `L` to the candidate:
1. Walk aligned rules left to right, tracking the last emitted *consonant* letter position `k`.
2. Rule with `GEM` ⇒ shadda on its letter.
3. Rule with `VOWEL(v)` and `α = ε` right after letter `k` ⇒ short vowel on `k` from
   `data/seed/vowel_marks.tsv` (per dialect: `a → fatha`, `i → kasra`, `u/o/ou → damma`, `e → kasra`
   for LEV/EGY/MAG/IRQ else fatha).
4. Vowel rule with `α ∈ {ا, و, ي}` (long vowel) after letter `k` ⇒ fatha / damma / kasra on `k` respectively.
5. Final `TANWEEN` rule ⇒ fathatan per §10.3 style.
6. `style.vowel_harakat = "full"` additionally puts sukun on a consonant followed directly by another
   consonant rule (not on the last letter, not on ا/و/ي/ى/ة, not on the article's ل).
7. Never mark: the article's ا and ل; long-vowel letters; ى; ة.
Examples (tests): `3allam → عَلَّم` (ع U+064E ل U+0651 U+064E م), `3ilm → عِلم` (light), `3ilm → عِلْم` (full),
`madrase → مَدرَسة` (light: fatha on م and ر from the typed `a`s; nothing on د because no vowel was typed after it;
the final `e` maps to ة, which is never marked).
The harakat always follow the alignment that won for the chosen word, so they reflect what the user typed.

## 11. Public API (Rust; implemented as skeleton in `crates/t3a-engine/src/lib.rs`)

```rust
pub struct Engine;                                   // immutable, shareable across sessions (Send + Sync)
impl Engine {
    pub fn new(data: t3a_data::DataView<'_>) -> Result<Self, EngineError>;   // M3 (returns LexiconNotImplemented today)
    pub fn seed_only(seed: SeedTables) -> Self;      // bootstrap: rules only, no lexicon (OOV path)
    pub fn builtin() -> Self;                        // seed_only over the seed files compiled into the binary
    pub fn params(&self) -> &EngineParams;
}
pub struct Session<'e> { /* columns, arenas, context, pi, settings */ }
impl Session<'_> {
    pub fn new(engine: &Engine, settings: EngineSettings) -> Self;   // EngineSettings: From<&Config>
    pub fn set_context(&mut self, prev_words: &[&str]);
    pub fn set_dialect(&mut self, pi: [f32; 6]);
    pub fn push(&mut self, c: InputChar, user: &dyn UserScorer) -> &CandidateList;
    pub fn pop(&mut self, user: &dyn UserScorer) -> Option<&CandidateList>;   // None ⇒ buffer empty
    pub fn restore(&mut self, latin: &str, user: &dyn UserScorer) -> &CandidateList;
    pub fn reset(&mut self);
    pub fn candidates(&self) -> &CandidateList;
    pub fn commit(&mut self, index: usize, how: CommitHow) -> Commit;          // resets the session
    pub fn vocalizations(&self, index: usize) -> Vec<String>;               // tashkeel quick picks (UI-time only)
    pub fn vowel_harakat(&self, index: usize) -> Option<String>;
    pub fn explain(&self, index: usize) -> Explanation;                    // debug (CLI only)
}
pub trait UserScorer { fn usr(&self, key: &str, word: &str) -> f32; fn sticky(&self, key: &str) -> Option<String>; }
```
Implemented today (seed-only): everything above except `Engine::new` and `vocalizations` (M3).
M3 target representation: `CandidateList` owns a reusable `String` arena and `Candidate { text: Range<u32>,
base: Range<u32>, kind, score, word_id }` (the current `String` fields allocate; allowed only until M3).

## 12. `EngineParams` (defaults; overwritten by the data file's `PARM` section)

| Param | Default | Meaning |
|---|---|---|
| `beam` | 96 | states per column (lexicon lattice) |
| `beam_oov` | 32 | states per column (OOV lattice) |
| `prune_delta` | 12.0 | drop states with `f < f_best − delta` |
| `k_exact` / `k_completion` / `k_oov` | 12 / 2 / 2 | candidates kept per generator |
| `min_completion_len` | 3 | completions only after 3 symbols |
| `m_completion_seeds` / `completion_node_budget` | 6 / 256 | completion search bounds |
| `gamma_completion` | 0.9 | per extra letter penalty |
| `p_gem` / `p_waw_alif` | 0.75 / 0.25 | §4.3 / §4.4 |
| `oov_penalty` / `custom_bonus` / `custom_lm` | 4.0 / 1.0 / −9.0 | §7.1 / §9.5 |
| `unseen_dialect_lp` | −18.0 | §7.2 |
| `dialect_eta` / `dialect_floor` | 0.35 / 0.02 | §7.2 |
| `user_half_life_days` | 90 | §9.2 |
| `lambda_{tm,lm,ctx,usr,chr}` | 1.0 / 0.8 / 0.6 / 1.5 / 0.35 | §7.1 |
| `max_candidates` | 21 | §8 |

## 13. Debuggability
`t3a-cli explain "<arabizi>"` prints, per candidate: alignment (`7|a|b|i|b|i` → `ح|ε|ب|ي|ب|ي`), each
feature value, weighted sum, and why it ranks where it does. Every accuracy bug report must include this output.
