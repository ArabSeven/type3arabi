//! "Harakat from your vowels" (docs/03 §10.6): derive diacritics for a candidate from the Latin vowels
//! and doubled consonants the user actually typed, using the winning alignment.

use crate::alphabet;
use crate::arabic::{FATHA, FATHATAN, SHADDA, SUKUN};
use crate::dialect::Dialect;
use crate::display::TanweenStyle;
use crate::oov::Step;
use crate::seed::{SeedTables, F_ARTICLE, F_GEM, F_TANWEEN, F_VOWEL};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HarakatMode {
    #[default]
    Light,
    Full,
}

#[derive(Clone, Debug)]
struct Slot {
    letter: char,
    shadda: bool,
    vowel: Option<char>,
    /// may receive a vowel (consonant-like, not article, not long-vowel letter)
    markable: bool,
    /// emitted by a consonant rule (for sukun in Full mode)
    consonant: bool,
}

const LONG_VOWELS: [char; 3] = ['ا', 'و', 'ي'];
const NEVER_MARK: [char; 3] = ['ا', 'ى', 'ة'];

/// Build the vocalized string for an alignment. Returns `None` if the alignment is empty.
pub fn vowel_harakat(
    steps: &[Step],
    seed: &SeedTables,
    d: Dialect,
    mode: HarakatMode,
    tanween: TanweenStyle,
) -> Option<String> {
    let mut slots: Vec<Slot> = Vec::new();
    let mut last_cons: Option<usize> = None;
    let latin_str = |s: &Step| -> String { s.latin.iter().collect() };

    for (si, step) in steps.iter().enumerate() {
        let letters: Vec<char> = step
            .arabic
            .iter()
            .filter_map(|&c| alphabet::char_of(c))
            .collect();
        let latin = latin_str(step);
        let is_vowel = step.flags & F_VOWEL != 0;
        let is_last_step = si + 1 == steps.len();

        if is_vowel {
            if letters.is_empty() {
                if let (Some(i), Some(m)) = (last_cons, seed.vowel_mark(&latin, d)) {
                    if slots[i].vowel.is_none() {
                        slots[i].vowel = Some(m);
                    }
                }
                last_cons = None;
            } else if letters.len() == 1 && LONG_VOWELS.contains(&letters[0]) && last_cons.is_some()
            {
                let i = last_cons.unwrap();
                if slots[i].vowel.is_none() {
                    slots[i].vowel = Some(match letters[0] {
                        'ا' => FATHA,
                        'و' => crate::arabic::DAMMA,
                        _ => crate::arabic::KASRA,
                    });
                }
                slots.push(Slot {
                    letter: letters[0],
                    shadda: false,
                    vowel: None,
                    markable: false,
                    consonant: false,
                });
                last_cons = None;
            } else {
                // e.g. initial a → أ / ع: the vowel sits on its own carrier.
                for &l in &letters {
                    let carrier = matches!(l, 'أ' | 'ع');
                    let v = if carrier {
                        seed.vowel_mark(&latin[..1], d)
                    } else {
                        None
                    };
                    slots.push(Slot {
                        letter: l,
                        shadda: false,
                        vowel: v,
                        markable: false,
                        consonant: false,
                    });
                }
                last_cons = None;
            }
            continue;
        }

        // consonant / ending rule
        let article = step.flags & F_ARTICLE != 0;
        let start = slots.len();
        for (li, &l) in letters.iter().enumerate() {
            let in_article =
                article && li < 2 && letters.len() >= 2 && letters[0] == 'ا' && letters[1] == 'ل';
            let markable = !in_article && !NEVER_MARK.contains(&l);
            slots.push(Slot {
                letter: l,
                shadda: false,
                vowel: None,
                markable,
                consonant: markable,
            });
        }
        if slots.len() > start {
            let last = slots.len() - 1;
            if step.flags & F_GEM != 0 && slots[last].markable {
                slots[last].shadda = true;
            }
            if step.flags & F_TANWEEN != 0
                && slots[last].letter == 'ا'
                && tanween != TanweenStyle::Off
            {
                match tanween {
                    TanweenStyle::OnAlif => slots[last].vowel = Some(FATHATAN),
                    _ => {
                        if last > 0 {
                            slots[last - 1].vowel = Some(FATHATAN);
                        }
                    }
                }
            }
            last_cons = if slots[last].markable {
                Some(last)
            } else {
                None
            };
        }
        let _ = is_last_step;
    }
    if slots.is_empty() {
        return None;
    }

    if mode == HarakatMode::Full {
        let n = slots.len();
        for i in 0..n.saturating_sub(1) {
            if slots[i].consonant && slots[i].vowel.is_none() && slots[i + 1].consonant {
                slots[i].vowel = Some(SUKUN);
            }
        }
    }

    let mut out = String::new();
    for s in &slots {
        out.push(s.letter);
        if s.shadda {
            out.push(SHADDA);
        }
        if let Some(v) = s.vowel {
            // fathatan on a bare alif is only valid as tanween (docs/05 §4.3)
            if s.letter != 'ا' || v == FATHATAN {
                out.push(v);
            }
        }
    }
    Some(out)
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
    /// Shift+→/← (visual): move the focus and add the new letter to the selection.
    ExtendNext,
    ExtendPrev,
    /// Remove every mark from every letter.
    ClearAll,
    /// Mouse selection of letter `index` (logical order).
    Select(u8, SelectMode),
}

