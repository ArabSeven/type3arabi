//! The T3A alphabet: one byte per Arabic letter (docs/12-binary-format.md §3).
//! Code 0 is reserved. Codes 1..=42 follow Unicode order.

/// Arabic letters indexed by `code - 1`.
pub const LETTERS: [char; 42] = [
    '\u{0621}', '\u{0622}', '\u{0623}', '\u{0624}', '\u{0625}', '\u{0626}', '\u{0627}', '\u{0628}',
    '\u{0629}', '\u{062A}', '\u{062B}', '\u{062C}', '\u{062D}', '\u{062E}', '\u{062F}', '\u{0630}',
    '\u{0631}', '\u{0632}', '\u{0633}', '\u{0634}', '\u{0635}', '\u{0636}', '\u{0637}', '\u{0638}',
    '\u{0639}', '\u{063A}', '\u{0641}', '\u{0642}', '\u{0643}', '\u{0644}', '\u{0645}', '\u{0646}',
    '\u{0647}', '\u{0648}', '\u{0649}', '\u{064A}', '\u{067E}', '\u{0686}', '\u{06A4}', '\u{06A8}',
    '\u{06AD}', '\u{06AF}',
];

/// Number of letters in the alphabet.
pub const COUNT: u8 = 42;

/// T3A code of an Arabic letter, or `None` if `c` is not in the alphabet.
pub fn code_of(c: char) -> Option<u8> {
    LETTERS.iter().position(|&l| l == c).map(|i| i as u8 + 1)
}

/// Arabic letter for a T3A code (1..=42).
pub fn char_of(code: u8) -> Option<char> {
    if code == 0 || code > COUNT {
        None
    } else {
        Some(LETTERS[code as usize - 1])
    }
}

/// True if `c` is a letter of the T3A alphabet.
pub fn is_letter(c: char) -> bool {
    code_of(c).is_some()
}

/// Encode an Arabic string (letters only) to T3A codes. Returns `None` on any non-letter.
pub fn encode(s: &str) -> Option<Vec<u8>> {
    s.chars().map(code_of).collect()
}

/// Decode T3A codes to a `String`. Invalid codes are skipped.
pub fn decode(codes: &[u8]) -> String {
    codes.iter().filter_map(|&c| char_of(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_codes() {
        for code in 1..=COUNT {
            let c = char_of(code).unwrap();
            assert_eq!(code_of(c), Some(code));
        }
        assert_eq!(char_of(0), None);
        assert_eq!(char_of(43), None);
    }

    #[test]
    fn table_matches_docs() {
        // docs/12 §3 spot checks
        assert_eq!(code_of('ء'), Some(1));
        assert_eq!(code_of('ا'), Some(7));
        assert_eq!(code_of('ع'), Some(25));
        assert_eq!(code_of('ي'), Some(36));
        assert_eq!(code_of('گ'), Some(42));
        assert_eq!(encode("سلام"), Some(vec![19, 30, 7, 31]));
        assert_eq!(decode(&[13, 8, 36, 8, 36]), "حبيبي");
    }
}
