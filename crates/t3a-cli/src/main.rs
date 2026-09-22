//! t3a-cli — developer tool (AGENTS.md §7). Seed-only mode until M2/M3 wire the data file.
//!
//!   t3a-cli repl [--dialect LEV]
//!   t3a-cli eval <file.tsv>... [--dialect auto|LEV|...] [--k 5]
//!   t3a-cli explain <arabizi> [--dialect LEV]
//!   t3a-cli bench [<file.tsv>] [--iters 3]
//!   t3a-cli build-data ...        (M2)

mod build;
mod inspect;

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::Instant;
use t3a_engine::arabic::strip_marks;
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
        "build-data" => cmd_build_data(&args[1..]),
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
                expected: c[1].split('|').map(|s| strip_marks(s.trim())).collect(),
                dialect: c[2].trim().to_string(),
            })
        })
        .collect()
}

fn eval(args: &[String]) -> i32 {
    let files: Vec<&String> = args.iter().take_while(|a| !a.starts_with("--")).collect();
    if files.is_empty() {
        eprintln!("usage: t3a-cli eval <file.tsv>... [--dialect oracle|auto|LEV...] [--k 5]");
        return 2;
    }
    let k: usize = arg_value(args, "--k")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let mode = arg_value(args, "--dialect").unwrap_or_else(|| "oracle".into());
    let engine = load_engine(arg_value(args, "--data"));
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
            let bases: Vec<String> = s
                .candidates()
                .items
                .iter()
                .map(|c| strip_marks(&c.text))
                .collect();
            let rank = bases.iter().position(|b| r.expected.contains(b));
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
    for _ in 0..iters {
        for r in &rows {
            s.reset();
            for ch in r.arabizi.chars() {
                let t = Instant::now();
                s.push(InputChar::new(ch), &NoUser);
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| times[((times.len() as f64 - 1.0) * p) as usize];
    println!("keystrokes: {}  p50: {:.3} ms  p95: {:.3} ms  p99: {:.3} ms  max: {:.3} ms  (budget P1: p50 ≤ 0.8, p99 ≤ 3.0)",
        times.len(), pct(0.5), pct(0.95), pct(0.99), times.last().unwrap());
    0
}