/// How a mouse click on a letter changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    /// Plain click: select only this letter.
    Only,
    /// Ctrl+click: add or remove this letter.
    Toggle,
    /// Shift+click (or drag): select the range from the focused letter to this one.
    Range,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TashkeelAction {
    Continue,
    BackToList,
    Commit(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LetterSlot {
    pub base: char,
    pub shadda: bool,
    pub dagger_alif: bool,
    pub vowel: Option<char>,
}

/// The in-popup tashkeel editor (docs/05 §4). Marks apply to every *selected* letter; the focused
/// letter is always selected. Entering the editor selects the first letter.
#[derive(Clone, Debug, PartialEq)]
pub struct TashkeelEditor {
    pub slots: Vec<LetterSlot>,
    pub focused_letter: usize,
    /// Selected letters (logical order), parallel to `slots`.
    pub selected: Vec<bool>,
    pub picks: Vec<String>,
    pub from_typing: bool,
    pub highlighted_pick: Option<usize>,
}

impl TashkeelEditor {
    pub fn new(word: &str, picks: Vec<String>, from_typing: bool) -> Self {
        let mut editor = Self {
            slots: Vec::new(),
            focused_letter: 0,
            selected: Vec::new(),
            picks,
            from_typing,
            highlighted_pick: None,
        };
        editor.load_word(word);
        editor
    }

    pub fn load_word(&mut self, word: &str) {
        let mut slots: Vec<LetterSlot> = Vec::new();
        for c in word.chars() {
            if crate::arabic::is_mark(c) {
                if let Some(slot) = slots.last_mut() {
                    match c {
                        crate::arabic::SHADDA => slot.shadda = true,
                        crate::arabic::SUPERSCRIPT_ALEF => slot.dagger_alif = true,
                        v if crate::arabic::is_vowel_mark(v) => slot.vowel = Some(v),
                        _ => {}
                    }
                }
            } else {
                slots.push(LetterSlot {
                    base: c,
                    shadda: false,
                    dagger_alif: false,
                    vowel: None,
                });
            }
        }
        if slots.is_empty() {
            slots.push(LetterSlot {
                base: ' ',
                shadda: false,
                dagger_alif: false,
                vowel: None,
            });
        }
        self.slots = slots;
        if self.focused_letter >= self.slots.len() {
            self.focused_letter = self.slots.len().saturating_sub(1);
        }
        self.select_only(self.focused_letter);
    }

    /// Indices of the selected letters, in logical order (never empty for a non-empty word).
    pub fn selection(&self) -> Vec<usize> {
        let v: Vec<usize> = (0..self.slots.len())
            .filter(|&i| self.selected[i])
            .collect();
        if v.is_empty() && !self.slots.is_empty() {
            vec![self.focused_letter]
        } else {
            v
        }
    }

    fn select_only(&mut self, i: usize) {
        self.selected = vec![false; self.slots.len()];
        if i < self.selected.len() {
            self.selected[i] = true;
        }
    }

    fn move_focus(&mut self, to: usize, extend: bool) {
        if self.slots.is_empty() {
            return;
        }
        self.focused_letter = to.min(self.slots.len() - 1);
        if extend {
            self.selected[self.focused_letter] = true;
        } else {
            self.select_only(self.focused_letter);
        }
    }

    fn clear_marks(&mut self, i: usize) {
        let slot = &mut self.slots[i];
        slot.shadda = false;
        slot.dagger_alif = false;
        slot.vowel = None;
    }

    fn has_marks(&self, i: usize) -> bool {
        let s = &self.slots[i];
        s.shadda || s.dagger_alif || s.vowel.is_some()
    }

    /// After a vowel/tanween/sukun on a single selected letter, focus moves to the next letter.
    fn advance_if_single(&mut self) {
        if self.selection().len() == 1 && self.focused_letter + 1 < self.slots.len() {
            let next = self.focused_letter + 1;
            self.move_focus(next, false);
        }
    }

    /// Render the current editor slots into a canonical marked string.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for slot in &self.slots {
            out.push(slot.base);
            if slot.shadda {
                out.push(crate::arabic::SHADDA);
            }
            if let Some(v) = slot.vowel {
                out.push(v);
            }
            if slot.dagger_alif {
                out.push(crate::arabic::SUPERSCRIPT_ALEF);
            }
        }
        out
    }

    pub fn apply_cmd(&mut self, cmd: TashkeelCmd) -> TashkeelAction {
        use crate::arabic::{DAMMA, DAMMATAN, FATHA, FATHATAN, KASRA, KASRATAN, SUKUN};
        let n = self.slots.len();
        match cmd {
            TashkeelCmd::Back => TashkeelAction::BackToList,
            TashkeelCmd::Ignore => TashkeelAction::Continue,
            TashkeelCmd::LetterNext => {
                self.move_focus(self.focused_letter + 1, false);
                TashkeelAction::Continue
            }
            TashkeelCmd::LetterPrev => {
                self.move_focus(self.focused_letter.saturating_sub(1), false);
                TashkeelAction::Continue
            }
            TashkeelCmd::ExtendNext => {
                self.move_focus(self.focused_letter + 1, true);
                TashkeelAction::Continue
            }
            TashkeelCmd::ExtendPrev => {
                self.move_focus(self.focused_letter.saturating_sub(1), true);
                TashkeelAction::Continue
            }
            TashkeelCmd::LetterFirst => {
                self.move_focus(0, false);
                TashkeelAction::Continue
            }
            TashkeelCmd::LetterLast => {
                self.move_focus(n.saturating_sub(1), false);
                TashkeelAction::Continue
            }
            TashkeelCmd::Select(index, mode) => {
                let i = index as usize;
                if i < n {
                    match mode {
                        SelectMode::Only => self.move_focus(i, false),
                        SelectMode::Toggle => {
                            self.selected[i] = !self.selected[i];
                            if self.selected[i] {
                                self.focused_letter = i;
                            } else if self.selection().is_empty()
                                || !self.selected.iter().any(|&b| b)
                            {
                                self.move_focus(i, false);
                            } else if self.focused_letter == i {
                                self.focused_letter = self.selection()[0];
                            }
                        }
                        SelectMode::Range => {
                            let (a, b) = if i < self.focused_letter {
                                (i, self.focused_letter)
                            } else {
                                (self.focused_letter, i)
                            };
                            self.selected = (0..n).map(|k| k >= a && k <= b).collect();
                            self.focused_letter = i;
                        }
                    }
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::ClearAll => {
                for i in 0..n {
                    self.clear_marks(i);
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::Clear => {
                for i in self.selection() {
                    self.clear_marks(i);
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::ClearOrBack => {
                let sel = self.selection();
                if sel.iter().any(|&i| self.has_marks(i)) {
                    for i in sel {
                        self.clear_marks(i);
                    }
                    TashkeelAction::Continue
                } else {
                    TashkeelAction::BackToList
                }
            }
            TashkeelCmd::ShaddaToggle => {
                let sel = self.selection();
                // With several letters selected, the toggle is "all on" unless all already have it.
                let target = !sel
                    .iter()
                    .filter(|&&i| !matches!(self.slots[i].base, 'ا' | 'ى'))
                    .all(|&i| self.slots[i].shadda);
                for i in sel {
                    if !matches!(self.slots[i].base, 'ا' | 'ى') {
                        self.slots[i].shadda = target;
                    }
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::DaggerAlif => {
                let sel = self.selection();
                let target = !sel.iter().all(|&i| self.slots[i].dagger_alif);
                for i in sel {
                    self.slots[i].dagger_alif = target;
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::Fatha
            | TashkeelCmd::Damma
            | TashkeelCmd::Kasra
            | TashkeelCmd::Sukun
            | TashkeelCmd::Fathatan
            | TashkeelCmd::Dammatan
            | TashkeelCmd::Kasratan => {
                let v = match cmd {
                    TashkeelCmd::Fatha => FATHA,
                    TashkeelCmd::Damma => DAMMA,
                    TashkeelCmd::Kasra => KASRA,
                    TashkeelCmd::Sukun => SUKUN,
                    TashkeelCmd::Fathatan => FATHATAN,
                    TashkeelCmd::Dammatan => DAMMATAN,
                    _ => KASRATAN,
                };
                let mut applied = false;
                for i in self.selection() {
                    // A bare alif / alif maqsura takes no vowel; fathatan on alif is tanween.
                    let bare = matches!(self.slots[i].base, 'ا' | 'ى');
                    if !bare || v == FATHATAN {
                        self.slots[i].vowel = Some(v);
                        applied = true;
                    }
                }
                if applied {
                    self.advance_if_single();
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::QuickPick(k) => {
                let idx = (k as usize).saturating_sub(1);
                if idx < self.picks.len() {
                    let word = self.picks[idx].clone();
                    self.load_word(&word);
                    self.highlighted_pick = Some(idx);
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::PickDown | TashkeelCmd::PickUp => {
                if !self.picks.is_empty() {
                    let len = self.picks.len();
                    let next_idx = match (self.highlighted_pick, cmd) {
                        (None, _) => 0,
                        (Some(cur), TashkeelCmd::PickDown) => (cur + 1) % len,
                        (Some(cur), _) => (cur + len - 1) % len,
                    };
                    self.highlighted_pick = Some(next_idx);
                    let word = self.picks[next_idx].clone();
                    self.load_word(&word);
                }
                TashkeelAction::Continue
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::fixed_profile;
    use crate::normalize::LatinBuffer;
    use crate::oov::search;
    use crate::params::EngineParams;

    /// Vocalize the reading of `latin` whose base form is `want`.
    fn voc(latin: &str, want: &str, d: Dialect, mode: HarakatMode) -> String {
        let seed = SeedTables::builtin();
        let mut b = LatinBuffer::new();
        b.set(latin);
        let hyps = search(
            &seed,
            &EngineParams::default(),
            &fixed_profile(d),
            b.syms(),
            10,
            None,
        );
        let h = hyps
            .iter()
            .find(|h| alphabet::decode(&h.letters) == want)
            .unwrap_or_else(|| panic!("{want} not among readings of {latin}"));
        vowel_harakat(&h.steps, &seed, d, mode, TanweenStyle::OnAlif).unwrap()
    }

    #[test]
    fn documented_examples() {
        // docs/03 §10.6: 3allam → عَلَّم  (ع U+064E ل U+0651 U+064E م)
        assert_eq!(
            voc("3allam", "علم", Dialect::Msa, HarakatMode::Light),
            "\u{0639}\u{064E}\u{0644}\u{0651}\u{064E}\u{0645}"
        );
        // 3ilm → عِلم (light), عِلْم (full)
        assert_eq!(
            voc("3ilm", "علم", Dialect::Msa, HarakatMode::Light),
            "\u{0639}\u{0650}\u{0644}\u{0645}"
        );
        assert_eq!(
            voc("3ilm", "علم", Dialect::Msa, HarakatMode::Full),
            "\u{0639}\u{0650}\u{0644}\u{0652}\u{0645}"
        );
        // madrase → مَدرَسة
        assert_eq!(
            voc("madrase", "مدرسة", Dialect::Lev, HarakatMode::Light),
            "\u{0645}\u{064E}\u{062F}\u{0631}\u{064E}\u{0633}\u{0629}"
        );
    }

    #[test]
    fn mark_order_invariant() {
        let s = voc("3allam", "علم", Dialect::Msa, HarakatMode::Light);
        assert_eq!(crate::arabic::canonical_mark_order(&s), s);
    }

    #[test]
    fn test_editor_navigation_and_marks() {
        let mut editor = TashkeelEditor::new("علم", vec!["عِلْم".into(), "عَلَم".into()], true);
        assert_eq!(editor.focused_letter, 0);
        assert_eq!(editor.render(), "علم");

        // Apply Kasra on first letter 'ع'
        editor.apply_cmd(TashkeelCmd::Kasra);
        // Auto-advances to letter 1 ('ل')
        assert_eq!(editor.focused_letter, 1);
        assert_eq!(editor.render(), "\u{0639}\u{0650}\u{0644}\u{0645}"); // عِلم

        // Apply Shadda on 'ل'
        editor.apply_cmd(TashkeelCmd::ShaddaToggle);
        // Does NOT auto-advance
        assert_eq!(editor.focused_letter, 1);
        // Apply Fatha on 'ل'
        editor.apply_cmd(TashkeelCmd::Fatha);
        // Auto-advances to letter 2 ('م')
        assert_eq!(editor.focused_letter, 2);
        assert_eq!(
            editor.render(),
            "\u{0639}\u{0650}\u{0644}\u{0651}\u{064E}\u{0645}"
        ); // عَلَّم

        // Quick pick 1
        editor.apply_cmd(TashkeelCmd::QuickPick(1));
        assert_eq!(editor.render(), "عِلْم");
        assert_eq!(editor.highlighted_pick, Some(0));

        // Quick pick down -> pick 2
        editor.apply_cmd(TashkeelCmd::PickDown);
        assert_eq!(editor.render(), "عَلَم");
        assert_eq!(editor.highlighted_pick, Some(1));
    }

    #[test]
    fn test_editor_clear_and_back() {
        let mut editor = TashkeelEditor::new("عِلم", vec![], false);
        assert_eq!(editor.focused_letter, 0);
        // First letter has Kasra -> ClearOrBack should clear it and continue
        let action = editor.apply_cmd(TashkeelCmd::ClearOrBack);
        assert_eq!(action, TashkeelAction::Continue);
        assert_eq!(editor.render(), "علم");

        // First letter now has no marks -> ClearOrBack should return BackToList
        let action2 = editor.apply_cmd(TashkeelCmd::ClearOrBack);
        assert_eq!(action2, TashkeelAction::BackToList);
    }

    /// Every command, any letter index (also past the end), quick picks whose words have a different
    /// number of letters than the word being edited: the editor must never panic (a panic in the host
    /// app would disable the keyboard there) and must keep a valid selection and render.
    #[test]
    fn property_every_command_any_index_never_panics() {
        let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || -> usize {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (rng >> 33) as usize
        };
        let words = ["علم", "مرحبا", "و", "الله", "مستشفيات", "لا"];
        let picks = || {
            vec![
                "عَلَّمَ".to_string(),   // U+0639 U+064E U+0644 U+0651 U+064E U+0645 U+064E
                "مَرْحَباً".to_string(), // longer than علم
                "و".to_string(),
                String::new(),
            ]
        };
        let modes = [SelectMode::Only, SelectMode::Toggle, SelectMode::Range];
        for word in words {
            let mut e = TashkeelEditor::new(word, picks(), next() % 2 == 0);
            for _ in 0..4000 {
                let cmd = match next() % 28 {
                    0 => TashkeelCmd::Fatha,
                    1 => TashkeelCmd::Damma,
                    2 => TashkeelCmd::Kasra,
                    3 => TashkeelCmd::Sukun,
                    4 => TashkeelCmd::ShaddaToggle,
                    5 => TashkeelCmd::Fathatan,
                    6 => TashkeelCmd::Dammatan,
                    7 => TashkeelCmd::Kasratan,
                    8 => TashkeelCmd::DaggerAlif,
                    9 => TashkeelCmd::Clear,
                    10 => TashkeelCmd::ClearAll,
                    11 => TashkeelCmd::ClearOrBack,
                    12 => TashkeelCmd::LetterNext,
                    13 => TashkeelCmd::LetterPrev,
                    14 => TashkeelCmd::LetterFirst,
                    15 => TashkeelCmd::LetterLast,
                    16 => TashkeelCmd::ExtendNext,
                    17 => TashkeelCmd::ExtendPrev,
                    18 => TashkeelCmd::PickUp,
                    19 => TashkeelCmd::PickDown,
                    20 | 21 => TashkeelCmd::QuickPick((next() % 12) as u8),
                    22 => TashkeelCmd::Ignore,
                    23 => TashkeelCmd::Back,
                    _ => TashkeelCmd::Select((next() % 14) as u8, modes[next() % 3]),
                };
                let _ = e.apply_cmd(cmd);
                let rendered = e.render();
                assert_eq!(crate::arabic::canonical_mark_order(&rendered), rendered);
                let sel = e.selection();
                assert!(sel.iter().all(|&i| i < rendered.chars().count().max(1)));
            }
        }
    }

    #[test]
    fn property_random_editor_paths_preserve_mark_order() {
        let commands = [
            TashkeelCmd::Fatha,
            TashkeelCmd::Damma,
            TashkeelCmd::Kasra,
            TashkeelCmd::Sukun,
            TashkeelCmd::ShaddaToggle,
            TashkeelCmd::Fathatan,
            TashkeelCmd::Dammatan,
            TashkeelCmd::Kasratan,
            TashkeelCmd::DaggerAlif,
            TashkeelCmd::Clear,
            TashkeelCmd::LetterNext,
            TashkeelCmd::LetterPrev,
            TashkeelCmd::LetterFirst,
            TashkeelCmd::LetterLast,
            TashkeelCmd::ExtendNext,
            TashkeelCmd::ExtendPrev,
            TashkeelCmd::ClearAll,
            TashkeelCmd::Select(1, SelectMode::Toggle),
            TashkeelCmd::Select(0, SelectMode::Range),
            TashkeelCmd::Select(3, SelectMode::Only),
        ];
        let test_words = ["علم", "مرحبا", "كتاب", "شكرا", "الله", "معلم"];
        let mut rng_state: u64 = 0x12345678;
        let mut next_rand = || -> usize {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng_state >> 33) as usize
        };

        for word in &test_words {
            let mut editor = TashkeelEditor::new(word, vec![], false);
            for _ in 0..1000 {
                let cmd = commands[next_rand() % commands.len()];
                let _ = editor.apply_cmd(cmd);
                let rendered = editor.render();
                assert_eq!(
                    crate::arabic::canonical_mark_order(&rendered),
                    rendered,
                    "Mark order violation for sequence on word {}",
                    word
                );
            }
        }
    }

    #[test]
    fn first_letter_selected_on_entry() {
        let e = TashkeelEditor::new("علم", vec![], false);
        assert_eq!(e.focused_letter, 0);
        assert_eq!(e.selection(), vec![0]);
    }

    /// Owner request 2026-09-23: select several letters and apply one mark to all of them.
    #[test]
    fn marks_apply_to_every_selected_letter() {
        let mut e = TashkeelEditor::new("علم", vec![], false);
        e.apply_cmd(TashkeelCmd::ExtendNext);
        e.apply_cmd(TashkeelCmd::ExtendNext);
        assert_eq!(e.selection(), vec![0, 1, 2]);
        e.apply_cmd(TashkeelCmd::Fatha);
        assert_eq!(e.render(), "عَلَمَ"); // U+0639 U+064E U+0644 U+064E U+0645 U+064E
                                       // several letters selected: focus does not auto-advance
        assert_eq!(e.selection(), vec![0, 1, 2]);
        e.apply_cmd(TashkeelCmd::Select(1, SelectMode::Only));
        e.apply_cmd(TashkeelCmd::ShaddaToggle);
        // عَلَّمَ = U+0639 U+064E U+0644 U+0651 U+064E U+0645 U+064E (shadda before the vowel)
        assert_eq!(
            e.render(),
            "\u{0639}\u{064E}\u{0644}\u{0651}\u{064E}\u{0645}\u{064E}"
        );
        e.apply_cmd(TashkeelCmd::ClearAll);
        assert_eq!(e.render(), "علم");
    }

    #[test]
    fn mouse_range_and_toggle_selection() {
        let mut e = TashkeelEditor::new("كتاب", vec![], false);
        e.apply_cmd(TashkeelCmd::Select(2, SelectMode::Range));
        assert_eq!(e.selection(), vec![0, 1, 2]);
        e.apply_cmd(TashkeelCmd::Select(1, SelectMode::Toggle));
        assert_eq!(e.selection(), vec![0, 2]);
        e.apply_cmd(TashkeelCmd::Select(3, SelectMode::Only));
        assert_eq!(e.selection(), vec![3]);
        assert_eq!(e.focused_letter, 3);
    }
}
