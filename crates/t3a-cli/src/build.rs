//! Data compiler: builds `type3arabi.dat` from pipeline outputs and seed tables (docs/04 §2, docs/12).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use t3a_data::{
    quantize_lp, BigramPair, Chunk, DiacHeader, DiacVariant, Node, PhraseEntry, RegionEntry, Rule,
    StringPool, WordRec, Writer,
};
use t3a_engine::alphabet::{t3a_code, T3A_ALPHABET};
use t3a_engine::arabic::normalize_word;
use t3a_engine::dialect::Dialect;
use t3a_engine::normalize::emphatic_symbol;
use t3a_engine::seed::{POS_ANY, POS_F, POS_I, POS_M};

/// Compile binary `type3arabi.dat`.
pub fn build_data(
    in_dir: Option<&Path>,
    seed_dir: &Path,
    out_path: &Path,
    mode: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Building type3arabi.dat ({mode} mode) ===");

    let mut writer = Writer::new(2026092301);
    writer.build_id = [
        0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x0F, 0xED, 0xCB, 0xA9, 0x87, 0x65, 0x43,
        0x21,
    ];

    // 1. ALPH section: 42 codepoints
    let alph_codes: Vec<u32> = T3A_ALPHABET.iter().map(|&c| c as u32).collect();
    writer.add_alph(&alph_codes);
    println!("  [ALPH] 42 alphabet codes written");

    // 2. Rules & Chunks (RULE + CHNK)
    let mappings_file = if let Some(d) = in_dir {
        let p = d.join("rules.tsv");
        if p.exists() {
            p
        } else {
            seed_dir.join("mappings.tsv")
        }
    } else {
        seed_dir.join("mappings.tsv")
    };

    println!(
        "  [RULE+CHNK] Loading mappings from {}",
        mappings_file.display()
    );
    let (chunks, rules) = compile_rules(&mappings_file)?;
    writer.add_rules(&rules);
    writer.add_chunks(&chunks);
    println!(
        "  [RULE+CHNK] {} chunks, {} rules compiled",
        chunks.len(),
        rules.len()
    );

    // 3. String pool + Words + Trie + Diacritics + Bigrams
    let mut strs = StringPool::new();

    let lexicon_file = in_dir.map(|d| d.join("lexicon.tsv"));
    let raw_words = if let Some(ref lf) = lexicon_file {
        if lf.exists() {
            println!("  [WREC+TRIE] Loading lexicon from {}", lf.display());
            load_lexicon_tsv(lf)?
        } else {
            println!("  [WREC+TRIE] Compiling seed lexicon from smoke & phrases");
            collect_seed_lexicon(seed_dir)?
        }
    } else {
        println!("  [WREC+TRIE] Compiling seed lexicon from smoke & phrases");
        collect_seed_lexicon(seed_dir)?
    };

    let diac_file = in_dir.map(|d| d.join("diac.tsv"));
    let raw_diac = if let Some(ref df) = diac_file {
        if df.exists() {
            println!("  [DIAC] Loading diacritics from {}", df.display());
            load_diac_tsv(df)?
        } else {
            default_seed_diac()
        }
    } else {
        default_seed_diac()
    };

    let mut words = Vec::new();
    let mut trie_builder = TrieBuilder::new();
    let mut diac_headers = Vec::new();
    let mut diac_variants = Vec::new();
    let mut word_to_idx: HashMap<String, u32> = HashMap::new();

    for (i, (word_text, _, _)) in raw_words.iter().enumerate() {
        word_to_idx.insert(word_text.clone(), i as u32);
    }

    for (word_text, q_dialect, flags) in &raw_words {
        let surf_off = strs.add(word_text);
        let word_idx = words.len() as u32;

        let diac_idx = if let Some(vars) = raw_diac.get(word_text) {
            let header_idx = diac_headers.len() as u32;
            let first_variant = diac_variants.len() as u32;
            let count = vars.len().min(8) as u8;
            for (var_str, lp, mask) in vars.iter().take(8) {
                let v_off = strs.add(var_str);
                diac_variants.push(DiacVariant {
                    s: v_off,
                    q: quantize_lp(*lp),
                    dialect_mask: *mask,
                    pad: 0,
                });
            }
            diac_headers.push(DiacHeader {
                first: first_variant,
                n: count,
                pad: [0; 3],
            });
            header_idx + 1
        } else {
            0
        };

        words.push(WordRec {
            q: *q_dialect,
            flags: *flags,
            surface: surf_off,
            diac: diac_idx,
        });

        // Add to trie
        let codes: Vec<u8> = word_text
            .chars()
            .map(|c| t3a_code(c).unwrap_or(0))
            .filter(|&c| c > 0)
            .collect();
        if !codes.is_empty() {
            let min_q = *q_dialect.iter().min().unwrap_or(&10);
            trie_builder.insert(&codes, word_idx + 1, min_q);
        }
    }

    let trie_nodes = trie_builder.build();
    writer.add_words(&words);
    writer.add_trie(&trie_nodes);
    println!(
        "  [WREC+TRIE] {} words, {} trie nodes compiled",
        words.len(),
        trie_nodes.len()
    );

    if !diac_headers.is_empty() {
        writer.add_diac(&diac_headers, &diac_variants);
        println!(
            "  [DIAC] {} headers, {} variants written",
            diac_headers.len(),
            diac_variants.len()
        );
    }

    // Bigrams (BIGR)
    let bigr_file = in_dir.map(|d| d.join("bigrams.tsv"));
    let raw_bigr = if let Some(ref bf) = bigr_file {
        if bf.exists() {
            println!("  [BIGR] Loading bigrams from {}", bf.display());
            load_bigrams_tsv(bf)?
        } else {
            default_seed_bigrams()
        }
    } else {
        default_seed_bigrams()
    };

    let mut offsets = Vec::with_capacity(words.len() + 1);
    let mut bigr_pairs = Vec::new();
    for (word_text, _, _) in &raw_words {
        offsets.push(bigr_pairs.len() as u32);
        if let Some(succs) = raw_bigr.get(word_text) {
            for (next_word, lp) in succs {
                if let Some(&next_idx) = word_to_idx.get(next_word) {
                    bigr_pairs.push(BigramPair {
                        next: next_idx,
                        q: quantize_lp(*lp),
                        pad: [0; 3],
                    });
                }
            }
        }
    }
    offsets.push(bigr_pairs.len() as u32);

    if !bigr_pairs.is_empty() {
        writer.add_bigrams(&offsets, &bigr_pairs);
        println!("  [BIGR] {} bigram pairs written", bigr_pairs.len());
    }

    // 4. Phrases (PHRS)
    let phrases_file = seed_dir.join("phrases.tsv");
    let mut phrases = Vec::new();
    if phrases_file.exists() {
        println!("  [PHRS] Loading phrases from {}", phrases_file.display());
        let content = fs::read_to_string(&phrases_file)?;
        for line in content.lines().filter(|l| !l.starts_with('#')) {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let key_norm = parts[0].trim().to_lowercase();
                let out_text = parts[1].trim();
                let key_off = strs.add(&key_norm);
                let out_off = strs.add(out_text);
                phrases.push(PhraseEntry {
                    key: key_off,
                    out: out_off,
                    dialect_mask: 0xFF,
                    flags: 1, // default candidate
                    pad: 0,
                });
            }
        }
    }
    writer.add_phrases(&phrases);
    println!("  [PHRS] {} phrases compiled", phrases.len());

    // 5. Region priors (REGN)
    let regions_file = seed_dir.join("region_priors.tsv");
    let mut regions = Vec::new();
    if regions_file.exists() {
        println!(
            "  [REGN] Loading region priors from {}",
            regions_file.display()
        );
        let content = fs::read_to_string(&regions_file)?;
        for line in content.lines().filter(|l| !l.starts_with('#')) {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 7 {
                let iso2 = parts[0].trim().as_bytes();
                if iso2.len() == 2 {
                    let mut weights = [0.0f32; 6];
                    for (i, p) in parts[1..7].iter().enumerate() {
                        weights[i] = p.trim().parse().unwrap_or(0.0);
                    }
                    regions.push(RegionEntry {
                        iso2: [iso2[0], iso2[1]],
                        pad: [0; 2],
                        weights,
                    });
                }
            }
        }
    }
    writer.add_regions(&regions);
    println!("  [REGN] {} regions compiled", regions.len());

    // 6. Character LM (CHLM, docs/03 §7.4, docs/12 §7)
    let chlm_file = in_dir.map(|d| d.join("charlm.tsv"));
    let chlm_rows = match chlm_file {
        Some(ref f) if f.exists() => {
            println!("  [CHLM] Loading char n-grams from {}", f.display());
            load_charlm_tsv(f)?
        }
        _ => Vec::new(),
    };
    let chlm_entries = t3a_engine::charlm::build_table(&chlm_rows);
    writer.add_chlm(&chlm_entries);
    println!(
        "  [CHLM] {} n-grams in {} slots",
        chlm_rows.len(),
        chlm_entries.len()
    );

    // 7. String pool (STRS)
    writer.add_strs(strs.finish());
    println!("  [STRS] Packed string pool");

    // 8. PARM
    // Tuned parameters (docs/04 §7) when present; otherwise the engine defaults apply.
    let parm_content = in_dir
        .map(|d| d.join("params.toml"))
        .filter(|p| p.exists())
        .and_then(|p| fs::read_to_string(p).ok())
        .unwrap_or_default();
    writer.add_parm(&parm_content);
    println!("  [PARM] Engine parameters stored");

    // 9. META
    let dist = if mode == "internal" {
        "internal-only"
    } else {
        "release"
    };
    let meta_json = format!(
        "{{\"format_version\": 1, \"distribution\": \"{dist}\", \"word_count\": {}, \"rule_count\": {}}}\n",
        words.len(),
        rules.len()
    );
    writer.add_meta(&meta_json);
    println!("  [META] Build metadata stored");

    // Finalize
    let bytes = writer.finish();
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, &bytes)?;

    println!(
        "=== Successfully built {} ({} bytes, {:.2} MB) ===",
        out_path.display(),
        bytes.len(),
        bytes.len() as f64 / (1024.0 * 1024.0)
    );

    Ok(())
}

