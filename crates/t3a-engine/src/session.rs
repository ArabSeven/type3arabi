//! `Engine` + `Session`: the public API of docs/03 §11.
//!
//! Current implementation = **seed-only mode** (M0/M1 bootstrap): candidates come from phrases, numbers,
//! laughter, article joiners and the unconstrained lattice (`oov`). M3 adds the lexicon trie, LM, context,
//! completions and the allocation-free incremental lattice behind the same API.

use crate::alphabet;
use crate::config::Config;
use crate::dialect::{self, Posterior};
use crate::display::{self, AllahForm, TanweenStyle};
use crate::normalize::{InputChar, LatinBuffer};
use crate::oov::{self, Hyp};
use crate::params::EngineParams;
use crate::seed::{SeedTables, F_TANWEEN};
use crate::tashkeel::{self, HarakatMode};
use crate::user::{UserScorer, RAW_LATIN};

#[derive(Debug)]
pub enum EngineError {
    /// The lexicon engine (data file) lands in M3; until then use `Engine::seed_only`.
    LexiconNotImplemented,
    BadData(String),
}

/// Immutable engine state shared by all sessions (Send + Sync).
pub struct Engine {
    seed: SeedTables,
    params: EngineParams,
}

impl Engine {
    /// Engine over a compiled data file (docs/12). M3.
    pub fn new(_data: t3a_data::DataView<'_>) -> Result<Self, EngineError> {
        Err(EngineError::LexiconNotImplemented)
    }

    /// Bootstrap engine over the seed tables only (OOV path + specials).
    pub fn seed_only(seed: SeedTables) -> Self {
        Self {
            seed,
            params: EngineParams::default(),
        }
    }

    /// Seed-only engine over the seed files compiled into the binary.
    pub fn builtin() -> Self {
        Self::seed_only(SeedTables::builtin())
    }

    pub fn params(&self) -> &EngineParams {
        &self.params
    }

    pub fn seed(&self) -> &SeedTables {
        &self.seed
    }
}

/// The subset of `Config` the engine needs.
#[derive(Clone, Debug)]
pub struct EngineSettings {
    pub allah_form: AllahForm,
    pub tanween: TanweenStyle,
    pub hamza_relaxed: bool,
    pub eastern_numerals: bool,
    pub article_joining: bool,
    pub harakat: HarakatMode,
    pub sticky_last_choice: bool,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self::from(&Config::default())
    }
}

