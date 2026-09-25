//! `t3a-cli self-train`: pseudo-labelled word pairs from monolingual Arabizi (docs/04 §6.4).
//!
//! The engine transliterates the most frequent word types of each monolingual corpus with the
//! dialect fixed to the corpus' dialect. A type becomes a training pair only when the engine is
//! confident: its top candidate is a lexicon word and beats the next different candidate by at least
//! `--margin` (score units = natural log). Pairs carry a weight (default 0.3) that `train-rules` applies
//! to their expected counts, so real parallel data keeps dominating.
//!
//! Output columns: latin, arabic, dialect, source, weight — the `align/train.tsv` format plus a weight.
//!
//! Which model labels the data matters for licensing (AGENTS.md R14): a release model must be
//! self-trained with a release-mode `.dat`, or internal sources would leak into it through the labels.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use t3a_engine::arabic::normalize_word;
use t3a_engine::dialect::{fixed_profile, Dialect};
use t3a_engine::session::CandidateKind;
use t3a_engine::{Engine, EngineSettings, InputChar, NoUser, Session};

struct Corpus {
    path: String,
    dialect: Dialect,
    source: String,
}

/// A pseudo-label: (latin, arabic).
type Label = (String, String);

struct Opts {
    types: usize,
    min_count: u32,
    margin: f32,
    weight: f32,
    max_lines: usize,
    threads: usize,
}

pub fn self_train(args: &[String], engine: Engine) -> i32 {
    let get = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    // --text <path>:<DIALECT>[:<source id>] (repeatable)
    let mut corpora = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if a != "--text" {
            continue;
        }
        let Some(spec) = args.get(i + 1) else {
            continue;
        };
        match parse_spec(spec) {
            Some(c) => corpora.push(c),
            None => {
                eprintln!(
                    "self-train: bad --text {spec} (expected <path>:<DIALECT>[:<source id>])"
                );
                return 2;
            }
        }
    }
    if corpora.is_empty() {
        eprintln!(
            "usage: t3a-cli self-train --text <file>:<DIALECT>[:<source>]... [--data model.dat] \
             [--out pipeline_data/align/selftrain.tsv] [--types 60000] [--min-count 5] [--margin 3.0] \
             [--weight 0.3] [--max-lines N] [--threads N]"
        );
        return 2;
    }
    let num = |flag: &str, def: f64| get(flag).and_then(|v| v.parse::<f64>().ok()).unwrap_or(def);
    let opts = Opts {
        types: num("--types", 60_000.0) as usize,
        min_count: num("--min-count", 5.0) as u32,
        margin: num("--margin", 3.0) as f32,
        weight: num("--weight", 0.3) as f32,
        max_lines: num("--max-lines", usize::MAX as f64) as usize,
        threads: num(
            "--threads",
            std::thread::available_parallelism().map_or(4, |n| n.get()) as f64,
        )
        .max(1.0) as usize,
    };
    let out_path = get("--out").unwrap_or_else(|| "pipeline_data/align/selftrain.tsv".into());

    let mut out = String::from(
        "# t3a-cli self-train (docs/04 §6.4): latin\tarabic\tdialect\tsource\tweight\n",
    );
    let mut total = 0usize;
    for c in &corpora {
        let t = Instant::now();
        let counts = match count_types(&c.path, opts.max_lines) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("self-train: {}: {e}", c.path);
                return 1;
            }
        };
        let mut types: Vec<(String, u32)> = counts
            .into_iter()
            .filter(|(_, n)| *n >= opts.min_count)
            .collect();
        types.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        types.truncate(opts.types);
        println!(
            "self-train: {} ({}) — {} frequent types counted in {:.0?}",
            c.source,
            c.dialect.code(),
            types.len(),
            t.elapsed()
        );
        let t = Instant::now();
        let labelled = label(&engine, &types, c.dialect, &opts);
        println!(
            "  kept {} / {} ({:.1}%) with margin ≥ {} in {:.0?}",
            labelled.len(),
            types.len(),
            100.0 * labelled.len() as f64 / types.len().max(1) as f64,
            opts.margin,
            t.elapsed()
        );
        for (latin, arabic) in labelled {
            out.push_str(&format!(
                "{latin}\t{arabic}\t{}\t{}\t{}\n",
                c.dialect.code(),
                c.source,
                opts.weight
            ));
            total += 1;
        }
    }
    if let Some(parent) = Path::new(&out_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(e) = File::create(&out_path).and_then(|mut f| f.write_all(out.as_bytes())) {
        eprintln!("self-train: {out_path}: {e}");
        return 1;
    }
    println!("self-train: {total} pairs written to {out_path}");
    0
}

