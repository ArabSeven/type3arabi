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

#[derive(Clone, Debug, PartialEq)]
pub struct TashkeelEditor {
    pub slots: Vec<LetterSlot>,
    pub focused_letter: usize,
    pub picks: Vec<String>,
    pub from_typing: bool,
    pub highlighted_pick: Option<usize>,
}

impl TashkeelEditor {
    pub fn new(word: &str, picks: Vec<String>, from_typing: bool) -> Self {
        let mut editor = Self {
            slots: Vec::new(),
            focused_letter: 0,
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
        match cmd {
            TashkeelCmd::Back => TashkeelAction::BackToList,
            TashkeelCmd::LetterNext => {
                if self.focused_letter + 1 < self.slots.len() {
                    self.focused_letter += 1;
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::LetterPrev => {
                self.focused_letter = self.focused_letter.saturating_sub(1);
                TashkeelAction::Continue
            }
            TashkeelCmd::LetterFirst => {
                self.focused_letter = 0;
                TashkeelAction::Continue
            }
            TashkeelCmd::LetterLast => {
                self.focused_letter = self.slots.len().saturating_sub(1);
                TashkeelAction::Continue
            }
            TashkeelCmd::Clear => {
                if self.focused_letter < self.slots.len() {
                    let slot = &mut self.slots[self.focused_letter];
                    slot.shadda = false;
                    slot.dagger_alif = false;
                    slot.vowel = None;
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::ClearOrBack => {
                if self.focused_letter < self.slots.len() {
                    let slot = &mut self.slots[self.focused_letter];
                    if slot.shadda || slot.dagger_alif || slot.vowel.is_some() {
                        slot.shadda = false;
                        slot.dagger_alif = false;
                        slot.vowel = None;
                        TashkeelAction::Continue
                    } else {
                        TashkeelAction::BackToList
                    }
                } else {
                    TashkeelAction::BackToList
                }
            }
            TashkeelCmd::ShaddaToggle => {
                if self.focused_letter < self.slots.len() {
                    let base = self.slots[self.focused_letter].base;
                    if base != 'ا' && base != 'ى' {
                        self.slots[self.focused_letter].shadda =
                            !self.slots[self.focused_letter].shadda;
                    }
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::DaggerAlif => {
                if self.focused_letter < self.slots.len() {
                    self.slots[self.focused_letter].dagger_alif =
                        !self.slots[self.focused_letter].dagger_alif;
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::Fatha | TashkeelCmd::Damma | TashkeelCmd::Kasra | TashkeelCmd::Sukun => {
                if self.focused_letter < self.slots.len() {
                    let base = self.slots[self.focused_letter].base;
                    if base != 'ا' && base != 'ى' {
                        let v = match cmd {
                            TashkeelCmd::Fatha => crate::arabic::FATHA,
                            TashkeelCmd::Damma => crate::arabic::DAMMA,
                            TashkeelCmd::Kasra => crate::arabic::KASRA,
                            _ => crate::arabic::SUKUN,
                        };
                        self.slots[self.focused_letter].vowel = Some(v);
                        if self.focused_letter + 1 < self.slots.len() {
                            self.focused_letter += 1;
                        }
                    }
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::Fathatan | TashkeelCmd::Dammatan | TashkeelCmd::Kasratan => {
                if self.focused_letter < self.slots.len() {
                    let base = self.slots[self.focused_letter].base;
                    if cmd == TashkeelCmd::Fathatan || (base != 'ا' && base != 'ى') {
                        let v = match cmd {
                            TashkeelCmd::Fathatan => crate::arabic::FATHATAN,
                            TashkeelCmd::Dammatan => crate::arabic::DAMMATAN,
                            _ => crate::arabic::KASRATAN,
                        };
                        self.slots[self.focused_letter].vowel = Some(v);
                        if self.focused_letter + 1 < self.slots.len() {
                            self.focused_letter += 1;
                        }
                    }
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::QuickPick(n) => {
                let idx = (n as usize).saturating_sub(1);
                if idx < self.picks.len() {
                    let word = self.picks[idx].clone();
                    self.load_word(&word);
                    self.highlighted_pick = Some(idx);
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::PickDown | TashkeelCmd::PickUp => {
                if !self.picks.is_empty() {
                    let next_idx = match self.highlighted_pick {
                        None => 0,
                        Some(cur) => {
                            if cmd == TashkeelCmd::PickDown {
                                (cur + 1) % self.picks.len()
                            } else if cur == 0 {
                                self.picks.len() - 1
                            } else {
                                cur - 1
                            }
                        }
                    };
                    self.highlighted_pick = Some(next_idx);
                    let word = self.picks[next_idx].clone();
                    self.load_word(&word);
                }
                TashkeelAction::Continue
            }
            TashkeelCmd::Ignore => TashkeelAction::Continue,
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
}
