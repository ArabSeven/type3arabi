//! Character 5-gram language model over T3A codes (docs/03 §7.4, docs/12 §7).
//!
//! The `CHLM` section is an open-addressing table (linear probing, power-of-two capacity) of
//! `{ fp, q, order }`. `fp` is the FNV-1a-32 hash of `[order, context codes…, next code]` (0 is
//! reserved for empty slots). `q` is the quantized conditional log-probability `ln P(next | context)`.
//! Scoring uses stupid backoff: longest available order first, `ln α` per backoff step.

use t3a_data::{dequantize_lp, ChlmEntry};

/// Beginning-of-word symbol (after the 42 letter codes).
pub const BOS: u8 = 43;
/// End-of-word symbol.
pub const EOS: u8 = 44;
/// Maximum n-gram order stored.
pub const ORDER: usize = 5;
/// Log-probability used when not even the unigram is known.
pub const FLOOR_LP: f32 = -12.0;
/// Stupid-backoff weight α (docs/12 §7).
pub const BACKOFF: f32 = 0.4;

/// FNV-1a-32 fingerprint of an n-gram key (`order` then the codes). Never returns 0.
pub fn fingerprint(codes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in std::iter::once(&(codes.len() as u8)).chain(codes) {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    if h == 0 {
        1
    } else {
        h
    }
}

/// Slot index for `fp` in a table of `capacity` (a power of two).
pub fn slot(fp: u32, capacity: usize) -> usize {
    (fp as usize) & (capacity - 1)
}

/// Read-only view of the `CHLM` table.
#[derive(Clone, Copy)]
pub struct CharLm<'a> {
    table: &'a [ChlmEntry],
}

impl<'a> CharLm<'a> {
    /// Wrap a table; `None` when it is empty or its capacity is not a power of two.
    pub fn new(table: &'a [ChlmEntry]) -> Option<Self> {
        (table.len() > 1 && table.len().is_power_of_two()).then_some(Self { table })
    }

    fn find(&self, codes: &[u8]) -> Option<f32> {
        let fp = fingerprint(codes);
        let cap = self.table.len();
        let mut i = slot(fp, cap);
        for _ in 0..cap {
            let e = &self.table[i];
            if e.fp == 0 {
                return None;
            }
            if e.fp == fp && e.order as usize == codes.len() {
                return Some(dequantize_lp(e.q));
            }
            i = (i + 1) & (cap - 1);
        }
        None
    }

    /// `ln P(next | ctx)` with stupid backoff; `ctx` is the preceding codes (BOS first), any length.
    pub fn lp(&self, ctx: &[u8], next: u8) -> f32 {
        let mut key = [0u8; ORDER];
        let max_ctx = ctx.len().min(ORDER - 1);
        let mut penalty = 0.0;
        for k in (0..=max_ctx).rev() {
            key[..k].copy_from_slice(&ctx[ctx.len() - k..]);
            key[k] = next;
            if let Some(lp) = self.find(&key[..=k]) {
                return lp + penalty;
            }
            penalty += BACKOFF.ln();
        }
        FLOOR_LP
    }

    /// Log-probability of a whole word (letters as T3A codes), including BOS context and EOS.
    pub fn word_lp(&self, letters: &[u8]) -> f32 {
        let mut ctx = Vec::with_capacity(letters.len() + 1);
        ctx.push(BOS);
        let mut total = 0.0;
        for &c in letters {
            total += self.lp(&ctx, c);
            ctx.push(c);
        }
        total + self.lp(&ctx, EOS)
    }
}

/// Build a table from `(codes, lp)` entries (codes = context… + next; 1..=ORDER long).
/// Capacity = smallest power of two ≥ 1.6 × entries (load ≤ ~0.63).
pub fn build_table(entries: &[(Vec<u8>, f32)]) -> Vec<ChlmEntry> {
    let cap = ((entries.len() as f32 * 1.6) as usize)
        .max(2)
        .next_power_of_two();
    let mut table = vec![
        ChlmEntry {
            fp: 0,
            q: 255,
            order: 0,
            pad: 0,
        };
        cap
    ];
    for (codes, lp) in entries {
        if codes.is_empty() || codes.len() > ORDER {
            continue;
        }
        let fp = fingerprint(codes);
        let mut i = slot(fp, cap);
        while table[i].fp != 0 {
            if table[i].fp == fp && table[i].order as usize == codes.len() {
                break; // duplicate key: keep the first (highest-count) entry
            }
            i = (i + 1) & (cap - 1);
        }
        if table[i].fp == 0 {
            table[i] = ChlmEntry {
                fp,
                q: t3a_data::quantize_lp(*lp),
                order: codes.len() as u8,
                pad: 0,
            };
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_backoff() {
        // P(ب | ^) = -1.0, P(ب) = -3.0 ; codes: ب = 8
        let t = build_table(&[(vec![BOS, 8], -1.0), (vec![8], -3.0)]);
        let lm = CharLm::new(&t).unwrap();
        assert!((lm.lp(&[BOS], 8) - -1.0).abs() < 0.2);
        // context (BOS, ب) unseen at orders 3 and 2 -> unigram after two backoff steps
        let backed = lm.lp(&[BOS, 8], 8);
        assert!((backed - (-3.0 + 2.0 * BACKOFF.ln())).abs() < 0.2);
        // unknown symbol -> floor
        assert_eq!(lm.lp(&[BOS], 20), FLOOR_LP);
    }

    #[test]
    fn fingerprint_never_zero_and_order_sensitive() {
        assert_ne!(fingerprint(&[]), 0);
        assert_ne!(fingerprint(&[1, 2]), fingerprint(&[2, 1]));
    }
}
