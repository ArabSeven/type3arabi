//! Enabling / disabling the Type3arabi keyboard for the signed-in user (installer helper), and a
//! read-only listing of what the Win+Space flyout shows.
//!
//! Every change goes through `input.dll!InstallLayoutOrTip`, the documented API behind the Settings
//! app (R6). Windows adds a language's *default keyboard* (Arabic 101 for ar-SA) whenever a TIP is
//! enabled for a language the user did not have, so `enable` removes the layouts that came with it:
//! a user who had no Arabic before sees exactly one new entry, "Arabic (Saudi Arabia) · Type3arabi".
//!
//! `tidy` (run by the companion at sign-in and whenever the session's loaded layouts change) covers
//! the other way Arabic 101 comes back: Windows *loads* the Arabic layout into the session although
//! the user's list does not contain it, and the switcher lists every loaded layout. Suspected sources: the
//! sign-in screen's own keyboards (`HKU\.DEFAULT`, e.g. Arabic (101) copied there by Windows setup)
//! and apps walking `Keyboard Layout\Preload` without substitutes. Unloading it
//! (`UnloadKeyboardLayout`, documented user32) changes no setting.
use windows::core::{w, Interface, PCSTR, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    RRF_RT_REG_MULTI_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayoutList, UnloadKeyboardLayout, HKL,
};
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfileMgr, ITfInputProcessorProfiles,
    TF_INPUTPROCESSORPROFILE, TF_IPP_FLAG_ENABLED, TF_PROFILETYPE_INPUTPROCESSOR,
};

/// ar-SA, the one language Type3arabi registers under (ADR-0009).
const LANG: &str = "ar-SA";
const TIP: &str =
    "0401:{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}{90D49398-54D3-4F08-9C15-0B38D0820A87}";
const ILOT_UNINSTALL: u32 = 0x1;

/// `input.dll!InstallLayoutOrTip` (no import library ships with the SDK: resolved at runtime).
fn install_layout_or_tip(item: &str, flags: u32) -> bool {
    type Fn = unsafe extern "system" fn(PCWSTR, u32) -> i32;
    let s: Vec<u16> = item.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: loads a system DLL and calls a documented export with a NUL-terminated string.
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("input.dll")) else {
            return false;
        };
        let Some(proc) = GetProcAddress(lib, PCSTR(c"InstallLayoutOrTip".as_ptr() as *const u8))
        else {
            return false;
        };
        let f: Fn = std::mem::transmute(proc);
        f(PCWSTR(s.as_ptr()), flags) != 0
    }
}

