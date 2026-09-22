//! Seed tables (`data/seed/*.tsv`): transliteration rules, phrases, vowel marks, region priors.
//! Used directly in seed-only mode (M0/M1) and as the prior for EM in the pipeline (docs/04 §6).

use crate::alphabet;
use crate::dialect::{self, Dialect, Posterior};
use crate::normalize::emphatic_symbol;
use std::collections::HashMap;

pub const POS_I: u8 = 1;
pub const POS_M: u8 = 2;
pub const POS_F: u8 = 4;
pub const POS_ANY: u8 = POS_I | POS_M | POS_F;

pub const F_VOWEL: u8 = 1;
pub const F_GEM: u8 = 2;
pub const F_TANWEEN: u8 = 4;
pub const F_ARTICLE: u8 = 8;

/// One row of `mappings.tsv`.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleRow {
    /// Latin chunk as normalized symbols (emphatics mapped).
    pub latin: Vec<char>,
    /// Arabic chunk as T3A codes (0..=3).
    pub arabic: Vec<u8>,
    pub weight: f32,
    /// `POS_*` bits; `explicit_pos` is false when the file said `*`.
    pub pos: u8,
    pub explicit_pos: bool,
    /// Dialect bitmask; 0 = `*`.
    pub dialects: u8,
    pub flags: u8,
}

/// A resolved rule for a given (position set, dialect posterior): log-probability mixed over dialects.
#[derive(Clone, Debug)]
pub struct EffRule {
    pub arabic: Vec<u8>,
    pub lp: f32,
    pub flags: u8,
}

#[derive(Clone, Debug)]
pub struct Phrase {
    pub key: String,
    pub output: String,
    pub dialects: u8,
    pub default: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SeedTables {
    pub rules: Vec<RuleRow>,
    pub phrases: Vec<Phrase>,
    /// (latin vowel chunk, dialect mask, mark char)
    pub vowel_marks: Vec<(String, u8, char)>,
    /// ISO-2 → prior
    pub region_priors: HashMap<String, Posterior>,
    by_chunk: HashMap<Vec<char>, Vec<usize>>,
    max_chunk: usize,
}

fn data_lines(tsv: &str) -> impl Iterator<Item = (usize, Vec<&str>)> {
    tsv.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .skip(1) // header row
        .map(|(i, l)| (i + 1, l.split('\t').collect()))
}

fn parse_latin(s: &str) -> Vec<char> {
    s.chars().map(|c| emphatic_symbol(c).unwrap_or(c)).collect()
}

fn parse_pos(s: &str) -> Result<(u8, bool), String> {
    if s == "*" {
        return Ok((POS_ANY, false));
    }
    let mut p = 0;
    for c in s.chars() {
        p |= match c {
            'I' => POS_I,
            'M' => POS_M,
            'F' => POS_F,
            _ => return Err(format!("bad pos '{s}'")),
        };
    }
    Ok((p, true))
}

fn parse_flags(s: &str) -> Result<u8, String> {
    if s == "-" || s.is_empty() {
        return Ok(0);
    }
    let mut f = 0;
    for part in s.split(',') {
        f |= match part.trim() {
            "VOWEL" => F_VOWEL,
            "GEM" => F_GEM,
            "TANWEEN" => F_TANWEEN,
            "ARTICLE" => F_ARTICLE,
            other => return Err(format!("bad flag '{other}'")),
        };
    }
    Ok(f)
}

impl SeedTables {
    /// Parse the four seed files. Errors carry file:line.
    pub fn parse(
        mappings: &str,
        phrases: &str,
        vowel_marks: &str,
        region_priors: &str,
    ) -> Result<Self, String> {
        let mut t = SeedTables::default();
        for (line, c) in data_lines(mappings) {
            if c.len() < 6 {
                return Err(format!("mappings.tsv:{line}: expected >= 6 columns"));
            }
            let arabic = if c[1] == "ε" {
                Vec::new()
            } else {
                alphabet::encode(c[1]).ok_or(format!(
                    "mappings.tsv:{line}: non-alphabet Arabic '{}'",
                    c[1]
                ))?
            };
            if arabic.len() > 3 {
                return Err(format!("mappings.tsv:{line}: Arabic chunk longer than 3"));
            }
            let latin = parse_latin(c[0]);
            if latin.is_empty() || latin.len() > 4 {
                return Err(format!(
                    "mappings.tsv:{line}: Latin chunk must be 1-4 symbols"
                ));
            }
            let weight: f32 = c[2]
                .parse()
                .map_err(|_| format!("mappings.tsv:{line}: bad weight"))?;
            let (pos, explicit_pos) =
                parse_pos(c[3]).map_err(|e| format!("mappings.tsv:{line}: {e}"))?;
            let dialects =
                dialect::parse_mask(c[4]).map_err(|e| format!("mappings.tsv:{line}: {e}"))?;
            let flags = parse_flags(c[5]).map_err(|e| format!("mappings.tsv:{line}: {e}"))?;
            t.rules.push(RuleRow {
                latin,
                arabic,
                weight,
                pos,
                explicit_pos,
                dialects,
                flags,
            });
        }
        for (line, c) in data_lines(phrases) {
            if c.len() < 4 {
                return Err(format!("phrases.tsv:{line}: expected >= 4 columns"));
            }
            t.phrases.push(Phrase {
                key: c[0].to_string(),
                output: c[1].to_string(),
                dialects: dialect::parse_mask(c[2])
                    .map_err(|e| format!("phrases.tsv:{line}: {e}"))?,
                default: c[3].trim() == "1",
            });
        }
        for (line, c) in data_lines(vowel_marks) {
            if c.len() < 3 {
                return Err(format!("vowel_marks.tsv:{line}: expected 3 columns"));
            }
            let mark = match c[2].trim() {
                "fatha" => crate::arabic::FATHA,
                "damma" => crate::arabic::DAMMA,
                "kasra" => crate::arabic::KASRA,
                other => return Err(format!("vowel_marks.tsv:{line}: bad mark '{other}'")),
            };
            let mask =
                dialect::parse_mask(c[1]).map_err(|e| format!("vowel_marks.tsv:{line}: {e}"))?;
            t.vowel_marks.push((c[0].to_string(), mask, mark));
        }
        for (line, c) in data_lines(region_priors) {
            if c.len() < 7 {
                return Err(format!("region_priors.tsv:{line}: expected 7 columns"));
            }
            let mut p = [0f32; 6];
            for i in 0..6 {
                p[i] = c[i + 1]
                    .parse()
                    .map_err(|_| format!("region_priors.tsv:{line}: bad number"))?;
            }
            t.region_priors.insert(c[0].to_string(), p);
        }
        t.index();
        Ok(t)
    }

