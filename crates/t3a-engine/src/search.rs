//! Lexicon-constrained incremental beam search over the T3A trie (docs/03 §5).

use crate::alphabet;
use crate::dialect::Posterior;
use crate::display;
use crate::oov::{self, Hyp};
use crate::params::EngineParams;
use crate::seed::{EffRule, SeedTables, F_GEM, POS_ANY, POS_F, POS_I, POS_M};
use crate::session::{Candidate, CandidateKind, EngineSettings};
use crate::user::UserScorer;
use t3a_data::{dequantize_lp, Chunk, DataView, Node};

#[derive(Clone, Copy, Debug)]
pub struct LatticeState {
    pub node: u32,
    pub g: f32,
    pub h: f32,
    pub back: u32,
    pub last_letter: u8,
    pub flags: u8,
    pub depth: u8,
}

#[derive(Clone, Debug)]
pub struct BackEdge {
    pub prev_back: u32,
    pub latin: Vec<char>,
    pub arabic: Vec<u8>,
    pub flags: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Column {
    pub states: Vec<LatticeState>,
}

pub struct TrieLattice {
    pub columns: Vec<Column>,
    pub arena: Vec<BackEdge>,
}

impl Default for TrieLattice {
    fn default() -> Self {
        Self::new()
    }
}

impl TrieLattice {
    pub fn new() -> Self {
        Self {
            columns: Vec::with_capacity(32),
            arena: Vec::with_capacity(512),
        }
    }

    pub fn clear(&mut self) {
        self.columns.clear();
        self.arena.clear();
    }
}

/// Find a child of `parent_idx` matching `label` in `nodes`.
pub fn find_child(nodes: &[Node], parent_idx: u32, label: u8) -> Option<u32> {
    let parent = nodes.get(parent_idx as usize)?;
    if parent.child_count == 0 {
        return None;
    }
    let start = parent.first_child as usize;
    let end = start + parent.child_count as usize;
    if end > nodes.len() {
        return None;
    }
    let children = &nodes[start..end];
    let pos = children.binary_search_by_key(&label, |c| c.label).ok()?;
    Some((start + pos) as u32)
}

/// Walk trie nodes along a sequence of Arabic letter codes.
pub fn walk_codes(nodes: &[Node], mut curr: u32, codes: &[u8]) -> Option<u32> {
    for &code in codes {
        curr = find_child(nodes, curr, code)?;
    }
    Some(curr)
}

/// Binary search chunks in `CHNK` section.
pub fn find_chunk<'a>(chunks: &'a [Chunk], latin_bytes: &[u8]) -> Option<&'a Chunk> {
    chunks
        .binary_search_by(|c| {
            let len = (c.len as usize).min(4);
            c.latin[..len].cmp(latin_bytes)
        })
        .ok()
        .map(|idx| &chunks[idx])
}

/// Compute dialect mixture log-probability: ln Σ_d π_d · exp(lp[d]) (docs/03 §7.2).
pub fn dialect_mixture_lm(q: [u8; 6], pi: &Posterior, unseen_lp: f32) -> f32 {
    let mut sum = 0.0f32;
    for (d, &w) in pi.iter().enumerate() {
        if w > 0.0 {
            let lp = if q[d] == 255 {
                unseen_lp
            } else {
                dequantize_lp(q[d])
            };
            sum += w * lp.exp();
        }
    }
    if sum > 0.0 {
        sum.ln()
    } else {
        unseen_lp
    }
}

/// Effective rules for `chunk_syms` at position set `pos`: from the compiled `RULE`/`CHNK` sections
/// when the chunk is there, else from the seed tables.
fn chunk_rules(
    chunks: &[t3a_data::Chunk],
    rules: &[t3a_data::Rule],
    seed: &SeedTables,
    chunk_syms: &[char],
    pos: u8,
    pi: &Posterior,
) -> Vec<EffRule> {
    let latin_bytes: Vec<u8> = chunk_syms
        .iter()
        .map(|&c| crate::normalize::emphatic_symbol(c).unwrap_or(c) as u8)
        .collect();
    let Some(ch_entry) = find_chunk(chunks, &latin_bytes) else {
        return seed.effective(chunk_syms, pos, pi);
    };
    let r_start = ch_entry.first_rule as usize;
    let r_end = (r_start + ch_entry.rule_count as usize).min(rules.len());
    // "Most specific wins" (mappings.tsv header): position-explicit rows before position-any rows.
    let applicable: Vec<&t3a_data::Rule> = rules[r_start.min(r_end)..r_end]
        .iter()
        .filter(|r| (r.pos_mask & pos) != 0)
        .collect();
    let explicit: Vec<&t3a_data::Rule> = applicable
        .iter()
        .copied()
        .filter(|r| r.pos_mask != POS_ANY)
        .collect();
    let chosen = if explicit.is_empty() {
        applicable
    } else {
        explicit
    };
    chosen
        .into_iter()
        .map(|r| EffRule {
            arabic: r.arabic[..r.arabic_len as usize].to_vec(),
            lp: if r.q == [255; 6] {
                dequantize_lp(r.q_any)
            } else {
                dialect_mixture_lm(r.q, pi, dequantize_lp(r.q_any))
            },
            flags: r.flags,
        })
        .collect()
}

