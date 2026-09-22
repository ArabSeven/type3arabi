//! User model interface (docs/03 §9). The persistent, multi-process store (journal + snapshot) is an M3/M4
//! task; `MemoryUser` implements the scoring and update rules in memory and is used by tests and the CLI.

use std::collections::HashMap;

/// What the ranking needs from the user model.
pub trait UserScorer {
    /// `USR(L, w)` feature (docs/03 §9.2). 0.0 when nothing is known.
    fn usr(&self, key: &str, word: &str) -> f32;
    /// Last choice for this key if it should be pinned to rank 1 (docs/03 §8 step 4).
    fn sticky(&self, key: &str) -> Option<String>;
}

/// A user model that knows nothing (learning disabled, or first run).
pub struct NoUser;

impl UserScorer for NoUser {
    fn usr(&self, _: &str, _: &str) -> f32 {
        0.0
    }
    fn sticky(&self, _: &str) -> Option<String> {
        None
    }
}

/// Word marker used for "the user chose raw Latin for this key".
pub const RAW_LATIN: &str = "\u{0}RAW";

#[derive(Clone, Debug, Default)]
struct Choice {
    word: String,
    count: u32,
    last: u64,
}

/// In-memory implementation of the docs/03 §9 rules (no time decay: `seq` is a logical clock).
#[derive(Default)]
pub struct MemoryUser {
    choices: HashMap<String, Vec<Choice>>,
    neg: HashMap<(String, String), u32>,
    words: HashMap<String, u32>,
    clock: u64,
}

impl MemoryUser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a commit (docs/03 §9.3). `word` is the base form (or `RAW_LATIN`); `top` is the rank-1 base form.
    pub fn record(
        &mut self,
        key: &str,
        word: &str,
        rank: usize,
        navigated: bool,
        top: Option<&str>,
    ) {
        if key.len() > 32 {
            return;
        }
        self.clock += 1;
        let list = self.choices.entry(key.to_string()).or_default();
        match list.iter_mut().find(|c| c.word == word) {
            Some(c) => {
                c.count += 1;
                c.last = self.clock;
            }
            None => list.push(Choice {
                word: word.to_string(),
                count: 1,
                last: self.clock,
            }),
        }
        list.sort_by_key(|c| std::cmp::Reverse(c.last));
        list.truncate(4);
        *self.words.entry(word.to_string()).or_default() += 1;
        // Choosing w explicitly clears any earlier negative evidence against it (docs/03 §9.3).
        self.neg.remove(&(key.to_string(), word.to_string()));
        if rank > 0 && navigated {
            if let Some(t) = top {
                if t != word {
                    *self
                        .neg
                        .entry((key.to_string(), t.to_string()))
                        .or_default() += 1;
                }
            }
        }
    }
}

impl UserScorer for MemoryUser {
    fn usr(&self, key: &str, word: &str) -> f32 {
        let c = self
            .choices
            .get(key)
            .and_then(|l| l.iter().find(|c| c.word == word))
            .map_or(0, |c| c.count) as f32;
        let cw = *self.words.get(word).unwrap_or(&0) as f32;
        let n = *self
            .neg
            .get(&(key.to_string(), word.to_string()))
            .unwrap_or(&0) as f32;
        (1.0 + c).ln() + 0.3 * (1.0 + cw).ln() - 0.7 * (1.0 + n).ln()
    }

    fn sticky(&self, key: &str) -> Option<String> {
        let last = self.choices.get(key)?.first()?;
        let negated = self
            .neg
            .get(&(key.to_string(), last.word.clone()))
            .copied()
            .unwrap_or(0);
        (negated == 0).then(|| last.word.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sticky_last_choice() {
        let mut u = MemoryUser::new();
        assert_eq!(u.sticky("hala2"), None);
        u.record("hala2", "هلق", 2, true, Some("هلأ"));
        assert_eq!(u.sticky("hala2").as_deref(), Some("هلق"));
        assert!(u.usr("hala2", "هلق") > u.usr("hala2", "هلأ"));
        u.record("hala2", "هلأ", 1, true, Some("هلق"));
        assert_eq!(u.sticky("hala2").as_deref(), Some("هلأ"));
    }
}