    /// The seed files compiled into the binary (for tests, CLI seed-only mode and the TIP bootstrap).
    pub fn builtin() -> Self {
        Self::parse(
            include_str!("../../../data/seed/mappings.tsv"),
            include_str!("../../../data/seed/phrases.tsv"),
            include_str!("../../../data/seed/vowel_marks.tsv"),
            include_str!("../../../data/seed/region_priors.tsv"),
        )
        .expect("built-in seed tables must parse")
    }

    fn index(&mut self) {
        self.by_chunk.clear();
        self.max_chunk = 0;
        for (i, r) in self.rules.iter().enumerate() {
            self.max_chunk = self.max_chunk.max(r.latin.len());
            self.by_chunk.entry(r.latin.clone()).or_default().push(i);
        }
    }

    pub fn max_chunk_len(&self) -> usize {
        self.max_chunk
    }

    pub fn has_chunk(&self, chunk: &[char]) -> bool {
        self.by_chunk.contains_key(chunk)
    }

    /// Rows applicable to `chunk` for position set `pos` and a single dialect, after "most specific wins"
    /// resolution (header of mappings.tsv), with weights normalized to probabilities.
    pub fn resolve(&self, chunk: &[char], pos: u8, d: Dialect) -> Vec<(&RuleRow, f32)> {
        let Some(ids) = self.by_chunk.get(chunk) else {
            return Vec::new();
        };
        let rows: Vec<&RuleRow> = ids
            .iter()
            .map(|&i| &self.rules[i])
            .filter(|r| r.pos & pos != 0)
            .collect();
        let explicit: Vec<&RuleRow> = rows.iter().copied().filter(|r| r.explicit_pos).collect();
        let rows = if explicit.is_empty() { rows } else { explicit };
        let named: Vec<&RuleRow> = rows
            .iter()
            .copied()
            .filter(|r| r.dialects & d.bit() != 0)
            .collect();
        let rows: Vec<&RuleRow> = if named.is_empty() {
            rows.into_iter().filter(|r| r.dialects == 0).collect()
        } else {
            named
        };
        let total: f32 = rows.iter().map(|r| r.weight).sum();
        if total <= 0.0 {
            return Vec::new();
        }
        rows.into_iter().map(|r| (r, r.weight / total)).collect()
    }

