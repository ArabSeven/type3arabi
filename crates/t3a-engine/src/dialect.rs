//! Dialect groups (docs/03 §2, §7.2).

/// The six dialect groups, in the fixed index order used everywhere (data file, config, π).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    Msa = 0,
    Lev = 1,
    Egy = 2,
    Glf = 3,
    Irq = 4,
    Mag = 5,
}

pub const ALL: [Dialect; 6] = [
    Dialect::Msa,
    Dialect::Lev,
    Dialect::Egy,
    Dialect::Glf,
    Dialect::Irq,
    Dialect::Mag,
];

impl Dialect {
    pub fn code(self) -> &'static str {
        ["MSA", "LEV", "EGY", "GLF", "IRQ", "MAG"][self as usize]
    }
    #[inline]
    pub fn as_str(self) -> &'static str {
        self.code()
    }
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
    /// Arabic display name for the popup badge (docs/05 §7).
    pub fn arabic_name(self) -> &'static str {
        ["فصحى", "شامي", "مصري", "خليجي", "عراقي", "مغربي"][self as usize]
    }
    pub fn parse(s: &str) -> Option<Dialect> {
        ALL.iter()
            .copied()
            .find(|d| d.code().eq_ignore_ascii_case(s.trim()))
    }
    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// Parse a dialect list like `*` or `LEV,EGY` into a bitmask (0 = any).
pub fn parse_mask(s: &str) -> Result<u8, String> {
    let s = s.trim();
    if s == "*" || s.is_empty() {
        return Ok(0);
    }
    let mut m = 0u8;
    for part in s.split(',') {
        m |= Dialect::parse(part)
            .ok_or_else(|| format!("unknown dialect '{part}'"))?
            .bit();
    }
    Ok(m)
}

/// Dialect posterior π (sums to 1).
pub type Posterior = [f32; 6];

/// A fixed-profile posterior (docs/03 §7.2): 0.8 on `d`, 0.15 MSA (or 0.95 MSA if d = MSA), rest spread.
pub fn fixed_profile(d: Dialect) -> Posterior {
    let mut p = [0.0f32; 6];
    if d == Dialect::Msa {
        p = [0.95, 0.01, 0.01, 0.01, 0.01, 0.01];
    } else {
        for (i, v) in p.iter_mut().enumerate() {
            *v = if i == d as usize {
                0.8
            } else if i == 0 {
                0.15
            } else {
                0.05 / 4.0
            };
        }
    }
    p
}

/// Default prior when nothing is known (docs/03 §7.2; region table row `XX`).
pub const DEFAULT_PRIOR: Posterior = [0.25, 0.20, 0.25, 0.15, 0.05, 0.10];

/// The dominant dialect of a posterior (for the popup badge).
pub fn argmax(p: &Posterior) -> Dialect {
    let mut best = 0;
    for i in 1..6 {
        if p[i] > p[best] {
            best = i;
        }
    }
    ALL[best]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_and_profiles() {
        assert_eq!(parse_mask("*").unwrap(), 0);
        assert_eq!(parse_mask("LEV,EGY").unwrap(), 0b110);
        assert!(parse_mask("XYZ").is_err());
        for d in ALL {
            let p = fixed_profile(d);
            assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-5);
            assert_eq!(argmax(&p), d);
        }
    }
}
