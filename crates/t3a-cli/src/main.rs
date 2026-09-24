//! t3a-cli — developer tool (AGENTS.md §7). Seed-only mode until M2/M3 wire the data file.
//!
//!   t3a-cli repl [--dialect LEV]
//!   t3a-cli eval <file.tsv>... [--dialect auto|LEV|...] [--k 5]
//!   t3a-cli explain <arabizi> [--dialect LEV]
//!   t3a-cli bench [<file.tsv>] [--iters 3]
//!   t3a-cli build-data ...        (M2)
//!   t3a-cli train-rules [--pairs pipeline_data/align/train.tsv] [--out pipeline_data/out/rules.tsv]

mod build;
mod inspect;
mod train;

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::Instant;
use t3a_engine::arabic::{normalize_word, orth_fold, strip_marks};
use t3a_engine::dialect::{fixed_profile, Dialect, DEFAULT_PRIOR};
use t3a_engine::{
    CommitHow, Engine, EngineSettings, InputChar, MemoryUser, NoUser, Posterior, Session,
};

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn prior_for(s: Option<String>) -> Posterior {
    match s.as_deref().and_then(Dialect::parse) {
        Some(d) => fixed_profile(d),
        None => DEFAULT_PRIOR,
    }
}

fn load_engine(data_arg: Option<String>) -> Engine {
    let path = data_arg.unwrap_or_else(|| "target/type3arabi.dat".to_string());
    if Path::new(&path).exists() {
        match std::fs::read(&path) {
            Ok(bytes) => match Engine::from_bytes(bytes) {
                Ok(eng) => {
                    eprintln!("Loaded engine with lexicon from {path}");
                    return eng;
                }
                Err(e) => eprintln!("Warning: failed to load {path}: {e:?}, using builtin"),
            },
            Err(e) => eprintln!("Warning: failed to read {path}: {e}, using builtin"),
        }
    }
    Engine::builtin()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let code = match cmd {
        "repl" => repl(&args[1..]),
        "eval" => eval(&args[1..]),
        "explain" => explain(&args[1..]),
        "bench" => bench(&args[1..]),
        "adapt" => adapt(&args[1..]),
        "build-data" => cmd_build_data(&args[1..]),
        "train-rules" => train::train_rules(&args[1..]),
        "tune" => tune(&args[1..]),
        "inspect" => cmd_inspect(&args[1..]),
        _ => {
            eprintln!(
                "usage: t3a-cli <repl|eval|explain|bench|build-data|inspect> [args]\nsee AGENTS.md §7"
            );
            if cmd == "help" {
                0
            } else {
                2
            }
        }
    };
    std::process::exit(code);
}

fn cmd_build_data(args: &[String]) -> i32 {
    let out_path = arg_value(args, "--out").unwrap_or_else(|| "target/type3arabi.dat".to_string());
    let seed_dir = arg_value(args, "--seed").unwrap_or_else(|| "data/seed".to_string());
    let in_dir = arg_value(args, "--in").or_else(|| {
        if Path::new("pipeline_data/out").exists() {
            Some("pipeline_data/out".to_string())
        } else {
            None
        }
    });
    let mode = arg_value(args, "--mode").unwrap_or_else(|| "internal".to_string());

    match build::build_data(
        in_dir.as_deref().map(Path::new),
        Path::new(&seed_dir),
        Path::new(&out_path),
        &mode,
    ) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("build-data error: {e}");
            1
        }
    }
}

fn cmd_inspect(args: &[String]) -> i32 {
    let data_path =
        arg_value(args, "--data").unwrap_or_else(|| "target/type3arabi.dat".to_string());
    let word = args.iter().find(|a| !a.starts_with("--"));
    let Some(word) = word else {
        eprintln!("usage: t3a-cli inspect <word> [--data target/type3arabi.dat]");
        return 2;
    };
    match inspect::inspect_word(Path::new(&data_path), word) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("inspect error: {e}");
            1
        }
    }
}

