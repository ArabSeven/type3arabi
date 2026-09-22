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
}
