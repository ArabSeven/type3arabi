//! `EngineParams` — every algorithmic constant (AGENTS.md R17, docs/03 §12).
//! Defaults below; the data file's `PARM` section overrides them after tuning (docs/04 §7).

#[derive(Clone, Debug, PartialEq)]
pub struct EngineParams {
    pub beam: usize,
    pub beam_oov: usize,
    pub prune_delta: f32,
    pub k_exact: usize,
    pub k_completion: usize,
    pub k_oov: usize,
    /// Seed-only mode (no lexicon) shows more OOV readings.
    pub k_oov_seed_only: usize,
    pub min_completion_len: usize,
    pub m_completion_seeds: usize,
    pub completion_node_budget: usize,
    pub gamma_completion: f32,
    pub p_gem: f32,
    pub p_waw_alif: f32,
    pub oov_penalty: f32,
    pub custom_bonus: f32,
    pub custom_lm: f32,
    pub unseen_dialect_lp: f32,
    pub dialect_eta: f32,
    pub dialect_floor: f32,
    pub user_half_life_days: f32,
    pub lambda_tm: f32,
    pub lambda_lm: f32,
    pub lambda_ctx: f32,
    pub lambda_usr: f32,
    pub lambda_chr: f32,
    pub max_candidates: usize,
    /// Longest Latin buffer the engine accepts for one word.
    pub max_buffer: usize,
}

impl Default for EngineParams {
    fn default() -> Self {
        Self {
            beam: 96,
            beam_oov: 32,
            prune_delta: 12.0,
            k_exact: 12,
            k_completion: 2,
            k_oov: 2,
            k_oov_seed_only: 5,
            min_completion_len: 3,
            m_completion_seeds: 6,
            completion_node_budget: 256,
            gamma_completion: 0.9,
            p_gem: 0.75,
            p_waw_alif: 0.25,
            oov_penalty: 4.0,
            custom_bonus: 1.0,
            custom_lm: -9.0,
            unseen_dialect_lp: -18.0,
            dialect_eta: 0.08,
            dialect_floor: 0.02,
            user_half_life_days: 90.0,
            lambda_tm: 1.0,
            lambda_lm: 0.8,
            lambda_ctx: 0.6,
            lambda_usr: 1.5,
            lambda_chr: 0.35,
            max_candidates: 21,
            max_buffer: 48,
        }
    }
}

impl EngineParams {
    pub fn merge_toml(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim();
                let v = v.trim();
                match k {
                    "beam" => {
                        if let Ok(val) = v.parse() {
                            self.beam = val;
                        }
                    }
                    "beam_oov" => {
                        if let Ok(val) = v.parse() {
                            self.beam_oov = val;
                        }
                    }
                    "prune_delta" => {
                        if let Ok(val) = v.parse() {
                            self.prune_delta = val;
                        }
                    }
                    "k_exact" => {
                        if let Ok(val) = v.parse() {
                            self.k_exact = val;
                        }
                    }
                    "k_completion" => {
                        if let Ok(val) = v.parse() {
                            self.k_completion = val;
                        }
                    }
                    "k_oov" => {
                        if let Ok(val) = v.parse() {
                            self.k_oov = val;
                        }
                    }
                    "gamma_completion" => {
                        if let Ok(val) = v.parse() {
                            self.gamma_completion = val;
                        }
                    }
                    "p_gem" => {
                        if let Ok(val) = v.parse() {
                            self.p_gem = val;
                        }
                    }
                    "p_waw_alif" => {
                        if let Ok(val) = v.parse() {
                            self.p_waw_alif = val;
                        }
                    }
                    "oov_penalty" => {
                        if let Ok(val) = v.parse() {
                            self.oov_penalty = val;
                        }
                    }
                    "custom_bonus" => {
                        if let Ok(val) = v.parse() {
                            self.custom_bonus = val;
                        }
                    }
                    "custom_lm" => {
                        if let Ok(val) = v.parse() {
                            self.custom_lm = val;
                        }
                    }
                    "unseen_dialect_lp" => {
                        if let Ok(val) = v.parse() {
                            self.unseen_dialect_lp = val;
                        }
                    }
                    "dialect_eta" => {
                        if let Ok(val) = v.parse() {
                            self.dialect_eta = val;
                        }
                    }
                    "dialect_floor" => {
                        if let Ok(val) = v.parse() {
                            self.dialect_floor = val;
                        }
                    }
                    "user_half_life_days" => {
                        if let Ok(val) = v.parse() {
                            self.user_half_life_days = val;
                        }
                    }
                    "lambda_tm" => {
                        if let Ok(val) = v.parse() {
                            self.lambda_tm = val;
                        }
                    }
                    "lambda_lm" => {
                        if let Ok(val) = v.parse() {
                            self.lambda_lm = val;
                        }
                    }
                    "lambda_ctx" => {
                        if let Ok(val) = v.parse() {
                            self.lambda_ctx = val;
                        }
                    }
                    "lambda_usr" => {
                        if let Ok(val) = v.parse() {
                            self.lambda_usr = val;
                        }
                    }
                    "lambda_chr" => {
                        if let Ok(val) = v.parse() {
                            self.lambda_chr = val;
                        }
                    }
                    "max_candidates" => {
                        if let Ok(val) = v.parse() {
                            self.max_candidates = val;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
