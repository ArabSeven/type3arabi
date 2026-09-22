# AGENTS.md — Type3arabi master governance

> This is the single source of authority for every agent (human or AI) working in this repository.
> If any other file disagrees with this one, this file wins. If `docs/` disagree with each other,
> the file with the lower number wins, and you record the conflict in `STATUS.md → Conflicts`.

---

## 0. What we are building (one paragraph)

**Type3arabi** is a Windows input method that lets people type Arabic by writing *Arabizi*
(Arabic chat alphabet: `mar7aba`, `3ala`, `2albi`, `sba7 el 5er`) on any Latin keyboard. It installs
as a real Windows keyboard under the Arabic language (**AR – Type3arabi**), works in every app
(Word, Chrome, WhatsApp Desktop, Teams, Notepad, Start search…), shows a Yamli/Maren-style RTL candidate
popup with the most likely Arabic word pre-selected, understands dialects (Levantine, Egyptian, Gulf,
Iraqi, Maghrebi, MSA), applies critical diacritics automatically (e.g. **اللّه**, **شكراً**), and lets the
user add any diacritic to any letter from inside the same popup. It is 100% offline, private,
and idle-cost-free.

## 1. Owner, roles, and how decisions work

| Role | Who | Authority |
|---|---|---|
| **Owner** | Hassan Obaida, the human who commissioned this repo | Final say on product, licensing, money (certificates, data purchases), and any change to a *Locked decision*. |
| **Architect docs** | `docs/` + `docs/adr/` | Encode the locked decisions. Agents implement them; they do not re-litigate them. |
| **Implementing agent** | You | Build exactly what the docs specify, milestone by milestone, and keep `STATUS.md` truthful. |

**Locked decisions** (changing any of these requires a new ADR in `docs/adr/` with status `Proposed`
and explicit Owner approval recorded in `STATUS.md → Owner decisions`):

1. Primary integration = **TSF Text Input Processor (TIP)** in-proc COM DLL, registered under Arabic LANGIDs. (ADR-0001)
2. Implementation language = **Rust** (stable toolchain pinned in `rust-toolchain.toml`), `windows` crate **=0.62.2**. (ADR-0002)
3. Engine = **lexicon-constrained incremental beam search** with a log-linear model (transliteration model + dialect-mixture word LM + bigram context + user model), plus an unconstrained OOV path. No neural network in v1 runtime. (ADR-0003)
4. All runtime data = one **memory-mapped, read-only, zero-copy binary** `type3arabi.dat` shared by all processes. (ADR-0004)
5. **100% offline at runtime.** No network code in any shipped binary except the Settings app's optional "check for updates" (off by default, M7+). (ADR-0005)
6. Fallback "Overlay mode" (global keyboard hook) is **not built** unless Gate G1 fails (see `docs/09-roadmap.md`). (ADR-0006)
7. Settings app = **Tauri 2 (WebView2)**; it is the only component allowed to have a heavy UI stack. (ADR-0007)
8. Installer = **WiX Toolset MSI**, per-machine, x86 + x64 + ARM64 DLLs, registration through TSF APIs only. (ADR-0008)

When the docs are silent, follow the **Default rule**: choose the simplest option that keeps every
budget in `docs/06-performance-budget.md`, write it down in `STATUS.md → Agent decisions` (one line:
what, why), and continue. Never stop work to ask a question that has a documented default.

## 2. Read order (do this before your first change, every session)

1. `AGENTS.md` (this file) — rules.
2. `STATUS.md` — where the project is, current milestone, open questions.
3. `docs/09-roadmap.md` — the current milestone's scope and acceptance criteria.
4. The docs referenced by that milestone (each milestone lists them).
5. `docs/glossary.md` when a term is unclear.

Doc map:

| File | Contents |
|---|---|
| `docs/00-vision.md` | Product brief, principles, non-goals, target users |
| `docs/01-architecture.md` | Components, processes, data flow, repo layout, dependency policy |
| `docs/02-tsf-integration.md` | Exact TSF/COM design: interfaces, registration, key handling, composition, edge cases |
| `docs/03-engine-algorithm.md` | The prediction engine, precisely: model, search, scoring, dialects, learning, diacritics |
| `docs/04-data-pipeline.md` | Sources, licenses, build stages, training (EM), tuning, outputs |
| `docs/05-ux-spec.md` | Candidate popup, tashkeel editor, keys, mouse, RTL, themes, a11y, settings |
| `docs/06-performance-budget.md` | Hard latency / memory / size budgets and how they are measured |
| `docs/07-build-release.md` | Toolchain, CI, signing, installer, versioning, release checklist |
| `docs/08-testing-qa.md` | Test pyramid, eval gates, app-compat matrix, fuzzing, manual scripts |
| `docs/09-roadmap.md` | Milestones M0–M9 with scope, acceptance criteria, gates |
| `docs/10-risks.md` | Risk register with mitigations |
| `docs/11-overlay-fallback.md` | Plan B spec (only if Gate G1 fails) |
| `docs/12-binary-format.md` | `type3arabi.dat` byte layout |
| `docs/13-config-schema.md` | `config.toml` schema and defaults |
| `docs/research/prior-art.md` | Yamli, Maren, Google IME, ArabiziKit, academic results — what we take from each |
| `docs/adr/` | Architecture Decision Records |
| `data/sources.toml` | Every dataset: URL, license, allowed role, status |

