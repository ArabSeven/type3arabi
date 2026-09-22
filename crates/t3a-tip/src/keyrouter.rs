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
}

/// Commands inside the tashkeel editor (docs/05 §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TashkeelCmd {
    Fatha,
    Damma,
    Kasra,
    Sukun,
    ShaddaToggle,
    Fathatan,
    Dammatan,
    Kasratan,
    DaggerAlif,
    Clear,
    /// Backspace: clear marks on the focused letter, or back to the list if it has none.
    ClearOrBack,
    LetterNext,
    LetterPrev,
    LetterFirst,
    LetterLast,
    QuickPick(u8),
    PickUp,
    PickDown,
    Back,
    Ignore,
}

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
    CommitThenPunctuation(char),
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
    if s.context == ContextMode::Off {
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
    if s.composing && s.popup == Popup::Tashkeel {
        return eat(match key {
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
                c if is_token_char(c) => Action::Tashkeel(TashkeelCmd::Ignore),
                c => Action::CommitThenPunctuation(c),
            },
            Key::NumpadDigit(_) => Action::Tashkeel(TashkeelCmd::Ignore),
            Key::Backspace => Action::Tashkeel(TashkeelCmd::ClearOrBack),
            Key::Delete => Action::Tashkeel(TashkeelCmd::Clear),
            Key::Left => Action::Tashkeel(TashkeelCmd::LetterNext), // visual left = logical next (RTL)
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
            Key::Other => Action::CommitAndReinject,
        });
    }
    if s.composing {
        return eat(match key {
            Key::Enter if m.ctrl && !m.alt && !m.win => Action::CommitWithHarakat,
            _ if m.command() => Action::CommitAndReinject,
            Key::Char('-') if s.buffer_is_article => Action::ArticleHyphen,
            Key::Char(c) if is_token_char(c) => Action::AppendChar(c),
            Key::Char(c) => Action::CommitThenPunctuation(c),
            Key::NumpadDigit(d) => Action::AppendLiteralDigit(d),
            Key::Backspace => Action::Backspace,
            Key::Space => Action::CommitSpace,
            Key::Enter => Action::CommitNoSpace,
            Key::Tab if m.shift => Action::PrevCandidate,
            Key::Tab => Action::OpenTashkeel,
            Key::Down => Action::NextCandidate,
            Key::Up => Action::PrevCandidate,
            Key::PageDown => Action::NextPage,
            Key::PageUp => Action::PrevPage,
            Key::Escape => Action::CommitRaw,
            Key::Left | Key::Right | Key::Home | Key::End | Key::Delete | Key::Other => {
                Action::CommitAndReinject
            }
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
}