    /// Dialect-mixed effective rules: `lp = ln Σ_d π_d P_d(α | ℓ, pos)` (docs/03 §4.2).
    /// Rules with the same Arabic output and flags are merged.
    pub fn effective(&self, chunk: &[char], pos: u8, pi: &Posterior) -> Vec<EffRule> {
        let mut acc: Vec<(Vec<u8>, u8, f32)> = Vec::new();
        for d in dialect::ALL {
            let w = pi[d as usize];
            if w <= 0.0 {
                continue;
            }
            for (r, p) in self.resolve(chunk, pos, d) {
                match acc
                    .iter_mut()
                    .find(|(a, f, _)| *a == r.arabic && *f == r.flags)
                {
                    Some(e) => e.2 += w * p,
                    None => acc.push((r.arabic.clone(), r.flags, w * p)),
                }
            }
        }
        acc.into_iter()
            .filter(|(_, _, p)| *p > 0.0)
            .map(|(arabic, flags, p)| EffRule {
                arabic,
                lp: p.ln(),
                flags,
            })
            .collect()
    }

    /// Short-vowel mark for a Latin vowel chunk under dialect `d` (docs/03 §10.6), most specific row wins.
    pub fn vowel_mark(&self, latin: &str, d: Dialect) -> Option<char> {
        let rows: Vec<&(String, u8, char)> = self
            .vowel_marks
            .iter()
            .filter(|(l, _, _)| l == latin)
            .collect();
        rows.iter()
            .find(|(_, m, _)| m & d.bit() != 0)
            .or_else(|| rows.iter().find(|(_, m, _)| *m == 0))
            .map(|(_, _, c)| *c)
    }

    /// Phrases whose key equals `key`, filtered by dialect posterior (docs/03 §6.2).
    pub fn phrases_for<'a>(
        &'a self,
        key: &'a str,
        pi: &'a Posterior,
    ) -> impl Iterator<Item = &'a Phrase> + 'a {
        self.phrases.iter().filter(move |p| {
            p.key == key
                && (p.dialects == 0
                    || dialect::ALL
                        .iter()
                        .any(|d| p.dialects & d.bit() != 0 && pi[*d as usize] >= 0.15))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_parses_and_normalizes() {
        let t = SeedTables::builtin();
        assert!(t.rules.len() > 300);
        assert!(t.max_chunk_len() <= 4);
        // "9" in Morocco is mostly ق, elsewhere mostly ص.
        let mag = t.resolve(&['9'], POS_M, Dialect::Mag);
        let top = mag.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert_eq!(alphabet::decode(&top.0.arabic), "ق");
        let lev = t.resolve(&['9'], POS_M, Dialect::Lev);
        let top = lev.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert_eq!(alphabet::decode(&top.0.arabic), "ص");
        // Probabilities of a resolved group sum to 1.
        let s: f32 = t
            .resolve(&['a'], POS_F, Dialect::Egy)
            .iter()
            .map(|x| x.1)
            .sum();
        assert!((s - 1.0).abs() < 1e-4);
    }

    #[test]
    fn explicit_position_overrides_star() {
        let t = SeedTables::builtin();
        // 'h' has a '*' group and an explicit F group; at F only the explicit rows apply.
        let f = t.resolve(&['h'], POS_F, Dialect::Lev);
        assert!(f.iter().all(|(r, _)| r.explicit_pos));
        let m = t.resolve(&['h'], POS_M, Dialect::Lev);
        assert!(m.iter().all(|(r, _)| !r.explicit_pos));
    }

    #[test]
    fn region_priors_sum_to_one() {
        let t = SeedTables::builtin();
        assert!(t.region_priors.contains_key("JO") && t.region_priors.contains_key("XX"));
        for p in t.region_priors.values() {
            assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn vowel_marks_by_dialect() {
        let t = SeedTables::builtin();
        assert_eq!(t.vowel_mark("e", Dialect::Lev), Some(crate::arabic::KASRA));
        assert_eq!(t.vowel_mark("e", Dialect::Msa), Some(crate::arabic::FATHA));
        assert_eq!(t.vowel_mark("a", Dialect::Mag), Some(crate::arabic::FATHA));
    }
}