fn parse_spec(spec: &str) -> Option<Corpus> {
    // Windows paths contain ':' (C:\...), so split from the right.
    let (rest, last) = spec.rsplit_once(':')?;
    if let Some(dialect) = Dialect::parse(last) {
        // <path>:<DIALECT> — the source id is the file's folder name (raw/<id>/arabizi.txt)
        // Split on both separators: `Path` does not treat '\' as one on Linux (CI).
        let source = rest
            .rsplit(['/', '\\'])
            .nth(1)
            .filter(|s| !s.is_empty())
            .map_or("mono".into(), str::to_string);
        return Some(Corpus {
            path: rest.to_string(),
            dialect,
            source,
        });
    }
    let (path, d) = rest.rsplit_once(':')?;
    Some(Corpus {
        path: path.to_string(),
        dialect: Dialect::parse(d)?,
        source: last.to_string(),
    })
}

/// Word types of a monolingual Arabizi file: lower-cased runs of `[a-z0-9']`, with at least one
/// letter, 3–20 characters (one- and two-letter tokens are mostly function words and teach
/// position-specific rules badly, see `train.rs`).
fn count_types(path: &str, max_lines: usize) -> std::io::Result<HashMap<String, u32>> {
    let reader = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut counts: HashMap<String, u32> = HashMap::new();
    let mut word = String::new();
    for line in reader.lines().take(max_lines) {
        let line = line?;
        for ch in line.chars().chain(std::iter::once(' ')) {
            let c = ch.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '\'' {
                word.push(c);
                continue;
            }
            if (3..=20).contains(&word.len()) && word.chars().any(|c| c.is_ascii_lowercase()) {
                *counts.entry(word.clone()).or_default() += 1;
            }
            word.clear();
        }
    }
    Ok(counts)
}

/// Confident engine labels for `types`, in parallel (one session per thread).
fn label(
    engine: &Engine,
    types: &[(String, u32)],
    d: Dialect,
    opts: &Opts,
) -> Vec<(String, String)> {
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<Label>> = vec![None; types.len()];
    let chunks: Vec<Vec<(usize, Option<Label>)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..opts.threads)
            .map(|_| {
                s.spawn(|| {
                    let mut session = Session::new(engine, EngineSettings::default());
                    let mut got = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some((latin, _)) = types.get(i) else {
                            break;
                        };
                        got.push((i, confident(&mut session, latin, d, opts.margin)));
                    }
                    got
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });
    for (i, r) in chunks.into_iter().flatten() {
        results[i] = r;
    }
    results.into_iter().flatten().collect()
}

fn confident(s: &mut Session, latin: &str, d: Dialect, margin: f32) -> Option<(String, String)> {
    s.reset();
    s.set_dialect(fixed_profile(d));
    for ch in latin.chars() {
        s.push(InputChar::new(ch), &NoUser);
    }
    let list = s.candidates();
    let mut ranked = list
        .items
        .iter()
        .filter(|c| c.kind != CandidateKind::RawLatin);
    let top = ranked.next()?;
    if top.kind != CandidateKind::Word {
        return None;
    }
    let runner_up = ranked.find(|c| c.base != top.base);
    if runner_up.is_some_and(|r| top.score - r.score < margin) {
        return None;
    }
    let arabic = normalize_word(&top.base);
    (!arabic.is_empty()).then(|| (latin.to_string(), arabic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_parses_windows_paths_and_optional_source() {
        let c = parse_spec(r"C:\data\nilechat-arabizi-egy\arabizi.txt:EGY").unwrap();
        assert_eq!(c.path, r"C:\data\nilechat-arabizi-egy\arabizi.txt");
        assert_eq!(c.dialect, Dialect::Egy);
        assert_eq!(c.source, "nilechat-arabizi-egy");
        let c = parse_spec("raw/mor.txt:MAG:nilechat-arabizi-mor").unwrap();
        assert_eq!(c.path, "raw/mor.txt");
        assert_eq!(c.source, "nilechat-arabizi-mor");
        assert!(parse_spec("raw/mor.txt").is_none());
    }

    #[test]
    fn types_are_lowercased_filtered_and_counted() {
        let dir = std::env::temp_dir().join(format!("t3a-selftrain-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.txt");
        fs::write(&f, "Mar7aba ya 3am, mar7aba! 123 el-gowa\n").unwrap();
        let c = count_types(f.to_str().unwrap(), usize::MAX).unwrap();
        assert_eq!(c.get("mar7aba"), Some(&2));
        assert_eq!(c.get("3am"), Some(&1));
        assert_eq!(c.get("gowa"), Some(&1));
        assert!(!c.contains_key("ya") && !c.contains_key("123") && !c.contains_key("el"));
        let _ = fs::remove_dir_all(dir);
    }
}
