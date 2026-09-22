# 06 — Performance budget (hard limits)

Reference machine: **Intel Core i5-8250U (2018, 4C/8T, 1.6 GHz base), 8 GB RAM, SSD, Windows 11**, balanced
power plan. Also run on one ARM64 device (Snapdragon X or 8cx) before release. A budget miss is a bug.

| # | Metric | Budget | How measured |
|---|---|---|---|
| P1 | Engine time per keystroke (push/pop), p50 / p99 | ≤ 0.8 ms / ≤ 3 ms | `t3a-cli bench [file]` replays every word char by char, release build; default set `data/eval/smoke.tsv` until M2 adds `data/eval/bench_keystrokes.tsv` (10k words) |
| P2 | Keystroke → popup painted, p99 | ≤ 16 ms | ETW: TIP emits `T3A/KeyDown` and `T3A/Painted` TraceLogging events (no payload text); WPA profile `tools/perf/latency.wprp` (created in M4) |
| P3 | TIP `ActivateEx` wall time | ≤ 30 ms cold, ≤ 5 ms warm | TraceLogging start/stop |
| P4 | First popup show in a process (includes D2D/DWrite init) | ≤ 40 ms | TraceLogging |
| P5 | Private bytes added to a host process after 1 000 words | ≤ 6 MB | VMMap / `GetProcessMemoryInfo` in the compat harness |
| P6 | Shared data file mapped | ≤ 60 MB file; resident only as touched | build gate + RAMMap spot check |
| P7 | `t3a_tip.dll` size (x64, release, stripped) | ≤ 3 MB | CI artifact size check |
| P8 | Idle CPU (TIP loaded, not typing) | 0 wakeups/sec (no timers) | `powercfg /energy` style check or ETW context switches |
| P9 | `t3a-hotkey.exe` idle | ≤ 2 MB private, 0 CPU | Task Manager / ETW |
| P10 | Heap allocations per keystroke after warm-up (engine) | 0 | `t3a-cli bench --check-alloc` (counting global allocator) |
| P11 | Commit → user-journal write | off-thread, never on UI thread | code review + TraceLogging |
| P12 | Data build time (full) | ≤ 6 h on a 16-core box | pipeline log |

Engineering rules that make these achievable:
1. Everything on the keystroke path is O(beam × rules); no string formatting, no hashing of strings except
   the ≤ 32-byte user-key lookup, no locks.
2. `DataView` is zero-copy (`bytemuck` casts over the mmap); section offsets resolved once at load.
3. Popup redraw only when the model changed; text layouts cached per candidate string for the life of
   the list; D2D/DWrite factories created once per process (warm-up message after activation).
4. `release` profile: `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `panic = "unwind"`, `debug = 1`
   (line tables for symbolication; PDBs kept privately, never shipped).
5. CI perf gate: `t3a-cli bench` p99 regression > 20% vs `main` fails the build (Linux runner, relative).
