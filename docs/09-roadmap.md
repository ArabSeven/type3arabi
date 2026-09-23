# 09 — Roadmap, milestones, gates

Work strictly in order. A milestone is done when **every** acceptance criterion has evidence in
`STATUS.md` (AGENTS.md §4). Effort = focused agent-days, indicative only.

---

## M0 — Bootstrap (≈1 day) · Docs: AGENTS.md, 01, 07
Scope: make the delivered skeleton fully green; CI; STATUS discipline.
Acceptance:
- [ ] `cargo test --workspace`, `clippy -D warnings`, `fmt --check`, `cargo deny check` green on Linux.
- [ ] Windows CI job builds `t3a-tip` (stub) for x64/i686/aarch64.
- [ ] `cargo run -p t3a-cli -- repl` works in seed-only mode; `eval data/eval/smoke.tsv` prints a report.
- [ ] `STATUS.md` updated with baseline smoke numbers.

## M1 — TSF walking skeleton + spikes (≈6–8 days) · Docs: 02, 05 §2–3
Scope: a real TIP DLL: registration of the single ar-SA profile (docs/02 §2.1), activation, key sinks, composition with the **seed-only
engine** (OOV path), a plain popup (GDI allowed at this stage), Space/Enter/Esc/Backspace/arrows, mode toggle,
tray mode icon, safe-passthrough guard. Spikes S1–S5 (`docs/02 §17`).
Acceptance:
- [ ] Appears as **AR – Type3arabi** after `regsvr32` + `InstallLayoutOrTip` on Win10 22H2 and Win11.
- [ ] Types `7abibi` → حبيبي (seed OOV) in Notepad, Word, Chrome, Edge, Firefox, Teams, VS Code, Windows
      Terminal, Start search, Settings search. Screenshots in `docs/evidence/M1/`.
- [ ] Panic injected via a debug hotkey ⇒ host survives, TIP passes keys through.
- [ ] Spike write-ups S1–S5 committed with decisions applied to docs (via ADR if a locked decision moves).

### Gate G1 (end of M1) — is TSF viable?
Pass if the TIP works in ≥ 12 of the 14 Tier-1 apps, with a plausible fix path for the rest.
**Fail ⇒ stop and escalate to the Owner** with a proposal to build the overlay front-end (`docs/11`) reusing
engine + UI. (Expected outcome: pass — every major IME, incl. Maren and Google's, used TSF/IMM.)

## M2 — Data pipeline v1 + binary format (≈6–8 days) · Docs: 04, 12, `data/sources.toml`
Scope: `t3a-data` writer/reader; pipeline stages 1–5 on FineWeb-2; seed rules compiled (no EM yet);
`build-data`; eval harness on real data; golden-set collection kicked off (Owner recruits typists).
Acceptance:
- [ ] `type3arabi.dat` built from approved sources only (manifest proves it); size ≤ 60 MB.
- [ ] `t3a-cli inspect <word>` shows lp×6, flags, bigrams, vocalizations.
- [ ] Fuzzed `DataView` (30 min) — no panics.
- [ ] Normalization vectors identical in Python and Rust.

## M3 — Engine v1 (≈8–10 days) · Docs: 03 (all), 06
Scope: incremental lattice over the trie, completions, OOV with char-LM, phrases, numbers, laughter, article
joining, dialect mixture + online posterior, context bigram, ranking, display transforms, vocalizations API,
vowel-derived harakat, user model (in-memory + journal/snapshot format, portable implementation).
Acceptance:
- [ ] All invariant tests of `docs/08 §2` pass; property + fuzz tests in place.
- [ ] P1 and P10 budgets met (`bench` output in STATUS).
- [ ] Eval gate **E1**: golden-dev (or smoke+external eval if golden not ready) LEV+EGY top-1 ≥ 75%, hit@5 ≥ 90%.

## M4 — TIP integration & full popup UX (≈10–12 days) · Docs: 02, 05 §1–3, §5–6, 06
Scope: engine inside the TIP (mmap data, config load/reload, user store IO + writer thread, tailing,
compaction), Direct2D/DirectWrite popup per spec, DPI/theme/HC, mouse, paging, re-edit, surrounding context,
input-scope gating, punctuation mapping, Latin mode, UILess UIElement, light-dismiss events.
Acceptance:
- [ ] Every row of `docs/02 §5.2` verified in Notepad + Chrome + Word (checklist in `docs/evidence/M4/`).
- [ ] P2–P5, P8, P11 budgets met with ETW traces saved.
- [ ] Learning shared across apps within one word (type/choose in Word, see sticky choice in Chrome).
- [ ] AppContainer app (Settings search / Mail) works read-only without errors.

## M5 — Tashkeel editor (≈4–5 days) · Docs: 05 §4–5, 03 §10
Acceptance:
- [ ] Editor per `docs/05 §4` (keyboard + mouse), quick picks incl. "from your typing".
- [ ] `3allam` → Ctrl+Enter → عَلَّم; `allah` default → اللّه; `shukran` → شكراً (tests + screenshots).
- [ ] Mark-order invariant holds for every editor path (property test on random edit sequences).

## M6 — Accuracy push (≈8–12 days) · Docs: 04 §6–9, 03 §7
Scope: EM rule training on approved parallel data (golden dev + any sources the Owner has cleared),
tuning, self-training (optional), synthetic augmentation (if approved), dialect prior table, error analysis
loop with `explain`.
Acceptance — Eval gate **E2** (release targets, `docs/00 §7`):
- [ ] LEV+EGY golden-test top-1 ≥ 85%, hit@5 ≥ 96%; GLF/IRQ/MAG top-1 ≥ 75%.
- [ ] No per-dialect regression vs M3; P1 still met.

## M7 — Settings, hotkey companion, installer, signing (≈8–10 days) · Docs: 05 §7–8, 02 §11, 07
Acceptance:
- [ ] Settings app: all pages of `docs/05 §7` functional; config round-trips; wipe works across running apps.
- [ ] `t3a-hotkey` per S3 decision; conflict reporting.
- [ ] Signed MSI installs/upgrades/uninstalls cleanly on Win10 22H2 x64, Win11 x64, Win11 ARM64; x86 apps work.
- [ ] `NOTICE.md` generated (cargo-about) + data attributions.

## M8 — Hardening & beta (≈8–10 days) · Docs: 08
Acceptance:
- [ ] App-compat matrix: Tier-1 100%, Tier-2 ≥ 90% (`docs/app-compat.md`).
- [ ] UIA/Narrator per `docs/02 §9`; high contrast; mixed-DPI multi-monitor.
- [ ] 8-h soak: 0 crashes, flat memory/handles/GDI.
- [ ] GDI rendering fallback when D2D is unavailable.
- [ ] Beta build distributed to the golden-set typists; feedback triaged into `regressions.tsv`.

## v1.0 release = M8 + release checklist (`docs/07 §6`)
- Note: No source with status `internal` in the release data build: each must be cleared to `approved` or removed with data rebuilt under `--mode release`.

## M9 — Post-1.0 (ordered backlog, each needs its own mini-spec before work)
1. Reconversion: select Arabic text → hotkey → candidates (`ITfFnReconversion`).
2. Clitic lattice for unseen prefix/suffix combinations (`docs/03 §6.5`).
3. Code-switch detection (English/French words default to Latin when strongly indicated).
4. Tiny neural reranker (pure Rust, int8, ≤ 1 ms p99) — only if it adds ≥ 2 points top-1.
5. Personal phrase shortcuts (user-defined abbreviations).
6. Dead-key support in the Latin translation layer.
