//! # t3a-ui — popup model and renderer (docs/05-ux-spec.md §3–4, docs/02 §9)
//!
//! The model below is portable and is the ONLY interface between the TIP and the UI. The Windows
//! renderer (M1: GDI minimal; M4: Direct2D/DirectWrite per spec; M8: GDI fallback) lives in `win/`.

/// Row marker shown at the left end of a row (docs/05 §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowMarker {
    None,
    Completion,
    Custom,
    RawLatin,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub text: String,
    pub marker: RowMarker,
    /// Raw-Latin rows render LTR in the Latin font.
    pub rtl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Footer {
    Hidden,
    Hints,
    Paging { page: u8, pages: u8 },
}

/// Candidate list state (docs/05 §3).
#[derive(Clone, Debug, PartialEq)]
pub struct ListModel {
    /// Latin buffer shown in the header (LTR).
    pub latin: String,
    /// Arabic dialect badge text, e.g. "شامي"; None hides the badge.
    pub badge: Option<String>,
    /// Rows of the visible page; the raw-Latin row is always last.
    pub rows: Vec<Row>,
    pub highlighted: usize,
    pub footer: Footer,
}

/// Tashkeel editor state (docs/05 §4).
#[derive(Clone, Debug, PartialEq)]
pub struct TashkeelModel {
    /// Quick picks (vocalized strings); `from_typing` marks the first one if vowel-derived.
    pub picks: Vec<String>,
    pub from_typing: bool,
    pub highlighted_pick: Option<usize>,
    /// The word being edited, with marks, in logical order.
    pub word: String,
    /// Index (in base letters, logical order) of the focused letter.
    pub focused_letter: usize,
    /// Selected letters (logical order); the focused letter is always among them.
    pub selected: Vec<usize>,
}

/// Mouse input on the popup, delivered to the TIP's handler (docs/05 §3.4, §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupEvent {
    /// Click on visible candidate row `n` (0 = top row of the current page).
    Row(usize),
    /// Mouse wheel over the list: previous / next candidate.
    WheelUp,
    WheelDown,
    /// Click on letter `index` (logical order) in the tashkeel editor. `toggle` = Ctrl held,
    /// `range` = Shift held or a drag.
    Letter {
        index: usize,
        toggle: bool,
        range: bool,
    },
    /// Click on palette cell `n` of `MARK_PALETTE`.
    Mark(usize),
    /// Click on the "clear all diacritics" button.
    ClearAll,
    /// Click on quick-pick chip `n`.
    Pick(usize),
}

/// Label of the "clear all diacritics" button at the top of the tashkeel editor (drawn with ✕).
pub const CLEAR_ALL_AR: &str = "مسح الكل";

/// Footer hints of the tashkeel editor: (key, Arabic label), laid out right-to-left as keycaps.
pub const TASHKEEL_HINTS: [(&str, &str); 4] = [
    ("\u{2190} \u{2192}", "حرف"),
    ("Shift+\u{2190} \u{2192}", "تحديد"),
    ("Enter", "إدراج"),
    ("Esc", "رجوع"),
];

#[derive(Clone, Debug, PartialEq)]
pub enum PopupModel {
    Hidden,
    List(ListModel),
    Tashkeel(TashkeelModel),
}

/// Theme tokens (docs/05 §3.3). Colors are 0xRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub bg: u32,
    pub border: u32,
    pub text: u32,
    pub secondary: u32,
    pub accent: u32,
}

impl Theme {
    pub const LIGHT: Theme = Theme {
        bg: 0xFFFFFF,
        border: 0xD0D0D0,
        text: 0x1A1A1A,
        secondary: 0x666666,
        accent: 0x0067C0,
    };
    pub const DARK: Theme = Theme {
        bg: 0x2B2B2B,
        border: 0x3F3F3F,
        text: 0xF2F2F2,
        secondary: 0xA0A0A0,
        accent: 0x4CC2FF,
    };
}

/// Metrics at 96 DPI (docs/05 §3.2); multiply by `dpi / 96`.
pub mod metrics {
    pub const MIN_WIDTH: f32 = 180.0;
    pub const MAX_WIDTH: f32 = 420.0;
    pub const HEADER_H: f32 = 22.0;
    pub const ROW_H: f32 = 34.0;
    pub const FOOTER_H: f32 = 24.0;
    pub const PAD_X: f32 = 12.0;
    pub const HARAKAT_BTN: f32 = 28.0;
    pub const ACCENT_BAR: f32 = 3.0;
    pub const TASHKEEL_WORD_SIZE: f32 = 40.0;
    pub fn scale(v: f32, dpi: u32) -> f32 {
        v * dpi as f32 / 96.0
    }
}

/// Mark palette of the tashkeel editor, in RTL display order: (mark, key hint, Arabic name).
pub const MARK_PALETTE: [(char, &str, &str); 10] = [
    ('\u{064E}', "a", "فتحة"),
    ('\u{064F}', "u", "ضمة"),
    ('\u{0650}', "i", "كسرة"),
    ('\u{0652}', "o", "سكون"),
    ('\u{0651}', "w", "شدة"),
    ('\u{064B}', "A", "تنوين فتح"),
    ('\u{064C}', "U", "تنوين ضم"),
    ('\u{064D}', "I", "تنوين كسر"),
    ('\u{0670}', "^", "ألف خنجرية"),
    ('\u{2715}', "x", "مسح"),
];

/// Footer hints of the candidate list (docs/05 §3.1): (key, Arabic label), laid out right-to-left.
/// Keys follow the default `[keys]` config (docs/13).
pub const LIST_HINTS: [(&str, &str); 3] = [
    ("Space", "إدراج"),
    ("Tab", "تشكيل"),
    ("Shift+Space", "لاتيني"),
];

#[cfg(windows)]
pub mod win;

#[cfg(windows)]
pub use win::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_keys_match_router() {
        let keys: Vec<&str> = MARK_PALETTE.iter().map(|m| m.1).collect();
        assert_eq!(keys, ["a", "u", "i", "o", "w", "A", "U", "I", "^", "x"]);
        assert_eq!(metrics::scale(34.0, 144), 51.0);
    }
}
