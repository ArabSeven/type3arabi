//! Display transforms (docs/03 §10): sacred-name styling, adverbial tanween, hamza style, numerals,
//! punctuation. All functions take and return full strings; they never add tatweel or bidi controls.

use crate::arabic::{self, FATHATAN, SHADDA, SUPERSCRIPT_ALEF};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AllahForm {
    Plain,
    #[default]
    Shadda,
    ShaddaFatha,
    ShaddaDagger,
}

impl AllahForm {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "plain" => Self::Plain,
            "shadda" => Self::Shadda,
            "shadda_fatha" => Self::ShaddaFatha,
            "shadda_dagger" => Self::ShaddaDagger,
            _ => return None,
        })
    }
    fn marks(self) -> &'static [char] {
        match self {
            Self::Plain => &[],
            Self::Shadda => &[SHADDA],
            Self::ShaddaFatha => &[SHADDA, arabic::FATHA],
            Self::ShaddaDagger => &[SHADDA, SUPERSCRIPT_ALEF],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TanweenStyle {
    #[default]
    OnAlif,
    BeforeAlif,
    Off,
}

impl TanweenStyle {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "on_alif" => Self::OnAlif,
            "before_alif" => Self::BeforeAlif,
            "off" => Self::Off,
            _ => return None,
        })
    }
}

/// Words that receive sacred-name styling (exact base-form match), docs/03 §10.2.
pub const SACRED: [&str; 9] = [
    "الله",
    "والله",
    "بالله",
    "تالله",
    "فالله",
    "لله",
    "ولله",
    "فلله",
    "اللهم",
];

/// Style every sacred word inside `text` (word by word, whitespace-separated).
pub fn style_sacred(text: &str, form: AllahForm) -> String {
    if form == AllahForm::Plain {
        return text.to_string();
    }
    text.split(' ')
        .map(|w| {
            let base = arabic::strip_marks(w);
            if !SACRED.contains(&base.as_str()) {
                return w.to_string();
            }
            // Insert marks after the last ل that is immediately followed by ه.
            let chars: Vec<char> = base.chars().collect();
            let mut idx = None;
            for i in 0..chars.len().saturating_sub(1) {
                if chars[i] == 'ل' && chars[i + 1] == 'ه' {
                    idx = Some(i);
                }
            }
            let Some(i) = idx else { return w.to_string() };
            let mut out = String::new();
            for (j, c) in chars.iter().enumerate() {
                out.push(*c);
                if j == i {
                    out.extend(form.marks());
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// True if any word of `text` is sacred (so the plain form should also be offered).
pub fn contains_sacred(text: &str) -> bool {
    text.split(' ')
        .any(|w| SACRED.contains(&arabic::strip_marks(w).as_str()))
}

/// Add fathatan to a word ending in ا (docs/03 §10.3). Words not ending in ا are returned unchanged.
pub fn add_tanween_fath(word: &str, style: TanweenStyle) -> String {
    if style == TanweenStyle::Off || !word.ends_with('ا') {
        return word.to_string();
    }
    let mut chars: Vec<char> = word.chars().collect();
    match style {
        TanweenStyle::OnAlif => chars.push(FATHATAN),
        TanweenStyle::BeforeAlif => {
            let n = chars.len();
            chars.insert(n - 1, FATHATAN);
        }
        TanweenStyle::Off => {}
    }
    chars.into_iter().collect()
}

/// Relaxed hamza style: أ إ آ → ا (docs/03 §10.4).
pub fn relax_hamza(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, 'أ' | 'إ' | 'آ') {
                'ا'
            } else {
                c
            }
        })
        .collect()
}

/// Convert ASCII digits to Eastern Arabic digits when `eastern` (docs/03 §6.1).
pub fn numerals(text: &str, eastern: bool) -> String {
    if !eastern {
        return text.to_string();
    }
    text.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                char::from_u32(0x0660 + (c as u32 - '0' as u32)).unwrap()
            } else {
                c
            }
        })
        .collect()
}

/// Arabic punctuation mapping applied by the TIP (docs/03 §6.6).
pub fn punctuation(c: char, arabic_punct: bool) -> char {
    if !arabic_punct {
        return c;
    }
    match c {
        ',' => '،',
        ';' => '؛',
        '?' => '؟',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sacred_styles() {
        // U+0627 U+0644 U+0644 U+0651 U+0647
        assert_eq!(
            style_sacred("الله", AllahForm::Shadda),
            "\u{0627}\u{0644}\u{0644}\u{0651}\u{0647}"
        );
        assert_eq!(
            style_sacred("الله", AllahForm::ShaddaFatha),
            "\u{0627}\u{0644}\u{0644}\u{0651}\u{064E}\u{0647}"
        );
        assert_eq!(
            style_sacred("الله", AllahForm::ShaddaDagger),
            "\u{0627}\u{0644}\u{0644}\u{0651}\u{0670}\u{0647}"
        );
        assert_eq!(
            style_sacred("إن شاء الله", AllahForm::Shadda),
            "إن شاء اللّه"
        );
        assert_eq!(style_sacred("اللهم", AllahForm::Shadda), "اللّهم");
        assert_eq!(style_sacred("لله", AllahForm::Shadda), "للّه");
        // Negative cases (docs/08 §2.2): not sacred words.
        for w in ["كله", "له", "ظله", "إله"] {
            assert_eq!(style_sacred(w, AllahForm::Shadda), w);
        }
        assert_eq!(style_sacred("الله", AllahForm::Plain), "الله");
    }

    #[test]
    fn tanween_and_misc() {
        assert_eq!(
            add_tanween_fath("شكرا", TanweenStyle::OnAlif),
            "شكرا\u{064B}"
        );
        assert_eq!(
            add_tanween_fath("شكرا", TanweenStyle::BeforeAlif),
            "شكر\u{064B}ا"
        );
        assert_eq!(add_tanween_fath("مدرسة", TanweenStyle::OnAlif), "مدرسة");
        assert_eq!(relax_hamza("أحمد إلى آخر"), "احمد الى اخر");
        assert_eq!(numerals("2026", true), "٢٠٢٦");
        assert_eq!(punctuation('?', true), '؟');
        assert!(arabic::is_clean_output(&style_sacred(
            "والله",
            AllahForm::ShaddaDagger
        )));
    }
}
