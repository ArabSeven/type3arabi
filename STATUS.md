# STATUS — living progress log (AGENTS.md §4)

> Update at the end of every session. Newest entries on top within each section.

## Current milestone
**M1 — TSF walking skeleton + spikes S1–S5** (docs/09-roadmap.md). Next: Gate G1 & M2.

## Baseline (delivered by the architect, 2026-09-22)
- Workspace compiles; `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace` green on Linux & Windows (Rust 1.95.0, targets x86_64 and i686).
- Tests: 44 Rust unit tests + 4 Python tests passing.
- Seed-only engine (no lexicon) on `data/eval/smoke.tsv`, oracle dialect:
  `cargo run -p t3a-cli -- eval data/eval/smoke.tsv` → **top-1 38.7%, hit@5 81.3%, MRR 0.557** (235 rows).
- `cargo run --release -p t3a-cli -- bench` → p50 0.009 ms, p99 0.315 ms per keystroke (budget P1: p50 ≤ 0.8, p99 ≤ 3.0 ms).
- `cargo deny check` passing.

## M0 checklist
- [x] First real Windows build for both x86_64-pc-windows-msvc and i686-pc-windows-msvc.
- [x] `cargo deny check` green.
- [x] Record baseline numbers in STATUS.md.

## Owner decisions recorded
- **2026-09-22 (O3, O4)**: Train on all publicly accessible data now under new status `internal`; clear licenses before public release.
  Sources set to `internal`: parallel data (`talafha-jordanian`, `arbml-arabizi`, `akhanafer-levantine`, `elkababi-darija`, `atlasia-atam`, `doda`, `arabizikit-corpus`, `nilechat-arabizi-egy`), monolingual Arabizi (`arabizi-dataset-v2`), lexicons/text (`maknuune`, `tashkeela` stats only, `wikipedia-ar`).
  Parallel data split 80/10/10 (train/dev/test); held-out test splits never used for training or tuning. Pipeline supports `--mode internal` and `--mode release`.

## Owner decisions needed (defaults applied meanwhile)
| # | Question | Default applied |
|---|---|---|
| O1 | Project license: open source (which) or proprietary? | All rights reserved (`LicenseRef-Type3arabi-AllRightsReserved`) |
| O2 | Code-signing certificate: buy an OV cloud-signing cert (docs/07 §4) — which CA / budget? | Dev builds use a self-signed test cert |
| O5 | Recruit golden-set typists: ≥ 3 per dialect group (docs/04 §8) | M2 starts with LEV (Owner's own dialect) |
| O6 | Default Allah form: `shadda` (اللّه) vs `shadda_fatha` (اللَّه) vs `shadda_dagger` (اللّٰه) vs plain | `shadda` |
| O7 | Default global activation hotkey (Ctrl+Alt+A) and in-IME toggle (Ctrl+Space) OK? | as stated |
| O8 | Native-speaker review of `data/eval/smoke.tsv` and `data/seed/phrases.tsv` | pending (M2 task) |
| O9 | License clearance before public release: contacts/actions for each `internal` source (`talafha-jordanian`, `arbml-arabizi`, `akhanafer-levantine`, `elkababi-darija`, `atlasia-atam`, `doda`, `arabizikit-corpus`, `nilechat-arabizi-egy`, `arabizi-dataset-v2`, `maknuune`, `tashkeela`, `wikipedia-ar`) | Local test builds are internal only; release builds run `--mode release` until cleared |

## Agent decisions (one line each: what, why)
- D0: Applied Owner decision (2026-09-22) — added `internal` source status, 80/10/10 deterministic split, pipeline modes, and citations in NOTICE.md.

## Conflicts found between docs
- (none yet)

## Backlog (by milestone)
- M2: `data/eval/bench_keystrokes.tsv` (10k words from golden/FineWeb) replaces smoke as the default bench set.
- M2: `data/fixtures/mini/` tiny approved corpus for CI data-build + eval.
- M3: `Session::vocalizations()` (needs DIAC section).

## Session log
- 2026-09-22 — Architect: repository bootstrapped (docs, ADRs, seed data, skeleton crates, pipeline skeleton, CI).
