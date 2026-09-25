//! Pure key routing (docs/02 §5). No Windows types: the TIP converts `(vk, lParam, keyboard state)`
//! into `Key` + `Mods` via the Latin-layout translation (docs/02 §6) and then calls `classify`.
//! `OnTestKeyDown` and `OnKeyDown` MUST call this with identical inputs (docs/02 §5.1).

use t3a_engine::normalize::is_token_char;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable character produced by the Latin layout.
    Char(char),
    /// Numpad digit (always a literal number).
    NumpadDigit(char),
    Backspace,
    Space,
    Enter,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
    /// A modifier pressed on its own (Shift, Ctrl, Alt, Win, Caps Lock): never eaten, never commits.
    Modifier,
    /// Anything else (F-keys, media keys, …).
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Ctrl+Alt that produced a character (AltGr): treated as a plain printable key.
    pub altgr: bool,
}

impl Mods {
    fn command(&self) -> bool {
        !self.altgr && (self.ctrl || self.alt || self.win)
    }
}

/// Context mode from docs/02 §7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMode {
    Off,
    Latin,
    Arabic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popup {
    Hidden,
    List,
    Tashkeel,
}

/// A key chord for the mode toggle (docs/13 `general.mode_toggle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    CtrlSpace,
    ShiftSpace,
    CtrlShiftSpace,
    /// Handled on key-up by the TIP (not a chord).
    ShiftTap,
    None,
}

impl Toggle {
    pub fn parse(s: &str) -> Toggle {
        match s {
            "Ctrl+Space" => Toggle::CtrlSpace,
            "Shift+Space" => Toggle::ShiftSpace,
            "Ctrl+Shift+Space" => Toggle::CtrlShiftSpace,
            "ShiftTap" => Toggle::ShiftTap,
            "none" => Toggle::None,
            _ => Toggle::CtrlSpace,
        }
    }
    fn matches(self, key: Key, m: Mods) -> bool {
        key == Key::Space
            && !m.alt
            && !m.win
            && match self {
                Toggle::CtrlSpace => m.ctrl && !m.shift,
                Toggle::ShiftSpace => m.shift && !m.ctrl,
                Toggle::CtrlShiftSpace => m.ctrl && m.shift,
                Toggle::ShiftTap | Toggle::None => false,
            }
    }
}

/// Shift tapped on its own toggles Arabic/Latin (`general.mode_toggle = "ShiftTap"`): Shift pressed,
/// no other key before it is released, released within `SHIFT_TAP_MS`. Holding Shift for a capital,
/// Shift+click or a long hold never toggles.
pub const SHIFT_TAP_MS: u32 = 300; // docs/02 §10

/// Track a Shift tap. `down_at`: tick of the pending Shift press (None = not armed).
pub fn shift_tap(down_at: &mut Option<u32>, is_shift: bool, key_down: bool, now: u32) -> bool {
    match (is_shift, key_down) {
        (true, true) => {
            // Auto-repeat keeps the first press time.
            down_at.get_or_insert(now);
            false
        }
        (true, false) => down_at
            .take()
            .is_some_and(|t| now.wrapping_sub(t) <= SHIFT_TAP_MS),
        (false, true) => {
            *down_at = None;
            false
        }
        (false, false) => false,
    }
}