/// Generic gemination (docs/03 §4.3): a doubled consonant (`ll`, `bb`, `77`) may be one Arabic
/// letter (written once, optionally with shadda), with probability `p_gem`.
fn gemination_rules(
    chunks: &[t3a_data::Chunk],
    rules: &[t3a_data::Rule],
    seed: &SeedTables,
    chunk_syms: &[char],
    pos: u8,
    pi: &Posterior,
    params: &EngineParams,
) -> Vec<EffRule> {
    if chunk_syms.len() != 2
        || chunk_syms[0] != chunk_syms[1]
        || matches!(chunk_syms[0], 'a' | 'e' | 'i' | 'o' | 'u' | '\'')
    {
        return Vec::new();
    }
    chunk_rules(chunks, rules, seed, &chunk_syms[..1], pos, pi)
        .into_iter()
        .filter(|r| r.arabic.len() == 1 && r.flags & crate::seed::F_VOWEL == 0)
        .map(|r| EffRule {
            arabic: r.arabic,
            lp: r.lp + params.p_gem.ln(),
            flags: r.flags | F_GEM,
        })
        .collect()
}

/// Reading a doubled consonant as two separate letters pays `ln(1 − p_gem)` (docs/03 §4.3): applied
/// when this single-symbol step repeats the previous step's Latin and Arabic.
fn double_penalty(
    arena: &[BackEdge],
    back: u32,
    chunk_syms: &[char],
    r: &EffRule,
    params: &EngineParams,
) -> f32 {
    let Some(prev) = arena.get(back as usize) else {
        return 0.0;
    };
    if chunk_syms.len() == 1
        && prev.latin == chunk_syms
        && prev.arabic == r.arabic
        && r.flags & crate::seed::F_VOWEL == 0
        && !matches!(chunk_syms[0], 'a' | 'e' | 'i' | 'o' | 'u')
    {
        (1.0 - params.p_gem).max(1e-6).ln()
    } else {
        0.0
    }
}

/// Spelling key that ignores the hamza seat on alef (أ إ آ → ا). Arabizi never encodes it.
pub fn hamza_key(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            '\u{0623}' | '\u{0625}' | '\u{0622}' => '\u{0627}',
            c => c,
        })
        .collect()
}

/// Among exact candidates that differ only in hamza-on-alef spelling (اكتب / أكتب), the lead
/// spelling is the one most frequent in MSA-register text: Arabizi does not encode the hamza seat,
/// and dialect web text (and dialect parallel data) drops it, so neither the transliteration score
/// nor dialect frequencies should decide it (docs/03 §8, Owner feedback 2026-09-23). The lead
/// spelling takes the group's best score; the other spellings stay directly after it.
fn order_hamza_variants(cands: &mut [Candidate], orth: &[(f32, f32)]) {
    let keys: Vec<String> = cands.iter().map(|c| hamza_key(&c.base)).collect();
    let mut done = vec![false; cands.len()];
    for i in 0..cands.len() {
        if done[i] {
            continue;
        }
        let group: Vec<usize> = (i..cands.len()).filter(|&j| keys[j] == keys[i]).collect();
        for &j in &group {
            done[j] = true;
        }
        if group.len() < 2 {
            continue;
        }
        let orth_score = |j: usize| orth[j].1 + 1e-3 * orth[j].0;
        let lead = *group
            .iter()
            .max_by(|&&a, &&b| {
                orth_score(a)
                    .total_cmp(&orth_score(b))
                    .then_with(|| cands[b].base.cmp(&cands[a].base))
            })
            .unwrap_or(&i);
        let best = group
            .iter()
            .map(|&j| cands[j].score)
            .fold(f32::NEG_INFINITY, f32::max);
        cands[lead].score = best;
        for &j in &group {
            if j != lead && cands[j].score >= best {
                cands[j].score = best - 1e-3;
            }
        }
    }
}

