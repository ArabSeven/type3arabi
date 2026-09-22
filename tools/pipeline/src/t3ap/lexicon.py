from collections import defaultdict
import math
from pathlib import Path

from .arabic import base_form

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
COUNTS_DIR = PIPELINE_DATA / "counts"
OUT_DIR = PIPELINE_DATA / "out"
SEED_DIR = REPO / "data" / "seed"

GROUPS = ["msa", "lev", "egy", "glf", "irq", "mag"]
PRIORS = [0.35, 0.20, 0.20, 0.12, 0.05, 0.08]
SACRED_WORDS = {"الله", "لله", "بالله", "والله", "تالله", "فلله"}
ADVERBIAL_TANWEEN = {
    "شكرا", "جدا", "أهلا", "اهلا", "طبعا", "عفوا", "أبدا", "ابدا", "مثلا", "تقريبا", "حالا", "دائما", "معا", "جميعا", "تماما", "قليلا"
}


def run() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    print("=== Stage 4: lexicon (docs/04 §5) ===")

    # Load unigram counts per group
    counts = {g: {} for g in GROUPS}
    totals = {g: 0 for g in GROUPS}
    union_vocab = set()

    for g in GROUPS:
        uni_f = COUNTS_DIR / f"{g}.uni.tsv"
        if uni_f.exists():
            for line in uni_f.read_text(encoding="utf-8").splitlines():
                if line.strip():
                    parts = line.split("\t")
                    if len(parts) >= 2:
                        w, c = parts[0], int(parts[1])
                        counts[g][w] = c
                        totals[g] += c
                        union_vocab.add(w)

    # Load no_complete seed list
    no_complete = set()
    no_comp_f = SEED_DIR / "no_complete.tsv"
    if no_comp_f.exists():
        for line in no_comp_f.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                no_complete.add(line)

    # Load marked tanween counts
    tanween_counts = defaultdict(int)
    marked_f = COUNTS_DIR / "marked.tsv"
    if marked_f.exists():
        for line in marked_f.read_text(encoding="utf-8").splitlines():
            if line.strip():
                parts = line.split("\t")
                if len(parts) >= 2:
                    m_word, c = parts[0], int(parts[1])
                    if "\u064b" in m_word:
                        b = base_form(m_word)
                        if b:
                            tanween_counts[b] += c

    V = max(1, len(union_vocab))
    ranked_words = []

    for w in union_vocab:
        lps = []
        tot_c = 0
        for g in GROUPS:
            c = counts[g].get(w, 0)
            tot_c += c
            # Add-0.5 smoothing
            tot = totals[g]
            if tot > 0:
                prob = (c + 0.5) / (tot + 0.5 * V)
                lp = math.log(prob)
            else:
                lp = -31.75
            lps.append(lp)

        # Mixture score: max_d (lp[d] + ln prior_d)
        mix_score = max(lp + math.log(p) for lp, p in zip(lps, PRIORS))

        # Flags: bit0 = TANWEEN_FATH, bit1 = NO_COMPLETE, bit2 = SACRED
        flags = 0
        if w.endswith("ا"):
            is_tanween = False
            if w in ADVERBIAL_TANWEEN:
                is_tanween = True
            elif tanween_counts.get(w, 0) > 0 and tot_c > 0:
                if (tanween_counts[w] / tot_c >= 0.25) or (tanween_counts[w] >= 50):
                    is_tanween = True
            if is_tanween:
                flags |= 1
        if w in no_complete:
            flags |= 2
        if w in SACRED_WORDS:
            flags |= 4

        ranked_words.append((mix_score, w, lps, flags))

    ranked_words.sort(key=lambda x: x[0], reverse=True)

    # Write out/lexicon.tsv
    out_lex = OUT_DIR / "lexicon.tsv"
    lines = ["# word\tlp_msa\tlp_lev\tlp_egy\tlp_glf\tlp_irq\tlp_mag\tflags"]
    for _, w, lps, flags in ranked_words:
        lp_strs = "\t".join(f"{lp:.4f}" for lp in lps)
        lines.append(f"{w}\t{lp_strs}\t{flags}")

    out_lex.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"  [LEX] {len(ranked_words)} words written to {out_lex.name}")

    # Process bigrams
    in_bi = COUNTS_DIR / "all.bi.tsv"
    out_bi = OUT_DIR / "bigrams.tsv"
    if in_bi.exists():
        bi_lines = ["# prev\tnext\tlp"]
        for line in in_bi.read_text(encoding="utf-8").splitlines():
            parts = line.split("\t")
            if len(parts) >= 3:
                w1, w2, count_str = parts[0], parts[1], parts[2]
                if w1 in union_vocab and w2 in union_vocab:
                    c = float(count_str)
                    lp = math.log(max(0.0001, c / max(1.0, totals["msa"] + totals["lev"])))
                    bi_lines.append(f"{w1}\t{w2}\t{lp:.4f}")
        out_bi.write_text("\n".join(bi_lines) + "\n", encoding="utf-8")
        print(f"  [BIGR] {len(bi_lines) - 1} filtered bigrams written to {out_bi.name}")

    # Process charlm
    in_chr5 = COUNTS_DIR / "chr5.tsv"
    out_chr = OUT_DIR / "charlm.tsv"
    if in_chr5.exists():
        chr_lines = ["# ngram\tcount\tlp"]
        total_ch5 = sum(int(l.split("\t")[1]) for l in in_chr5.read_text(encoding="utf-8").splitlines() if l.strip())
        for line in in_chr5.read_text(encoding="utf-8").splitlines()[:50000]:
            parts = line.split("\t")
            if len(parts) >= 2:
                ng, c = parts[0], int(parts[1])
                lp = math.log(c / max(1, total_ch5))
                chr_lines.append(f"{ng}\t{c}\t{lp:.4f}")
        out_chr.write_text("\n".join(chr_lines) + "\n", encoding="utf-8")
        print(f"  [CHLM] {len(chr_lines) - 1} character n-grams written to {out_chr.name}")

    return 0
