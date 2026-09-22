//! Latin input normalization (docs/03 §3).

/// One typed character as delivered by the front-end (TIP / CLI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputChar {
    pub ch: char,
    /// True for numpad digits: forces the token to be a number (docs/03 §6.1).
    pub literal_digit: bool,
}

impl InputChar {
    pub fn new(ch: char) -> Self {
        Self {
            ch,
            literal_digit: false,
        }
    }
    pub fn numpad(ch: char) -> Self {
        Self {
            ch,
            literal_digit: true,
        }
    }
}

/// Internal emphatic symbols (docs/12 §5). Never displayed.
pub const EMPH_T: char = '\u{1}';
pub const EMPH_S: char = '\u{2}';
pub const EMPH_D: char = '\u{3}';
pub const EMPH_Z: char = '\u{4}';
pub const EMPH_H: char = '\u{5}';

/// Map an uppercase emphatic letter to its internal symbol.
pub fn emphatic_symbol(c: char) -> Option<char> {
    match c {
        'T' => Some(EMPH_T),
        'S' => Some(EMPH_S),
        'D' => Some(EMPH_D),
        'Z' => Some(EMPH_Z),
        'H' => Some(EMPH_H),
        _ => None,
    }
}

/// Fold accented Latin letters and apostrophe variants (docs/03 §3 steps 1 and 4).
pub fn fold(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' => 'a',
        'À' | 'Â' | 'Ä' => 'A',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'î' | 'ï' => 'i',
        'Î' | 'Ï' => 'I',
        'ô' | 'ö' => 'o',
        'Ô' | 'Ö' => 'O',
        'ù' | 'û' | 'ü' => 'u',
        'Ù' | 'Û' | 'Ü' => 'U',
        'ç' => 's',
        'Ç' => 'S',
        'ñ' => 'n',
        'Ñ' => 'N',
        '’' | '‘' | 'ʼ' | '´' | '`' => '\'',
        c => c,
    }
}

/// True if the character can be part of an Arabizi token (after folding).
pub fn is_token_char(c: char) -> bool {
    let f = fold(c);
    f.is_ascii_alphanumeric() || f == '\''
}

/// The Latin buffer of the word being typed: raw text + normalized symbols.
#[derive(Clone, Debug, Default)]
pub struct LatinBuffer {
    raw: Vec<InputChar>,
    syms: Vec<char>,
}

impl LatinBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a character. Returns false (and changes nothing) if it cannot be part of a token.
    pub fn push(&mut self, c: InputChar) -> bool {
        if !is_token_char(c.ch) {
            return false;
        }
        self.raw.push(c);
        self.recompute();
        true
    }

    /// Remove the last raw character. Returns false if empty.
    pub fn pop(&mut self) -> bool {
        if self.raw.pop().is_none() {
            return false;
        }
        self.recompute();
        true
    }

    pub fn clear(&mut self) {
        self.raw.clear();
        self.syms.clear();
    }

    pub fn set(&mut self, latin: &str) {
        self.clear();
        for ch in latin.chars() {
            if is_token_char(ch) {
                self.raw.push(InputChar::new(ch));
            }
        }
        self.recompute();
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Exactly what was typed (for the raw-Latin candidate and Esc).
    pub fn raw(&self) -> String {
        self.raw.iter().map(|c| c.ch).collect()
    }

    /// Normalized symbols `L` (lowercase, emphatic symbols, elongation collapsed).
    pub fn syms(&self) -> &[char] {
        &self.syms
    }

    /// Normalized key as a `String` with emphatics lowered back to ASCII (for phrase / user-model keys).
    pub fn key(&self) -> String {
        self.syms
            .iter()
            .map(|&c| match c {
                EMPH_T => 't',
                EMPH_S => 's',
                EMPH_D => 'd',
                EMPH_Z => 'z',
                EMPH_H => 'h',
                c => c,
            })
            .filter(|&c| c != '\'')
            .collect()
    }

    /// Token is a number: all digits, or contains a literal (numpad) digit (docs/03 §6.1).
    pub fn is_number(&self) -> bool {
        !self.raw.is_empty()
            && (self.raw.iter().any(|c| c.literal_digit)
                || self.raw.iter().all(|c| c.ch.is_ascii_digit()))
    }

    fn recompute(&mut self) {
        self.syms.clear();
        let folded: Vec<char> = self.raw.iter().map(|c| fold(c.ch)).collect();
        let letters: Vec<char> = folded
            .iter()
            .copied()
            .filter(|c| c.is_ascii_alphabetic())
            .collect();
        let all_upper = letters.len() >= 2 && letters.iter().all(|c| c.is_ascii_uppercase());
        for (i, &c) in folded.iter().enumerate() {
            let s = if c.is_ascii_uppercase() {
                if i == 0 || all_upper {
                    c.to_ascii_lowercase()
                } else {
                    emphatic_symbol(c).unwrap_or(c.to_ascii_lowercase())
                }
            } else {
                c
            };
            // Elongation: a run of >= 3 identical symbols collapses to 2.
            let n = self.syms.len();
            if n >= 2 && self.syms[n - 1] == s && self.syms[n - 2] == s {
                continue;
            }
            self.syms.push(s);
        }
    }
}

/// Printable form of a normalized symbol (emphatics shown as uppercase) for debugging output.
pub fn symbol_display(c: char) -> char {
    match c {
        EMPH_T => 'T',
        EMPH_S => 'S',
        EMPH_D => 'D',
        EMPH_Z => 'Z',
        EMPH_H => 'H',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> LatinBuffer {
        let mut b = LatinBuffer::new();
        b.set(s);
        b
    }

    #[test]
    fn case_rules() {
        assert_eq!(buf("Sba7").syms(), &['s', 'b', 'a', '7']); // first letter never emphatic
        assert_eq!(buf("mSTTi").syms(), &['m', EMPH_S, EMPH_T, EMPH_T, 'i']);
        assert_eq!(buf("ISA").syms(), &['i', 's', 'a']); // all caps → lowercase
        assert_eq!(buf("maRhaba").syms(), &['m', 'a', 'r', 'h', 'a', 'b', 'a']);
        // non-emphatic caps lowered
    }

    #[test]
    fn elongation_and_accents() {
        assert_eq!(buf("7abibiiiii").key(), "7abibii");
        assert_eq!(buf("chérie").key(), "cherie");
        assert_eq!(buf("3’").syms(), &['3', '\'']);
    }

    #[test]
    fn numbers_and_raw() {
        assert!(buf("2026").is_number());
        assert!(!buf("3ala").is_number());
        let mut b = LatinBuffer::new();
        b.push(InputChar::numpad('3'));
        assert!(b.is_number());
        assert_eq!(buf("Mar7aba").raw(), "Mar7aba");
        let mut b = LatinBuffer::new();
        assert!(!b.push(InputChar::new(',')));
        assert!(b.is_empty());
    }
}
