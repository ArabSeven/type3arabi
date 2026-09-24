# 01 — Architecture

## 1. Big picture

```
                     ┌──────────────────────────── any app process (Word, Chrome, Notepad …) ───────────────────────────┐
 keyboard ──► Windows│  TSF (msctf) ──► t3a_tip.dll  ──►  t3a-engine (Rust, pure)  ──► candidates                         │
                     │                   │  key sinks        │  reads (zero-copy)                                            │
                     │                   │  composition      ▼                                                               │
                     │                   │           type3arabi.dat  ◄── memory-mapped read-only, pages shared by ALL procs │
                     │                   ▼                                                                                   │
                     │             t3a-ui (owned popup window, Direct2D/DirectWrite, RTL)                                    │
                     │                   │ user choices                                                                      │
                     │                   ▼                                                                                   │
                     │            user store (append-only journal, %LOCALAPPDATA%\Type3arabi\user\)                          │
                     └───────────────────────────────────────────────────────────────────────────────────────────────────────┘

 Optional, separate processes (never required for typing):
   t3a-hotkey.exe  — ~1 MB idle companion: global "activate Type3arabi" hotkey (M7, default ON, can be disabled)
   Type3arabi Settings.exe (Tauri 2) — runs only when the user opens it; edits config.toml, user dictionary
```

There is **no resident service**. When the user is not using Type3arabi, nothing of ours runs.
When Type3arabi is the active keyboard, TSF loads `t3a_tip.dll` into the focused app; the heavy
data is a shared file mapping, so ten apps cost roughly one copy of the data in RAM.

## 2. Components