fn repl(args: &[String]) -> i32 {
    let engine = load_engine(arg_value(args, "--data"));
    let mut session = Session::new(&engine, EngineSettings::default());
    session.set_dialect(prior_for(arg_value(args, "--dialect")));
    let mut user = MemoryUser::new();
    println!("Type3arabi REPL (seed-only). Type a word + Enter to see candidates; ':N' commits candidate N;");
    println!("':h N' commits with harakat from your vowels; ':q' quits.");
    let stdin = io::stdin();
    let mut last_word = String::new();
    loop {
        print!("> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim();
        if line == ":q" {
            break;
        }
        if let Some(rest) = line.strip_prefix(':') {
            let (harakat, n) = match rest.strip_prefix("h ") {
                Some(n) => (true, n),
                None => (false, rest),
            };
            if let Ok(i) = n.trim().parse::<usize>() {
                session.restore(&last_word, &user);
                let top = session.candidates().items.first().map(|c| c.base.clone());
                let how = if harakat {
                    CommitHow::WithHarakat
                } else {
                    CommitHow::Space
                };
                let c = session.commit(i.saturating_sub(1), how);
                user.record(&c.key, &c.base, c.rank, c.rank > 0, top.as_deref());
                println!("  ⇒ «{}»", c.text);
            }
            continue;
        }
        for word in line.split_whitespace() {
            session.reset();
            for ch in word.chars() {
                session.push(InputChar::new(ch), &user);
            }
            last_word = word.to_string();
            for (i, c) in session.candidates().items.iter().enumerate() {
                println!("  {:>2}. {:<20} {:?} {:.2}", i + 1, c.text, c.kind, c.score);
            }
        }
    }
    0
}

struct Row {
    arabizi: String,
    expected: Vec<String>,
    dialect: String,
}

fn load_rows(path: &str) -> Vec<Row> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .skip(1)
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 3).then(|| Row {
                arabizi: c[0].to_string(),
                expected: c[1].split('|').map(|s| normalize_word(s.trim())).collect(),
                dialect: c[2].trim().to_string(),
            })
        })
        .collect()
}

/// (n, top-1 hits, hit@k, reciprocal-rank sum) of `engine` on `rows`, dialect prior per `mode`.
fn score_rows(
    engine: &Engine,
    rows: &[Row],
    mode: &str,
    k: usize,
    lenient: bool,
) -> (u32, u32, u32, f64) {
    let fold = |s: &str| {
        if lenient {
            orth_fold(s)
        } else {
            strip_marks(s)
        }
    };
    let mut s = Session::new(engine, EngineSettings::default());
    let mut acc = (0u32, 0u32, 0u32, 0f64);
    for r in rows {
        let pi = match mode {
            "oracle" => Dialect::parse(&r.dialect)
                .map(fixed_profile)
                .unwrap_or(DEFAULT_PRIOR),
            other => prior_for(Some(other.to_string())),
        };
        s.reset();
        s.set_dialect(pi);
        for ch in r.arabizi.chars() {
            s.push(InputChar::new(ch), &NoUser);
        }
        let expected: Vec<String> = r.expected.iter().map(|e| fold(e)).collect();
        let rank = s
            .candidates()
            .items
            .iter()
            .position(|c| expected.contains(&fold(&c.text)));
        acc.0 += 1;
        if rank == Some(0) {
            acc.1 += 1;
        }
        if rank.is_some_and(|x| x < k) {
            acc.2 += 1;
        }
        if let Some(x) = rank {
            acc.3 += 1.0 / (x as f64 + 1.0);
        }
    }
    acc
}

