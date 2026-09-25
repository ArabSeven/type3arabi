//! DLL entry points, COM exports, and TSF/COM registration (docs/02 §2, §3).

use crate::ids::{
    guid_string, CLSID_TIP, GUID_PROFILE, LANGID, LEGACY_LANGIDS, PROFILE_DESCRIPTION,
};
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
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE,
    KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, ITfCategoryMgr,
    ITfInputProcessorProfileMgr, ITfInputProcessorProfiles, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIPCAP_COMLESS, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT, GUID_TFCAT_TIPCAP_SECUREMODE,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT, GUID_TFCAT_TIPCAP_UIELEMENTENABLED, GUID_TFCAT_TIP_KEYBOARD,
};

static OBJECT_COUNT: AtomicI32 = AtomicI32::new(0);
static SERVER_LOCKS: AtomicI32 = AtomicI32::new(0);
static MODULE_HANDLE: AtomicIsize = AtomicIsize::new(0);

pub const CLSID_TYPE3ARABI_TIP: GUID = GUID::from_u128(CLSID_TIP);
pub const GUID_PROFILE_TYPE3ARABI: GUID = GUID::from_u128(GUID_PROFILE);

/// Categories we register. Only capabilities the TIP really implements: claiming
/// UIELEMENTENABLED / COMLESS / SECUREMODE / INPUTMODECOMPARTMENT without implementing them makes
/// hosts call into interfaces we do not provide.
/// Resource ID of the brand icon (build.rs: `101 ICON`), shown for the TSF profile (docs/02 §2).
const IDI_BRAND: i32 = 101;

const CATEGORIES: [GUID; 4] = [
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
];

/// Categories an earlier dev build registered; removed on unregister.
const LEGACY_CATEGORIES: [GUID; 4] = [
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
    GUID_TFCAT_TIPCAP_SECUREMODE,
    GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
];

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

/// Full path of this DLL (no terminating NUL).
pub fn dll_path() -> Option<std::path::PathBuf> {
    let w = dll_path_wide()?;
    Some(std::path::PathBuf::from(String::from_utf16_lossy(&w)))
}

fn dll_path_wide() -> Option<Vec<u16>> {
    let mut buf = vec![0u16; 1024];
    // SAFETY: buf is a valid writable slice; the module handle was stored by DllMain.
    let len = unsafe { GetModuleFileNameW(Some(module_handle().into()), &mut buf) } as usize;
    if len == 0 || len >= buf.len() {
        return None;
    }
    buf.truncate(len);
    Some(buf)
}

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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
        // SAFETY: pointers checked non-null above; COM contract guarantees validity.
        unsafe {
            *ppv = std::ptr::null_mut();
            if *rclsid != CLSID_TYPE3ARABI_TIP {
                return CLASS_E_CLASSNOTAVAILABLE;
            }
            let unk: windows::core::IUnknown = ClassFactory.into();
            unk.query(riid, ppv)
        }
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECT_COUNT.load(Ordering::Relaxed) == 0 && SERVER_LOCKS.load(Ordering::Relaxed) == 0 {
        // Our popup window class points at code in this DLL: unregister it before unload.
        t3a_ui::unregister_class();
        S_OK
    } else {
        S_FALSE
    }
}

unsafe fn set_sz(hkey: HKEY, name: Option<&str>, value: &[u16]) {
    let name_w = name.map(wide_z);
    let name_p = name_w
        .as_ref()
        .map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
    let _ = RegSetValueExW(hkey, name_p, Some(0), REG_SZ, Some(bytes));
}

unsafe fn create_key(parent: HKEY, path: &str) -> Option<HKEY> {
    let path_w = wide_z(path);
    let mut hkey = HKEY::default();
    RegCreateKeyExW(
        parent,
        PCWSTR(path_w.as_ptr()),
        Some(0),
        None,
        REG_OPTION_NON_VOLATILE,
        KEY_ALL_ACCESS,
        None,
        &mut hkey,
        None,
    )
    .ok()
    .ok()?;
    Some(hkey)
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllRegisterServer() -> HRESULT {
    guard(E_FAIL, || unsafe {
        let Some(path) = dll_path_wide() else {
            return E_FAIL;
        };
        let mut path_z = path.clone();
        path_z.push(0);

        // 1. COM InprocServer32 (HKLM only: registration requires elevation).
        let clsid_key = format!("Software\\Classes\\CLSID\\{}", guid_string(CLSID_TIP));
        let Some(hkey) = create_key(HKEY_LOCAL_MACHINE, &clsid_key) else {
            return E_FAIL;
        };
        set_sz(hkey, None, &wide_z(PROFILE_DESCRIPTION));
        let inproc = create_key(hkey, "InprocServer32");
        let _ = RegCloseKey(hkey);
        let Some(inproc) = inproc else {
            return E_FAIL;
        };
        set_sz(inproc, None, &path_z);
        set_sz(inproc, Some("ThreadingModel"), &wide_z("Apartment"));
        let _ = RegCloseKey(inproc);

        // 2. Exactly one language profile (docs/02 §2.1).
        let mgr: ITfInputProcessorProfileMgr =
            match CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) {
                Ok(m) => m,
                Err(e) => return e.code(),
            };
        let desc: Vec<u16> = PROFILE_DESCRIPTION.encode_utf16().collect();
        if let Err(e) = mgr.RegisterProfile(
            &CLSID_TYPE3ARABI_TIP,
            LANGID,
            &GUID_PROFILE_TYPE3ARABI,
            &desc,
            &path,
            // Negative = resource ID (SampleIME convention): the brand icon embedded by build.rs.
            (-IDI_BRAND) as u32,
            HKL::default(),
            0,
            true,
            0,
        ) {
            return e.code();
        }

        // 3. Categories.
        if let Ok(cat) =
            CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
        {
            for c in LEGACY_CATEGORIES {
                let _ = cat.UnregisterCategory(&CLSID_TYPE3ARABI_TIP, &c, &CLSID_TYPE3ARABI_TIP);
            }
            for c in CATEGORIES {
                let _ = cat.RegisterCategory(&CLSID_TYPE3ARABI_TIP, &c, &CLSID_TYPE3ARABI_TIP);
            }
        }
        S_OK
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
    guard(E_FAIL, || unsafe {
        if let Ok(cat) =
            CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
        {
            for c in CATEGORIES.iter().chain(LEGACY_CATEGORIES.iter()) {
                let _ = cat.UnregisterCategory(&CLSID_TYPE3ARABI_TIP, c, &CLSID_TYPE3ARABI_TIP);
            }
        }

        if let Ok(mgr) = CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        ) {
            // Remove every LANGID ever used, so installs from older dev builds are cleaned too.
            for langid in LEGACY_LANGIDS {
                let _ = mgr.UnregisterProfile(
                    &CLSID_TYPE3ARABI_TIP,
                    langid,
                    &GUID_PROFILE_TYPE3ARABI,
                    0,
                );
            }
        }
        // Then drop the whole TIP (HKLM\...\CTF\TIP\{clsid}): removing the last profile does not always
        // delete the key, and a leftover key shows up as a ghost keyboard after uninstall.
        if let Ok(profiles) = CoCreateInstance::<_, ITfInputProcessorProfiles>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        ) {
            let _ = profiles.Unregister(&CLSID_TYPE3ARABI_TIP);
        }

        let clsid_key = wide_z(&format!(
            "Software\\Classes\\CLSID\\{}",
            guid_string(CLSID_TIP)
        ));
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(clsid_key.as_ptr()));
        S_OK
    })
}