fn compile_rules(path: &Path) -> Result<(Vec<Chunk>, Vec<Rule>), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut chunk_map: BTreeMap<Vec<u8>, Vec<Rule>> = BTreeMap::new();

    for line in content.lines().filter(|l| !l.starts_with('#')) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }

        let latin_str = parts[0].trim();
        let arabic_str = parts[1].trim();
        let weight: f32 = parts[2].trim().parse().unwrap_or(0.1);
        let pos_str = parts[3].trim();
        let dialects_str = parts.get(4).unwrap_or(&"*").trim();
        let flags_str = parts.get(5).unwrap_or(&"-").trim();

        // Convert Latin symbols
        let mut latin_bytes = Vec::new();
        for c in latin_str.chars() {
            if let Some(emph) = emphatic_symbol(c) {
                latin_bytes.push(emph as u8);
            } else {
                latin_bytes.push(c as u8);
            }
        }
        if latin_bytes.is_empty() || latin_bytes.len() > 4 {
            continue;
        }

        // Convert Arabic symbols
        let mut arabic_codes = [0u8; 3];
        let mut arabic_len = 0u8;
        if arabic_str != "ε" {
            for c in arabic_str.chars().take(3) {
                if let Some(code) = t3a_code(c) {
                    arabic_codes[arabic_len as usize] = code;
                    arabic_len += 1;
                }
            }
        }

        // Pos mask
        let mut pos_mask = 0u8;
        if pos_str == "*" {
            pos_mask = POS_ANY;
        } else {
            if pos_str.contains('I') {
                pos_mask |= POS_I;
            }
            if pos_str.contains('M') {
                pos_mask |= POS_M;
            }
            if pos_str.contains('F') {
                pos_mask |= POS_F;
            }
        }

        // Flags
        let mut flags = 0u8;
        if flags_str.contains("VOWEL") {
            flags |= 1;
        }
        if flags_str.contains("GEM") {
            flags |= 2;
        }
        if flags_str.contains("TANWEEN") {
            flags |= 4;
        }
        if flags_str.contains("ARTICLE") {
            flags |= 8;
        }

        let lp = weight.ln();
        let q_any = quantize_lp(lp);
        let mut q_dialects = [255u8; 6];
        if dialects_str == "*" {
            q_dialects = [q_any; 6];
        } else {
            for d_name in dialects_str.split(',') {
                if let Some(d) = Dialect::parse(d_name.trim()) {
                    q_dialects[d.index()] = q_any;
                }
            }
        }

        let rule = Rule {
            arabic: arabic_codes,
            arabic_len,
            pos_mask,
            flags,
            q_any,
            _pad: 0,
            q: q_dialects,
            chunk: 0, // set later
        };

        chunk_map.entry(latin_bytes).or_default().push(rule);
    }

    let mut chunks = Vec::new();
    let mut rules = Vec::new();

    for (chunk_bytes, mut rule_list) in chunk_map {
        let chunk_idx = chunks.len() as u16;
        let first_rule = rules.len() as u16;
        let rule_count = rule_list.len() as u16;

        let mut latin_arr = [0u8; 4];
        for (i, &b) in chunk_bytes.iter().enumerate().take(4) {
            latin_arr[i] = b;
        }

        chunks.push(Chunk {
            latin: latin_arr,
            len: chunk_bytes.len() as u8,
            pad: 0,
            first_rule,
            rule_count,
            pad2: 0,
        });

        for r in &mut rule_list {
            r.chunk = chunk_idx;
            rules.push(*r);
        }
    }

    Ok((chunks, rules))
}

