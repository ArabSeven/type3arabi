# 10 — Risk register

| # | Risk | Likelihood | Impact | Mitigation | Owner of mitigation |
|---|---|---|---|---|---|
| K1 | A TIP bug crashes host apps (Word/Chrome) | Med | Critical | R1/R2 panic guard + safe passthrough; no unsafe outside Windows crates; fuzzing; soak tests; per-app exclusion list | Agent (M1, M8) |
| K2 | Base layout under Arabic language produces Arabic chars (password fields, IME-disabled boxes) | High | Med | Eat & translate all printable keys (§02 5.1); Spike S2 `hklSubstitute`; documented Win+Space fallback | Agent (M1) |
| K3 | Global hotkey cannot switch another app's input method reliably | Med | Low | Spike S3 with 3 mechanisms; Windows' own language hotkeys; Win+Space always works | Agent (M1, M7) |
| K4 | Data licenses block the best parallel corpora | High | High | Golden-set protocol (owned CC0 data) starts in M2; email authors (Talafha et al.); FineWeb-2 (ODC-By) covers lexicon/LM; seed rules + EM work with modest data | Owner + Agent |
| K5 | Accuracy below Yamli for some dialects (GLF/IRQ/MAG sparse) | Med | High | Dialect-mixture LM, region priors, synthetic augmentation (Owner-approved), user learning (sticky choices), continuous regression set | Agent (M6) |
| K6 | Latency spikes in slow hosts / cold page faults on data file | Med | Med | Budgets + ETW; prefetch hot sections (`PrefetchVirtualMemory` for TRIE top levels on activation, async) | Agent (M4) |
| K7 | Code-signing identity unavailable for Jordan via Azure Artifact Signing | Certain | Med | OV certificate from a worldwide CA with cloud signing (docs/07 §4) | Owner (buy) |
| K8 | AV/EDR flags an unsigned-or-new DLL loaded into every process | Med | High | Sign everything; stable publisher; submit to Microsoft Defender WDSI for analysis before launch; no suspicious APIs (no hooks, no injection) | Owner + Agent |
| K9 | `windows` crate API churn (0.62 → 0.100) | Med | Low | Pinned `=0.62.2`; upgrade only by ADR with a full compat run | Agent |
| K10 | Multi-process user-store corruption | Low | Med | Fixed-size CRC'd journal records, atomic snapshot replace, named mutex for compaction, readers tolerate truncation | Agent (M4) |
| K11 | Offensive/embarrassing completions | Med | Med | Completions only from `NO_COMPLETE`-filtered lexicon; exact matches unaffected; Owner curates `no_complete.tsv` | Owner + Agent |
| K12 | Windows 10 end-of-support reduces value of Win10 testing | Certain | Low | Support Win10 22H2 as long as it costs little; Win11 is primary | Owner |
| K13 | Legal: using "Maren"/"Yamli" names/branding | Low | Med | Never use their names/marks in product UI or marketing; the research doc references them factually only | Owner |