## 3. Hard rules (violating any of these is a release blocker)

### 3.1 Host-process safety (the TIP runs inside *other people's* apps)
- **R1. No panic may cross an FFI/COM boundary.** Every exported function and every COM method body
  is wrapped in `t3a_tip::guard(|| …)` which uses `std::panic::catch_unwind` and returns `E_FAIL` /
  `S_OK`-passthrough. Workspace profile uses `panic = "unwind"` — **never `abort`** (abort kills Word).
- **R2. After any caught panic the TIP enters *safe passthrough* for that process**: it eats no keys, shows
  no UI, and records one line (no user text) in the error log if writable.
- **R3. No blocking I/O, no allocation storms, no locks held across callbacks on the keystroke path.**
  The keystroke path is: `OnTestKeyDown → OnKeyDown → edit session → engine.push() → UI paint`.
  Allowed on that path: arithmetic, reads from the mmapped data, bounded `Vec` reuse (pre-allocated).
- **R4. No threads at activation.** At most one lazily-spawned background thread per process
  (the user-store writer), created on first commit that needs persistence.
- **R5. No global hooks, no DLL injection, no SetWindowsHookEx, no code patching** in the TIP.
- **R6. No registry writes except through TSF/COM registration APIs** in `DllRegisterServer`
  (`ITfInputProcessorProfileMgr::RegisterProfile`, `ITfCategoryMgr::RegisterCategory`) and standard
  COM `InprocServer32` keys. Never write the user's default-input-method registry directly.
- **R7. Respect app-container rules.** Never try to write files from an AppContainer process; never
  try to reach the network from the TIP. Detect AppContainer via token and degrade (no learning).

### 3.2 Privacy
- **R8. Never log, persist, or transmit typed text** except the user-learning store described in
  `docs/03 §9`, which stores (Latin key → chosen Arabic word) counts locally and can be wiped from Settings.
- **R9. Disable the engine** (pure Latin passthrough-by-commit, no learning) for input scopes
  `IS_PASSWORD`, `IS_PIN`… (full list in `docs/02 §7`). Disable **learning only** for `IS_PRIVATE`
  (incognito/InPrivate) and secure desktop (`TF_TMAE_SECUREMODE`).
- **R10. No telemetry.** No analytics, no crash upload. Ever. (Owner may revisit via ADR only.)

### 3.3 Dependencies & licensing
- **R11. Allowed licenses for anything linked into a shipped binary:** MIT, Apache-2.0, BSD-2/3,
  ISC, Zlib, Unicode-3.0, Unlicense, CC0, MPL-2.0 (file-level). **Forbidden:** GPL, LGPL, AGPL, SSPL,
  any "non-commercial" or "research only" license. Enforced by `cargo deny check` in CI (`deny.toml`).
- **R12. `t3a-engine` and `t3a-data` must have zero platform dependencies** and build/test on Linux.
  Allowed deps there: `bytemuck`, `memmap2` (loader only). Anything else needs an ADR.
- **R13. Every new dependency** gets one line in `docs/01-architecture.md §8 Dependency ledger`
  (crate, version, license, why, which binary). No line → PR rejected.
- **R14. Data:** a dataset may enter a *shipped* artifact only if `data/sources.toml` marks it
  `status = "approved"` and `role` includes that use. `eval-only` data must never influence shipped
  weights (not even tuning — tuning uses `dev` splits of approved data). Unknown license ⇒ `blocked`.
  *Owner amendment (2026-09-22):* Added status `internal`: publicly downloadable data whose license is
  unstated or restrictive; usable for every listed role in local and internal builds; must be cleared (→ `approved`)
  or removed, with the data rebuilt, before any public release. The pipeline supports `--mode internal` (approved + internal)
  and `--mode release` (approved only). Every data file built with internal sources carries `"distribution": "internal-only"`
  and the list of internal source ids in its `META` section.

### 3.4 Code quality
- **R15.** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` pass before every commit. Windows-only crates are checked in the Windows CI job.
- **R16.** `unsafe` only in `t3a-tip`, `t3a-ui`, `t3a-hotkey`, `t3a-paths`, and the mmap loader in `t3a-data`.
  Every `unsafe` block carries a `// SAFETY:` comment.