type RawLexiconEntry = (String, [u8; 6], u16);
type DiacMap = HashMap<String, Vec<(String, f32, u8)>>;
type BigramMap = HashMap<String, Vec<(String, f32)>>;

fn collect_seed_lexicon(
    seed_dir: &Path,
) -> Result<Vec<RawLexiconEntry>, Box<dyn std::error::Error>> {
    let mut word_set: BTreeSet<String> = BTreeSet::new();

    // Phrases
    let phrases_file = seed_dir.join("phrases.tsv");
    if phrases_file.exists() {
        let content = fs::read_to_string(&phrases_file)?;
        for line in content.lines().filter(|l| !l.starts_with('#')) {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                for token in parts[1].split_whitespace() {
                    let norm = normalize_word(token);
                    if !norm.is_empty() {
                        word_set.insert(norm);
                    }
                }
            }
        }
    }

    // Core Arabic common words
    let core_words = [
        "الله",
        "في",
        "من",
        "على",
        "ما",
        "أن",
        "إلى",
        "هذا",
        "عن",
        "لا",
        "مع",
        "كل",
        "هو",
        "كان",
        "يا",
        "التي",
        "الذي",
        "بعد",
        "أو",
        "ذلك",
        "قال",
        "حبيبي",
        "مرحبا",
        "سلام",
        "شكرا",
        "صباح",
        "خير",
        "مساء",
        "كيفك",
        "وينك",
        "شلونك",
        "بخير",
        "الحمد",
        "لله",
        "انت",
        "عليكم",
        "السلام",
        "الخير",
        "علم",
    ];
    for w in core_words {
        word_set.insert(normalize_word(w));
    }

    let mut result = Vec::new();
    for w in word_set {
        result.push((w, [12, 12, 12, 12, 12, 12], 0));
    }

    Ok(result)
}