/// A configurable key chord (docs/13 `[keys]`): exact modifier match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Chord {
    /// `None` for `"none"` or an unparsable string.
    pub fn parse(s: &str) -> Option<Chord> {
        if s.eq_ignore_ascii_case("none") {
            return None;
        }
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last()?;
        let key = match key.to_ascii_lowercase().as_str() {
            "space" => Key::Space,
            "enter" => Key::Enter,
            "tab" => Key::Tab,
            "esc" | "escape" => Key::Escape,
            "backspace" => Key::Backspace,
            k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => {
                Key::Char(k.chars().next()?)
            }
            _ => return None,
        };
        let mut c = Chord {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        };
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" => c.ctrl = true,
                "alt" => c.alt = true,
                "shift" => c.shift = true,
                _ => return None,
            }
        }
        Some(c)
    }

    fn matches(self, key: Key, m: Mods) -> bool {
        let key_eq = match (self.key, key) {
            (Key::Char(a), Key::Char(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        };
        key_eq && self.ctrl == m.ctrl && self.alt == m.alt && self.shift == m.shift && !m.win
    }
}

/// Configurable in-composition shortcuts (docs/13 `[keys]`); `None` = unbound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyMap {
    /// Commit the typed Latin as is, plus a space (default Shift+Space).
    pub commit_latin: Option<Chord>,
    /// Open the tashkeel editor on the highlighted candidate (default Tab).
    pub open_tashkeel: Option<Chord>,
    /// Commit with harakat derived from the typed vowels (default Ctrl+Enter).
    pub commit_harakat: Option<Chord>,
}