- **R17.** Public items in `t3a-engine` are documented; every algorithmic constant lives in
  `EngineParams` (loaded from the data file's `PARM` section) — no magic numbers in search code.
- **R18.** Every bug fix adds a regression test (unit test, eval row in `data/eval/regressions.tsv`,
  or app-compat script entry).
- **R19.** Arabic strings in code/tests are written as literal Arabic in UTF-8 **plus** a comment with
  the code points when marks are involved (marks are invisible in many editors):
  `"اللّه" // U+0627 U+0644 U+0644 U+0651 U+0647`.

## 4. Working protocol

1. **One milestone at a time.** Only work on tasks inside the current milestone (`STATUS.md`).
   Items discovered for later go to `STATUS.md → Backlog` with the milestone they belong to.
2. **Small vertical commits.** Conventional commits: `feat(engine): …`, `fix(tip): …`, `docs: …`,
   `data: …`, `test: …`, `build: …`. One logical change per commit.
3. **Update `STATUS.md` at the end of every session**: done, in progress, next, blockers,
   measured numbers (accuracy, latency) with the command that produced them.
4. **Acceptance = evidence.** A milestone is done only when every acceptance criterion in
   `docs/09-roadmap.md` has evidence recorded in `STATUS.md` (command output, eval report path,
   screenshot path under `docs/evidence/`).
5. **Spikes are time-boxed** (budget stated in the roadmap). A spike ends with a short write-up in
   `docs/spikes/Sx-name.md`: question, what was tried, answer, decision taken.
6. **Questions for the Owner** go to `STATUS.md → Owner questions` with the default you are
   applying meanwhile. Continue with the default.
7. **Never edit locked sections of docs silently.** To propose a change, add an ADR (`Proposed`) and
   an Owner question.

## 5. Definition of Done (per task)

- [ ] Behavior matches the spec section it implements (cite it in the commit body: `Spec: docs/03 §5.2`).
- [ ] Tests added/updated; `cargo test --workspace` green; Windows job green if Windows code changed.
- [ ] Budgets in `docs/06` still met (bench/eval numbers pasted into `STATUS.md` if the task touches the hot path or data).
- [ ] No new dependency without ledger entry; `cargo deny check` green.
- [ ] Docs updated if behavior visible to users or other components changed.

## 6. Repository layout (authoritative)

```
AGENTS.md                 ← this file
STATUS.md                 ← living progress log (agents update)
README.md                 ← human overview
Cargo.toml                ← workspace
rust-toolchain.toml
deny.toml                 ← license/advisory policy
config/config.default.toml
crates/
  t3a-engine/             ← pure Rust prediction engine (no Windows deps)
  t3a-data/               ← binary format: writer (builder) + zero-copy reader
  t3a-cli/                ← dev REPL, eval harness, benchmark, data builder CLI
  t3a-tip/                ← TSF TIP cdylib (Windows only)
  t3a-ui/                 ← candidate popup + tashkeel editor (Windows only, Direct2D/DirectWrite)
  t3a-hotkey/             ← optional tiny companion for the global activation hotkey (Windows only)
  t3a-paths/              ← shared Windows helpers: known folders, user dir + AppContainer ACL, error log
apps/settings/            ← Tauri 2 settings app (created in M7)
installer/                ← WiX MSI project (created in M7)
tools/pipeline/           ← Python (uv) data pipeline: fetch, count, align, tune
data/
  sources.toml            ← dataset registry + licenses
  seed/                   ← hand-authored rules and tables (source of truth, reviewed by humans)
  eval/                   ← evaluation sets (smoke, regressions); large sets are fetched, not committed
docs/                     ← architecture & specs (see §2)
.github/workflows/        ← CI
```

## 7. Commands

```bash
cargo test --workspace                          # all portable tests (Linux/Windows)
cargo run -p t3a-cli -- repl --dialect LEV      # type Arabizi, see ranked candidates (seed-only until M3)
cargo run -p t3a-cli -- eval data/eval/smoke.tsv --misses   # accuracy report (top-1, hit@5, MRR per dialect)
cargo run -p t3a-cli -- explain 3allam --dialect MSA       # alignment + harakat-from-vowels for each candidate
cargo run -p t3a-cli -- bench                   # per-keystroke latency percentiles
cargo run -p t3a-cli -- build-data --out target/type3arabi.dat   # (M2+) compile pipeline outputs
cargo deny check
cd tools/pipeline && uv run pytest && uv run t3ap sources        # pipeline tests + dataset registry
# Local note: if rustup cannot download the pinned toolchain, RUSTUP_TOOLCHAIN=stable uses your installed one.
# Windows only:
cargo build -p t3a-tip --release --target x86_64-pc-windows-msvc
regsvr32 target\x86_64-pc-windows-msvc\release\t3a_tip.dll      # dev registration (admin)
```

## 8. Glossary (minimum)

- **Arabizi** — Arabic written with Latin letters + digits (`3`=ع, `7`=ح, `2`=ء/ق, `5`=خ, `9`=ص/ق …).
- **TIP** — TSF Text Input Processor: the COM DLL Windows loads into apps to provide an input method.
- **Composition** — the underlined, not-yet-committed text inside the app while the user types a word.
- **Candidate** — one Arabic option in the popup; **default candidate** = row 1, committed by Space.
- **Latin buffer** — the Arabizi characters typed for the current word.
- **Tashkeel / harakat** — Arabic diacritics: fatha َ, damma ُ, kasra ِ, sukun ْ, shadda ّ, tanween ً ٌ ٍ, dagger alif ٰ.
- **Dialect group** — one of `MSA, LEV, EGY, GLF, IRQ, MAG` (see `docs/03 §7`).
Full glossary: `docs/glossary.md`.
