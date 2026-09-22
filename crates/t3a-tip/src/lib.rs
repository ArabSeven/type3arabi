//! # t3a-tip — the TSF Text Input Processor (docs/02-tsf-integration.md)
//!
//! Portable modules (compiled and tested everywhere): `ids`, `keyrouter`.
//! Windows modules (M1): COM class factory, `TextService`, sinks, compositions, edit sessions,
//! display attributes, lang-bar mode item, UIElement, registration, `guard` (panic safety).
//!
//! AGENTS.md R1: every exported function and COM method body goes through `guard()`.

pub mod ids;
pub mod keyrouter;

#[cfg(windows)]
mod win;

#[cfg(windows)]
#[allow(unused_imports)]
pub use win::*;
