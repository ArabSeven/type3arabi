//! Context inspection and gating (docs/02 §7).

use crate::keyrouter::ContextMode;
use crate::win::compose::EditSession;
use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use windows::core::{IUnknown, Interface};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::TextServices::{
    ITfCompartmentMgr, ITfContext, ITfEditSession, ITfInputScope, ITfReadOnlyProperty, InputScope,
    GUID_COMPARTMENT_EMPTYCONTEXT, GUID_COMPARTMENT_KEYBOARD_DISABLED, GUID_PROP_INPUTSCOPE,
    TF_DEFAULT_SELECTION, TF_ES_READ, TF_ES_SYNC, TF_SELECTION, TS_SD_READONLY,
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

/// The input scopes of the field at the selection (docs/02 §7): the `GUID_PROP_INPUTSCOPE` property of
/// the selection range, read in a synchronous read-only edit session. `None` when the app does not
/// report scopes or TSF refuses the lock (e.g. while our own edits are queued) — treated as "no scope".
pub fn input_scopes(ctx: &ITfContext, tid: u32) -> Option<Vec<i32>> {
    let out: Rc<RefCell<Option<Vec<i32>>>> = Rc::new(RefCell::new(None));
    let out2 = out.clone();
    let ctx2 = ctx.clone();
    let session: ITfEditSession = EditSession::new(move |ec| {
        // SAFETY: COM calls on a live context inside a granted read lock; the scope array is
        // CoTaskMemAlloc'ed by the callee and freed here exactly once.
        unsafe {
            let mut sel = [TF_SELECTION::default()];
            let mut fetched = 0u32;
            ctx2.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched)?;
            let [sel] = sel;
            // Owned here (GetSelection returns an AddRef'ed range): released when this scope ends.
            let range = ManuallyDrop::into_inner(sel.range);
            let Some(range) = range.filter(|_| fetched > 0) else {
                return Ok(());
            };
            let prop: ITfReadOnlyProperty = match ctx2.GetAppProperty(&GUID_PROP_INPUTSCOPE) {
                Ok(p) => p,
                Err(_) => ctx2.GetProperty(&GUID_PROP_INPUTSCOPE)?.cast()?,
            };
            let value = prop.GetValue(ec, &range)?;
            let Ok(unk) = IUnknown::try_from(&value) else {
                return Ok(());
            };
            let scope: ITfInputScope = unk.cast()?;
            let mut list: *mut InputScope = std::ptr::null_mut();
            let mut count = 0u32;
            scope.GetInputScopes(&mut list, &mut count)?;
            if !list.is_null() {
                let v = std::slice::from_raw_parts(list, count.min(64) as usize)
                    .iter()
                    .map(|s| s.0)
                    .collect();
                CoTaskMemFree(Some(list as *const _));
                if let Ok(mut o) = out2.try_borrow_mut() {
                    *o = Some(v);
                }
            }
        }
        Ok(())
    })
    .into();
    // SAFETY: plain COM call; the session runs synchronously or not at all.
    let hr = unsafe { ctx.RequestEditSession(tid, &session, TF_ES_SYNC | TF_ES_READ) };
    if !matches!(hr, Ok(h) if h.is_ok()) {
        return None;
    }
    out.try_borrow_mut().ok().and_then(|mut o| o.take())
}