/// `t3a-cli tune <dev.tsv>... --data F [--out pipeline_data/out/params.toml]`: coordinate ascent of
/// the scoring weights on dev splits (docs/04 §7). Objective = top1 + 0.25 · hit@5 (dialect = oracle:
/// the steady state after the online posterior has converged on the user's dialect),
/// lenient matching (hamza seat / final ة,ى folded) so tuning never learns to drop hamza to match
/// dialect gold spellings. A setting that loses any top-1 in `--guard` (default
/// `data/eval/regressions.tsv`) is rejected (R18: fixed bugs stay fixed).
fn tune(args: &[String]) -> i32 {
    let files: Vec<&String> = args.iter().take_while(|a| !a.starts_with("--")).collect();
    let out = arg_value(args, "--out").unwrap_or_else(|| "pipeline_data/out/params.toml".into());
    let mut engine = load_engine(arg_value(args, "--data"));
    let rows: Vec<Row> = files.iter().flat_map(|f| load_rows(f)).collect();
    let guard_file =
        arg_value(args, "--guard").unwrap_or_else(|| "data/eval/regressions.tsv".into());
    let guard_rows: Vec<Row> = load_rows(&guard_file);
    let guard_ok = |e: &Engine| {
        let (n, t1, _, _) = score_rows(e, &guard_rows, "oracle", 5, false);
        t1 == n
    };
    if rows.is_empty() {
        eprintln!("usage: t3a-cli tune <dev.tsv>... --data target/type3arabi.dat [--guard regressions.tsv]");
        return 2;
    }
    let objective = |e: &Engine| {
        let (n, t1, h5, _) = score_rows(e, &rows, "oracle", 5, true);
        (t1 as f64 + 0.25 * h5 as f64) / n as f64
    };
    type Knob = (&'static str, fn(&mut t3a_engine::EngineParams) -> &mut f32);
    let knobs: [Knob; 6] = [
        ("lambda_tm", |p| &mut p.lambda_tm),
        ("lambda_lm", |p| &mut p.lambda_lm),
        ("lambda_chr", |p| &mut p.lambda_chr),
        ("oov_penalty", |p| &mut p.oov_penalty),
        ("gamma_completion", |p| &mut p.gamma_completion),
        ("unseen_dialect_lp", |p| &mut p.unseen_dialect_lp),
    ];
    let mut best_p = engine.params().clone();
    let mut best = objective(&engine);
    println!("start: objective {best:.4}");
    for round in 0..3 {
        let mut improved = false;
        for (name, get) in &knobs {
            let base = *get(&mut best_p.clone());
            for f in [0.5f32, 0.7, 0.85, 1.15, 1.4, 2.0] {
                let mut p = best_p.clone();
                *get(&mut p) = base * f;
                engine.set_params(p.clone());
                let o = objective(&engine);
                if o > best + 1e-4 && guard_ok(&engine) {
                    best = o;
                    best_p = p;
                    improved = true;
                    println!(
                        "  round {round} {name} = {:.3}: objective {best:.4}",
                        base * f
                    );
                }
            }
        }
        if !improved {
            break;
        }
    }
    let text = format!(
        "# Tuned on dev splits by t3a-cli tune (docs/04 §7). Objective top1 + 0.25*hit@5 = {best:.4}\n\
         lambda_tm = {}\nlambda_lm = {}\nlambda_chr = {}\noov_penalty = {}\ngamma_completion = {}\n\
         unseen_dialect_lp = {}\nlambda_ctx = {}\nlambda_usr = {}\n",
        best_p.lambda_tm,
        best_p.lambda_lm,
        best_p.lambda_chr,
        best_p.oov_penalty,
        best_p.gamma_completion,
        best_p.unseen_dialect_lp,
        best_p.lambda_ctx,
        best_p.lambda_usr
    );
    match std::fs::write(&out, text) {
        Ok(()) => {
            println!("wrote {out}");
            0
        }
        Err(e) => {
            eprintln!("{out}: {e}");
            1
        }
    }
}

fn eval(args: &[String]) -> i32 {
    let files: Vec<&String> = args.iter().take_while(|a| !a.starts_with("--")).collect();
    if files.is_empty() {
        eprintln!("usage: t3a-cli eval <file.tsv>... [--dialect oracle|auto|LEV...] [--k 5] [--lenient] [--misses]");
        return 2;
    }
    let k: usize = arg_value(args, "--k")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let mode = arg_value(args, "--dialect").unwrap_or_else(|| "oracle".into());
    let engine = load_engine(arg_value(args, "--data"));
    let lenient = args.iter().any(|a| a == "--lenient");
    let fold = |s: &str| {
        if lenient {
            orth_fold(s)
        } else {
            strip_marks(s)
        }
    };
    let mut s = Session::new(&engine, EngineSettings::default());
    let mut per: BTreeMap<String, (u32, u32, u32, f64)> = BTreeMap::new(); // n, top1, hitk, mrr
    let mut misses = Vec::new();
    for f in files {
        for r in load_rows(f) {
            let pi = match mode.as_str() {
                "oracle" => Dialect::parse(&r.dialect)
                    .map(fixed_profile)
                    .unwrap_or(DEFAULT_PRIOR),
                other => prior_for(Some(other.to_string())),
            };
            s.reset();
            s.set_dialect(pi);
            for ch in r.arabizi.chars() {
                s.push(InputChar::new(ch), &NoUser);
            }
            let bases: Vec<String> = s.candidates().items.iter().map(|c| fold(&c.text)).collect();
            let expected: Vec<String> = r.expected.iter().map(|e| fold(e)).collect();
            let rank = bases.iter().position(|b| expected.contains(b));
            let e = per.entry(r.dialect.clone()).or_default();
            e.0 += 1;
            if rank == Some(0) {
                e.1 += 1;
            } else {
                misses.push(format!(
                    "{}\t{}\tgot: {}",
                    r.arabizi,
                    r.expected.join("|"),
                    bases
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" · ")
                ));
            }
            if rank.is_some_and(|x| x < k) {
                e.2 += 1;
            }
            if let Some(x) = rank {
                e.3 += 1.0 / (x as f64 + 1.0);
            }
        }
    }
    println!("| dialect | n | top-1 | hit@{k} | MRR |\n|---|---|---|---|---|");
    let mut tot = (0u32, 0u32, 0u32, 0f64);
    for (d, (n, t1, hk, mrr)) in &per {
        println!(
            "| {d} | {n} | {:.1}% | {:.1}% | {:.3} |",
            100.0 * *t1 as f64 / *n as f64,
            100.0 * *hk as f64 / *n as f64,
            mrr / *n as f64
        );
        tot = (tot.0 + n, tot.1 + t1, tot.2 + hk, tot.3 + mrr);
    }
    if tot.0 > 0 {
        println!(
            "| **all** | {} | {:.1}% | {:.1}% | {:.3} |",
            tot.0,
            100.0 * tot.1 as f64 / tot.0 as f64,
            100.0 * tot.2 as f64 / tot.0 as f64,
            tot.3 / tot.0 as f64
        );
    }
    if args.iter().any(|a| a == "--misses") {
        println!("\nmisses (arabizi, expected, got top-3):");
        for m in misses {
            println!("  {m}");
        }
    }
    0
}