impl KeyMap {
    pub fn from_config(commit_latin: &str, open_tashkeel: &str, commit_harakat: &str) -> Self {
        Self {
            commit_latin: Chord::parse(commit_latin),
            open_tashkeel: Chord::parse(open_tashkeel),
            commit_harakat: Chord::parse(commit_harakat),
        }
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::from_config("Shift+Space", "Tab", "Ctrl+Enter")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouterState {
    pub context: ContextMode,
    pub composing: bool,
    pub popup: Popup,
    /// A re-edit anchor exists (docs/02 §12.2).
    pub reedit_anchor: bool,
    /// The current buffer is an article (`el`, `al`, `il`, `l`) — a following `-` is swallowed.
    pub buffer_is_article: bool,
    pub toggle: Toggle,
    pub keys: KeyMap,
}

/// Commands inside the tashkeel editor (docs/05 §4.3).
pub use t3a_engine::tashkeel::TashkeelCmd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pass,
    AppendChar(char),
    AppendLiteralDigit(char),
    InsertLatin(char),
    InsertPunctuation(char),
    Backspace,
    ReEdit,
    ArticleHyphen,
    CommitSpace,
    CommitNoSpace,
    CommitWithHarakat,
    CommitRaw,
    /// Commit the typed Latin as is, plus a space (keys.commit_latin).
    CommitRawSpace,
    CommitThenPunctuation(char),
    /// Diacritics editor open and a letter/digit that is not an editor command was typed: insert the
    /// edited word and start a new word with that character (it is never silently dropped).
    CommitThenType(char),
    CommitAndReinject,
    OpenTashkeel,
    NextCandidate,
    PrevCandidate,
    NextPage,
    PrevPage,
    ToggleMode,
    CommitThenToggle,
    Tashkeel(TashkeelCmd),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub eat: bool,
    pub action: Action,
}

const fn eat(action: Action) -> Decision {
    Decision { eat: true, action }
}
const PASS: Decision = Decision {
    eat: false,
    action: Action::Pass,
};

/// The routing table of docs/02 §5.2.
pub fn classify(s: &RouterState, key: Key, m: Mods) -> Decision {
    if s.context == ContextMode::Off || key == Key::Modifier {
        return PASS;
    }
    if s.toggle.matches(key, m) {
        return eat(if s.composing {
            Action::CommitThenToggle
        } else {
            Action::ToggleMode
        });
    }
    if s.context == ContextMode::Latin {
        return match key {
            Key::Char(c) if !m.command() => eat(Action::InsertLatin(c)),
            Key::NumpadDigit(d) if !m.command() => eat(Action::InsertLatin(d)),
            _ => PASS,
        };
    }
    // Arabic mode
    let bound = |c: Option<Chord>| c.is_some_and(|c| c.matches(key, m));
    if s.composing && s.popup == Popup::Tashkeel {
        return eat(match key {
            _ if bound(s.keys.commit_latin) => Action::CommitRawSpace,
            // The key that opened the editor closes it again (Owner, 2026-09-25): back to the list.
            _ if bound(s.keys.open_tashkeel) => Action::Tashkeel(TashkeelCmd::Back),
            _ if m.command() && key != Key::Enter => Action::CommitAndReinject,
            Key::Char(c) => match c {
                'a' => Action::Tashkeel(TashkeelCmd::Fatha),
                'u' => Action::Tashkeel(TashkeelCmd::Damma),
                'i' => Action::Tashkeel(TashkeelCmd::Kasra),
                'o' => Action::Tashkeel(TashkeelCmd::Sukun),
                'w' => Action::Tashkeel(TashkeelCmd::ShaddaToggle),
                'A' => Action::Tashkeel(TashkeelCmd::Fathatan),
                'U' => Action::Tashkeel(TashkeelCmd::Dammatan),
                'I' => Action::Tashkeel(TashkeelCmd::Kasratan),
                '^' => Action::Tashkeel(TashkeelCmd::DaggerAlif),
                'x' | 'X' => Action::Tashkeel(TashkeelCmd::Clear),
                '1'..='8' => Action::Tashkeel(TashkeelCmd::QuickPick(c as u8 - b'0')),
                c if is_token_char(c) => Action::CommitThenType(c),
                c => Action::CommitThenPunctuation(c),
            },
            Key::NumpadDigit(_) => Action::Tashkeel(TashkeelCmd::Ignore),
            Key::Backspace => Action::Tashkeel(TashkeelCmd::ClearOrBack),
            Key::Delete => Action::Tashkeel(TashkeelCmd::Clear),
            // visual left = logical next (RTL); Shift extends the selection
            Key::Left if m.shift => Action::Tashkeel(TashkeelCmd::ExtendNext),
            Key::Right if m.shift => Action::Tashkeel(TashkeelCmd::ExtendPrev),
            Key::Left => Action::Tashkeel(TashkeelCmd::LetterNext),
            Key::Right => Action::Tashkeel(TashkeelCmd::LetterPrev),
            Key::Home => Action::Tashkeel(TashkeelCmd::LetterFirst),
            Key::End => Action::Tashkeel(TashkeelCmd::LetterLast),
            Key::Up => Action::Tashkeel(TashkeelCmd::PickUp),
            Key::Down => Action::Tashkeel(TashkeelCmd::PickDown),
            Key::Tab if m.shift => Action::Tashkeel(TashkeelCmd::PickUp),
            Key::Tab => Action::Tashkeel(TashkeelCmd::PickDown),
            Key::Enter => Action::CommitNoSpace,
            Key::Space => Action::CommitSpace,
            Key::Escape => Action::Tashkeel(TashkeelCmd::Back),
            Key::PageUp | Key::PageDown => Action::Tashkeel(TashkeelCmd::Ignore),
            Key::Other | Key::Modifier => Action::CommitAndReinject,
        });
    }
    if s.composing {
        return eat(match key {
            _ if bound(s.keys.commit_harakat) => Action::CommitWithHarakat,
            _ if bound(s.keys.commit_latin) => Action::CommitRawSpace,
            _ if bound(s.keys.open_tashkeel) => Action::OpenTashkeel,
            _ if m.command() => Action::CommitAndReinject,
            Key::Char('-') if s.buffer_is_article => Action::ArticleHyphen,
            Key::Char(c) if is_token_char(c) => Action::AppendChar(c),
            Key::Char(c) => Action::CommitThenPunctuation(c),
            Key::NumpadDigit(d) => Action::AppendLiteralDigit(d),
            Key::Backspace => Action::Backspace,
            Key::Space => Action::CommitSpace,
            Key::Enter => Action::CommitNoSpace,
            Key::Tab if m.shift => Action::PrevCandidate,
            Key::Tab => Action::CommitAndReinject, // Tab rebound elsewhere: behaves like any other key
            Key::Down => Action::NextCandidate,
            Key::Up => Action::PrevCandidate,
            Key::PageDown => Action::NextPage,
            Key::PageUp => Action::PrevPage,
            Key::Escape => Action::CommitRaw,
            Key::Left
            | Key::Right
            | Key::Home
            | Key::End
            | Key::Delete
            | Key::Other
            | Key::Modifier => Action::CommitAndReinject,
        });
    }
    // Idle
    match key {
        Key::Char(c) if !m.command() && is_token_char(c) => eat(Action::AppendChar(c)),
        Key::Char(c) if !m.command() => eat(Action::InsertPunctuation(c)),
        Key::NumpadDigit(d) if !m.command() => eat(Action::AppendLiteralDigit(d)),
        Key::Backspace if s.reedit_anchor && !m.command() => eat(Action::ReEdit),
        _ => PASS,
    }
}

/// What a text field's input scopes (docs/02 §7) allow. Values are the Windows `InputScope`
/// numbers (inputscope.h), so this stays portable and testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeClass {
    /// Normal text: Arabic or Latin per the mode toggle, learning allowed.
    Normal,
    /// Passwords, PINs, numbers, phone numbers, dates, times, amounts; URL and e-mail fields when
    /// `typing.latin_in_url_email` is on: Latin characters only, no transliteration, no learning.
    Latin,
    /// `IS_PRIVATE` (InPrivate/incognito): Arabic as usual, but nothing is learned (R9).
    NoLearning,
}

