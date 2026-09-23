"""normalize stage (docs/04 §3): raw Arabic text -> norm/<group>/base.txt + norm/<group>/marked.tsv.

Streams every raw file of a group; one worker process per dialect group. Per line:
NFC -> drop tatweel/joiners/bidi/Quranic marks, map letter variants (arabic.py tables) -> skip lines with
< 50% Arabic letters -> tokens = maximal runs of alphabet letters (+ marks) of <= 20 letters.
`base.txt` holds one sentence of base-form tokens per line. Tokens that carry harakat are counted in
their marked form into `marked.tsv` (input of the diac stage). Lines containing an explicit term from
data/seed/no_complete.tsv (drop:/prefix:) are skipped.

Sources: FineWeb-2 only (the only approved/internal source with the `lexicon`/`lm` role that is fetched).
"""
import re
import unicodedata
from collections import Counter
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

from .arabic import DROP, MAP, MARKS, marked_form
from .seedlists import no_complete

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
RAW_DIR = PIPELINE_DATA / "raw"
NORM_DIR = PIPELINE_DATA / "norm"

GROUPS = ["msa", "lev", "egy", "glf", "irq", "mag"]

# Letters of the engine's T3A alphabet (crates/t3a-engine/src/alphabet.rs); anything else splits tokens.
ALPHABET = ("ءآأؤإئابةتثجحخد"
            "ذرزسشصضطظعغفقكل"
            "منهوىيپچڤڨڭگ")
MARK_CHARS = "".join(sorted(MARKS))
TOKEN_RE = re.compile(f"[{ALPHABET}{MARK_CHARS}]+")
LETTER_RE = re.compile(f"[{ALPHABET}]")
# DROP chars removed and letter variants mapped; marks kept (needed for marked tokens).
CLEAN_TABLE = str.maketrans({**{c: None for c in DROP}, **MAP})
STRIP_MARKS = str.maketrans({c: None for c in MARKS})
MAX_LETTERS = 20


def normalize_group(group: str, raw_dir: Path, norm_dir: Path) -> tuple[str, int, int, int]:
    files = sorted((raw_dir / "fineweb2").glob(f"{group}.*.txt"))
    out_dir = norm_dir / group
    out_dir.mkdir(parents=True, exist_ok=True)
    _, drop_words, drop_prefixes = no_complete()
    marked = Counter()
    kept = skipped = tokens = 0
    with (out_dir / "base.txt").open("w", encoding="utf-8", newline="\n") as out:
        for f in files:
            with f.open(encoding="utf-8", errors="replace") as fh:
                for line in fh:
                    line = unicodedata.normalize("NFC", line).translate(CLEAN_TABLE)
                    n_letters = len(LETTER_RE.findall(line))
                    if n_letters == 0 or n_letters * 2 < len(line.replace(" ", "")):
                        skipped += 1
                        continue
                    base_tokens = []
                    bad = False
                    for t in TOKEN_RE.findall(line):
                        b = t.translate(STRIP_MARKS)
                        if not b or len(b) > MAX_LETTERS:
                            continue
                        if b in drop_words or (drop_prefixes and b.startswith(drop_prefixes)):
                            bad = True
                            break
                        if len(b) != len(t):
                            marked[marked_form(t)] += 1
                        base_tokens.append(b)
                    if bad or not base_tokens:
                        skipped += 1
                        continue
                    out.write(" ".join(base_tokens) + "\n")
                    kept += 1
                    tokens += len(base_tokens)
    with (out_dir / "marked.tsv").open("w", encoding="utf-8", newline="\n") as mf:
        for w, c in marked.most_common():
            if c >= 2:
                mf.write(f"{w}\t{c}\n")
    return group, kept, skipped, tokens


def run() -> int:
    NORM_DIR.mkdir(parents=True, exist_ok=True)
    print("=== Stage 2: normalize (docs/04 §3) ===")
    with ProcessPoolExecutor(max_workers=len(GROUPS)) as ex:
        # Directories are passed explicitly: worker processes re-import this module.
        n = len(GROUPS)
        for group, kept, skipped, tokens in ex.map(normalize_group, GROUPS, [RAW_DIR] * n, [NORM_DIR] * n):
            print(f"  [{group.upper()}] {kept:,} sentences kept, {skipped:,} skipped, {tokens:,} tokens")
    return 0
