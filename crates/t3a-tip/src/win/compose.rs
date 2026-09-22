//! Edit sessions and composition management (docs/02 §8).

use std::cell::RefCell;
use windows::core::implement;
use windows::Win32::UI::TextServices::{ITfEditSession, ITfEditSession_Impl};

#[implement(ITfEditSession)]
pub struct EditSession<F>
where
    F: FnOnce(u32) -> windows::core::Result<()> + 'static,
{
    callback: RefCell<Option<F>>,
}

impl<F> EditSession<F>
where
    F: FnOnce(u32) -> windows::core::Result<()> + 'static,
{
    pub fn new(f: F) -> Self {
        Self {
            callback: RefCell::new(Some(f)),
        }
    }
}

impl<F> ITfEditSession_Impl for EditSession_Impl<F>
where
    F: FnOnce(u32) -> windows::core::Result<()> + 'static,
{
    fn DoEditSession(&self, ec: u32) -> windows::core::Result<()> {
        if let Some(cb) = self.callback.borrow_mut().take() {
            cb(ec)
        } else {
            Ok(())
        }
    }
}
