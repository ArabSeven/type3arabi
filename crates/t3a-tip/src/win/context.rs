//! Context inspection and gating (docs/02 §7).

use crate::keyrouter::ContextMode;
use windows::core::Interface;
use windows::Win32::UI::TextServices::{
    ITfCompartmentMgr, ITfContext, GUID_COMPARTMENT_EMPTYCONTEXT,
    GUID_COMPARTMENT_KEYBOARD_DISABLED, TS_SD_READONLY,
};

/// A context compartment set to a non-zero integer.
fn compartment_set(ctx: &ITfContext, guid: &windows::core::GUID) -> bool {
    // SAFETY: plain COM queries on a live context; failures mean "not set".
    unsafe {
        let Ok(mgr) = ctx.cast::<ITfCompartmentMgr>() else {
            return false;
        };
        let Ok(comp) = mgr.GetCompartment(guid) else {
            return false;
        };
        let Ok(v) = comp.GetValue() else {
            return false;
        };
        i32::try_from(&v).map(|n| n != 0).unwrap_or(false)
    }
}

/// Determines the context mode (Off, Latin, Arabic) for the given TSF context.
///
/// Off when: no context, read-only document, keyboard disabled for the context (TSF sets this for
/// password boxes and for apps that disable IMEs — AGENTS.md R9), or an empty/placeholder context.
/// Secure mode (logon/UAC desktop) still transliterates; only learning is disabled there (R9), which
/// the caller enforces at commit time.
pub fn evaluate_context_mode(context: Option<&ITfContext>, arabic_mode: bool) -> ContextMode {
    let Some(ctx) = context else {
        return ContextMode::Off;
    };

    // SAFETY: GetStatus on a live context.
    if let Ok(status) = unsafe { ctx.GetStatus() } {
        if (status.dwDynamicFlags & TS_SD_READONLY) != 0 {
            return ContextMode::Off;
        }
    }
    if compartment_set(ctx, &GUID_COMPARTMENT_KEYBOARD_DISABLED)
        || compartment_set(ctx, &GUID_COMPARTMENT_EMPTYCONTEXT)
    {
        return ContextMode::Off;
    }

    if arabic_mode {
        ContextMode::Arabic
    } else {
        ContextMode::Latin
    }
}
