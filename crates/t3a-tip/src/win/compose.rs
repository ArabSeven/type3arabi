//! Closure-based edit session (docs/02 §3, §8).

use crate::win::guard::set_disabled;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use windows::core::implement;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::UI::TextServices::{ITfEditSession, ITfEditSession_Impl};

type Callback = Box<dyn FnOnce(u32) -> windows::core::Result<()>>;

#[implement(ITfEditSession)]
pub struct EditSession {
    callback: RefCell<Option<Callback>>,
}

impl EditSession {
    pub fn new(f: impl FnOnce(u32) -> windows::core::Result<()> + 'static) -> Self {
        Self {
            callback: RefCell::new(Some(Box::new(f))),
        }
    }
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> windows::core::Result<()> {
        let cb = self
            .callback
            .try_borrow_mut()
            .ok()
            .and_then(|mut c| c.take());
        // Not `guard`: after a panic the TIP still needs one edit session to end its composition
        // (safe passthrough cleanup, R2). A panic here still disables the TIP.
        match cb {
            Some(cb) => catch_unwind(AssertUnwindSafe(|| cb(ec))).unwrap_or_else(|_| {
                set_disabled();
                Err(E_FAIL.into())
            }),
            None => Ok(()),
        }
    }
}
