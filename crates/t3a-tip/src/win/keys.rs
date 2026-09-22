//! Keystroke translation from Windows virtual keys and scan codes into `Key` and `Mods` (docs/02 §6).

use crate::keyrouter::{Key, Mods};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardState, ToUnicode, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_HOME, VK_LEFT, VK_MENU, VK_NEXT, VK_NUMPAD0, VK_NUMPAD9, VK_PRIOR, VK_RETURN,
    VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

pub fn read_mods() -> Mods {
    unsafe {
        let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
        let ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
        let alt = (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
        let altgr = ctrl && alt;
        Mods {
            shift,
            ctrl: ctrl && !altgr,
            alt: alt && !altgr,
            win: false,
            altgr,
        }
    }
}

pub fn translate_key(wparam: WPARAM, lparam: LPARAM) -> (Key, Mods) {
    let mods = read_mods();
    let vk = wparam.0 as u16;

    let key = match vk {
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
            let digit = (b'0' + (v - VK_NUMPAD0.0) as u8) as char;
            Key::NumpadDigit(digit)
        }
        _ => {
            let scan_code = ((lparam.0 >> 16) & 0xFF) as u32;
            let mut keyboard_state = [0u8; 256];
            let _ = unsafe { GetKeyboardState(&mut keyboard_state) };
            let mut buf = [0u16; 4];
            let count =
                unsafe { ToUnicode(vk as u32, scan_code, Some(&keyboard_state), &mut buf, 0) };
            if count > 0 {
                if let Some(ch) = char::from_u32(buf[0] as u32) {
                    if !ch.is_control() {
                        Key::Char(ch)
                    } else {
                        Key::Other
                    }
                } else {
                    Key::Other
                }
            } else if (b'A' as u16..=b'Z' as u16).contains(&vk) {
                let base = if mods.shift {
                    (vk as u8) as char
                } else {
                    ((vk as u8) + 32) as char
                };
                Key::Char(base)
            } else if (b'0' as u16..=b'9' as u16).contains(&vk) && !mods.shift {
                Key::Char((vk as u8) as char)
            } else {
                Key::Other
            }
        }
    };

    (key, mods)
}