impl From<&Config> for EngineSettings {
    fn from(c: &Config) -> Self {
        Self {
            allah_form: c.allah_form(),
            tanween: c.tanween(),
            hamza_relaxed: c.hamza == "relaxed",
            eastern_numerals: c.numerals == "eastern",
            article_joining: c.article_joining,
            harakat: c.harakat_mode(),
            sticky_last_choice: c.sticky_last_choice && c.learning_enabled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateKind {
    Word,
    Completion,
    Oov,
    Phrase,
    Number,
    Custom,
    Joiner,
    Laughter,
    RawLatin,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    /// What is inserted / displayed (display transforms applied).
    pub text: String,
    /// Base form (marks stripped) — identity for dedup, learning and eval.
    pub base: String,
    pub kind: CandidateKind,
    pub score: f32,
    hyp: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct CandidateList {
    /// Ranked candidates; the raw-Latin candidate is always the last item.
    pub items: Vec<Candidate>,
    /// Normalized key of the buffer (phrase / user-model key).
    pub key: String,
    /// Exactly what was typed.
    pub raw: String,
}

impl CandidateList {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitHow {
    /// Space or mouse click: trailing space unless the candidate is a joiner.
    Space,
    /// Enter: no trailing space.
    Enter,
    /// Punctuation key: no trailing space (the front-end inserts the punctuation).
    Punctuation,
    /// Ctrl+Enter: insert with harakat derived from the typed vowels.
    WithHarakat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trailing {
    Space,
    None,
}

/// Result of a commit. The front-end inserts `text` (+ trailing) and, if `learnable`, records
/// `(key, base, rank)` in the user store.
#[derive(Clone, Debug)]
pub struct Commit {
    pub text: String,
    pub trailing: Trailing,
    pub key: String,
    pub base: String,
    pub rank: usize,
    pub kind: CandidateKind,
    pub learnable: bool,
}

pub struct Session<'e> {
    engine: &'e Engine,
    settings: EngineSettings,
    buf: LatinBuffer,
    list: CandidateList,
    hyps: Vec<Hyp>,
    pi: Posterior,
    context: Vec<String>,
}

const JOINER_ARTICLE: [&str; 4] = ["el", "al", "il", "l"];
const JOINER_CONJ: [&str; 3] = ["w", "wa", "we"];

impl<'e> Session<'e> {
    pub fn new(engine: &'e Engine, settings: EngineSettings) -> Self {
        Self {
            engine,
            settings,
            buf: LatinBuffer::new(),
            list: CandidateList::default(),
            hyps: Vec::new(),
            pi: dialect::DEFAULT_PRIOR,
            context: Vec::new(),
        }
    }

    pub fn settings(&self) -> &EngineSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, s: EngineSettings) {
        self.settings = s;
    }

    /// Previous words (nearest last), used by the context bigram in M3.
    pub fn set_context(&mut self, prev_words: &[&str]) {
        self.context = prev_words.iter().map(|w| w.to_string()).collect();
    }

    pub fn set_dialect(&mut self, pi: Posterior) {
        self.pi = pi;
    }

    pub fn dialect(&self) -> Posterior {
        self.pi
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Append a typed character. If the character cannot be part of a token the list is unchanged
    /// (the front-end treats it as punctuation).
    pub fn push(&mut self, c: InputChar, user: &dyn UserScorer) -> &CandidateList {
        if self.buf.syms().len() < self.engine.params.max_buffer && self.buf.push(c) {
            self.rebuild(user);
        }
        &self.list
    }

    /// Remove the last character; `None` when the buffer became empty.
    pub fn pop(&mut self, user: &dyn UserScorer) -> Option<&CandidateList> {
        self.buf.pop();
        if self.buf.is_empty() {
            self.reset();
            return None;
        }
        self.rebuild(user);
        Some(&self.list)
    }

    /// Re-open a committed word from its Latin buffer (re-edit, docs/02 §12.2).
    pub fn restore(&mut self, latin: &str, user: &dyn UserScorer) -> &CandidateList {
        self.buf.set(latin);
        self.rebuild(user);
        &self.list
    }

    pub fn reset(&mut self) {
        self.buf.clear();
        self.list = CandidateList::default();
        self.hyps.clear();
    }

    pub fn candidates(&self) -> &CandidateList {
        &self.list
    }

    /// Commit candidate `index` and reset the session.
    pub fn commit(&mut self, index: usize, how: CommitHow) -> Commit {
        let index = index.min(self.list.items.len().saturating_sub(1));
        let Some(c) = self.list.items.get(index).cloned() else {
            let raw = self.buf.raw();
            self.reset();
            return Commit {
                text: raw.clone(),
                trailing: Trailing::None,
                key: String::new(),
                base: raw,
                rank: 0,
                kind: CandidateKind::RawLatin,
                learnable: false,
            };
        };
        let mut text = c.text.clone();
        if how == CommitHow::WithHarakat {
            if let Some(v) = self.vowel_harakat(index) {
                text = v;
            }
        }
        let trailing = match (how, c.kind) {
            (_, CandidateKind::Joiner) => Trailing::None,
            (CommitHow::Space, _) => Trailing::Space,
            _ => Trailing::None,
        };
        let base = if c.kind == CandidateKind::RawLatin {
            RAW_LATIN.to_string()
        } else {
            c.base.clone()
        };
        let learnable = !matches!(
            c.kind,
            CandidateKind::Number | CandidateKind::Laughter | CandidateKind::Joiner
        );
        let commit = Commit {
            text,
            trailing,
            key: self.list.key.clone(),
            base,
            rank: index,
            kind: c.kind,
            learnable,
        };
        self.reset();
        commit
    }

    /// "Harakat from your vowels" for candidate `index` (docs/03 §10.6).
    pub fn vowel_harakat(&self, index: usize) -> Option<String> {
        let c = self.list.items.get(index)?;
        let h = self.hyps.get(c.hyp?)?;
        let d = dialect::argmax(&self.pi);
        let v = tashkeel::vowel_harakat(
            &h.steps,
            &self.engine.seed,
            d,
            self.settings.harakat,
            self.settings.tanween,
        )?;
        Some(display::style_sacred(&v, self.settings.allah_form))
    }

    /// Human-readable alignment and scores (for `t3a-cli explain`).
    pub fn explain(&self, index: usize) -> String {
        let Some(c) = self.list.items.get(index) else {
            return String::from("(no candidate)");
        };
        let mut s = format!("{} [{:?}] score={:.3}", c.text, c.kind, c.score);
        if let Some(h) = c.hyp.and_then(|i| self.hyps.get(i)) {
            let parts: Vec<String> = h
                .steps
                .iter()
                .map(|st| {
                    let l: String = st
                        .latin
                        .iter()
                        .map(|&ch| crate::normalize::symbol_display(ch))
                        .collect();
                    let a = if st.arabic.is_empty() {
                        "ε".to_string()
                    } else {
                        alphabet::decode(&st.arabic)
                    };
                    format!("{l}→{a}")
                })
                .collect();
            s.push_str(&format!("  align: {}", parts.join(" | ")));
        }
        s
    }

    fn rebuild(&mut self, user: &dyn UserScorer) {
        let p = &self.engine.params;
        let key = self.buf.key();
        let raw = self.buf.raw();
        let mut items: Vec<Candidate> = Vec::new();
        self.hyps.clear();

        let push = |items: &mut Vec<Candidate>, text: String, kind, score, hyp| {
            let base = crate::arabic::strip_marks(&text);
            items.push(Candidate {
                text,
                base,
                kind,
                score,
                hyp,
            });
        };

        // 1. Numbers (docs/03 §6.1)
        let digits_only = raw.chars().all(|c| c.is_ascii_digit());
        if self.buf.is_number() {
            push(
                &mut items,
                display::numerals(&raw, self.settings.eastern_numerals),
                CandidateKind::Number,
                f32::MAX,
                None,
            );
        }
        let arabizi_allowed = !(self.buf.is_number() && (raw.chars().count() >= 2 || !digits_only));

        if arabizi_allowed {
            // 2. Laughter (docs/03 §6.3)
            let lower = raw.to_lowercase();
            let h = lower.chars().filter(|&c| c == 'h').count();
            if h >= 3
                && lower.starts_with('h')
                && lower.chars().all(|c| matches!(c, 'h' | 'a' | 'e'))
            {
                push(
                    &mut items,
                    "ه".repeat(h.min(8)),
                    CandidateKind::Laughter,
                    f32::MAX,
                    None,
                );
            }
            // 3. Joiners (docs/03 §6.4)
            if self.settings.article_joining {
                if JOINER_ARTICLE.contains(&key.as_str()) {
                    push(
                        &mut items,
                        "ال".to_string(),
                        CandidateKind::Joiner,
                        f32::MAX,
                        None,
                    );
                } else if JOINER_CONJ.contains(&key.as_str()) {
                    push(
                        &mut items,
                        "و".to_string(),
                        CandidateKind::Joiner,
                        f32::MAX,
                        None,
                    );
                }
            }
            // 4. Lattice readings (seed-only: OOV path)
            self.hyps = oov::search(
                &self.engine.seed,
                p,
                &self.pi,
                self.buf.syms(),
                p.k_oov_seed_only,
            );
            let mut scored: Vec<Candidate> = self
                .hyps
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    let base = alphabet::decode(&h.letters);
                    let tanween = h.steps.last().is_some_and(|s| s.flags & F_TANWEEN != 0);
                    let text = if tanween {
                        display::add_tanween_fath(&base, self.settings.tanween)
                    } else {
                        base.clone()
                    };
                    let score = p.lambda_tm * h.score + p.lambda_usr * user.usr(&key, &base);
                    Candidate {
                        text,
                        base,
                        kind: CandidateKind::Oov,
                        score,
                        hyp: Some(i),
                    }
                })
                .collect();
            scored.sort_by(|a, b| b.score.total_cmp(&a.score));
            // 5. Phrases (docs/03 §6.2): default=1 → before readings, default=0 → rank 2.
            let phrases: Vec<(String, bool)> = self
                .engine
                .seed
                .phrases_for(&key, &self.pi)
                .map(|ph| (ph.output.clone(), ph.default))
                .collect();
            for (out, _) in phrases.iter().filter(|(_, d)| *d) {
                push(
                    &mut items,
                    out.clone(),
                    CandidateKind::Phrase,
                    f32::MAX,
                    None,
                );
            }
            let mut rest = scored;
            let mut late: Vec<Candidate> = phrases
                .iter()
                .filter(|(_, d)| !*d)
                .map(|(o, _)| Candidate {
                    text: o.clone(),
                    base: o.clone(),
                    kind: CandidateKind::Phrase,
                    score: 0.0,
                    hyp: None,
                })
                .collect();
            if !late.is_empty() {
                let at = if items.is_empty() {
                    1.min(rest.len())
                } else {
                    0
                };
                for (k, c) in late.drain(..).enumerate() {
                    rest.insert((at + k).min(rest.len()), c);
                }
            }
            items.extend(rest);
        }

        // 6. Display transforms: sacred styling (+ plain alternative), hamza style.
        let mut styled: Vec<Candidate> = Vec::with_capacity(items.len() + 2);
        for c in items {
            if display::contains_sacred(&c.base) && self.settings.allah_form != AllahForm::Plain {
                let mut s = c.clone();
                s.text = display::style_sacred(&c.text, self.settings.allah_form);
                styled.push(s);
                let mut plain = c;
                plain.text = plain.base.clone();
                styled.push(plain);
            } else {
                styled.push(c);
            }
        }
        if self.settings.hamza_relaxed {
            for c in &mut styled {
                c.text = display::relax_hamza(&c.text);
                c.base = display::relax_hamza(&c.base);
            }
        }
        // 7. Dedup by displayed text.
        let mut seen = std::collections::HashSet::new();
        styled.retain(|c| seen.insert(c.text.clone()));

        // 8. Sticky last choice (docs/03 §8 step 4); numbers stay first.
        let pinned_number = styled
            .first()
            .is_some_and(|c| c.kind == CandidateKind::Number);
        let mut raw_first = false;
        if self.settings.sticky_last_choice && !pinned_number {
            if let Some(w) = user.sticky(&key) {
                if w == RAW_LATIN {
                    raw_first = true;
                } else if let Some(pos) = styled.iter().position(|c| c.base == w) {
                    let c = styled.remove(pos);
                    styled.insert(0, c);
                } else {
                    styled.insert(
                        0,
                        Candidate {
                            text: w.clone(),
                            base: w,
                            kind: CandidateKind::Custom,
                            score: f32::MAX,
                            hyp: None,
                        },
                    );
                }
            }
        }
        styled.truncate(p.max_candidates.saturating_sub(1));
        let raw_c = Candidate {
            text: raw.clone(),
            base: raw.clone(),
            kind: CandidateKind::RawLatin,
            score: f32::MIN,
            hyp: None,
        };
        if raw_first {
            styled.insert(0, raw_c);
        } else {
            styled.push(raw_c);
        }
        self.list = CandidateList {
            items: styled,
            key,
            raw,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{fixed_profile, Dialect};
    use crate::user::{MemoryUser, NoUser};

    fn type_word(s: &mut Session, w: &str, u: &dyn UserScorer) {
        for ch in w.chars() {
            s.push(InputChar::new(ch), u);
        }
    }

    fn texts(l: &CandidateList) -> Vec<String> {
        l.items.iter().map(|c| c.text.clone()).collect()
    }

    #[test]
    fn raw_latin_is_always_last_and_exact() {
        let e = Engine::builtin();
        let mut s = Session::new(&e, EngineSettings::default());
        type_word(&mut s, "Mar7aba", &NoUser);
        let l = s.candidates();
        let last = l.items.last().unwrap();
        assert_eq!(last.kind, CandidateKind::RawLatin);
        assert_eq!(last.text, "Mar7aba");
        assert_eq!(l.items[0].text, "مرحبا");
    }

    #[test]
    fn allah_is_styled_and_plain_offered() {
        let e = Engine::builtin();
        let mut s = Session::new(&e, EngineSettings::default());
        type_word(&mut s, "inshallah", &NoUser);
        let t = texts(s.candidates());
        assert_eq!(t[0], "إن شاء اللّه");
        assert_eq!(t[1], "إن شاء الله");
    }

    #[test]
    fn numbers_first_and_exclusive() {
        let e = Engine::builtin();
        let mut s = Session::new(&e, EngineSettings::default());
        type_word(&mut s, "2026", &NoUser);
        let l = s.candidates();
        assert_eq!(l.items[0].kind, CandidateKind::Number);
        assert_eq!(l.items.len(), 2); // number + raw Latin
    }

    #[test]
    fn article_joiner_has_no_trailing_space() {
        let e = Engine::builtin();
        let mut s = Session::new(&e, EngineSettings::default());
        type_word(&mut s, "el", &NoUser);
        let c = s.commit(0, CommitHow::Space);
        assert_eq!(c.text, "ال");
        assert_eq!(c.trailing, Trailing::None);
        assert!(s.is_empty());
    }

    #[test]
    fn push_pop_symmetry() {
        let e = Engine::builtin();
        let mut s = Session::new(&e, EngineSettings::default());
        type_word(&mut s, "7abib", &NoUser);
        let before = texts(s.candidates());
        s.push(InputChar::new('i'), &NoUser);
        s.pop(&NoUser);
        assert_eq!(texts(s.candidates()), before);
        for _ in 0..5 {
            s.pop(&NoUser);
        }
        assert!(s.is_empty());
    }

    #[test]
    fn sticky_choice_wins_next_time() {
        let e = Engine::builtin();
        let mut u = MemoryUser::new();
        let mut s = Session::new(&e, EngineSettings::default());
        s.set_dialect(fixed_profile(Dialect::Lev));
        type_word(&mut s, "hala2", &u);
        let l = s.candidates().clone();
        let idx = 2.min(l.len() - 2);
        let want = l.items[idx].base.clone();
        let c = s.commit(idx, CommitHow::Space);
        u.record(&c.key, &c.base, c.rank, true, Some(&l.items[0].base));
        type_word(&mut s, "hala2", &u);
        assert_eq!(s.candidates().items[0].base, want);
    }

    #[test]
    fn ctrl_enter_gives_vowel_harakat() {
        let e = Engine::builtin();
        let mut s = Session::new(&e, EngineSettings::default());
        s.set_dialect(fixed_profile(Dialect::Msa));
        type_word(&mut s, "3allam", &NoUser);
        let idx = s
            .candidates()
            .items
            .iter()
            .position(|c| c.base == "علم")
            .expect("علم offered");
        let c = s.commit(idx, CommitHow::WithHarakat);
        assert_eq!(c.text, "\u{0639}\u{064E}\u{0644}\u{0651}\u{064E}\u{0645}"); // عَلَّم
    }

    #[test]
    fn outputs_are_clean() {
        let e = Engine::builtin();
        let mut s = Session::new(&e, EngineSettings::default());
        for w in [
            "mar7aba", "shukran", "wallahi", "3ala", "2albi", "hhhhh", "ok",
        ] {
            type_word(&mut s, w, &NoUser);
            for c in &s.candidates().items {
                assert!(crate::arabic::is_clean_output(&c.text), "{w}: {}", c.text);
                assert_eq!(crate::arabic::canonical_mark_order(&c.text), c.text);
            }
            s.reset();
        }
    }
}
