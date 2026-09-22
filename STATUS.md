# STATUS — living progress log (AGENTS.md §4)

> Update at the end of every session. Newest entries on top within each section.

## Current milestone
**M0 — Bootstrap** (docs/09-roadmap.md). Next: M1 — TSF walking skeleton + spikes S1–S5.

## Baseline (delivered by the architect, 2026-09-22)
- Workspace compiles; `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace` green on Linux (Rust 1.95.0).
- Tests: 44 Rust unit tests (engine 31, data 4, tip 6, hotkey 1, ui 1, …) + 3 Python tests (normalization vectors shared with Rust, registry).
- Seed-only engine (no lexicon) on `data/eval/smoke.tsv`, oracle dialect:
  `cargo run -p t3a-cli -- eval data/eval/smoke.tsv` → **top-1 38.7%, hit@5 81.3%, MRR 0.557** (235 rows).
  This is the floor the lexicon engine (M3) must beat by a wide margin (gate E1 ≥ 75% top-1 on LEV+EGY).
- `cargo run --release -p t3a-cli -- bench` → p50 0.011 ms, p99 0.369 ms per keystroke (seed-only; not allocation-free yet).
- Windows crates contain portable logic only (ids, key router, popup model, hotkey parser, path constants);
  the COM/TSF code is M1. **The Windows CI job has not run yet** — first action of M0.

## M0 checklist
- [ ] Push to a Git host; enable CI; make the `windows` job green (it type-checks `windows =0.62.2` features for the first time — fix feature names if any are missing).
- [ ] `cargo deny check` green.
- [ ] Record baseline numbers above from CI.

## Owner decisions needed (defaults applied meanwhile)
| # | Question | Default applied |
|---|---|---|
| O1 | Project license: open source (which) or proprietary? | All rights reserved (`LicenseRef-Type3arabi-AllRightsReserved`) |
| O2 | Code-signing certificate: buy an OV cloud-signing cert (docs/07 §4) — which CA / budget? | Dev builds use a self-signed test cert |
| O3 | Email Talafha et al. for permission to train on the Jordanian Arabizi corpus (data/sources.toml `talafha-jordanian`) | eval-only |
| O4 | Approve/deny: Tashkeela (license conflict), Maknuune & Wikipedia (CC BY-SA ShareAlike on the data file), NileChat synthetic, LLM synthetic augmentation + budget | all treated as eval-only / not used |
| O5 | Recruit golden-set typists: ≥ 3 per dialect group (docs/04 §8) | M2 starts with LEV (Owner's own dialect) |
| O6 | Default Allah form: `shadda` (اللّه) vs `shadda_fatha` (اللَّه) vs `shadda_dagger` (اللّٰه) vs plain | `shadda` |
| O7 | Default global activation hotkey (Ctrl+Alt+A) and in-IME toggle (Ctrl+Space) OK? | as stated |
| O8 | Native-speaker review of `data/eval/smoke.tsv` and `data/seed/phrases.tsv` | pending (M2 task) |

## Agent decisions (one line each: what, why)
- (none yet)

## Conflicts found between docs
- (none yet)

## Backlog (by milestone)
- M2: `data/eval/bench_keystrokes.tsv` (10k words from golden/FineWeb) replaces smoke as the default bench set.
- M2: `data/fixtures/mini/` tiny approved corpus for CI data-build + eval.
- M3: `Session::vocalizations()` (needs DIAC section).

## Session log
- 2026-09-22 — Architect: repository bootstrapped (docs, ADRs, seed data, skeleton crates, pipeline skeleton, CI).
