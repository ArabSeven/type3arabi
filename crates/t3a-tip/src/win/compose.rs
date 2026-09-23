//! Closure-based edit session (docs/02 §3, §8).

use crate::win::guard::guard;
use std::cell::RefCell;
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
        match cb {
            Some(cb) => guard(Err(E_FAIL.into()), || cb(ec)),
            None => Ok(()),
        }
    }
}