fn default_seed_diac() -> DiacMap {
    let mut m = HashMap::new();
    m.insert("حبيبي".to_string(), vec![("حَبِيبِي".to_string(), -1.0, 0xFF)]);
    m.insert(
        "الله".to_string(),
        vec![
            ("اللّه".to_string(), -0.5, 0xFF),
            ("اللَّه".to_string(), -0.8, 0xFF),
        ],
    );
    m.insert("شكرا".to_string(), vec![("شكراً".to_string(), -0.5, 0xFF)]);
    m.insert("مرحبا".to_string(), vec![("مَرْحَبًا".to_string(), -1.0, 0xFF)]);
    m.insert(
        "علم".to_string(),
        vec![
            ("عَلَّمَ".to_string(), -1.0, 0xFF),
            ("عِلْم".to_string(), -1.2, 0xFF),
        ],
    );
    m
}

fn load_diac_tsv(path: &Path) -> Result<DiacMap, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut m: DiacMap = HashMap::new();
    for line in content.lines().filter(|l| !l.starts_with('#')) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let base = normalize_word(parts[0].trim());
            let variant = parts[1].trim().to_string();
            let lp: f32 = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(-1.0);
            let mask: u8 = parts.get(3).and_then(|p| p.parse().ok()).unwrap_or(0xFF);
            m.entry(base).or_default().push((variant, lp, mask));
        }
    }
    Ok(m)
}

fn default_seed_bigrams() -> BigramMap {
    let mut m = HashMap::new();
    m.insert(
        "صباح".to_string(),
        vec![("خير".to_string(), -0.5), ("الخير".to_string(), -0.3)],
    );
    m.insert(
        "مساء".to_string(),
        vec![("خير".to_string(), -0.5), ("الخير".to_string(), -0.3)],
    );
    m.insert(
        "حبيبي".to_string(),
        vec![("انت".to_string(), -0.8), ("يا".to_string(), -1.0)],
    );
    m.insert("الحمد".to_string(), vec![("لله".to_string(), -0.1)]);
    m.insert("السلام".to_string(), vec![("عليكم".to_string(), -0.1)]);
    m
}

