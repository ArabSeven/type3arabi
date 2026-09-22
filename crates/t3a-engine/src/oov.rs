//! Unconstrained lattice search (no lexicon). In seed-only mode this is the whole engine; with a
//! lexicon (M3) it becomes the OOV generator and gains the char-LM term (docs/03 §5.6.3).
//!
//! M3 NOTE: this baseline recomputes the lattice per keystroke and allocates. The M3 implementation
//! must be incremental and allocation-free (docs/03 §5.1, §5.7). Keep this module's *behavior* as the
//! reference for the OOV path's tests.

use crate::dialect::Posterior;
use crate::params::EngineParams;
use crate::seed::{EffRule, SeedTables, F_GEM, F_VOWEL, POS_F, POS_I, POS_M};
use std::collections::{HashMap, HashSet};

const CODE_ALEF: u8 = 7;
const CODE_WAW: u8 = 34;
/// Penalty for skipping a symbol that no rule covers (e.g. `0`, `1`).
const UNKNOWN_SYMBOL_LP: f32 = -10.0;

/// One aligned chunk of the winning path.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub latin: Vec<char>,
    pub arabic: Vec<u8>,
    pub flags: u8,
}

/// A hypothesis: Arabic letters (T3A codes), score (Σ lp), and its alignment.
#[derive(Clone, Debug)]
pub struct Hyp {
    pub letters: Vec<u8>,
    pub score: f32,
    pub steps: Vec<Step>,
}

fn is_vowel_sym(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

fn pos_set(start: usize, end: usize, n: usize) -> u8 {
    let mut p = 0;
    if start == 0 {
        p |= POS_I;
    }
    if end == n {
        p |= POS_F;
    }
    if p == 0 {
        p = POS_M;
    }
    p
}

/// Rules for `chunk` at a position set, including generic gemination (docs/03 §4.3).
fn rules_for(
    seed: &SeedTables,
    params: &EngineParams,
    pi: &Posterior,
    chunk: &[char],
    pos: u8,
) -> Vec<EffRule> {
    let mut rules = seed.effective(chunk, pos, pi);
    let k = chunk.len();
    let half = k / 2;
    let doubled = k.is_multiple_of(2) && k >= 2 && chunk[..half] == chunk[half..];
    if doubled && chunk[..half].iter().all(|&c| !is_vowel_sym(c) && c != '\'') {
        let single = &chunk[..half];
        if half == 1 || seed.has_chunk(single) {
            for r in seed.effective(single, pos, pi) {
                if r.arabic.len() == 1 && r.flags & F_VOWEL == 0 {
                    rules.push(EffRule {
                        arabic: r.arabic,
                        lp: r.lp + params.p_gem.ln(),
                        flags: r.flags | F_GEM,
                    });
                }
            }
        }
    }
    if rules.is_empty() && k == 1 {
        rules.push(EffRule {
            arabic: Vec::new(),
            lp: UNKNOWN_SYMBOL_LP,
            flags: 0,
        });
    }
    rules
}

/// Best `k` distinct Arabic readings of `syms`.
pub fn search(
    seed: &SeedTables,
    params: &EngineParams,
    pi: &Posterior,
    syms: &[char],
    k: usize,
) -> Vec<Hyp> {
    let n = syms.len();
    if n == 0 {
        return Vec::new();
    }
    let beam = params.beam_oov * 2;
    let max_chunk = seed.max_chunk_len().max(2);
    let mut cache: HashMap<(usize, usize), Vec<EffRule>> = HashMap::new();
    let mut cols: Vec<Vec<Hyp>> = vec![Vec::new(); n + 1];
    cols[0].push(Hyp {
        letters: Vec::new(),
        score: 0.0,
        steps: Vec::new(),
    });
    let double_penalty = (1.0 - params.p_gem).max(1e-6).ln();

    for j in 0..n {
        prune(&mut cols[j], beam);
        let current = std::mem::take(&mut cols[j]);
        for len in 1..=max_chunk.min(n - j) {
            let end = j + len;
            let chunk = &syms[j..end];
            let rules = cache
                .entry((j, end))
                .or_insert_with(|| rules_for(seed, params, pi, chunk, pos_set(j, end, n)))
                .clone();
            if rules.is_empty() {
                continue;
            }
            for h in &current {
                for r in &rules {
                    let mut score = h.score + r.lp;
                    // Two separate transitions of the same consonant to the same letter: the gemination
                    // reading is preferred, so the two-letter reading pays ln(1 - p_gem) (docs/03 §4.3).
                    if let Some(prev) = h.steps.last() {
                        if prev.latin == chunk
                            && prev.arabic == r.arabic
                            && r.flags & F_VOWEL == 0
                            && !chunk.iter().any(|&c| is_vowel_sym(c))
                        {
                            score += double_penalty;
                        }
                    }
                    let mut letters = h.letters.clone();
                    letters.extend_from_slice(&r.arabic);
                    let mut steps = h.steps.clone();
                    steps.push(Step {
                        latin: chunk.to_vec(),
                        arabic: r.arabic.clone(),
                        flags: r.flags,
                    });
                    cols[end].push(Hyp {
                        letters,
                        score,
                        steps,
                    });
                }
            }
        }
        cols[j] = current;
    }

    let mut finals = std::mem::take(&mut cols[n]);
    // wāw al-jamāʿa insertion (docs/03 §4.4)
    let extra: Vec<Hyp> = finals
        .iter()
        .filter(|h| {
            h.letters.last() == Some(&CODE_WAW)
                && h.steps.last().is_some_and(|s| s.flags & F_VOWEL != 0)
        })
        .map(|h| {
            let mut h2 = h.clone();
            h2.letters.push(CODE_ALEF);
            h2.score += params.p_waw_alif.ln();
            h2.steps.push(Step {
                latin: Vec::new(),
                arabic: vec![CODE_ALEF],
                flags: 0,
            });
            h2
        })
        .collect();
    finals.extend(extra);
    prune(&mut finals, k);
    finals.retain(|h| !h.letters.is_empty());
    finals
}

/// Sort by score, keep the best hypothesis per letter sequence (Viterbi recombination), truncate.
fn prune(col: &mut Vec<Hyp>, beam: usize) {
    col.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    col.retain(|h| seen.insert(h.letters.clone()));
    col.truncate(beam);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::decode;
    use crate::dialect::{fixed_profile, Dialect};
    use crate::normalize::LatinBuffer;

    fn top(s: &str, d: Dialect, k: usize) -> Vec<String> {
        let seed = SeedTables::builtin();
        let mut b = LatinBuffer::new();
        b.set(s);
        search(
            &seed,
            &EngineParams::default(),
            &fixed_profile(d),
            b.syms(),
            k,
        )
        .iter()
        .map(|h| decode(&h.letters))
        .collect()
    }

    #[test]
    fn simple_words_rank_first() {
        assert_eq!(top("7abibi", Dialect::Lev, 3)[0], "حبيبي");
        assert_eq!(top("mar7aba", Dialect::Lev, 3)[0], "مرحبا");
        assert!(top("3allam", Dialect::Msa, 3).contains(&"علم".to_string()));
    }

    #[test]
    fn dialect_changes_digit_nine() {
        assert_eq!(top("n9ra", Dialect::Mag, 1)[0], "نقرا");
        assert!(top("9a7", Dialect::Lev, 1)[0].starts_with('ص'));
    }

    #[test]
    fn never_empty_for_unknown_symbols() {
        assert!(!top("a0b", Dialect::Lev, 1).is_empty());
    }
}
