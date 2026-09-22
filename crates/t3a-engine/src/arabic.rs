//! Arabic text helpers: marks, base forms, normalization (docs/04 §3) and mark ordering (docs/03 §10.1).

pub const FATHATAN: char = '\u{064B}';
pub const DAMMATAN: char = '\u{064C}';
pub const KASRATAN: char = '\u{064D}';
pub const FATHA: char = '\u{064E}';
pub const DAMMA: char = '\u{064F}';
pub const KASRA: char = '\u{0650}';
pub const SHADDA: char = '\u{0651}';
pub const SUKUN: char = '\u{0652}';
pub const SUPERSCRIPT_ALEF: char = '\u{0670}';
pub const TATWEEL: char = '\u{0640}';

/// Harakat recognised by the engine.
pub fn is_mark(c: char) -> bool {
    matches!(c, '\u{064B}'..='\u{0652}' | '\u{0670}')
}

/// Vowel-class marks (at most one per letter).
pub fn is_vowel_mark(c: char) -> bool {
    matches!(c, '\u{064B}'..='\u{0650}' | '\u{0652}')
}

/// Remove all harakat (base form).
pub fn strip_marks(s: &str) -> String {
    s.chars().filter(|&c| !is_mark(c)).collect()
}

/// Normalize arbitrary Arabic text to its **base form** exactly as the pipeline does (docs/04 §3),
/// minus Unicode NFC (the pipeline applies NFC before calling its equivalent).
pub fn normalize_word(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            TATWEEL | '\u{200C}' | '\u{200D}' => {}
            '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => {}
            '\u{06D6}'..='\u{06ED}' | '\u{0615}'..='\u{061A}' => {}
            '\u{0671}' => out.push('\u{0627}'), // alef wasla → alef
            '\u{06CC}' => out.push('\u{064A}'), // Farsi yeh → yeh
            '\u{06A9}' => out.push('\u{0643}'), // keheh → kaf
            '\u{06C1}' => out.push('\u{0647}'), // heh goal → heh
            c if is_mark(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// Reorder marks after each base letter to the product's canonical *typing order*:
/// shadda, then one vowel/sukun/tanween, then superscript alef (docs/03 §10.1).
/// Duplicate vowel marks on one letter: the last one wins.
pub fn canonical_mark_order(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut shadda = false;
    let mut vowel: Option<char> = None;
    let mut dagger = false;
    let flush =
        |out: &mut String, shadda: &mut bool, vowel: &mut Option<char>, dagger: &mut bool| {
            if *shadda {
                out.push(SHADDA);
            }
            if let Some(v) = vowel.take() {
                out.push(v);
            }
            if *dagger {
                out.push(SUPERSCRIPT_ALEF);
            }
            *shadda = false;
            *dagger = false;
        };
    for c in s.chars() {
        if c == SHADDA {
            shadda = true;
        } else if c == SUPERSCRIPT_ALEF {
            dagger = true;
        } else if is_vowel_mark(c) {
            vowel = Some(c);
        } else {
            flush(&mut out, &mut shadda, &mut vowel, &mut dagger);
            out.push(c);
        }
    }
    flush(&mut out, &mut shadda, &mut vowel, &mut dagger);
    out
}

/// True if `s` contains only characters the engine may output (docs/03 §10.1 invariant).
pub fn is_clean_output(s: &str) -> bool {
    !s.chars().any(|c| {
        c == TATWEEL
            || matches!(c, '\u{FB50}'..='\u{FDFF}' if c != '\u{FDFA}')
            || matches!(c, '\u{FE70}'..='\u{FEFF}')
            || matches!(c, '\u{200C}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_vectors_match_pipeline() {
        let tsv = include_str!("../../../data/eval/normalization.tsv");
        let mut n = 0;
        for line in tsv.lines().filter(|l| !l.starts_with('#')).skip(1) {
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(normalize_word(cols[0]), cols[1], "vector: {}", cols[2]);
            n += 1;
        }
        assert!(n >= 10);
    }

    #[test]
    fn mark_order_is_shadda_then_vowel() {
        // اللَّه typed in NFC order (fatha before shadda) → shadda first.
        let nfc = "\u{0627}\u{0644}\u{0644}\u{064E}\u{0651}\u{0647}";
        assert_eq!(
            canonical_mark_order(nfc),
            "\u{0627}\u{0644}\u{0644}\u{0651}\u{064E}\u{0647}"
        );
    }

    #[test]
    fn clean_output_detects_tatweel() {
        assert!(is_clean_output("مرحبا"));
        assert!(!is_clean_output("مرحـبا"));
        assert!(is_clean_output("ﷺ")); // U+FDFA allowed on purpose (phrase output)
    }
}
