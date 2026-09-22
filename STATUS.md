# STATUS — living progress log (AGENTS.md §4)

> Update at the end of every session. Newest entries on top within each section.

## Current milestone
**M2 — Data pipeline v1 + binary format** (docs/09-roadmap.md). Next: M3 Engine v1.

## M1 checklist (Complete 2026-09-23)
- [x] Implemented `t3a-paths` (app directories, AppContainer ACLs via S-1-15-2-1/2, safe error logger).
- [x] Implemented `t3a-ui` Windows candidate window (GDI double-buffering, Segoe UI RTL text rendering, high-DPI scaling, non-activating topmost window).
- [x] Implemented `t3a-tip` Windows COM/TSF TextService:
  - `DllMain`, `DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer` (16 Arabic LANGID profiles, 8 categories, HKCU fallback for unelevated dev registration), `DllUnregisterServer`.
  - Panic safe guard (`guard.rs`, `AssertUnwindSafe`, AGENTS.md R1/R2).
  - Physical key translation (`keys.rs`, scan-code remapping under active Arabic layout).
  - TSF sinks (`ITfTextInputProcessorEx`, `ITfThreadMgrEventSink`, `ITfThreadFocusSink`, `ITfTextLayoutSink`, `ITfKeyEventSink`, `ITfCompositionSink`, `ITfDisplayAttributeProvider`, `ITfCompartmentEventSink`, `ITfFunctionProvider`, `ITfFunction`, `ITfFnConfigure`).
  - Edit sessions (`compose.rs`), composition preview, inline commit, candidate popup positioning.
- [x] Spikes S1–S5 written and committed in `docs/spikes/`:
  - `S1-scan-code-translation.md`: verified scan-code to Latin VK translation.
  - `S2-base-layout.md`: verified `hklSubstitute` Latin fallback and password context gating.
  - `S3-global-hotkey.md`: verified activation hotkey and input language switching.
  - `S4-narrator-candidate-provider.md`: verified accessibility candidate categorization.
  - `S5-uiless-fullscreen.md`: verified immersive and fullscreen support categories.
- [x] Machine safety & developer scripts created and tested:
  - `scripts/dev-install.ps1`: builds x86_64 and i686 release DLLs, registers COM, enables AR-Type3arabi (0401) via `InstallLayoutOrTip`, sets AppContainer ACLs.
  - `scripts/dev-uninstall.ps1`: disables layout, unregisters 64-bit and 32-bit DLLs cleanly.
  - `scripts/dev-reset-learning.ps1`: resets user store.
  - `scripts/dev-build.ps1`: builds all crates and architecture binaries.
- [x] Gate G1 passed: TSF integration fully functional across Windows desktop architecture. Release DLL sizes: x86_64 = 404 KB, i686 = 340 KB (budget <= 3 MB).

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
