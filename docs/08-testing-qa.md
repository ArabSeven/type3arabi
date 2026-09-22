# 08 — Testing & QA

## 1. Test pyramid
| Layer | Where | What |
|---|---|---|
| Unit | `t3a-engine`, `t3a-data` (Linux + Windows) | normalization, alphabet, rule loading, lattice transitions, recombination, pruning, ranking rules, display transforms (mark order!), user model math, journal CRC/parse |
| Property | `proptest` in engine | push/pop symmetry (`push(x); pop()` restores the identical CandidateList), no panic on any input string (ASCII + Unicode), scores finite, list ≤ max, raw Latin always present |
| Fuzz | `cargo fuzz` targets: `fuzz_session` (random key sequences), `fuzz_dataview` (random bytes as data file must never panic/UB), `fuzz_journal` | 30 min per target in nightly CI |
| Golden/eval | `t3a-cli eval` | accuracy gates per milestone (§3) |
| Key-router | `t3a-tip` pure module tests (portable: `KeyRouter` has no Windows types) | every row of `docs/02 §5.2` as a table-driven test |
| Windows integration | `tests/win/` PowerShell + a small Rust harness using UI Automation | drive Notepad/WordPad-like targets: type sequences via `SendInput`, read resulting text via UIA, assert |
| Manual compat | §5 matrix | before each release |

## 2. Invariant tests that must exist from M3
1. Output strings contain no tatweel, presentation forms, bidi controls; marks only in the order of `docs/03 §10.1`.
2. `اللّه` styling applies only to the SACRED set (negative tests: كله، له، الله؟ with punctuation stripped, ظله).
3. Raw Latin candidate equals the raw buffer byte-for-byte (including case).
4. Digits-only tokens yield a number candidate at rank 1.
5. Article joining: `el` + Space + `yom` + Space ⇒ `اليوم ` (no space between ال and يوم).
6. Sticky choice: choose rank 3 for `L`, retype `L` ⇒ it is rank 1.
7. Re-edit: commit then Backspace×2 restores the Latin buffer with the previous choice highlighted.

## 3. Evaluation
- Metrics (per dialect and overall): **top-1**, **hit@3**, **hit@5**, **MRR**, **OOV rate**, **KSR**
  (keystrokes per word incl. navigation: 1 per Latin char + ranks moved + 1 commit), and latency p50/p99.
- Comparison is on base forms (marks stripped, `docs/03 §10.1`); a row may list alternatives `a|b|c`.
- Sets: `data/eval/smoke.tsv` (hand-written, in repo, ~200 rows — sanity, not a benchmark),
  `data/eval/regressions.tsv` (every fixed bug), `golden/*.test.tsv` (primary), external eval-only sets from
  `data/sources.toml` (reported, never tuned on).
- Report: `reports/<date>-<git sha>.md` + `.json`; CI posts the summary table.
- **Gates** are defined per milestone in `docs/09`; release gates equal the targets in `docs/00 §7`.

## 4. Simulated-user test (tunes `λ_usr`, validates learning)
Replay a golden dev set as a "user" who always picks the reference: count keystrokes and navigation across
two passes; pass 2 must reach ≥ 97% top-1 on words seen in pass 1 (learning works) while not reducing
top-1 on unseen words by more than 0.5 points (learning doesn't over-generalize).

## 5. App-compatibility matrix (Tier-1 must pass 100% for release)
Checks per app: switch to Type3arabi; type `mar7aba ya 7abibi, kifak?` + Enter; Tab-edit `3allam`; Esc raw
Latin; Backspace re-edit; Ctrl+Space Latin; popup position correct (incl. multi-monitor mixed DPI); no focus
steal; Narrator reads candidates (Tier-1 Win32 + browsers).

| Tier-1 | Tier-2 |
|---|---|
| Notepad (Win11), Word 365, Outlook (new + classic), Excel cell edit, Chrome, Edge, Firefox, Windows Search / Start, File Explorer rename, WhatsApp Desktop, Microsoft Teams, VS Code, Windows Terminal, Settings search box (immersive) | Telegram Desktop, Discord, Slack, LibreOffice Writer, Notion, Obsidian, OneNote, Zoom chat, Photoshop text tool, a full-screen DirectX game chat (UILess), Sticky Notes, Mail/Calendar (immersive), elevated Notepad (admin), Win32 password box (expect Latin per §7 of `docs/02`) |
Results table lives in `docs/app-compat.md` (app, version, OS, date, pass/fail per check, notes → `docs/app-quirks.md`).

## 6. Soak & stability
8-hour scripted typing (AutoHotkey or the Rust harness) rotating across Notepad, Chrome, Word, Teams, VS Code:
0 crashes in host processes, private bytes flat (±1 MB), no handle growth (`Handles` counter), no GDI object
growth.

## 7. Manual exploratory charter (each release, 60 min)
IME switching mid-word; typing during app hangs; fast typing bursts (150 wpm); remote desktop session;
multi-monitor with different DPI; dark ↔ light switch while popup open; sleep/resume; user switch; high
contrast; RTL paragraphs in Word with mixed English.
