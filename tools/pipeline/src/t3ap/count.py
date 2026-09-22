"""count stage: unigrams, bigrams, char 5-grams, marked tokens (docs/04 §5)."""
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
NORM_DIR = PIPELINE_DATA / "norm"
COUNTS_DIR = PIPELINE_DATA / "counts"

GROUPS = ["msa", "lev", "egy", "glf", "irq", "mag"]


def run() -> int:
    COUNTS_DIR.mkdir(parents=True, exist_ok=True)
    print("=== Stage 3: count (docs/04 §5) ===")

    unigrams = defaultdict(Counter)
    bigrams = Counter()
    char5 = Counter()
    marked_counts = Counter()

    for grp in GROUPS:
        base_file = NORM_DIR / grp / "sentences.base.txt"
        marked_file = NORM_DIR / grp / "sentences.marked.txt"
        weight = 0.5 if grp == "msa" else 1.0

        if base_file.exists():
            for line in base_file.read_text(encoding="utf-8").splitlines():
                words = line.split()
                for w in words:
                    unigrams[grp][w] += 1
                    # Char 5-grams with boundary markers
                    padded = f"^{w}$"
                    for i in range(len(padded) - 4):
                        char5[padded[i : i + 5]] += 1

                for i in range(len(words) - 1):
                    pair = (words[i], words[i + 1])
                    bigrams[pair] += weight

        if marked_file.exists():
            for line in marked_file.read_text(encoding="utf-8").splitlines():
                for w in line.split():
                    # If it contains any mark
                    if any(c in "ًٌٍَُِّْٰ" for c in w):
                        marked_counts[w] += 1

    # Write unigrams per group
    for grp in GROUPS:
        out_f = COUNTS_DIR / f"{grp}.uni.tsv"
        lines = [f"{w}\t{c}" for w, c in unigrams[grp].most_common()]
        out_f.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"  [{grp.upper()}] {len(lines)} unigrams written to {out_f.name}")

    # Write bigrams
    out_bi = COUNTS_DIR / "all.bi.tsv"
    bi_lines = [f"{w1}\t{w2}\t{c:.2f}" for (w1, w2), c in bigrams.most_common() if c >= 0.5]
    out_bi.write_text("\n".join(bi_lines) + "\n", encoding="utf-8")
    print(f"  [BIGR] {len(bi_lines)} bigrams written to {out_bi.name}")

    # Write char 5-grams
    out_chr5 = COUNTS_DIR / "chr5.tsv"
    chr_lines = [f"{ngram}\t{c}" for ngram, c in char5.most_common(600_000)]
    out_chr5.write_text("\n".join(chr_lines) + "\n", encoding="utf-8")
    print(f"  [CHR5] {len(chr_lines)} char 5-grams written to {out_chr5.name}")

    # Write marked counts
    out_marked = COUNTS_DIR / "marked.tsv"
    m_lines = [f"{w}\t{c}" for w, c in marked_counts.most_common()]
    out_marked.write_text("\n".join(m_lines) + "\n", encoding="utf-8")
    print(f"  [MARK] {len(m_lines)} marked tokens written to {out_marked.name}")

    return 0