/// docs/02 §7, AGENTS.md R9.
pub fn classify_scopes(scopes: &[i32], latin_in_url_email: bool) -> ScopeClass {
    const LATIN: &[i32] = &[
        31, // IS_PASSWORD
        63, // IS_NUMERIC_PASSWORD
        64, // IS_NUMERIC_PIN
        65, // IS_ALPHANUMERIC_PIN
        66, // IS_ALPHANUMERIC_PIN_SET
        28, // IS_DIGITS
        29, // IS_NUMBER
        39, // IS_NUMBER_FULLWIDTH
        32, // IS_TELEPHONE_FULLTELEPHONENUMBER
        33, // IS_TELEPHONE_COUNTRYCODE
        34, // IS_TELEPHONE_AREACODE
        35, // IS_TELEPHONE_LOCALNUMBER
        20, // IS_CURRENCY_AMOUNTANDSYMBOL
        21, // IS_CURRENCY_AMOUNT
        22, 23, 24, 25, 26, 27, // IS_DATE_FULLDATE, _MONTH, _DAY, _YEAR, _MONTHNAME, _DAYNAME
        36, 37, 38, // IS_TIME_FULLTIME, _HOUR, _MINORSEC
    ];
    const URL_EMAIL: &[i32] = &[
        1,  // IS_URL
        4,  // IS_EMAIL_USERNAME
        5,  // IS_EMAIL_SMTPEMAILADDRESS
        6,  // IS_LOGINNAME
        60, // IS_EMAILNAME_OR_ADDRESS
    ];
    const IS_PRIVATE: i32 = 61;
    if scopes.iter().any(|s| LATIN.contains(s))
        || (latin_in_url_email && scopes.iter().any(|s| URL_EMAIL.contains(s)))
    {
        ScopeClass::Latin
    } else if scopes.contains(&IS_PRIVATE) {
        ScopeClass::NoLearning
    } else {
        ScopeClass::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(context: ContextMode, composing: bool, popup: Popup) -> RouterState {
        RouterState {
            context,
            composing,
            popup,
            reedit_anchor: false,
            buffer_is_article: false,
            toggle: Toggle::CtrlSpace,
            keys: KeyMap::default(),
        }
    }
    const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        altgr: false,
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        alt: false,
        shift: false,
        win: false,
        altgr: false,
    };
    const SHIFT: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: true,
        win: false,
        altgr: false,
    };

    #[test]
    fn idle_row_of_docs_table() {
        let s = st(ContextMode::Arabic, false, Popup::Hidden);
        assert_eq!(
            classify(&s, Key::Char('7'), NONE),
            eat(Action::AppendChar('7'))
        );
        assert_eq!(
            classify(&s, Key::Char('\''), NONE),
            eat(Action::AppendChar('\''))
        );
        assert_eq!(
            classify(&s, Key::NumpadDigit('3'), NONE),
            eat(Action::AppendLiteralDigit('3'))
        );
        assert_eq!(
            classify(&s, Key::Char('?'), NONE),
            eat(Action::InsertPunctuation('?'))
        );
        assert_eq!(classify(&s, Key::Space, NONE), PASS);
        assert_eq!(classify(&s, Key::Enter, NONE), PASS);
        assert_eq!(classify(&s, Key::Backspace, NONE), PASS);
        assert_eq!(classify(&s, Key::Char('c'), CTRL), PASS); // Ctrl+C untouched
        let s2 = RouterState {
            reedit_anchor: true,
            ..s
        };
        assert_eq!(classify(&s2, Key::Backspace, NONE), eat(Action::ReEdit));
    }

    #[test]
    fn composing_rows() {
        let s = st(ContextMode::Arabic, true, Popup::List);
        assert_eq!(classify(&s, Key::Space, NONE), eat(Action::CommitSpace));
        assert_eq!(classify(&s, Key::Enter, NONE), eat(Action::CommitNoSpace));
        assert_eq!(
            classify(&s, Key::Enter, CTRL),
            eat(Action::CommitWithHarakat)
        );
        assert_eq!(classify(&s, Key::Tab, NONE), eat(Action::OpenTashkeel));
        assert_eq!(classify(&s, Key::Tab, SHIFT), eat(Action::PrevCandidate));
        assert_eq!(classify(&s, Key::Escape, NONE), eat(Action::CommitRaw));
        assert_eq!(
            classify(&s, Key::Left, NONE),
            eat(Action::CommitAndReinject)
        );
        assert_eq!(
            classify(&s, Key::Char('s'), CTRL),
            eat(Action::CommitAndReinject)
        );
        assert_eq!(
            classify(&s, Key::Char(','), NONE),
            eat(Action::CommitThenPunctuation(','))
        );
        assert_eq!(
            classify(&s, Key::Space, CTRL),
            eat(Action::CommitThenToggle)
        );
        let art = RouterState {
            buffer_is_article: true,
            ..s
        };
        assert_eq!(
            classify(&art, Key::Char('-'), NONE),
            eat(Action::ArticleHyphen)
        );
    }

    #[test]
    fn tashkeel_rows() {
        let s = st(ContextMode::Arabic, true, Popup::Tashkeel);
        assert_eq!(
            classify(&s, Key::Char('w'), NONE),
            eat(Action::Tashkeel(TashkeelCmd::ShaddaToggle))
        );
        assert_eq!(
            classify(&s, Key::Char('A'), SHIFT),
            eat(Action::Tashkeel(TashkeelCmd::Fathatan))
        );
        assert_eq!(
            classify(&s, Key::Char('3'), NONE),
            eat(Action::Tashkeel(TashkeelCmd::QuickPick(3)))
        );
        assert_eq!(
            classify(&s, Key::Left, NONE),
            eat(Action::Tashkeel(TashkeelCmd::LetterNext))
        );
        assert_eq!(
            classify(&s, Key::Escape, NONE),
            eat(Action::Tashkeel(TashkeelCmd::Back))
        );
        assert_eq!(classify(&s, Key::Enter, NONE), eat(Action::CommitNoSpace));
    }

    #[test]
    fn latin_mode_and_off() {
        let s = st(ContextMode::Latin, false, Popup::Hidden);
        assert_eq!(
            classify(&s, Key::Char('a'), NONE),
            eat(Action::InsertLatin('a'))
        );
        assert_eq!(
            classify(
                &s,
                Key::Char('€'),
                Mods {
                    altgr: true,
                    ctrl: true,
                    alt: true,
                    ..NONE
                }
            ),
            eat(Action::InsertLatin('€'))
        );
        assert_eq!(classify(&s, Key::Space, CTRL), eat(Action::ToggleMode));
        assert_eq!(classify(&s, Key::Enter, NONE), PASS);
        let off = st(ContextMode::Off, false, Popup::Hidden);
        assert_eq!(classify(&off, Key::Char('a'), NONE), PASS);
        assert_eq!(classify(&off, Key::Space, CTRL), PASS);
    }

    /// Owner report 2026-09-25: Tab opened the editor but pressing it again did not leave it. The
    /// open-editor key now goes back to the list; Shift+Tab / Up / Down still cycle the vowellings.
    #[test]
    fn open_key_closes_the_tashkeel_editor() {
        let s = st(ContextMode::Arabic, true, Popup::Tashkeel);
        let back = eat(Action::Tashkeel(TashkeelCmd::Back));
        assert_eq!(classify(&s, Key::Tab, NONE), back);
        assert_eq!(
            classify(&s, Key::Tab, SHIFT),
            eat(Action::Tashkeel(TashkeelCmd::PickUp))
        );
        assert_eq!(
            classify(&s, Key::Down, NONE),
            eat(Action::Tashkeel(TashkeelCmd::PickDown))
        );
        // a custom open key closes it too, and Tab then cycles the vowellings
        let custom = RouterState {
            keys: KeyMap::from_config("Shift+Space", "Ctrl+T", "Ctrl+Enter"),
            ..s
        };
        let ctrl_t = Mods { ctrl: true, ..NONE };
        assert_eq!(classify(&custom, Key::Char('t'), ctrl_t), back);
        assert_eq!(
            classify(&custom, Key::Tab, NONE),
            eat(Action::Tashkeel(TashkeelCmd::PickDown))
        );
    }

    /// Owner request 2026-09-23: Shift+Space commits the typed Latin word without scrolling to it.
    #[test]
    fn shift_space_commits_latin() {
        let s = st(ContextMode::Arabic, true, Popup::List);
        assert_eq!(classify(&s, Key::Space, SHIFT), eat(Action::CommitRawSpace));
        let t = st(ContextMode::Arabic, true, Popup::Tashkeel);
        assert_eq!(classify(&t, Key::Space, SHIFT), eat(Action::CommitRawSpace));
        // idle Shift+Space is an ordinary space
        let idle = st(ContextMode::Arabic, false, Popup::Hidden);
        assert_eq!(classify(&idle, Key::Space, SHIFT), PASS);
    }

    #[test]
    fn shortcuts_follow_the_key_map() {
        let s = RouterState {
            keys: KeyMap::from_config("Ctrl+L", "Ctrl+T", "none"),
            ..st(ContextMode::Arabic, true, Popup::List)
        };
        let ctrl_l = Mods { ctrl: true, ..NONE };
        assert_eq!(
            classify(&s, Key::Char('l'), ctrl_l),
            eat(Action::CommitRawSpace)
        );
        assert_eq!(
            classify(&s, Key::Char('t'), ctrl_l),
            eat(Action::OpenTashkeel)
        );
        // Ctrl+Enter unbound: behaves like any Ctrl combo
        assert_eq!(
            classify(&s, Key::Enter, CTRL),
            eat(Action::CommitAndReinject)
        );
        // Tab no longer opens the editor
        assert_eq!(classify(&s, Key::Tab, NONE), eat(Action::CommitAndReinject));
        // Shift+Space unbound: commits the candidate with a space like Space
        assert_eq!(classify(&s, Key::Space, SHIFT), eat(Action::CommitSpace));
        assert_eq!(Chord::parse("Hyper+Q"), None);
    }

    #[test]
    fn shift_arrows_extend_tashkeel_selection() {
        let s = st(ContextMode::Arabic, true, Popup::Tashkeel);
        assert_eq!(
            classify(&s, Key::Left, SHIFT),
            eat(Action::Tashkeel(TashkeelCmd::ExtendNext))
        );
        assert_eq!(
            classify(&s, Key::Right, SHIFT),
            eat(Action::Tashkeel(TashkeelCmd::ExtendPrev))
        );
    }

    #[test]
    fn lone_modifiers_never_commit() {
        // Regression: Shift pressed mid-word (to type a capital) used to commit the word.
        for s in [
            st(ContextMode::Arabic, true, Popup::List),
            st(ContextMode::Arabic, true, Popup::Tashkeel),
            st(ContextMode::Arabic, false, Popup::Hidden),
            st(ContextMode::Latin, false, Popup::Hidden),
        ] {
            assert_eq!(classify(&s, Key::Modifier, SHIFT), PASS);
            assert_eq!(classify(&s, Key::Modifier, CTRL), PASS);
        }
    }

    #[test]
    fn decisions_are_pure() {
        // OnTestKeyDown and OnKeyDown must agree: same inputs → same decision, every time.
        let s = st(ContextMode::Arabic, true, Popup::List);
        for k in [
            Key::Char('a'),
            Key::Space,
            Key::Tab,
            Key::Escape,
            Key::Other,
        ] {
            assert_eq!(classify(&s, k, NONE), classify(&s, k, NONE));
        }
    }

    #[test]
    fn input_scopes_gate_transliteration_and_learning() {
        // R9: passwords/PINs/numbers are Latin-only, private fields never learn.
        assert_eq!(classify_scopes(&[], true), ScopeClass::Normal);
        assert_eq!(classify_scopes(&[0], true), ScopeClass::Normal); // IS_DEFAULT
        assert_eq!(classify_scopes(&[31], false), ScopeClass::Latin); // IS_PASSWORD
        assert_eq!(classify_scopes(&[64], false), ScopeClass::Latin); // IS_NUMERIC_PIN
        assert_eq!(classify_scopes(&[29], false), ScopeClass::Latin); // IS_NUMBER
        assert_eq!(classify_scopes(&[32], false), ScopeClass::Latin); // IS_TELEPHONE_FULLTELEPHONENUMBER
        assert_eq!(classify_scopes(&[22], false), ScopeClass::Latin); // IS_DATE_FULLDATE
        assert_eq!(classify_scopes(&[1], true), ScopeClass::Latin); // IS_URL, setting on
        assert_eq!(classify_scopes(&[1], false), ScopeClass::Normal); // IS_URL, setting off
        assert_eq!(classify_scopes(&[5], true), ScopeClass::Latin); // IS_EMAIL_SMTPEMAILADDRESS
        assert_eq!(classify_scopes(&[61], true), ScopeClass::NoLearning); // IS_PRIVATE
                                                                          // A private password field is still a password field.
        assert_eq!(classify_scopes(&[61, 31], true), ScopeClass::Latin);
        assert_eq!(classify_scopes(&[57, 58], true), ScopeClass::Normal); // IS_TEXT, IS_CHAT
    }

    /// Regression (Owner, 2026-09-25): letters that are not editor commands were swallowed while the
    /// diacritics editor was open, so typing on looked like a frozen editor.
    #[test]
    fn editor_never_swallows_typing() {
        let s = st(ContextMode::Arabic, true, Popup::Tashkeel);
        for c in ['b', 's', 't', 'k', '9', '0'] {
            assert_eq!(
                classify(&s, Key::Char(c), NONE),
                eat(Action::CommitThenType(c))
            );
        }
        // Commands keep their meaning.
        assert_eq!(
            classify(&s, Key::Char('a'), NONE),
            eat(Action::Tashkeel(TashkeelCmd::Fatha))
        );
        assert_eq!(
            classify(&s, Key::Char('3'), NONE),
            eat(Action::Tashkeel(TashkeelCmd::QuickPick(3)))
        );
        // Shortcuts leave the editor and reach the app.
        assert_eq!(
            classify(&s, Key::Char('c'), CTRL),
            eat(Action::CommitAndReinject)
        );
    }

    #[test]
    fn shift_tap_toggles_only_for_a_short_lone_tap() {
        let mut armed = None;
        assert!(!shift_tap(&mut armed, true, true, 1000));
        assert!(shift_tap(&mut armed, true, false, 1200)); // quick tap
        assert!(!shift_tap(&mut armed, true, true, 2000));
        assert!(!shift_tap(&mut armed, false, true, 2050)); // Shift+A: a capital
        assert!(!shift_tap(&mut armed, true, false, 2100));
        assert!(!shift_tap(&mut armed, true, true, 3000));
        assert!(!shift_tap(&mut armed, true, false, 3000 + SHIFT_TAP_MS + 1)); // held too long
        assert!(!shift_tap(&mut armed, true, false, 4000)); // release without press
    }
}
