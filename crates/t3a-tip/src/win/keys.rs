//! Keystroke translation from Windows virtual keys and scan codes into `Key` and `Mods` (docs/02 §6).
//!
//! While Type3arabi is active the thread's keyboard layout is Arabic, so the VK / character the
//! system derives from a key press is the Arabic 101 one. We re-derive the character from the
//! physical scan code through the user's Latin layout (first non-RTL HKL in the loaded list), or
//! through a built-in US table when no Latin layout is loaded. `LoadKeyboardLayout` is never called.

use crate::keyrouter::{Key, Mods};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayoutList, GetKeyboardState, MapVirtualKeyExW, ToUnicodeEx, HKL,
    MAPVK_VSC_TO_VK_EX, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE,
    VK_HOME, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LWIN, VK_MENU, VK_NEXT, VK_NUMPAD0, VK_NUMPAD9,
    VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB,
    VK_UP,
};

const LANG_ARABIC: u16 = 0x01;
const LANG_HEBREW: u16 = 0x0D;
const LANG_URDU: u16 = 0x20;
const LANG_FARSI: u16 = 0x29;

fn down(vk: u16) -> bool {
    // SAFETY: GetKeyState has no preconditions.
    (unsafe { GetKeyState(vk as i32) } as u16 & 0x8000) != 0
}

pub fn read_mods() -> Mods {
    let shift = down(VK_SHIFT.0);
    let ctrl = down(VK_CONTROL.0);
    let alt = down(VK_MENU.0);
    let win = down(VK_LWIN.0) || down(VK_RWIN.0);
    // Right Alt on AltGr layouts arrives as Ctrl+Alt; treat it as a character modifier.
    let altgr = down(VK_RMENU.0) && ctrl;
    Mods {
        shift,
        ctrl: ctrl && !altgr,
        alt: alt && !altgr,
        win,
        altgr,
    }
}

/// The user's Latin layout: first loaded HKL whose primary language is not RTL. `None` ⇒ US table.
fn latin_layout() -> Option<HKL> {
    let mut list = [HKL::default(); 32];
    // SAFETY: the slice is a valid writable buffer.
    let n = unsafe { GetKeyboardLayoutList(Some(&mut list)) } as usize;
    list[..n.min(list.len())].iter().copied().find(|hkl| {
        let primary = (hkl.0 as usize as u16) & 0x3FF;
        !matches!(primary, LANG_ARABIC | LANG_HEBREW | LANG_URDU | LANG_FARSI)
    })
}

/// US QWERTY by scan code: (scan, normal, shifted). Used when no Latin layout is loaded.
const US_FALLBACK: [(u8, char, char); 47] = [
    (0x02, '1', '!'),
    (0x03, '2', '@'),
    (0x04, '3', '#'),
    (0x05, '4', '$'),
    (0x06, '5', '%'),
    (0x07, '6', '^'),
    (0x08, '7', '&'),
    (0x09, '8', '*'),
    (0x0A, '9', '('),
    (0x0B, '0', ')'),
    (0x0C, '-', '_'),
    (0x0D, '=', '+'),
    (0x10, 'q', 'Q'),
    (0x11, 'w', 'W'),
    (0x12, 'e', 'E'),
    (0x13, 'r', 'R'),
    (0x14, 't', 'T'),
    (0x15, 'y', 'Y'),
    (0x16, 'u', 'U'),
    (0x17, 'i', 'I'),
    (0x18, 'o', 'O'),
    (0x19, 'p', 'P'),
    (0x1A, '[', '{'),
    (0x1B, ']', '}'),
    (0x1E, 'a', 'A'),
    (0x1F, 's', 'S'),
    (0x20, 'd', 'D'),
    (0x21, 'f', 'F'),
    (0x22, 'g', 'G'),
    (0x23, 'h', 'H'),
    (0x24, 'j', 'J'),
    (0x25, 'k', 'K'),
    (0x26, 'l', 'L'),
    (0x27, ';', ':'),
    (0x28, '\'', '"'),
    (0x29, '`', '~'),
    (0x2B, '\\', '|'),
    (0x2C, 'z', 'Z'),
    (0x2D, 'x', 'X'),
    (0x2E, 'c', 'C'),
    (0x2F, 'v', 'V'),
    (0x30, 'b', 'B'),
    (0x31, 'n', 'N'),
    (0x32, 'm', 'M'),
    (0x33, ',', '<'),
    (0x34, '.', '>'),
    (0x35, '/', '?'),
];

