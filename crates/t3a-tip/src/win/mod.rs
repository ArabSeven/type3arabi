//! Windows implementation (M1). Planned module layout — create each file as the milestone reaches it:
//!
//! | module          | contents (docs/02 section) |
//! |-----------------|----------------------------|
//! | `guard.rs`      | `guard(f)` catch_unwind wrapper, process-wide DISABLED flag, error log (§15) |
//! | `dll.rs`        | DllMain (store HMODULE only), DllGetClassObject, DllCanUnloadNow, DllRegisterServer/Unregister (§2, §3) |
//! | `factory.rs`    | IClassFactory |
//! | `service.rs`    | TextService: ITfTextInputProcessorEx + all sinks (§3, §4) |
//! | `keys.rs`       | scan-code → Latin char translation (§6), builds keyrouter::Key/Mods |
//! | `context.rs`    | ContextMode computation: input scopes, read-only, AppContainer, secure mode (§7) |
//! | `compose.rs`    | edit sessions, composition start/update/commit/cancel, display attributes (§8) |
//! | `popup_host.rs` | owner window, placement, UILess UIElement, light-dismiss events (§9) |
//! | `mode.rs`       | compartments, preserved key, lang-bar input-mode item (§10) |
//! | `reedit.rs`     | surrounding context and re-edit anchor (§12) |
//! | `userstore.rs`  | journal/snapshot IO + writer thread + tailing (docs/03 §9.4) |
//!
//! Until M1 lands, this crate exports nothing on Windows, so the DLL cannot be registered by accident.
