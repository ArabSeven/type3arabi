//! Lexicon-constrained incremental beam search over the T3A trie (docs/03 §5).

use crate::alphabet;
use crate::dialect::Posterior;
use crate::display;
use crate::oov::{self, Hyp};
use crate::params::EngineParams;
use crate::seed::{EffRule, SeedTables, F_GEM, POS_F, POS_I, POS_M};
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

/// Online dialect posterior update after committing a word (docs/03 §7.2).
pub fn update_dialect_posterior(
    pi: &mut Posterior,
    q: [u8; 6],
    eta: f32,
    unseen_lp: f32,
    floor: f32,
) {
    let eps = 1e-7f32;
    let mut new_pi = [0.0f32; 6];
    for (d, (item, &pi_d)) in new_pi.iter_mut().zip(pi.iter()).enumerate() {
        let lp = if q[d] == 255 {
            unseen_lp
        } else {
            dequantize_lp(q[d])
        };
        let p_w_d = lp.exp();
        *item = pi_d.powf(1.0 - eta) * (p_w_d + eps).powf(eta);
    }
    for (d, item) in new_pi.iter_mut().enumerate() {
        let fl = if d == 0 { 0.10 } else { floor };
        *item = item.max(fl);
    }
    let sum: f32 = new_pi.iter().sum();
    if sum > 0.0 {
        for (d, p) in pi.iter_mut().enumerate() {
            *p = new_pi[d] / sum;
        }
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

            // Convert chunk to Latin bytes
            let mut latin_bytes = Vec::with_capacity(k);
            for &c in chunk_syms {
                if let Some(code) = crate::normalize::emphatic_symbol(c) {
                    latin_bytes.push(code as u8);
                } else {
                    latin_bytes.push(c as u8);
                }
            }

            // Get effective rules for chunk: from binary data if present, or fallback to seed
            let eff_rules: Vec<EffRule> = if let Some(ch_entry) = find_chunk(chunks, &latin_bytes) {
                let r_start = ch_entry.first_rule as usize;
                let r_end = r_start + ch_entry.rule_count as usize;
                rules[r_start..r_end.min(rules.len())]
                    .iter()
                    .filter(|r| (r.pos_mask & pos) != 0)
                    .map(|r| {
                        let arabic_len = r.arabic_len as usize;
                        let arabic = r.arabic[..arabic_len].to_vec();
                        let lp_eff = if r.q == [255; 6] {
                            dequantize_lp(r.q_any)
                        } else {
                            dialect_mixture_lm(r.q, pi, dequantize_lp(r.q_any))
                        };
                        EffRule {
                            arabic,
                            lp: lp_eff,
                            flags: r.flags,
                        }
                    })
                    .collect()
            } else {
                seed.effective(chunk_syms, pos, pi)
            };

            for state in &lattice.columns[start].states {
                for r in &eff_rules {
                    // Step trie
                    if let Some(next_node) = walk_codes(nodes, state.node, &r.arabic) {
                        let node_ref = &nodes[next_node as usize];
                        let last_letter = r.arabic.last().copied().unwrap_or(state.last_letter);
                        let g = state.g + params.lambda_tm * r.lp;
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

    // Carry over states in column n that are terminal
    if let Some(col_n) = lattice.columns.get(n) {
        for s in &col_n.states {
            let node_ref = &nodes[s.node as usize];
            if node_ref.word > 0 {
                exact_candidates.push((s.g, node_ref.word - 1, s.back));
            }
        }
    }

    // Apply final rules (F / IF) from columns n-k
    for k in 1..=4.min(n) {
        let start = n - k;
        let chunk_syms = &syms[start..n];
        let pos = POS_F;

        let eff_rules: Vec<EffRule> = seed.effective(chunk_syms, pos, pi);
        if let Some(col_start) = lattice.columns.get(start) {
            for state in &col_start.states {
                for r in &eff_rules {
                    if let Some(next_node) = walk_codes(nodes, state.node, &r.arabic) {
                        let node_ref = &nodes[next_node as usize];
                        if node_ref.word > 0 {
                            let g = state.g + params.lambda_tm * r.lp;
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
        let total_score = params.lambda_tm * g
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

        scored_exact.push(Candidate {
            text,
            base,
            kind: CandidateKind::Word,
            score: total_score,
            hyp: Some(hyp_idx),
        });
    }

    scored_exact.sort_by(|a, b| b.score.total_cmp(&a.score));
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
                            let score = params.lambda_tm * state.g + params.lambda_lm * lm
                                - params.gamma_completion * (extra_depth as f32);

                            let mut text = surf.to_string();
                            text = display::style_sacred(&text, settings.allah_form);
                            completions.push(Candidate {
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
    let oov_hyps = oov::search(seed, params, pi, syms, params.k_oov + 2);
    let mut oov_cands = Vec::new();
    for h in oov_hyps {
        let base = alphabet::decode(&h.letters);
        if scored_exact.iter().any(|c| c.base == base) || completions.iter().any(|c| c.base == base)
        {
            continue;
        }
        let score = params.lambda_tm * h.score - params.oov_penalty;
        let hyp_idx = hyps.len();
        hyps.push(h);
        oov_cands.push(Candidate {
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

    // Assemble final candidates: exact + completions + oov
    let mut all = scored_exact;
    all.extend(completions);
    all.extend(oov_cands);

    SearchResult {
        candidates: all,
        hyps,
    }
}