| Component | Kind | Platform | Responsibility |
|---|---|---|---|
| `t3a-engine` | Rust lib | portable | Normalization, candidate generation, scoring, dialect posterior, display transforms (diacritics), user-model scoring. Pure functions over `&DataView` + `&UserModel`. |
| `t3a-data` | Rust lib | portable | `type3arabi.dat` format: writer (used by builder) and zero-copy reader (`DataView<'a>` over `&'a [u8]`); mmap loader behind `feature = "mmap"`. |
| `t3a-cli` | Rust bin | portable | `repl`, `eval`, `bench`, `build-data`, `inspect` (dump a word's entry), `explain` (show the scored lattice for an input). |
| `t3a-tip` | Rust cdylib | Windows | COM class factory, TSF TIP, key handling, compositions, display attributes, mode compartment + tray mode icon, UIElement for UILess mode, config/user-store IO, safe-passthrough guard. |
| `t3a-ui` | Rust lib | Windows | Candidate popup and tashkeel editor: window class, layout, Direct2D/DirectWrite rendering, hit-testing, DPI, theme, UIA provider. Stateless w.r.t. engine; receives a `PopupModel`. |
| `t3a-paths` | Rust lib | Windows | Install/user paths, user-dir creation with the AppContainer read ACL, error log (no user text). |
| `t3a-hotkey` | Rust bin | Windows | `RegisterHotKey` → switch foreground window's input language to Arabic/Type3arabi (see `docs/02 §11`). |
| `apps/settings` | Tauri 2 | Windows | Settings UI, user dictionary editor, learning wipe, hotkey config, about/licenses. |
| `installer/` | WiX MSI | Windows | Per-machine install, register x86/x64/ARM64 DLLs, `InstallLayoutOrTip`, ACLs for AppContainer read access, uninstall cleanup. |
| `tools/pipeline` | Python (uv) | any | Fetch approved datasets, count, normalize, align (EM), tune weights, emit TSVs for `build-data`. |

## 3. Process & thread model (TIP)
- TSF calls the TIP on the **host app's UI thread** (STA). All TIP state lives in a per-thread
  `TextService` COM object; nothing global except: the `Arc<DataFile>` (process-wide `OnceLock`),
  the config snapshot (`ArcSwap`-like manual `RwLock<Arc<Config>>`, refreshed on activation and on
  composition start if the file's mtime changed), and the user-store writer channel.
- The candidate popup window is created on that same UI thread, owned by the context window.
- Engine calls are **synchronous** and must meet `docs/06` budgets; no async/worker offload.
- One lazily-spawned background thread per process: the **user-store writer** (appends journal
  records, compacts when needed). It never touches COM.

## 4. Data flow per keystroke (Arabic mode)
1. `ITfKeyEventSink::OnTestKeyDown(vk, lParam)` → translate the scan code through the configured **Latin
   layout** into `Key` + `Mods` (`docs/02 §6`) → `keyrouter::classify()` (pure) decides *Eat* / *Pass*.
2. `OnKeyDown` → the same translation and classification → execute the `Action`
   (`AppendChar('7')`, `Backspace`, `CommitSpace`, `NextCandidate`, `OpenTashkeel`, …).
3. Request a read-write edit session (`TF_ES_READWRITE | TF_ES_ASYNCDONTCARE`, `docs/02 §8`).
4. In the session: update `Session` (engine), get `CandidateList`, set composition text to the
   inline preview (default candidate's display string), apply display attribute.
5. Get text extent of the composition range → position popup → `t3a-ui::show(model, rect)`.
6. On commit: write final text, end composition, record `UserEvent::Chose{latin, word, rank}` to the
   in-memory user model and enqueue a journal record (if learning allowed in this context).

## 5. Files on disk

| Path | Writer | Readers | Notes |
|---|---|---|---|
| `%ProgramFiles%\Type3arabi\type3arabi.dat` | installer | TIP (all procs), CLI | Readable from AppContainers by default (Program Files). |
| `%ProgramFiles%\Type3arabi\{x64,x86,arm64}\t3a_tip.dll` | installer | TSF | Same CLSID registered in each registry view. |
| `%LOCALAPPDATA%\Type3arabi\config.toml` | Settings app | TIP | Directory ACL grants **read** to `ALL APPLICATION PACKAGES` (S-1-15-2-1) and `ALL RESTRICTED APPLICATION PACKAGES` (S-1-15-2-2). |
| `%LOCALAPPDATA%\Type3arabi\user\snapshot.t3u` | TIP (compaction) | TIP | Compact user model; same read ACL. |
| `%LOCALAPPDATA%\Type3arabi\user\journal.t3j` | TIP (append) | TIP | Append-only fixed-size records; see `docs/03 §9.4`. |
| `%LOCALAPPDATA%\Type3arabi\logs\errors.log` | TIP/apps | human | Max 256 KB, rotated, **never contains typed text**. |

The per-user directory and its ACL are created by the **Settings app on first run** *and* by the
TIP on first activation in a non-AppContainer process (whichever happens first). Creation code lives
in the small Windows-only crate `t3a-paths::ensure_user_dir()`, shared by the TIP, the hotkey
companion and the settings app (see §7).

## 6. Runtime configuration & live reload
- `config.toml` schema: `docs/13-config-schema.md`. Missing file ⇒ built-in defaults.
- TIP checks the file's last-write time (one `GetFileAttributesExW`, ~µs) on `Activate` and on each
  composition *start* (not per key). Changed ⇒ re-parse (tiny TOML parser in `t3a-engine::config`,
  no serde) and swap the `Arc<Config>`.

## 7. Crate dependency graph
```
t3a-cli ──► t3a-engine ──► t3a-data
t3a-tip ──► t3a-engine, t3a-data(mmap), t3a-ui, t3a-paths, windows
t3a-ui  ──► windows            (knows nothing about the engine; takes PopupModel)
t3a-hotkey ──► t3a-paths, windows
t3a-paths ──► windows          (known folders, user-dir creation + AppContainer read ACL, error log)
apps/settings (Tauri) ──► t3a-engine (for live preview), t3a-paths (shared path/ACL helpers)
```
`t3a-ui` must not depend on `t3a-engine`: the TIP converts `CandidateList` → `PopupModel`.
This keeps the UI reusable by the Plan-B overlay front-end (`docs/11`).

## 8. Dependency ledger (R13 — every dependency must be listed here)

| Crate | Version | License | Used by | Why |
|---|---|---|---|---|
| `windows` | =0.62.2 | MIT/Apache-2.0 | tip, ui, hotkey, paths | Win32/COM/TSF/D2D/DWrite bindings, `#[implement]` for COM. Do **not** move to 0.100 until an ADR (API churn). |
| `windows-core` | (via windows) | MIT/Apache-2.0 | tip, ui | COM core types. |
| `bytemuck` | 1.x | Zlib/MIT/Apache-2.0 | data | Zero-copy `#[repr(C)]` casts of mmapped sections. |
| `memmap2` | 0.9.x | MIT/Apache-2.0 | data (feature `mmap`) | Portable read-only file mapping. |
| `tauri` | 2.x | MIT/Apache-2.0 | apps/settings | Settings UI shell (WebView2), ADR-0007. Separate Cargo workspace (own `Cargo.lock`); `cargo deny --manifest-path apps/settings/Cargo.toml check` green. |
| `tauri-build` | 2.x | MIT/Apache-2.0 | apps/settings (build-dep) | Embeds `tauri.conf.json`, the `ui/` folder and the icon. |
| `serde` | 1.x | MIT/Apache-2.0 | apps/settings | Settings ⇄ UI command payloads (engine/TIP stay serde-free, R12). |
| `serde_json` | 1.x | MIT/Apache-2.0 | apps/settings | Same. |
| `embed-resource` | 3.0.11 | MIT | tip, hotkey (build-dep) | Compile the generated `.rc` (brand icon `IDI_BRAND` for the TSF profile, VERSIONINFO) — `build.rs` of each crate. |

Dev-only (not shipped): `criterion` (bench), `proptest` (property tests), `cargo-fuzz`/`libfuzzer-sys` (fuzzing);
`windows` features `Win32_Storage_Xps` (t3a-ui `popup_paint` example screenshots).
Pipeline tooling (Python, never shipped): `datasets`, `huggingface_hub`, `pyarrow`, `regex`, `numpy`, `tqdm`,
`zstandard`, `openpyxl` (reads the Talafha corpus .xlsx).

## 9. Architecture invariants (checked in review)
1. The engine never allocates per keystroke after warm-up (all buffers reused; asserted by a
   counting allocator in `t3a-cli bench --check-alloc`).
2. The TIP never reads the whole data file; only touched pages are faulted in.
3. The UI never calls the engine; the engine never calls Windows.
4. Config and user-store formats are versioned; unknown newer versions ⇒ ignore (read-only mode), never crash.
5. Every Windows API failure on the keystroke path degrades to *pass the key through* — never to eating input silently.
