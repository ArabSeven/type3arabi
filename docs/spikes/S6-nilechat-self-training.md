# S6 — Self-training the transliteration rules on NileChat (EGY, MOR)

**Date:** 2026-09-24 · **Agent:** Claude · **Time box:** one session · **Spec:** docs/04 §6.4

## Question
NileChat (`nilechat-arabizi-egy`, 552k docs; `nilechat-arabizi-mor`, 1.40M docs; CC BY-NC 4.0, approved for
`rules`) is monolingual synthetic Arabizi. Can pseudo-labels from our own engine improve the rules, above all for
Egyptian, which has almost no parallel data (203 training pairs)?

## What was built (kept, off by default)
- `t3a-cli self-train --text <file>:<DIALECT>[:<source>] … --data <model.dat>`: counts word types (lower-cased
  `[a-z0-9']`, 3–20 chars), labels the most frequent `--types` with the engine (dialect fixed), keeps a label when the
  top candidate is a lexicon word and beats the next different candidate by `--margin`; writes
  `latin, arabic, dialect, source, weight` (`align/train.tsv` format + weight). Multithreaded: 60k types in ~10 s.
- `train-rules --pairs a.tsv,b.tsv`: several pair files; optional 5th column = weight. Rows with weight < 1 are
  pseudo-labels: they count for **their own dialect's rows only**, never the pooled `*` rows the other dialects back
  off to. With no pseudo rows the output is byte-identical to before (verified: `rules.tsv` and the release `.dat`).
- Licensing: a release model must be labelled by a release-mode `.dat` (else internal sources leak in via labels).

## What was tried (release mode: approved data only, the RC configuration)
Selection on **dev** splits; test reported once. Baseline = current release model (reproduced byte-identically).

| Variant | kept labels (EGY / MOR) | dev all top-1 / hit@5 | dev LEV | dev MAG | regressions |
|---|---|---|---|---|---|
| baseline | — | 47.2% / 75.4% | 58.6% | 44.8% | 9/11 |
| margin 3, w 0.3, pooled (first try) | 19.4k / 17.5k | test: +1.1 / +2.9 pts; but smoke LEV −3.2, MSA −10.5 | | | 9/11 |
| margin 3, w 0.3, own-dialect | 19.4k / 17.5k | 47.1% / 75.5% | 58.5% | 44.6% | 10/11 (`ahlan`) |
| margin 5, w 0.3 | 8.5k / 8.0k | 47.3% / 75.6% | 58.4% | 45.0% | 9/11 |
| margin 3, w 0.6 / w 0.1 | 19.4k / 17.5k | 47.0% / 47.2% | 58.5% | 44.5% / 44.8% | 10/11 |
| margin 0 (label everything), w 0.3 | 56.9k / 54.4k | 47.2% / 75.5% | 58.3% | 44.8% | 9/11 |

EGY dev has 2 words and EGY test 47 (LLM-annotated), so Egyptian cannot be measured on held-out data. Instead,
2,500 frequent NileChat-EGY words that were **not** pseudo-labels were compared baseline vs. variant (top-1, EGY):
~140–200 change. Many improve (feminine -a → ة: طلبة، مهمة، عالمية، دورة، نظرة، علامة; جسم; أوي; برة; مشاكل; طاقة;
معاهم; الاطفال), but several of the most frequent words regress in **every** variant: `kaman → كماً` (كمان),
`elly → إلي` (اللي, margin ≥ 3 variants), `bet → بة` (بيت), `wgod → وقد` (وجود), `teb2a → طبقة` (تبقى),
`enno → أن` (أنه), `7agat → حاجة` (حاجات).

## Answer
Not shippable as is. Once Egyptian gets its own rows (≥ 50 weighted occurrences of a chunk), they are estimated from
self-labels whose errors are systematic, not random: the EM credits the whole final `an` chunk to tanween words
(`gedan → جدا`), while words like كمان align letter by letter; so the chunk's own distribution tips to ا+tanween. Even
labelling every frequent word with the model's own top-1 (margin 0) does not anchor it.

## Decision
- The RC ships the baseline release rules (no self-training). The tooling stays in the repo, off by default.
- Next attempts (backlog M6): (1) use NileChat on the **LM side** — pseudo-labelled Arabic words as extra
  Egyptian unigram evidence, rules untouched; (2) a real Egyptian dev/test set (golden set, O5) before any
  Egyptian-specific rule is accepted; (3) chunk-level guard: accept a dialect row only if it does not change the
  top-1 of the N most frequent words of that dialect's corpus.
