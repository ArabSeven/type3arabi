//! Hotkey spec parsing shared by the companion and the Settings app (docs/13 `general.global_hotkey`),
//! and the companion's keyboard-list rules (`stray_arabic_layout`, `arabic_hotkey_target`).
//! `profile` (Windows): enabling the keyboard for the signed-in user, used by the companion and by
//! the Settings app.

#[cfg(windows)]
pub mod profile;

/// Should the companion drop this ar-SA keyboard layout ("0401:<KLID>")? Only an *Arabic* layout
/// (KLID ending in 0401, e.g. Arabic 101 = "0401:00000401") that the user did not keep. Windows'
/// hidden US layout under Arabic ("0401:00000409"), which the TIP runs on, is never a stray.
pub fn stray_arabic_layout(layout: &str, keep: &[String]) -> bool {
    layout.len() == 13
        && layout[..5].eq_ignore_ascii_case("0401:")
        && layout.ends_with("0401")
        && !keep.iter().any(|k| k.eq_ignore_ascii_case(layout))
}

/// The Arabic keyboard the hotkey switches to, from the session's loaded HKLs (raw values). Type3arabi
/// runs on Windows' hidden non-Arabic base layout under Arabic (e.g. 0x04090401), so that one wins
/// over a real Arabic layout (0x04010401 = Arabic 101) whatever the load order.
pub fn arabic_hotkey_target(loaded: &[usize]) -> Option<usize> {
    const ARABIC: usize = 0x01;
    let arabic = || loaded.iter().copied().filter(|h| h & 0x3FF == ARABIC);
    arabic()
        .find(|h| (h >> 16) & 0x3FF != ARABIC)
        .or_else(|| arabic().next())
}

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

    // Regression (Owner 2026-09-25): Arabic 101 back in Win+Space after a reinstall and restart.
    #[test]
    fn only_unchosen_arabic_layouts_are_strays() {
        let tip =
            "0401:{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}{90D49398-54D3-4F08-9C15-0B38D0820A87}";
        let only_tip = vec![tip.to_string()];
        assert!(stray_arabic_layout("0401:00000401", &only_tip));
        assert!(!stray_arabic_layout("0401:00000409", &only_tip)); // hidden base layout
        let with_101 = vec![tip.to_string(), "0401:00000401".to_string()];
        assert!(!stray_arabic_layout("0401:00000401", &with_101)); // the user's own choice
        assert!(!stray_arabic_layout(tip, &[])); // never a TIP
        assert!(!stray_arabic_layout("0409:00000401", &[])); // other languages untouched
    }
    // Regression (Owner 2026-09-26): with a stray Arabic 101 loaded first, the hotkey picked it.
    #[test]
    fn hotkey_prefers_the_type3arabi_base_layout() {
        let (us, ar101, ar_on_us) = (0x0409_0409, 0x0401_0401, 0x0409_0401);
        assert_eq!(arabic_hotkey_target(&[us, ar101, ar_on_us]), Some(ar_on_us));
        assert_eq!(arabic_hotkey_target(&[us, ar101]), Some(ar101)); // only Arabic 101: still Arabic
        assert_eq!(arabic_hotkey_target(&[us]), None);
    }
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
