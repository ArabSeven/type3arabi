//! User model (docs/03 §9): `MemoryUser` implements the scoring and update rules in memory and its
//! snapshot encoding; `store::UserStore` persists it (journal + snapshot) across processes.

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

impl MemoryUser {
    /// Replay of a journal NEGATIVE record: `word` was rank 1 for `key` and the user navigated
    /// past it. Only the negative evidence is recorded — never a choice (docs/03 §9.3).
    pub fn record_negative(&mut self, key: &str, word: &str) {
        if key.len() > 32 || word.is_empty() {
            return;
        }
        *self
            .neg
            .entry((key.to_string(), word.to_string()))
            .or_default() += 1;
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

// ---------------------------------------------------------------------------------------------
// Snapshot (docs/03 §9.4): the compacted model, so the journal can be truncated.
//
// Layout (little-endian), all strings length-prefixed with one byte (keys ≤ 32 B, words ≤ 84 B):
//   magic u32 = 0x54335501 ('T3U' v1) | consumed u64 | prefix_hash u64 | clock u64
//   n_keys u32, per key: key, n u8, per choice: word, count u32, last u64
//   n_neg u32, per entry: key, word, count u32
//   n_words u32, per entry: word, count u32
//   fnv64 u64 over everything before it
// `consumed` = journal bytes already merged into this snapshot and `prefix_hash` = FNV-1a of the first
// min(consumed, 4096) bytes of that journal: a reader replays the journal from `consumed` only if the
// journal still starts with those bytes (not yet truncated), otherwise from 0 (already truncated to
// the unmerged tail). Either way nothing is applied twice or lost, even if compaction is interrupted.

pub const SNAPSHOT_MAGIC: u32 = 0x5433_5501;

/// FNV-1a 64 (snapshot checksum and journal prefix identity).
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Where the journal replay starts for a snapshot (see the layout above).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotMark {
    pub consumed: u64,
    pub prefix_hash: u64,
}

impl SnapshotMark {
    /// Journal prefix identity used by `prefix_hash`.
    pub fn prefix_of(journal: &[u8], consumed: u64) -> u64 {
        fnv64(&journal[..journal.len().min(consumed.min(4096) as usize)])
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) -> bool {
    let Ok(n) = u8::try_from(s.len()) else {
        return false;
    };
    out.push(n);
    out.extend_from_slice(s.as_bytes());
    true
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_le_bytes(a)
        })
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

impl MemoryUser {
    /// Keep the `max_keys` most recently used keys (docs/03 §9.4 cap, LRU by last use).
    pub fn evict(&mut self, max_keys: usize) {
        if self.choices.len() <= max_keys {
            return;
        }
        let mut lasts: Vec<u64> = self
            .choices
            .values()
            .map(|l| l.iter().map(|c| c.last).max().unwrap_or(0))
            .collect();
        lasts.sort_unstable_by(|a, b| b.cmp(a));
        let cutoff = lasts[max_keys - 1];
        self.choices
            .retain(|_, l| l.iter().map(|c| c.last).max().unwrap_or(0) >= cutoff);
        let keys: std::collections::HashSet<String> = self.choices.keys().cloned().collect();
        self.neg.retain(|(k, _), _| keys.contains(k));
    }

    /// Serialize the model (docs/03 §9.4 snapshot).
    pub fn encode_snapshot(&self, mark: SnapshotMark) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SNAPSHOT_MAGIC.to_le_bytes());
        out.extend_from_slice(&mark.consumed.to_le_bytes());
        out.extend_from_slice(&mark.prefix_hash.to_le_bytes());
        out.extend_from_slice(&self.clock.to_le_bytes());

        let mut keys: Vec<&String> = self.choices.keys().collect();
        keys.sort();
        let mut body = Vec::new();
        let mut n = 0u32;
        for k in keys {
            let list = &self.choices[k];
            let mut entry = Vec::new();
            if !put_str(&mut entry, k) {
                continue;
            }
            let kept: Vec<&Choice> = list.iter().filter(|c| c.word.len() <= 255).collect();
            entry.push(kept.len() as u8);
            for c in kept {
                put_str(&mut entry, &c.word);
                entry.extend_from_slice(&c.count.to_le_bytes());
                entry.extend_from_slice(&c.last.to_le_bytes());
            }
            body.extend_from_slice(&entry);
            n += 1;
        }
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&body);

