//! `t3a-cli train-rules`: EM training of transliteration rules on Arabizi↔Arabic word pairs
//! (docs/04 §6). Output has the `data/seed/mappings.tsv` format and feeds `build-data` as `rules.tsv`.
//!
//! Model: P(α | ℓ, position class, dialect), where ℓ is a Latin chunk (1–4 symbols) and α an Arabic
//! chunk (0–3 letters). Candidate pairs = every seed row + (single Latin symbol → any single letter;
//! vowel → ε; `l` → ال), so EM can discover mappings the seed lacks. E-step: forward–backward over the
//! segmentation lattice of each pair; doubled consonants may map to one letter with `p_gem` (the
//! engine's generic gemination, docs/03 §4.3). M-step with a Dirichlet prior: pooled (all dialects
//! except MAG) `P* = (c + κ·P_seed) / (Σc + κ)`; per dialect `P_d = (c_d + κ·P*) / (Σc_d + κ)`.
//! Emission: pooled rows as `*`; a dialect gets its own rows for a chunk when it has ≥ MIN_DIALECT
//! occurrences there; otherwise the seed's dialect-specific rows for that dialect are kept verbatim.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use t3a_engine::alphabet;
use t3a_engine::arabic::normalize_word;
use t3a_engine::dialect::{self, Dialect, Posterior};
use t3a_engine::normalize::LatinBuffer;
use t3a_engine::seed::{SeedTables, F_ARTICLE, F_GEM, F_TANWEEN, F_VOWEL, POS_F, POS_I, POS_M};
use t3a_engine::EngineParams;

const KAPPA: f64 = 5.0;
const ITERATIONS: usize = 10;
const MIN_DIALECT: f64 = 50.0;
const KEEP_P: f64 = 0.005;
const KEEP_DISCOVERED: f64 = 0.03;
const DISCOVER_MASS: f64 = 0.03;
const POOLED: usize = 6;
/// Dialect indices whose data forms the pooled `*` distribution (all but MAG).
const POOLED_FROM: [usize; 5] = [0, 1, 2, 3, 4];
const POS_CLASSES: [u8; 3] = [POS_I, POS_M, POS_F];

type Chunk = Vec<char>;
type Alpha = Vec<u8>;

fn pos_index(p: u8) -> usize {
    match p {
        POS_I => 0,
        POS_M => 1,
        _ => 2,
    }
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Distribution over the allowed Arabic outputs of one Latin chunk, per position class.
struct Dist {
    alphas: Vec<Alpha>,
    prior: [Vec<f64>; 3],
    /// [dialect 0..6 + pooled][pos] → probabilities aligned with `alphas`.
    p: Vec<[Vec<f64>; 3]>,
    counts: Vec<[Vec<f64>; 3]>,
    index: HashMap<Alpha, usize>,
}

struct Pair {
    syms: Vec<char>,
    codes: Vec<u8>,
    dialect: usize,
}

fn uniform() -> Posterior {
    [1.0 / 6.0; 6]
}

pub fn train_rules(args: &[String]) -> i32 {
    let get = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let pairs_path = get("--pairs").unwrap_or_else(|| "pipeline_data/align/train.tsv".into());
    let seed_dir = get("--seed").unwrap_or_else(|| "data/seed".into());
    let out_path = get("--out").unwrap_or_else(|| "pipeline_data/out/rules.tsv".into());
    match run(
        Path::new(&pairs_path),
        Path::new(&seed_dir),
        Path::new(&out_path),
    ) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("train-rules failed: {e}");
            1
        }
    }
}