/// Online dialect posterior update after committing a word (docs/03 §7.2).
pub fn update_dialect_posterior(
    pi: &mut Posterior,
    q: [u8; 6],
    eta: f32,
    unseen_lp: f32,
    floor: f32,
) {
    // Geometric blend pi_d ∝ pi_d^(1-η) · P(w|d)^η in log space (word likelihoods are ~1e-5, so
    // linear space underflows toward the floors). Normalize first, then apply the floors — a
    // floor applied to unnormalized likelihoods froze the posterior at the floors (bug 2026-09-24).
    let mut logit = [0.0f32; 6];
    for (d, item) in logit.iter_mut().enumerate() {
        let lp = if q[d] == 255 {
            unseen_lp
        } else {
            dequantize_lp(q[d])
        };
        *item = (1.0 - eta) * pi[d].max(1e-9).ln() + eta * lp;
    }
    let max = logit.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut new_pi = logit.map(|l| (l - max).exp());
    let sum: f32 = new_pi.iter().sum();
    new_pi.iter_mut().for_each(|p| *p /= sum);
    for (d, item) in new_pi.iter_mut().enumerate() {
        let fl = if d == 0 { 0.10 } else { floor };
        *item = item.max(fl);
    }
    let sum: f32 = new_pi.iter().sum();
    for (p, n) in pi.iter_mut().zip(new_pi) {
        *p = n / sum;
    }
}

pub struct SearchResult {
    pub candidates: Vec<Candidate>,
    pub hyps: Vec<Hyp>,
}

pub struct SearchQuery<'a> {
    pub data: &'a DataView<'a>,
    pub seed: &'a SeedTables,
    pub params: &'a EngineParams,
    pub settings: &'a EngineSettings,
    pub pi: &'a Posterior,
    pub syms: &'a [char],
    pub prev_words: &'a [String],
    pub user: &'a dyn UserScorer,
    pub key: &'a str,
}