        let mut negs: Vec<(&(String, String), &u32)> = self.neg.iter().collect();
        negs.sort();
        let mut body = Vec::new();
        let mut n = 0u32;
        for ((k, w), c) in negs {
            let mut entry = Vec::new();
            if put_str(&mut entry, k) && put_str(&mut entry, w) {
                entry.extend_from_slice(&c.to_le_bytes());
                body.extend_from_slice(&entry);
                n += 1;
            }
        }
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&body);

        let mut words: Vec<(&String, &u32)> = self.words.iter().collect();
        words.sort();
        let mut body = Vec::new();
        let mut n = 0u32;
        for (w, c) in words {
            let mut entry = Vec::new();
            if put_str(&mut entry, w) {
                entry.extend_from_slice(&c.to_le_bytes());
                body.extend_from_slice(&entry);
                n += 1;
            }
        }
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&body);

        let sum = fnv64(&out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    /// Parse a snapshot. `None` for anything malformed (bad magic, checksum or bounds).
    pub fn decode_snapshot(bytes: &[u8]) -> Option<(MemoryUser, SnapshotMark)> {
        if bytes.len() < 8 + 36 {
            return None;
        }
        let (data, sum) = bytes.split_at(bytes.len() - 8);
        let mut s = [0u8; 8];
        s.copy_from_slice(sum);
        if fnv64(data) != u64::from_le_bytes(s) {
            return None;
        }
        let mut r = Reader { b: data, at: 0 };
        if r.u32()? != SNAPSHOT_MAGIC {
            return None;
        }
        let mark = SnapshotMark {
            consumed: r.u64()?,
            prefix_hash: r.u64()?,
        };
        let mut u = MemoryUser::new();
        u.clock = r.u64()?;
        for _ in 0..r.u32()? {
            let key = r.str()?;
            let n = r.u8()?;
            let mut list = Vec::with_capacity(n as usize);
            for _ in 0..n {
                list.push(Choice {
                    word: r.str()?,
                    count: r.u32()?,
                    last: r.u64()?,
                });
            }
            u.choices.insert(key, list);
        }
        for _ in 0..r.u32()? {
            let k = r.str()?;
            let w = r.str()?;
            u.neg.insert((k, w), r.u32()?);
        }
        for _ in 0..r.u32()? {
            let w = r.str()?;
            u.words.insert(w, r.u32()?);
        }
        (r.at == data.len()).then_some((u, mark))
    }

    /// The model as journal records, for a `.t3learn` export after compaction (docs/03 §9.5): choices
    /// in the order they were last used (so the most recent choice per key stays first), each
    /// repeated `count` times up to `max_repeat`, then the negative evidence.
    pub fn to_records(&self, ts: u32, max_repeat: u32) -> Vec<crate::journal::JournalRecord> {
        use crate::journal::{JournalRecord, KIND_CHOOSE, KIND_NEGATIVE};
        let mut picks: Vec<(u64, &str, &Choice)> = self
            .choices
            .iter()
            .flat_map(|(k, l)| l.iter().map(move |c| (c.last, k.as_str(), c)))
            .collect();
        picks.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
        let mut out = Vec::new();
        for (_, key, c) in picks {
            for _ in 0..c.count.clamp(1, max_repeat) {
                if let Some(r) = JournalRecord::new(KIND_CHOOSE, ts, key, &c.word) {
                    out.push(r);
                }
            }
        }
        let mut negs: Vec<(&(String, String), &u32)> = self.neg.iter().collect();
        negs.sort();
        for ((k, w), n) in negs {
            for _ in 0..(*n).clamp(1, max_repeat) {
                if let Some(r) = JournalRecord::new(KIND_NEGATIVE, ts, k, w) {
                    out.push(r);
                }
            }
        }
        out
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

    #[test]
    fn snapshot_round_trips_and_rejects_corruption() {
        let mut u = MemoryUser::new();
        u.record("hala2", "هلق", 2, true, Some("هلأ")); // U+0647 U+0644 U+0642 / U+0647 U+0644 U+0623
        u.record("7abibi", "حبيبي", 0, false, None);
        u.record("hello", RAW_LATIN, 1, true, Some("هلو"));
        let mark = SnapshotMark {
            consumed: 384,
            prefix_hash: 7,
        };
        let bytes = u.encode_snapshot(mark);
        let (v, m) = MemoryUser::decode_snapshot(&bytes).expect("valid snapshot");
        assert_eq!(m, mark);
        for (k, w) in [
            ("hala2", "هلق"),
            ("hala2", "هلأ"),
            ("7abibi", "حبيبي"),
            ("hello", "هلو"),
        ] {
            assert_eq!(v.usr(k, w), u.usr(k, w), "{k} {w}");
        }
        assert_eq!(v.sticky("hello").as_deref(), Some(RAW_LATIN));
        assert_eq!(v.encode_snapshot(mark), bytes); // deterministic
        let mut bad = bytes.clone();
        bad[20] ^= 1;
        assert!(MemoryUser::decode_snapshot(&bad).is_none());
        assert!(MemoryUser::decode_snapshot(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn evict_keeps_the_most_recent_keys() {
        let mut u = MemoryUser::new();
        for i in 0..10 {
            u.record(&format!("k{i}"), "كلمة", 0, false, None); // U+0643 U+0644 U+0645 U+0629
        }
        u.record("k0", "كلمة", 0, false, None); // k0 is the most recent now
        u.evict(3);
        assert!(u.sticky("k0").is_some() && u.sticky("k9").is_some() && u.sticky("k8").is_some());
        assert!(u.sticky("k1").is_none());
    }

    #[test]
    fn records_replay_to_the_same_choices() {
        let mut u = MemoryUser::new();
        u.record("ahlan", "أهلا", 0, false, None); // U+0623 U+0647 U+0644 U+0627
        u.record("ahlan", "اهلا", 1, true, Some("أهلا")); // later choice wins the sticky slot
        let mut v = MemoryUser::new();
        for r in u.to_records(0, 16) {
            let (l, a) = (r.latin_str().unwrap(), r.arabic_str().unwrap());
            match r.kind {
                crate::journal::KIND_CHOOSE => v.record(l, a, 0, false, None),
                _ => v.record_negative(l, a),
            }
        }
        assert_eq!(v.sticky("ahlan"), u.sticky("ahlan"));
        assert!(v.usr("ahlan", "اهلا") > v.usr("ahlan", "أهلا"));
    }
}