fn run(pairs_path: &Path, seed_dir: &Path, out_path: &Path) -> Result<(), String> {
    let read = |f: &str| fs::read_to_string(seed_dir.join(f)).map_err(|e| format!("{f}: {e}"));
    let seed = SeedTables::parse(
        &read("mappings.tsv")?,
        &read("phrases.tsv")?,
        &read("vowel_marks.tsv")?,
        &read("region_priors.tsv")?,
    )?;
    let params = EngineParams::default();

    // ---- training pairs
    let text =
        fs::read_to_string(pairs_path).map_err(|e| format!("{}: {e}", pairs_path.display()))?;
    let mut pairs = Vec::new();
    let mut buf = LatinBuffer::new();
    for line in text.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() < 3 {
            continue;
        }
        buf.set(c[0]);
        let syms = buf.syms().to_vec();
        let Some(codes) = alphabet::encode(&normalize_word(c[1])) else {
            continue;
        };
        let Some(d) = Dialect::parse(c[2]) else {
            continue;
        };
        // One-symbol tokens (Moroccan `o` = و "and", `f` = في) are function words, not evidence of
        // how a word starts: they would teach "initial o → و" (regression: oktob → وكتب).
        if syms.len() < 2 || codes.is_empty() || syms.len() > 24 || codes.len() > 20 {
            continue;
        }
        pairs.push(Pair {
            syms,
            codes,
            dialect: d.index(),
        });
    }
    println!("train-rules: {} usable pairs", pairs.len());

    // ---- candidate chunks and outputs
    let mut allowed: BTreeMap<Chunk, Vec<Alpha>> = BTreeMap::new();
    let mut flags: HashMap<(Chunk, Alpha), u8> = HashMap::new();
    for r in &seed.rules {
        let e = allowed.entry(r.latin.clone()).or_default();
        if !e.contains(&r.arabic) {
            e.push(r.arabic.clone());
        }
        *flags
            .entry((r.latin.clone(), r.arabic.clone()))
            .or_default() |= r.flags;
    }
    let mut symbols: HashSet<char> = HashSet::new();
    for p in &pairs {
        symbols.extend(p.syms.iter().copied());
    }
    for &s in &symbols {
        let e = allowed.entry(vec![s]).or_default();
        for code in 1..=alphabet::COUNT {
            if !e.contains(&vec![code]) {
                e.push(vec![code]);
            }
        }
        if is_vowel(s) && !e.contains(&Vec::new()) {
            e.push(Vec::new());
        }
        if s == 'l' {
            if let Some(al) = alphabet::encode("ال") {
                if !e.contains(&al) {
                    e.push(al);
                }
            }
        }
    }
    let max_chunk = allowed.keys().map(|k| k.len()).max().unwrap_or(1);

    let mut dists: HashMap<Chunk, Dist> = HashMap::new();
    for (chunk, alphas) in &allowed {
        let index: HashMap<Alpha, usize> = alphas
            .iter()
            .enumerate()
            .map(|(i, a)| (a.clone(), i))
            .collect();
        let n = alphas.len();
        let mut prior: [Vec<f64>; 3] = [vec![0.0; n], vec![0.0; n], vec![0.0; n]];
        for (pi_idx, &pos) in POS_CLASSES.iter().enumerate() {
            let eff = seed.effective(chunk, pos, &uniform());
            let seeded: f64 = eff.iter().map(|r| (r.lp as f64).exp()).sum();
            let (seed_mass, disc_mass) = if seeded > 0.0 {
                (1.0 - DISCOVER_MASS, DISCOVER_MASS)
            } else {
                (0.0, 1.0)
            };
            for r in &eff {
                if let Some(&i) = index.get(&r.arabic) {
                    prior[pi_idx][i] += seed_mass * (r.lp as f64).exp() / seeded;
                }
            }
            for v in prior[pi_idx].iter_mut() {
                *v += disc_mass / n as f64;
            }
        }
        let p = vec![prior.clone(); POOLED + 1];
        let counts = vec![[vec![0.0; n], vec![0.0; n], vec![0.0; n]]; POOLED + 1];
        dists.insert(
            chunk.clone(),
            Dist {
                alphas: alphas.clone(),
                prior,
                p,
                counts,
                index,
            },
        );
    }

    // ---- EM
    for it in 0..ITERATIONS {
        for d in dists.values_mut() {
            for set in d.counts.iter_mut() {
                for v in set.iter_mut() {
                    v.iter_mut().for_each(|x| *x = 0.0);
                }
            }
        }
        let mut ll = 0.0f64;
        let mut aligned = 0usize;
        for pair in &pairs {
            if let Some(l) = expect(pair, &mut dists, max_chunk, params.p_gem as f64) {
                ll += l;
                aligned += 1;
            }
        }
        for d in dists.values_mut() {
            for (pi_idx, _) in POS_CLASSES.iter().enumerate() {
                let n = d.alphas.len();
                // Pooled (`*`) = Mashriqi + MSA data only: Maghrebi Arabizi conventions (o = و,
                // ch = ش, 9 = ق) are MAG's own rows and must not leak into every other dialect.
                let pooled_total: f64 = POOLED_FROM
                    .iter()
                    .map(|&s| d.counts[s][pi_idx].iter().sum::<f64>())
                    .sum();
                let mut pooled = vec![0.0; n];
                for (i, v) in pooled.iter_mut().enumerate() {
                    let c: f64 = POOLED_FROM.iter().map(|&s| d.counts[s][pi_idx][i]).sum();
                    *v = (c + KAPPA * d.prior[pi_idx][i]) / (pooled_total + KAPPA);
                }
                for s in 0..6 {
                    let total: f64 = d.counts[s][pi_idx].iter().sum();
                    let probs: Vec<f64> = d.counts[s][pi_idx]
                        .iter()
                        .zip(&pooled)
                        .map(|(c, p)| (c + KAPPA * p) / (total + KAPPA))
                        .collect();
                    d.p[s][pi_idx] = probs;
                }
                d.p[POOLED][pi_idx] = pooled;
                // keep pooled counts for the emission note
                for i in 0..n {
                    d.counts[POOLED][pi_idx][i] =
                        POOLED_FROM.iter().map(|&s| d.counts[s][pi_idx][i]).sum();
                }
            }
        }
        println!(
            "  iteration {:>2}: log-lik {:.1}, aligned {}/{}",
            it + 1,
            ll,
            aligned,
            pairs.len()
        );
    }

    // ---- emission
    let mut out = String::from(
        "# Trained transliteration rules (t3a-cli train-rules, docs/04 §6). Same columns as data/seed/mappings.tsv.\n\
         latin\tarabic\tweight\tpos\tdialects\tflags\tnote\n",
    );
    let mut rows = 0usize;
    let mut chunks: Vec<&Chunk> = dists.keys().collect();
    chunks.sort();
    for chunk in chunks {
        let d = &dists[chunk];
        let latin: String = chunk.iter().map(|&c| latin_char(c)).collect();
        for (pi_idx, &pos) in POS_CLASSES.iter().enumerate() {
            let pos_s = ["I", "M", "F"][pi_idx];
            let emit =
                |out: &mut String, rows: &mut usize, probs: &[f64], dial: &str, note: &str| {
                    for (i, &p) in probs.iter().enumerate() {
                        // Reviewed seed pairs survive at lower probability than EM-discovered ones.
                        let seeded = flags.contains_key(&(chunk.clone(), d.alphas[i].clone()));
                        let keep = if seeded {
                            KEEP_P / 5.0
                        } else {
                            KEEP_DISCOVERED
                        };
                        if p < keep {
                            continue;
                        }
                        let a = &d.alphas[i];
                        let arabic = if a.is_empty() {
                            "ε".to_string()
                        } else {
                            alphabet::decode(a)
                        };
                        let mut f = flags.get(&(chunk.clone(), a.clone())).copied().unwrap_or(0);
                        if a.is_empty() || chunk.iter().all(|&c| is_vowel(c)) {
                            f |= F_VOWEL;
                        }
                        out.push_str(&format!(
                            "{latin}\t{arabic}\t{p:.5}\t{pos_s}\t{dial}\t{}\t{note}\n",
                            flag_names(f)
                        ));
                        *rows += 1;
                    }
                };
            let pooled_n: f64 = d.counts[POOLED][pi_idx].iter().sum();
            let note = format!("n={pooled_n:.0}");
            emit(&mut out, &mut rows, &d.p[POOLED][pi_idx], "*", &note);
            for dia in dialect::ALL {
                let s = dia.index();
                let n: f64 = d.counts[s][pi_idx].iter().sum();
                if n >= MIN_DIALECT {
                    emit(
                        &mut out,
                        &mut rows,
                        &d.p[s][pi_idx],
                        dia.code(),
                        &format!("n={n:.0}"),
                    );
                } else {
                    // keep the reviewed seed knowledge for dialects without enough data
                    let named: Vec<_> = seed
                        .resolve(chunk, pos, dia)
                        .into_iter()
                        .filter(|(r, _)| r.dialects & dia.bit() != 0)
                        .collect();
                    for (r, p) in named {
                        let arabic = if r.arabic.is_empty() {
                            "ε".to_string()
                        } else {
                            alphabet::decode(&r.arabic)
                        };
                        out.push_str(&format!(
                            "{latin}\t{arabic}\t{p:.5}\t{pos_s}\t{}\t{}\tseed\n",
                            dia.code(),
                            flag_names(r.flags)
                        ));
                        rows += 1;
                    }
                }
            }
        }
    }
    if let Some(parent) = out_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(out_path, out).map_err(|e| format!("{}: {e}", out_path.display()))?;
    println!(
        "train-rules: {rows} rules written to {}",
        out_path.display()
    );
    Ok(())
}

