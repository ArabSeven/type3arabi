//! Lexicon and entry inspector for `type3arabi.dat` (docs/01 §3, docs/12).

use std::path::Path;
use t3a_data::{dequantize_lp, DataFile};
use t3a_engine::dialect::Dialect;

pub fn inspect_word(data_path: &Path, word: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Inspecting '{word}' in {} ===", data_path.display());

    let file = DataFile::open(data_path)?;
    let view = file.view()?;

    let words = view.words()?;
    let mut found = None;

    for (idx, w) in words.iter().enumerate() {
        if let Ok(surf) = view.string(w.surface) {
            if surf == word {
                found = Some((idx, *w, surf));
                break;
            }
        }
    }

    let Some((idx, w, surf)) = found else {
        println!(
            "Word '{word}' not found in lexicon ({} total words).",
            words.len()
        );
        return Ok(());
    };

    println!("Word:        {surf}");
    println!("WREC index:  {idx}");
    println!("STRS offset: {}", w.surface);
    println!(
        "Flags:       0x{:04X} [TANWEEN_FATH: {}, NO_COMPLETE: {}, SACRED: {}]",
        w.flags,
        (w.flags & 1) != 0,
        (w.flags & 2) != 0,
        (w.flags & 4) != 0
    );

    println!("Per-dialect log-probs (q / lp):");
    let dialects = [
        Dialect::Msa,
        Dialect::Lev,
        Dialect::Egy,
        Dialect::Glf,
        Dialect::Irq,
        Dialect::Mag,
    ];
    for d in dialects {
        let q = w.q[d.index()];
        let lp = dequantize_lp(q);
        if q == 255 {
            println!("  {:<4}: absent (q=255)", d.as_str());
        } else {
            println!("  {:<4}: {:>7.3} (q={:>3})", d.as_str(), lp, q);
        }
    }

    // Vocalization variants
    if let Ok(Some(diac_view)) = view.diacritics() {
        if w.diac > 0 {
            let variants = diac_view.variants_for((w.diac - 1) as usize);
            println!("Vocalizations ({} variants):", variants.len());
            for v in variants {
                let v_surf = view.string(v.s).unwrap_or("<?>");
                let lp = dequantize_lp(v.q);
                println!(
                    "  - {:<15} lp={:>7.3} (q={:>3}, dialects=0x{:02X})",
                    v_surf, lp, v.q, v.dialect_mask
                );
            }
        } else {
            println!("Vocalizations: none");
        }
    } else {
        println!("Vocalizations: DIAC section not present");
    }

    // Bigrams
    if let Ok(Some(bigr_view)) = view.bigrams() {
        let succs = bigr_view.successors_of(idx);
        if !succs.is_empty() {
            println!("Bigram successors ({} entries):", succs.len());
            for pair in succs.iter().take(10) {
                if let Some(next_rec) = words.get(pair.next as usize) {
                    let next_str = view.string(next_rec.surface).unwrap_or("<?>");
                    let lp = dequantize_lp(pair.q);
                    println!("  -> {:<15} lp={:>7.3} (q={:>3})", next_str, lp, pair.q);
                }
            }
            if succs.len() > 10 {
                println!("  ... and {} more", succs.len() - 10);
            }
        } else {
            println!("Bigram successors: none");
        }
    } else {
        println!("Bigram successors: BIGR section not present");
    }

    Ok(())
}
