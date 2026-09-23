"""count stage (docs/04 §5): per dialect group unigram and bigram counts from norm/<group>/base.txt.

Outputs counts/<group>.uni.tsv (word, count; count >= 2) and counts/<group>.bi.tsv (w1, w2, count;
count >= 2), plus counts/marked.tsv (marked token counts merged over groups). Bigram tables are kept
bounded by dropping singletons whenever they grow past BIGRAM_SOFT_LIMIT (standard lossy counting; the
lexicon stage keeps only count >= 3 pairs anyway). Char n-grams are derived later from unigram types.
"""
from collections import Counter
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
NORM_DIR = PIPELINE_DATA / "norm"
COUNTS_DIR = PIPELINE_DATA / "counts"

GROUPS = ["msa", "lev", "egy", "glf", "irq", "mag"]
BIGRAM_SOFT_LIMIT = 12_000_000


def count_group(group: str, norm_dir: Path, counts_dir: Path) -> tuple[str, int, int, int]:
    base = norm_dir / group / "base.txt"
    uni = Counter()
    bi = Counter()
    if base.exists():
        with base.open(encoding="utf-8") as fh:
            for line in fh:
                words = line.split()
                uni.update(words)
                bi.update(a + " " + b for a, b in zip(words, words[1:]))
                if len(bi) > BIGRAM_SOFT_LIMIT:
                    bi = Counter({k: v for k, v in bi.items() if v > 1})
    with (counts_dir / f"{group}.uni.tsv").open("w", encoding="utf-8", newline="\n") as f:
        for w, c in uni.most_common():
            if c < 2:
                break
            f.write(f"{w}\t{c}\n")
    n_bi = 0
    with (counts_dir / f"{group}.bi.tsv").open("w", encoding="utf-8", newline="\n") as f:
        for k, c in bi.most_common():
            if c < 2:
                break
            a, b = k.split(" ", 1)
            f.write(f"{a}\t{b}\t{c}\n")
            n_bi += 1
    return group, sum(uni.values()), len(uni), n_bi


def run() -> int:
    COUNTS_DIR.mkdir(parents=True, exist_ok=True)
    print("=== Stage 3: count (docs/04 §5) ===")
    with ProcessPoolExecutor(max_workers=3) as ex:
        # Directories are passed explicitly: worker processes re-import this module.
        n = len(GROUPS)
        for group, tokens, types, n_bi in ex.map(count_group, GROUPS, [NORM_DIR] * n, [COUNTS_DIR] * n):
            print(f"  [{group.upper()}] {tokens:,} tokens, {types:,} types, {n_bi:,} bigrams (count >= 2)")
    marked = Counter()
    for g in GROUPS:
        mf = NORM_DIR / g / "marked.tsv"
        if mf.exists():
            for line in mf.read_text(encoding="utf-8").splitlines():
                w, c = line.split("\t")
                marked[w] += int(c)
    with (COUNTS_DIR / "marked.tsv").open("w", encoding="utf-8", newline="\n") as f:
        for w, c in marked.most_common():
            f.write(f"{w}\t{c}\n")
    print(f"  [MARK] {len(marked):,} marked token types")
    return 0