/// Run full trie search over `syms`.
pub fn search_trie(lattice: &mut TrieLattice, query: &SearchQuery<'_>) -> SearchResult {
    let data = query.data;
    let seed = query.seed;
    let params = query.params;
    let settings = query.settings;
    let pi = query.pi;
    let syms = query.syms;
    let prev_words = query.prev_words;
    let user = query.user;
    let key = query.key;

    let nodes = match data.trie_nodes() {
        Ok(n) => n,
        Err(_) => {
            return SearchResult {
                candidates: Vec::new(),
                hyps: Vec::new(),
            }
        }
    };
    let words = match data.words() {
        Ok(w) => w,
        Err(_) => {
            return SearchResult {
                candidates: Vec::new(),
                hyps: Vec::new(),
            }
        }
    };
    let chunks = data.chunks().unwrap_or(&[]);
    let rules = data.rules().unwrap_or(&[]);

    let n = syms.len();
    if n == 0 {
        return SearchResult {
            candidates: Vec::new(),
            hyps: Vec::new(),
        };
    }

    lattice.clear();

    // Column 0: root state
    let root_state = LatticeState {
        node: 0,
        g: 0.0,
        h: params.lambda_lm * dequantize_lp(nodes[0].max_q),
        back: u32::MAX,
        last_letter: 0,
        flags: 0,
        depth: 0,
    };
    lattice.columns.push(Column {
        states: vec![root_state],
    });

    // Build columns 1..=n incrementally
    for col_idx in 1..=n {
        let mut new_states: Vec<LatticeState> = Vec::new();

        // Chunks of length 1..=4 ending at col_idx
        for k in 1..=4.min(col_idx) {
            let start = col_idx - k;
            let chunk_syms = &syms[start..col_idx];
            let pos = if start == 0 { POS_I } else { POS_M };

            let mut eff_rules = chunk_rules(chunks, rules, seed, chunk_syms, pos, pi);
            eff_rules.extend(gemination_rules(
                chunks, rules, seed, chunk_syms, pos, pi, params,
            ));

            for state in &lattice.columns[start].states {
                for r in &eff_rules {
                    // Step trie
                    if let Some(next_node) = walk_codes(nodes, state.node, &r.arabic) {
                        let node_ref = &nodes[next_node as usize];
                        let last_letter = r.arabic.last().copied().unwrap_or(state.last_letter);
                        let g = state.g
                            + params.lambda_tm
                                * (r.lp
                                    + double_penalty(
                                        &lattice.arena,
                                        state.back,
                                        chunk_syms,
                                        r,
                                        params,
                                    ));
                        let h = params.lambda_lm * dequantize_lp(node_ref.max_q);

                        // Back edge
                        let back_idx = lattice.arena.len() as u32;
                        lattice.arena.push(BackEdge {
                            prev_back: state.back,
                            latin: chunk_syms.to_vec(),
                            arabic: r.arabic.clone(),
                            flags: r.flags,
                        });

                        let new_state = LatticeState {
                            node: next_node,
                            g,
                            h,
                            back: back_idx,
                            last_letter,
                            flags: r.flags,
                            depth: state.depth + r.arabic.len() as u8,
                        };

                        // Recombination
                        if let Some(existing) = new_states.iter_mut().find(|s| {
                            s.node == new_state.node
                                && s.last_letter == new_state.last_letter
                                && (s.flags & F_GEM) == (new_state.flags & F_GEM)
                        }) {
                            if new_state.g > existing.g {
                                *existing = new_state;
                            }
                        } else {
                            new_states.push(new_state);
                        }
                    }
                }
            }
        }

        // Pruning
        new_states.sort_by(|a, b| (b.g + b.h).total_cmp(&(a.g + a.h)));
        if let Some(best) = new_states.first() {
            let f_best = best.g + best.h;
            new_states.retain(|s| (s.g + s.h) >= f_best - params.prune_delta);
        }
        new_states.truncate(params.beam);
        lattice.columns.push(Column { states: new_states });
    }

    // Final expansion
    let mut exact_candidates: Vec<(f32, u32, u32)> = Vec::new(); // (g, word_idx, back_idx)

    // A word ends only through a final-position rule (F; I|F when one chunk is the whole word).
    // States of column n were built with medial rules and must not be accepted as complete words:
    // that let e.g. the final `a` of `ana` use the medial a → ε probability (ana → أن).
    for k in 1..=4.min(n) {
        let start = n - k;
        let chunk_syms = &syms[start..n];
        let pos = if start == 0 { POS_I | POS_F } else { POS_F };

        let mut eff_rules = chunk_rules(chunks, rules, seed, chunk_syms, pos, pi);
        eff_rules.extend(gemination_rules(
            chunks, rules, seed, chunk_syms, pos, pi, params,
        ));
        if let Some(col_start) = lattice.columns.get(start) {
            for state in &col_start.states {
                for r in &eff_rules {
                    if let Some(next_node) = walk_codes(nodes, state.node, &r.arabic) {
                        let node_ref = &nodes[next_node as usize];
                        if node_ref.word > 0 {
                            let g = state.g
                                + params.lambda_tm
                                    * (r.lp
                                        + double_penalty(
                                            &lattice.arena,
                                            state.back,
                                            chunk_syms,
                                            r,
                                            params,
                                        ));
                            let back_idx = lattice.arena.len() as u32;
                            lattice.arena.push(BackEdge {
                                prev_back: state.back,
                                latin: chunk_syms.to_vec(),
                                arabic: r.arabic.clone(),
                                flags: r.flags,
                            });
                            exact_candidates.push((g, node_ref.word - 1, back_idx));
                        }
                    }
                }
            }
        }
    }

    // Deduplicate exact candidates by word_idx, keeping best g
    let mut best_exact: std::collections::HashMap<u32, (f32, u32)> =
        std::collections::HashMap::new();
    for (g, word_idx, back_idx) in exact_candidates {
        best_exact
            .entry(word_idx)
            .and_modify(|(best_g, best_back)| {
                if g > *best_g {
                    *best_g = g;
                    *best_back = back_idx;
                }
            })
            .or_insert((g, back_idx));
    }

    // Context bigrams
    let bigr_view = data.bigrams().ok().flatten();
    let prev_word_idx = prev_words.last().and_then(|pw| {
        let codes = alphabet::encode(&crate::arabic::strip_marks(pw))?;
        let node_idx = walk_codes(nodes, 0, &codes)?;
        let w = nodes[node_idx as usize].word;
        if w > 0 {
            Some(w - 1)
        } else {
            None
        }
    });

    let mut scored_exact: Vec<Candidate> = Vec::new();
    let mut hyps: Vec<Hyp> = Vec::new();
    // Per exact candidate: (transliteration score g, MSA log-prob) for hamza-variant ordering.
    let mut orth: Vec<(f32, f32)> = Vec::new();

    for (&word_idx, &(g, back_idx)) in &best_exact {
        let w_rec = &words[word_idx as usize];
        let surface = data.string(w_rec.surface).unwrap_or("").to_string();
        let base = crate::arabic::strip_marks(&surface);

        let lm = dialect_mixture_lm(w_rec.q, pi, params.unseen_dialect_lp);

        // Bigram bonus
        let mut ctx_score = 0.0f32;
        if let (Some(bv), Some(pw_idx)) = (bigr_view, prev_word_idx) {
            let succs = bv.successors_of(pw_idx as usize);
            if let Some(pair) = succs.iter().find(|p| p.next == word_idx) {
                let lp_bi = dequantize_lp(pair.q);
                let diff = lp_bi - lm;
                ctx_score = diff.clamp(-2.0, 4.0);
            }
        }

        let usr_score = user.usr(key, &base);
        // `g` already carries λ_tm (applied per rule in the lattice).
        let total_score = g
            + params.lambda_lm * lm
            + params.lambda_ctx * ctx_score
            + params.lambda_usr * usr_score;

        // Reconstruct steps
        let mut steps = Vec::new();
        let mut curr_back = back_idx;
        while curr_back != u32::MAX && (curr_back as usize) < lattice.arena.len() {
            let edge = &lattice.arena[curr_back as usize];
            steps.push(oov::Step {
                latin: edge.latin.clone(),
                arabic: edge.arabic.clone(),
                flags: edge.flags,
            });
            curr_back = edge.prev_back;
        }
        steps.reverse();

        let hyp_idx = hyps.len();
        hyps.push(Hyp {
            letters: alphabet::encode(&base).unwrap_or_default(),
            score: g,
            chr: 0.0,
            steps,
        });

        // Display transforms
        let mut text = surface.clone();
        if (w_rec.flags & 1) != 0 {
            text = display::add_tanween_fath(&text, settings.tanween);
        }
        text = display::style_sacred(&text, settings.allah_form);
        if settings.hamza_relaxed {
            text = display::relax_hamza(&text);
        }

        let msa_lp = if w_rec.q[0] == 255 {
            params.unseen_dialect_lp
        } else {
            dequantize_lp(w_rec.q[0])
        };
        orth.push((g, msa_lp));
        scored_exact.push(Candidate {
            word: Some(word_idx),
            text,
            base,
            kind: CandidateKind::Word,
            score: total_score,
            hyp: Some(hyp_idx),
        });
    }

    order_hamza_variants(&mut scored_exact, &orth);
    scored_exact.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.text.chars().count().cmp(&b.text.chars().count()))
            .then_with(|| a.text.cmp(&b.text))
    });
    scored_exact.truncate(params.k_exact);

    // Completions (docs/03 §5.6.2)
    let mut completions = Vec::new();
    if n >= params.min_completion_len {
        if let Some(col_n) = lattice.columns.get(n) {
            for state in col_n.states.iter().take(params.m_completion_seeds) {
                let mut budget = params.completion_node_budget / params.m_completion_seeds.max(1);
                let mut queue = std::collections::VecDeque::new();
                queue.push_back((state.node, 0u8));

                while let Some((curr_node, extra_depth)) = queue.pop_front() {
                    if budget == 0 || extra_depth >= 6 {
                        break;
                    }
                    budget -= 1;

                    let node_ref = &nodes[curr_node as usize];
                    if extra_depth > 0 && node_ref.word > 0 && (node_ref.flags & 2) == 0 {
                        let w_idx = node_ref.word - 1;
                        let w_rec = &words[w_idx as usize];
                        let surf = data.string(w_rec.surface).unwrap_or("");
                        let base = crate::arabic::strip_marks(surf);

                        // Don't duplicate exact words
                        if !scored_exact.iter().any(|c| c.base == base) {
                            let lm = dialect_mixture_lm(w_rec.q, pi, params.unseen_dialect_lp);
                            let score = state.g + params.lambda_lm * lm
                                - params.gamma_completion * (extra_depth as f32);

                            let mut text = surf.to_string();
                            text = display::style_sacred(&text, settings.allah_form);
                            completions.push(Candidate {
                                word: Some(w_idx),
                                text,
                                base,
                                kind: CandidateKind::Completion,
                                score,
                                hyp: None,
                            });
                        }
                    }

                    // Enqueue children
                    let start = node_ref.first_child as usize;
                    let end = start + node_ref.child_count as usize;
                    if end <= nodes.len() {
                        for c_idx in start..end {
                            queue.push_back((c_idx as u32, extra_depth + 1));
                        }
                    }
                }
            }
        }
    }
    completions.sort_by(|a, b| b.score.total_cmp(&a.score));
    completions.truncate(params.k_completion);

    // OOV candidates (unconstrained beam)
    let chlm = data.chlm().ok().and_then(crate::charlm::CharLm::new);
    let oov_hyps = oov::search(seed, params, pi, syms, params.k_oov + 2, chlm.as_ref());
    let mut oov_cands = Vec::new();
    for h in oov_hyps {
        let base = alphabet::decode(&h.letters);
        if scored_exact.iter().any(|c| c.base == base) || completions.iter().any(|c| c.base == base)
        {
            continue;
        }
        let score = params.lambda_tm * h.score + params.lambda_chr * h.chr - params.oov_penalty;
        let hyp_idx = hyps.len();
        hyps.push(h);
        oov_cands.push(Candidate {
            word: None,
            text: base.clone(),
            base,
            kind: CandidateKind::Oov,
            score,
            hyp: Some(hyp_idx),
        });
        if oov_cands.len() >= params.k_oov {
            break;
        }
    }

    // Assemble final candidates: exact lexicon > exact OOV > completions
    let mut all = scored_exact;
    all.extend(oov_cands);
    all.extend(completions);

    SearchResult {
        candidates: all,
        hyps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialect_posterior_follows_words_and_keeps_floors() {
        // Regression (2026-09-24): floors were applied before normalizing, so with realistic word
        // likelihoods (~e^-12) every dialect sat on its floor and the posterior never moved.
        let q = |lps: [f32; 6]| lps.map(t3a_data::quantize_lp);
        let mag_word = q([-14.0, -14.0, -14.0, -14.0, -14.0, -9.0]);
        let mut pi = crate::dialect::DEFAULT_PRIOR;
        for _ in 0..4 {
            update_dialect_posterior(&mut pi, mag_word, 0.35, -18.0, 0.02);
        }
        assert_eq!(crate::dialect::argmax(&pi), crate::dialect::Dialect::Mag);
        assert!((pi.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        // Floors hold up to the final renormalization (MSA 0.10, others 0.02 before it).
        assert!(pi[0] >= 0.08 && pi.iter().all(|&p| p >= 0.015));
    }

    fn cand(base: &str, score: f32) -> Candidate {
        Candidate {
            word: None,
            text: base.to_string(),
            base: base.to_string(),
            kind: CandidateKind::Word,
            score,
            hyp: None,
        }
    }

    #[test]
    fn hamza_key_folds_alef_seats_only() {
        assert_eq!(hamza_key("أكتب"), "اكتب"); // U+0623 → U+0627
        assert_eq!(hamza_key("إن"), "ان");
        assert_eq!(hamza_key("آسف"), "اسف");
        assert_eq!(hamza_key("سؤال"), "سؤال"); // hamza on waw is not folded
    }

    /// Regression (Owner 2026-09-23): `oktob` must lead with أكتب although dialect text (and the
    /// transliteration score) prefer the hamza-less اكتب. MSA frequency decides the lead spelling.
    #[test]
    fn hamza_group_lead_is_msa_spelling() {
        // اكتب scores higher, but أكتب is more frequent in MSA text.
        let mut c = vec![
            cand("اكتب", -12.5),
            cand("أكتب", -13.0),
            cand("وكتب", -14.0),
        ];
        let orth = vec![(-3.0, -11.4), (-5.0, -11.2), (-4.0, -10.0)];
        order_hamza_variants(&mut c, &orth);
        c.sort_by(|a, b| b.score.total_cmp(&a.score));
        let order: Vec<&str> = c.iter().map(|x| x.base.as_str()).collect();
        assert_eq!(order, ["أكتب", "اكتب", "وكتب"]);
    }
}