fn explain(args: &[String]) -> i32 {
    let Some(word) = args.first() else {
        eprintln!("usage: t3a-cli explain <arabizi> [--dialect LEV]");
        return 2;
    };
    let engine = load_engine(arg_value(args, "--data"));
    let mut s = Session::new(&engine, EngineSettings::default());
    s.set_dialect(prior_for(arg_value(args, "--dialect")));
    for ch in word.chars() {
        s.push(InputChar::new(ch), &NoUser);
    }
    for i in 0..s.candidates().len() {
        println!("{:>2}. {}", i + 1, s.explain(i));
        if let Some(v) = s.vowel_harakat(i) {
            println!("     harakat from vowels: {v}");
        }
    }
    0
}

fn bench(args: &[String]) -> i32 {
    let path = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "data/eval/smoke.tsv".into());
    let iters: usize = arg_value(args, "--iters")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let rows = load_rows(&path);
    let engine = load_engine(arg_value(args, "--data"));
    let mut s = Session::new(&engine, EngineSettings::default());
    let mut times = Vec::new();
    let mut commits = Vec::new();
    for _ in 0..iters {
        for r in &rows {
            s.reset();
            for ch in r.arabizi.chars() {
                let t = Instant::now();
                s.push(InputChar::new(ch), &NoUser);
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            // Space commit: includes the dialect-posterior update (keystroke path, R3).
            let t = Instant::now();
            let _ = s.commit(0, CommitHow::Space);
            commits.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let report = |name: &str, v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        let pct = |p: f64| v[((v.len() as f64 - 1.0) * p) as usize];
        println!("{name}: {}  p50: {:.3} ms  p95: {:.3} ms  p99: {:.3} ms  max: {:.3} ms  (budget P1: p50 ≤ 0.8, p99 ≤ 3.0)",
            v.len(), pct(0.5), pct(0.95), pct(0.99), v.last().unwrap());
    };
    report("keystrokes", &mut times);
    report("commits", &mut commits);
    0
}

/// `t3a-cli adapt <test.tsv>... --data F [--eta X]`: how fast the automatic dialect posterior follows
/// a writer who switches dialect (docs/03 §7.2). For each ordered pair (A, B) of dialects present in
/// the rows: type 40 words of A (committing the gold word when it is listed, like a user picking it),
/// then words of B; report how many B words it takes until B is the most likely dialect.
fn adapt(args: &[String]) -> i32 {
    let files: Vec<&String> = args.iter().take_while(|a| !a.starts_with("--")).collect();
    let mut engine = load_engine(arg_value(args, "--data"));
    if let Some(eta) = arg_value(args, "--eta").and_then(|v| v.parse::<f32>().ok()) {
        let mut p = engine.params().clone();
        p.dialect_eta = eta;
        engine.set_params(p);
    }
    let rows: Vec<Row> = files.iter().flat_map(|f| load_rows(f)).collect();
    let mut by: std::collections::BTreeMap<String, Vec<&Row>> = Default::default();
    for r in &rows {
        by.entry(r.dialect.clone()).or_default().push(r);
    }
    by.retain(|_, v| v.len() >= 200);
    let type_word = |s: &mut Session, r: &Row| {
        s.reset();
        for ch in r.arabizi.chars() {
            s.push(InputChar::new(ch), &NoUser);
        }
        let idx = s
            .candidates()
            .items
            .iter()
            .position(|c| {
                r.expected
                    .iter()
                    .any(|e| orth_fold(e) == orth_fold(&c.base))
            })
            .unwrap_or(0);
        let _ = s.commit(idx, CommitHow::Space);
    };
    println!("dialect_eta = {}", engine.params().dialect_eta);
    for (a, ra) in &by {
        let mut s = Session::new(&engine, EngineSettings::default());
        for i in 0..40 {
            type_word(&mut s, ra[(i * 7) % ra.len()]);
        }
        let p = s.dialect();
        println!(
            "after 40 {a} words: MSA {:.2} LEV {:.2} EGY {:.2} GLF {:.2} IRQ {:.2} MAG {:.2}",
            p[0], p[1], p[2], p[3], p[4], p[5]
        );
    }
    // Accuracy while adapting: blocks of 25 words, dialects alternating, top-1 (lenient) is scored
    // before each commit. Compare with `eval --dialect oracle` (the dialect known in advance).
    {
        let dialects: Vec<&Vec<&Row>> = by.values().collect();
        let mut s = Session::new(&engine, EngineSettings::default());
        let (mut n, mut hit) = (0u32, 0u32);
        for block in 0..120usize {
            let rs = dialects[block % dialects.len()];
            for i in 0..25usize {
                let r = rs[(block * 25 + i) * 13 % rs.len()];
                s.reset();
                for ch in r.arabizi.chars() {
                    s.push(InputChar::new(ch), &NoUser);
                }
                if let Some(top) = s.candidates().items.first() {
                    n += 1;
                    hit += r
                        .expected
                        .iter()
                        .any(|e| orth_fold(e) == orth_fold(&top.base))
                        as u32;
                }
                type_word(&mut s, r);
            }
        }
        println!(
            "mixed stream (blocks of 25, dialects alternating): top-1 {:.1}% of {n}",
            100.0 * hit as f64 / n.max(1) as f64
        );
    }
    println!("| from → to | words until `to` leads (median of 20 runs) | max |");
    println!("|---|---|---|");
    for (a, ra) in &by {
        for (b, rb) in &by {
            if a == b {
                continue;
            }
            let Some(target) = Dialect::parse(b) else {
                continue;
            };
            let mut needed = Vec::new();
            for run in 0..20usize {
                let mut s = Session::new(&engine, EngineSettings::default());
                for i in 0..40 {
                    type_word(&mut s, ra[(run * 131 + i * 7) % ra.len()]);
                }
                let mut n = 0;
                while n < 200 && t3a_engine::dialect::argmax(&s.dialect()) != target {
                    type_word(&mut s, rb[(run * 97 + n * 11) % rb.len()]);
                    n += 1;
                }
                needed.push(n);
            }
            needed.sort();
            println!(
                "| {a} → {b} | {} | {} |",
                needed[needed.len() / 2],
                needed.last().unwrap()
            );
        }
    }
    0
}