fn us_fallback(scan: u32, shift: bool, caps: bool) -> Option<char> {
    let (_, normal, shifted) = US_FALLBACK.iter().find(|e| e.0 as u32 == scan)?;
    let upper = if normal.is_ascii_alphabetic() {
        shift ^ caps
    } else {
        shift
    };
    Some(if upper { *shifted } else { *normal })
}

fn latin_char(scan: u32, extended: bool, mods: Mods) -> Option<char> {
    if extended {
        return None; // numpad '/', arrows, etc. have no Latin character we route
    }
    let caps = (unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1) != 0;
    let Some(hkl) = latin_layout() else {
        return us_fallback(scan, mods.shift, caps);
    };
    // SAFETY: plain Win32 queries with valid buffers; flag 0x4 = do not change kernel keyboard
    // state (dead keys), so the host's own translation is unaffected.
    unsafe {
        let vk = MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, Some(hkl));
        if vk == 0 {
            return us_fallback(scan, mods.shift, caps);
        }
        let mut state = [0u8; 256];
        let _ = GetKeyboardState(&mut state);
        if !mods.altgr {
            for k in [
                VK_CONTROL,
                VK_LCONTROL,
                VK_RCONTROL,
                VK_MENU,
                VK_LMENU,
                VK_RMENU,
            ] {
                state[k.0 as usize] = 0;
            }
        }
        let mut buf = [0u16; 4];
        let n = ToUnicodeEx(vk, scan, &state, &mut buf, 0x4, Some(hkl));
        if n == 1 {
            char::from_u32(buf[0] as u32).filter(|c| !c.is_control())
        } else {
            None
        }
    }
}

pub fn translate_key(wparam: WPARAM, lparam: LPARAM) -> (Key, Mods) {
    let mods = read_mods();
    let vk = wparam.0 as u16;
    let scan = ((lparam.0 >> 16) & 0xFF) as u32;
    let extended = ((lparam.0 >> 24) & 1) != 0;

    let key = match vk {
        // Shift/Ctrl/Alt (+L/R variants 0xA0..=0xA5), Caps Lock, Win keys, Num/Scroll Lock.
        0x10 | 0x11 | 0x12 | 0x14 | 0x5B | 0x5C | 0x90 | 0x91 | 0xA0..=0xA5 => Key::Modifier,
        v if v == VK_SPACE.0 => Key::Space,
        v if v == VK_RETURN.0 => Key::Enter,
        v if v == VK_ESCAPE.0 => Key::Escape,
        v if v == VK_BACK.0 => Key::Backspace,
        v if v == VK_TAB.0 => Key::Tab,
        v if v == VK_UP.0 => Key::Up,
        v if v == VK_DOWN.0 => Key::Down,
        v if v == VK_LEFT.0 => Key::Left,
        v if v == VK_RIGHT.0 => Key::Right,
        v if v == VK_PRIOR.0 => Key::PageUp,
        v if v == VK_NEXT.0 => Key::PageDown,
        v if v == VK_HOME.0 => Key::Home,
        v if v == VK_END.0 => Key::End,
        v if v == VK_DELETE.0 => Key::Delete,
        v if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&v) => {
            Key::NumpadDigit((b'0' + (v - VK_NUMPAD0.0) as u8) as char)
        }
        _ => match latin_char(scan, extended, mods) {
            Some(c) => Key::Char(c),
            None => Key::Other,
        },
    };
    (key, mods)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_fallback_letters_digits_and_caps() {
        assert_eq!(us_fallback(0x32, false, false), Some('m')); // m
        assert_eq!(us_fallback(0x32, true, false), Some('M'));
        assert_eq!(us_fallback(0x32, false, true), Some('M'));
        assert_eq!(us_fallback(0x32, true, true), Some('m'));
        assert_eq!(us_fallback(0x04, false, true), Some('3')); // caps does not shift digits
        assert_eq!(us_fallback(0x08, false, false), Some('7'));
        assert_eq!(us_fallback(0x28, false, false), Some('\''));
        assert_eq!(us_fallback(0x3B, false, false), None); // F1
    }
}