fn latin_char(c: char) -> char {
    use t3a_engine::normalize::{EMPH_D, EMPH_H, EMPH_S, EMPH_T, EMPH_Z};
    match c {
        EMPH_T => 'T',
        EMPH_S => 'S',
        EMPH_D => 'D',
        EMPH_Z => 'Z',
        EMPH_H => 'H',
        c => c,
    }
}

fn flag_names(f: u8) -> String {
    let mut v = Vec::new();
    if f & F_VOWEL != 0 {
        v.push("VOWEL");
    }
    if f & F_GEM != 0 {
        v.push("GEM");
    }
    if f & F_TANWEEN != 0 {
        v.push("TANWEEN");
    }
    if f & F_ARTICLE != 0 {
        v.push("ARTICLE");
    }
    if v.is_empty() {
        "-".into()
    } else {
        v.join(",")
    }
}

/// One transition of the segmentation lattice.
struct Arc {
    i: usize,
    j: usize,
    a: usize,
    b: usize,
    chunk: Chunk,
    alpha_idx: usize,
    pos_idx: usize,
    prob: f64,
}

/// E-step for one pair: forward–backward, accumulates expected counts. Returns log-likelihood, or
/// None when the pair has no alignment under the candidate set.
fn expect(
    pair: &Pair,
    dists: &mut HashMap<Chunk, Dist>,
    max_chunk: usize,
    p_gem: f64,
) -> Option<f64> {
    let n = pair.syms.len();
    let m = pair.codes.len();
    let s = pair.dialect;
    let mut arcs: Vec<Arc> = Vec::new();
    for i in 0..n {
        for a in 1..=max_chunk.min(n - i) {
            let chunk: Chunk = pair.syms[i..i + a].to_vec();
            let pos = if i == 0 {
                POS_I
            } else if i + a == n {
                POS_F
            } else {
                POS_M
            };
            let pos_idx = pos_index(pos);
            // direct rules for this chunk
            if let Some(d) = dists.get(&chunk) {
                for j in 0..=m {
                    for b in 0..=3.min(m - j) {
                        if let Some(&ai) = d.index.get(&pair.codes[j..j + b]) {
                            let prob = d.p[s][pos_idx][ai];
                            if prob > 0.0 {
                                arcs.push(Arc {
                                    i,
                                    j,
                                    a,
                                    b,
                                    chunk: chunk.clone(),
                                    alpha_idx: ai,
                                    pos_idx,
                                    prob,
                                });
                            }
                        }
                    }
                }
            }
            // generic gemination: xx (consonant) → the single letter of x, credited to x
            if a == 2 && chunk[0] == chunk[1] && !is_vowel(chunk[0]) && chunk[0] != '\'' {
                let single = vec![chunk[0]];
                if let Some(d) = dists.get(&single) {
                    for j in 0..m {
                        if let Some(&ai) = d.index.get(&pair.codes[j..j + 1]) {
                            let prob = d.p[s][pos_idx][ai] * p_gem;
                            if prob > 0.0 {
                                arcs.push(Arc {
                                    i,
                                    j,
                                    a,
                                    b: 1,
                                    chunk: single.clone(),
                                    alpha_idx: ai,
                                    pos_idx,
                                    prob,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    // forward / backward over (i, j)
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    let mut fwd = vec![0.0f64; (n + 1) * (m + 1)];
    fwd[idx(0, 0)] = 1.0;
    let mut order: Vec<usize> = (0..arcs.len()).collect();
    order.sort_by_key(|&k| (arcs[k].i, arcs[k].j));
    for &k in &order {
        let arc = &arcs[k];
        let f = fwd[idx(arc.i, arc.j)];
        if f > 0.0 {
            fwd[idx(arc.i + arc.a, arc.j + arc.b)] += f * arc.prob;
        }
    }
    let total = fwd[idx(n, m)];
    if total <= 0.0 || !total.is_finite() {
        return None;
    }
    let mut bwd = vec![0.0f64; (n + 1) * (m + 1)];
    bwd[idx(n, m)] = 1.0;
    for &k in order.iter().rev() {
        let arc = &arcs[k];
        let b = bwd[idx(arc.i + arc.a, arc.j + arc.b)];
        if b > 0.0 {
            bwd[idx(arc.i, arc.j)] += arc.prob * b;
        }
    }
    for arc in &arcs {
        let post =
            fwd[idx(arc.i, arc.j)] * arc.prob * bwd[idx(arc.i + arc.a, arc.j + arc.b)] / total;
        if post > 1e-9 {
            if let Some(d) = dists.get_mut(&arc.chunk) {
                d.counts[s][arc.pos_idx][arc.alpha_idx] += post;
            }
        }
    }
    Some(total.ln())
}
