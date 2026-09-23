"""Readers for hand-authored seed lists in data/seed/ used by several stages."""
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
SEED_DIR = REPO / "data" / "seed"


def no_complete():
    """Returns (no_complete_words, drop_words, drop_prefixes) from data/seed/no_complete.tsv."""
    words, drop, prefixes = set(), set(), []
    f = SEED_DIR / "no_complete.tsv"
    if not f.exists():
        return words, drop, prefixes
    lines = [l.strip() for l in f.read_text(encoding="utf-8").splitlines()]
    for line in lines:
        if not line or line.startswith("#") or line == "word":
            continue
        if line.startswith("drop:"):
            w = line[5:]
            drop.add(w)
            words.add(w)
        elif line.startswith("prefix:"):
            prefixes.append(line[7:])
        else:
            words.add(line)
    return words, drop, tuple(prefixes)
