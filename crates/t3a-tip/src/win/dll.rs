//! DLL entry points, COM exports, and TSF/COM registration (docs/02 §2, §3).

use crate::ids::{guid_string, CLSID_TIP, GUID_PROFILE, LANGIDS, PROFILE_DESCRIPTION};
use crate::win::factory::ClassFactory;
use crate::win::guard::guard;
use core::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};
use windows::core::{Interface, BOOL, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_FAIL, E_POINTER, HINSTANCE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, ITfCategoryMgr,
    ITfInputProcessorProfileMgr, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER, GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
    GUID_TFCAT_TIPCAP_SECUREMODE, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED, GUID_TFCAT_TIP_KEYBOARD,
};

static OBJECT_COUNT: AtomicI32 = AtomicI32::new(0);
static SERVER_LOCKS: AtomicI32 = AtomicI32::new(0);
static MODULE_HANDLE: AtomicIsize = AtomicIsize::new(0);

pub const CLSID_TYPE3ARABI_TIP: GUID = GUID::from_u128(CLSID_TIP);
pub const GUID_PROFILE_TYPE3ARABI: GUID = GUID::from_u128(GUID_PROFILE);

pub fn add_object() {
    OBJECT_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn release_object() {
    OBJECT_COUNT.fetch_sub(1, Ordering::Relaxed);
}

pub fn lock_server() {
    SERVER_LOCKS.fetch_add(1, Ordering::Relaxed);
}

pub fn unlock_server() {
    SERVER_LOCKS.fetch_sub(1, Ordering::Relaxed);
}

pub fn module_handle() -> HINSTANCE {
    HINSTANCE(MODULE_HANDLE.load(Ordering::Relaxed) as *mut _)
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *const c_void,
) -> BOOL {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        MODULE_HANDLE.store(hinst.0 as isize, Ordering::Relaxed);
    }
    BOOL::from(true)
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    guard(E_FAIL, || {
        if ppv.is_null() || rclsid.is_null() || riid.is_null() {
            return E_POINTER;
        }
        *ppv = std::ptr::null_mut();

        if *rclsid != CLSID_TYPE3ARABI_TIP {
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        let factory = ClassFactory;
        let unk: windows::core::IUnknown = factory.into();
        match unk.query(riid, ppv) {
            windows::core::HRESULT(0) => S_OK,
            hr => hr,
        }
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECT_COUNT.load(Ordering::Relaxed) == 0 && SERVER_LOCKS.load(Ordering::Relaxed) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

fn get_dll_path() -> windows::core::Result<Vec<u16>> {
    unsafe {
        let mut buf = vec![0u16; 1024];
        let len = GetModuleFileNameW(Some(module_handle().into()), &mut buf);
        if len == 0 {
            return Err(windows::core::Error::from(E_FAIL));
        }
        buf.truncate(len as usize);
        buf.push(0); // Null terminator
        Ok(buf)
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllRegisterServer() -> HRESULT {
    guard(E_FAIL, || {
        let dll_path = match get_dll_path() {
            Ok(p) => p,
            Err(e) => return e.code(),
        };

        // 1. COM InprocServer32 registration
        let clsid_str = guid_string(CLSID_TIP);
        let key_path = format!("Software\\Classes\\CLSID\\{clsid_str}");
        let key_path_w: Vec<u16> = key_path.encode_utf16().chain(std::iter::once(0)).collect();

        let mut hkey = HKEY::default();
        let mut status = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_path_w.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut hkey,
            None,
        );
        if status.is_err() {
            status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path_w.as_ptr()),
                Some(0),
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut hkey,
                None,
            );
        }
        if status.is_err() {
            return E_FAIL;
        }

        let desc_w: Vec<u16> = "Type3arabi"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = RegSetValueExW(
            hkey,
            PCWSTR::null(),
            Some(0),
            REG_SZ,
            Some(std::slice::from_raw_parts(
                desc_w.as_ptr() as *const u8,
                desc_w.len() * 2,
            )),
        );

        let mut hkey_inproc = HKEY::default();
        let inproc_w: Vec<u16> = "InprocServer32"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let status = RegCreateKeyExW(
            hkey,
            PCWSTR(inproc_w.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut hkey_inproc,
            None,
        );
        let _ = RegCloseKey(hkey);

        if status.is_err() {
            return E_FAIL;
        }

        let _ = RegSetValueExW(
            hkey_inproc,
            PCWSTR::null(),
            Some(0),
            REG_SZ,
            Some(std::slice::from_raw_parts(
                dll_path.as_ptr() as *const u8,
                dll_path.len() * 2,
            )),
        );

        let threading_model: Vec<u16> = "Apartment"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let threading_key: Vec<u16> = "ThreadingModel"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = RegSetValueExW(
            hkey_inproc,
            PCWSTR(threading_key.as_ptr()),
            Some(0),
            REG_SZ,
            Some(std::slice::from_raw_parts(
                threading_model.as_ptr() as *const u8,
                threading_model.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey_inproc);

        // 2. Register TSF Profiles for all Arabic LANGIDs
        let profile_mgr: windows::core::Result<ITfInputProcessorProfileMgr> =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER);

        if let Ok(mgr) = profile_mgr {
            let profile_desc: Vec<u16> = PROFILE_DESCRIPTION
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            for langid in LANGIDS {
                let _ = mgr.RegisterProfile(
                    &CLSID_TYPE3ARABI_TIP,
                    langid,
                    &GUID_PROFILE_TYPE3ARABI,
                    &profile_desc,
                    &dll_path,
                    0,
                    windows::Win32::UI::Input::KeyboardAndMouse::HKL::default(),
                    0,
                    true,
                    0,
                );
            }
        }

        // 3. Register TSF Categories
        let cat_mgr: windows::core::Result<ITfCategoryMgr> =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER);

        if let Ok(mgr) = cat_mgr {
            let categories = [
                GUID_TFCAT_TIP_KEYBOARD,
                GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
                GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
                GUID_TFCAT_TIPCAP_SECUREMODE,
                GUID_TFCAT_TIPCAP_COMLESS,
                GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
                GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
            ];

            for cat in categories {
                let _ = mgr.RegisterCategory(&CLSID_TYPE3ARABI_TIP, &cat, &CLSID_TYPE3ARABI_TIP);
            }
        }

        S_OK
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
    guard(E_FAIL, || {
        // 1. Unregister Categories
        let cat_mgr: windows::core::Result<ITfCategoryMgr> =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER);

        if let Ok(mgr) = cat_mgr {
            let categories = [
                GUID_TFCAT_TIP_KEYBOARD,
                GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
                GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
                GUID_TFCAT_TIPCAP_SECUREMODE,
                GUID_TFCAT_TIPCAP_COMLESS,
                GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
                GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
            ];

            for cat in categories {
                let _ = mgr.UnregisterCategory(&CLSID_TYPE3ARABI_TIP, &cat, &CLSID_TYPE3ARABI_TIP);
            }
        }

        // 2. Unregister Profiles
        let profile_mgr: windows::core::Result<ITfInputProcessorProfileMgr> =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER);

        if let Ok(mgr) = profile_mgr {
            for langid in LANGIDS {
                let _ = mgr.UnregisterProfile(
                    &CLSID_TYPE3ARABI_TIP,
                    langid,
                    &GUID_PROFILE_TYPE3ARABI,
                    0,
                );
            }
        }

        // 3. Remove COM registry keys
        let clsid_str = guid_string(CLSID_TIP);
        let key_path = format!("Software\\Classes\\CLSID\\{clsid_str}");
        let key_path_w: Vec<u16> = key_path.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(key_path_w.as_ptr()));
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key_path_w.as_ptr()));

        S_OK
    })
}
