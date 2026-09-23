//! COM class factory for TextService (docs/02 §3).

use crate::win::dll;
use crate::win::guard::guard;
use crate::win::service::TextService;
use core::ffi::c_void;
use windows::core::{implement, Interface, BOOL, GUID};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_POINTER};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};

#[implement(IClassFactory)]
pub struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: windows_core::Ref<'_, windows::core::IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if punkouter.as_ref().is_some() {
            return Err(windows::core::Error::from(CLASS_E_NOAGGREGATION));
        }
        if ppvobject.is_null() || riid.is_null() {
            return Err(windows::core::Error::from(E_POINTER));
        }

        guard(Err(windows::Win32::Foundation::E_FAIL.into()), || unsafe {
            // SAFETY: out-pointers checked non-null above.
            *ppvobject = std::ptr::null_mut();
            let service = TextService::new()?;
            let unk: windows::core::IUnknown = service.into();
            unk.query(riid, ppvobject).ok()
        })
    }

    fn LockServer(&self, flock: BOOL) -> windows::core::Result<()> {
        if flock.as_bool() {
            dll::lock_server();
        } else {
            dll::unlock_server();
        }
        Ok(())
    }
}