/// Read a REG_MULTI_SZ / value names under `HKCU\Control Panel\International\User Profile`.
fn user_languages() -> Vec<String> {
    let mut buf = vec![0u16; 4096];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: buffer and size describe the same writable allocation.
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\International\\User Profile"),
            w!("Languages"),
            RRF_RT_REG_MULTI_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if ok.is_err() {
        return Vec::new();
    }
    buf.truncate(size as usize / 2);
    String::from_utf16_lossy(&buf)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// The user's saved inputs for `LANG` (Settings → Language → Arabic → Keyboards), as
/// InstallLayoutOrTip strings: the value names under `User Profile\ar-SA`, e.g. "0401:00000401".
fn saved_inputs() -> Vec<String> {
    let path: Vec<u16> = format!("Control Panel\\International\\User Profile\\{LANG}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut out = Vec::new();
    // SAFETY: read-only key opened and closed here; every buffer and size pair matches.
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumValueW(
                key,
                i,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
            .is_err()
            {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out
}

/// Keyboard layouts (not TIPs) that TSF currently lists for ar-SA, as InstallLayoutOrTip strings
/// with their HKLs, e.g. ("0401:00000401", 0x04010401) = Arabic 101.
fn arabic_layouts() -> Vec<(String, HKL)> {
    list_profiles(0x0401)
        .into_iter()
        .filter(|p| p.enabled && !p.tip)
        .filter_map(|p| p.klid.map(|k| (format!("0401:{k}"), p.hkl)))
        .collect()
}

/// What one tidy-up did: layouts unloaded, and layouts Windows would not unload (retried later).
#[derive(Default)]
pub struct Tidy {
    pub unloaded: usize,
    pub failed: usize,
}

/// The keyboard layouts loaded in this session (what `GetKeyboardLayoutList` returns), as raw HKLs.
/// A cheap user32 call: the companion compares it on a timer and runs `tidy` only when it changes.
pub fn loaded_layouts() -> Vec<usize> {
    let mut list = [HKL::default(); 64];
    // SAFETY: the slice is a valid writable buffer.
    let n = unsafe { GetKeyboardLayoutList(Some(&mut list)) } as usize;
    list[..n.min(list.len())]
        .iter()
        .map(|h| h.0 as usize)
        .collect()
}

/// "04090409 04010401 …" for the error log (layout ids only, never text; R8).
fn hkl_list() -> String {
    loaded_layouts()
        .iter()
        .map(|h| format!("{:08X}", *h as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Remove from the session the Arabic layouts the user did not choose (not in `keep`); the ones
/// in the saved list are removed from it too (they came with the TIP, see `enable`).
fn drop_layouts_except(keep: &[String]) -> Tidy {
    let saved = saved_inputs();
    let mut done = Tidy::default();
    for (layout, hkl) in arabic_layouts() {
        if !crate::stray_arabic_layout(&layout, keep) {
            continue;
        }
        let before = hkl_list();
        if saved.iter().any(|k| k.eq_ignore_ascii_case(&layout))
            && !install_layout_or_tip(&layout, ILOT_UNINSTALL)
        {
            t3a_paths::log_error("enable-profile: could not remove an added Arabic layout");
        }
        // A loaded layout stays listed by Win+Space until sign-out otherwise.
        // SAFETY: plain Win32 call; it fails harmlessly while a window still uses the layout.
        let ok = unsafe { UnloadKeyboardLayout(hkl) }.is_ok()
            && !loaded_layouts().contains(&(hkl.0 as usize));
        if ok {
            done.unloaded += 1;
        } else {
            done.failed += 1;
        }
        // Evidence for where it came from (docs/02 §2 step 4): which layouts were loaded with it.
        t3a_paths::log_error(&format!(
            "tidy: {} stray layout {:08X}; loaded before: {before}",
            if ok { "unloaded" } else { "could not unload" },
            hkl.0 as usize as u32,
        ));
    }
    done
}

/// `--enable-profile`. Exit code 0 = enabled.
pub fn enable() -> i32 {
    let had_arabic = user_languages()
        .iter()
        .any(|l| l.eq_ignore_ascii_case(LANG));
    // What the user chose, from the saved list: a layout that is merely loaded is not a choice.
    let before = if had_arabic {
        saved_inputs()
    } else {
        Vec::new()
    };
    if !install_layout_or_tip(TIP, 0) {
        return 1;
    }
    // Keep only the layouts the user already had for Arabic; drop what Windows added with the TIP.
    let _ = drop_layouts_except(&before);
    0
}

/// If Type3arabi is one of the user's keyboards, unload the Arabic layouts that are loaded but not
/// in their saved list. Changes no setting. Run by the companion at sign-in and whenever the loaded
/// layouts change (see its main), and by `--tidy`.
pub fn tidy() -> Tidy {
    let saved = saved_inputs();
    if !saved.iter().any(|k| k.eq_ignore_ascii_case(TIP)) {
        return Tidy::default();
    }
    drop_layouts_except(&saved)
}

/// Is Type3arabi one of the signed-in user's keyboards (their saved list, what Settings shows)?
pub fn is_enabled() -> bool {
    saved_inputs().iter().any(|k| k.eq_ignore_ascii_case(TIP))
}

/// `--disable-profile`. Exit code 0 = removed.
pub fn disable() -> i32 {
    if install_layout_or_tip(TIP, ILOT_UNINSTALL) {
        0
    } else {
        1
    }
}

pub struct Profile {
    pub lang: u16,
    pub tip: bool,
    pub enabled: bool,
    pub name: String,
    pub hkl: HKL,
    /// Keyboard layout id ("00000401"), for layouts whose HKL encodes it directly.
    pub klid: Option<String>,
}

/// Every input profile TSF knows for `lang` (0 = all languages): what the Win+Space flyout lists.
pub fn list_profiles(lang: u16) -> Vec<Profile> {
    let mut out = Vec::new();
    // SAFETY: standard COM activation and enumeration; all out-parameters are local.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(mgr) = CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        ) else {
            return out;
        };
        let names: Option<ITfInputProcessorProfiles> = mgr.cast().ok();
        let Ok(en) = mgr.EnumProfiles(lang) else {
            return out;
        };
        loop {
            let mut p = [TF_INPUTPROCESSORPROFILE::default()];
            let mut n = 0u32;
            if en.Next(&mut p, &mut n).is_err() || n == 0 {
                break;
            }
            let p = p[0];
            let tip = p.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR;
            let name = if tip {
                names
                    .as_ref()
                    .and_then(|n| {
                        n.GetLanguageProfileDescription(&p.clsid, p.langid, &p.guidProfile)
                            .ok()
                    })
                    .map(|b| b.to_string())
                    .unwrap_or_default()
            } else {
                format!("{:08X}", p.hkl.0 as usize as u32)
            };
            let high = ((p.hkl.0 as usize) >> 16) as u16;
            // HKLs with a 0xFxxx high word are layout variants whose KLID lives in the registry.
            let klid = (!tip && high & 0xF000 == 0).then(|| format!("{high:08X}"));
            out.push(Profile {
                lang: p.langid,
                tip,
                enabled: p.dwFlags & TF_IPP_FLAG_ENABLED != 0,
                name,
                hkl: p.hkl,
                klid,
            });
        }
    }
    out
}