fn load_bigrams_tsv(path: &Path) -> Result<BigramMap, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut m: BigramMap = HashMap::new();
    for line in content.lines().filter(|l| !l.starts_with('#')) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let prev = normalize_word(parts[0].trim());
            let next = normalize_word(parts[1].trim());
            let lp: f32 = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(-1.0);
            m.entry(prev).or_default().push((next, lp));
        }
    }
    Ok(m)
}

/// `charlm.tsv` rows (`ngram \t order \t lp`, `^`/`$` boundaries) as (T3A codes, lp).
/// N-grams with a character outside the alphabet are skipped.
type CharLmRows = Vec<(Vec<u8>, f32)>;

fn load_charlm_tsv(path: &Path) -> Result<CharLmRows, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in content
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let codes: Option<Vec<u8>> = parts[0]
            .chars()
            .map(|c| match c {
                '^' => Some(t3a_engine::charlm::BOS),
                '$' => Some(t3a_engine::charlm::EOS),
                c => t3a_code(c),
            })
            .collect();
        let (Some(codes), Ok(lp)) = (codes, parts[2].trim().parse::<f32>()) else {
            continue;
        };
        out.push((codes, lp));
    }
    Ok(out)
}

fn load_lexicon_tsv(path: &Path) -> Result<Vec<RawLexiconEntry>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut result = Vec::new();
    for line in content.lines().filter(|l| !l.starts_with('#')) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.is_empty() {
            continue;
        }
        let word = normalize_word(parts[0].trim());
        if word.is_empty() {
            continue;
        }

        let mut q = [12u8; 6];
        if parts.len() >= 7 {
            for (i, p) in parts[1..7].iter().enumerate() {
                if let Ok(lp) = p.trim().parse::<f32>() {
                    q[i] = quantize_lp(lp);
                }
            }
        }

        let flags = parts
            .get(7)
            .and_then(|f| f.parse::<u16>().ok())
            .unwrap_or(0);
        result.push((word, q, flags));
    }
    Ok(result)
}

struct TrieBuilderNode {
    label: u8,
    word: u32,
    min_q: u8,
    children: BTreeMap<u8, usize>,
}

struct TrieBuilder {
    nodes: Vec<TrieBuilderNode>,
}

impl TrieBuilder {
    fn new() -> Self {
        Self {
            nodes: vec![TrieBuilderNode {
                label: 0,
                word: 0,
                min_q: 255,
                children: BTreeMap::new(),
            }],
        }
    }

    fn insert(&mut self, codes: &[u8], word_idx: u32, min_q: u8) {
        let mut curr = 0;
        self.nodes[curr].min_q = self.nodes[curr].min_q.min(min_q);

        for &c in codes {
            let next = if let Some(&next_idx) = self.nodes[curr].children.get(&c) {
                next_idx
            } else {
                let new_idx = self.nodes.len();
                self.nodes.push(TrieBuilderNode {
                    label: c,
                    word: 0,
                    min_q: 255,
                    children: BTreeMap::new(),
                });
                self.nodes[curr].children.insert(c, new_idx);
                new_idx
            };
            self.nodes[next].min_q = self.nodes[next].min_q.min(min_q);
            curr = next;
        }

        self.nodes[curr].word = word_idx;
    }

    fn build(&self) -> Vec<Node> {
        let mut out = Vec::with_capacity(self.nodes.len());
        // Map original index to BFS flattened index
        let mut index_map: HashMap<usize, usize> = HashMap::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(0);

        while let Some(idx) = queue.pop_front() {
            let flat_idx = out.len();
            index_map.insert(idx, flat_idx);

            out.push(Node {
                first_child: 0,
                word: self.nodes[idx].word,
                label: self.nodes[idx].label,
                child_count: self.nodes[idx].children.len() as u8,
                max_q: self.nodes[idx].min_q,
                flags: 0,
            });

            for &child_idx in self.nodes[idx].children.values() {
                queue.push_back(child_idx);
            }
        }

        // Second pass: set first_child
        for (&orig_idx, &flat_idx) in &index_map {
            if let Some(&first_child_orig) = self.nodes[orig_idx].children.values().next() {
                if let Some(&first_child_flat) = index_map.get(&first_child_orig) {
                    out[flat_idx].first_child = first_child_flat as u32;
                }
            }
        }

        out
    }
}
