//! Hotkey spec parsing shared by the companion and the Settings app (docs/13 `general.global_hotkey`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Windows virtual-key code of the main key.
    pub vk: u16,
}

/// Parse `"Ctrl+Alt+A"`, `"Win+Shift+Space"`, `"Ctrl+Alt+F9"`… Modifiers are case-insensitive; at least one
/// modifier is required (bare keys would hijack typing).
pub fn parse(s: &str) -> Result<HotkeySpec, String> {
    let mut h = HotkeySpec {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        vk: 0,
    };
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last().ok_or("empty hotkey")?;
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => h.ctrl = true,
            "alt" => h.alt = true,
            "shift" => h.shift = true,
            "win" | "windows" => h.win = true,
            other => return Err(format!("unknown modifier '{other}'")),
        }
    }
    if !(h.ctrl || h.alt || h.win) {
        return Err("hotkey needs Ctrl, Alt or Win".into());
    }
    h.vk = vk_of(key).ok_or_else(|| format!("unknown key '{key}'"))?;
    Ok(h)
}

fn vk_of(key: &str) -> Option<u16> {
    let k = key.to_ascii_uppercase();
    let b = k.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
        return Some(b[0] as u16); // VK_A..VK_Z, VK_0..VK_9 equal their ASCII codes
    }
    if let Some(n) = k.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    Some(match k.as_str() {
        "SPACE" => 0x20,
        "TAB" => 0x09,
        "ENTER" => 0x0D,
        "`" | "BACKQUOTE" => 0xC0,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_defaults_and_rejects_bare_keys() {
        let h = parse("Ctrl+Alt+A").unwrap();
        assert!(h.ctrl && h.alt && !h.shift && !h.win && h.vk == 0x41);
        assert_eq!(parse("win+shift+space").unwrap().vk, 0x20);
        assert_eq!(parse("Ctrl+F9").unwrap().vk, 0x78);
        assert!(parse("Shift+A").is_err());
        assert!(parse("A").is_err());
        assert!(parse("Ctrl+Hyper+A").is_err());
    }
}
