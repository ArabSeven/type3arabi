"""Word-level Arabizi↔Arabic pairs from the parallel sources (docs/04 §6.1) and the eval sets built from them.

`pairs` stage outputs (pipeline_data/):
  align/train.tsv            latin \t arabic \t dialect \t source      (train split; input of `t3a-cli train-rules`)
  eval/<source>.dev.tsv      arabizi \t expected \t dialect \t note     (tuning allowed)
  eval/<source>.test.tsv     same format                             (held out: reporting only, never tuning)

Sentence pairs are tokenized on whitespace (Latin) and on Arabic-letter runs (Arabic); pairs with equal
token counts are aligned 1:1, others are dropped (the DP aligner of docs/04 §6.1 is backlog). Only sources
whose registry status allows the `rules`/`tuning`/`eval` roles in the current mode are used (R14).
"""
import re
import unicodedata
from collections import Counter
from pathlib import Path

from . import sources
from .normalize import ALPHABET, CLEAN_TABLE, STRIP_MARKS

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
RAW_DIR = PIPELINE_DATA / "raw"

LATIN_TOKEN = re.compile(r"[A-Za-z0-9']+")
ARABIC_TOKEN = re.compile(f"[{ALPHABET}]+")


def arabic_tokens(text: str) -> list[str]:
    text = unicodedata.normalize("NFC", text).translate(CLEAN_TABLE).translate(STRIP_MARKS)
    return ARABIC_TOKEN.findall(text)


EDGE_PUNCT = ".,!?;:\"()[]{}«»؟،-…"


def latin_tokens(text: str) -> list[str | None]:
    """Whitespace tokens with edge punctuation trimmed. A token containing any other symbol (e.g. `$`
    used for ش in some sources) becomes None: it is skipped but keeps its position for alignment."""
    out = []
    for raw in text.split():
        t = raw.strip(EDGE_PUNCT)
        if not t:
            continue
        out.append(t if LATIN_TOKEN.fullmatch(t) and any(c.isalpha() for c in t) else None)
    return out


def word_pairs(source_id: str):
    f = RAW_DIR / source_id / "pairs.tsv"
    if not f.exists():
        return
    with f.open(encoding="utf-8") as fh:
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 4:
                continue
            latin, arabic, dialect, split = parts[:4]
            lt, at = latin_tokens(latin), arabic_tokens(arabic)
            if not lt or len(lt) != len(at):
                continue
            for a, b in zip(lt, at):
                if a is not None:
                    yield a, b, dialect, split


def run(mode: str = "internal") -> int:
    print("=== pairs: word pairs for rule training + held-out eval sets ===")
    rules_ok = {s["id"] for s in sources.allowed("rules", mode)}
    eval_ok = {s["id"] for s in sources.load() if "eval" in s["roles"] and s["status"] != "blocked"}
    align_dir, eval_dir = PIPELINE_DATA / "align", PIPELINE_DATA / "eval"
    align_dir.mkdir(parents=True, exist_ok=True)
    eval_dir.mkdir(parents=True, exist_ok=True)
    n_train = 0
    with (align_dir / "train.tsv").open("w", encoding="utf-8", newline="\n") as train:
        for sid in sorted(rules_ok | eval_ok):
            counts = Counter()
            outs = {}
            if sid in eval_ok:
                for split in ("dev", "test"):
                    outs[split] = (eval_dir / f"{sid}.{split}.tsv").open("w", encoding="utf-8", newline="\n")
                    outs[split].write("# held-out word pairs from " + sid + f" ({split} split)\n")
                    outs[split].write("arabizi\texpected\tdialect\tnote\n")
            seen = set()
            for latin, arabic, dialect, split in word_pairs(sid) or []:
                counts[split] += 1
                if split == "train" and sid in rules_ok:
                    train.write(f"{latin}\t{arabic}\t{dialect}\t{sid}\n")
                    n_train += 1
                elif split in outs and (latin.lower(), arabic) not in seen:
                    seen.add((latin.lower(), arabic))
                    outs[split].write(f"{latin}\t{arabic}\t{dialect}\t{sid}\n")
            for split, f in outs.items():
                f.close()
                if not counts[split]:
                    (eval_dir / f"{sid}.{split}.tsv").unlink()
            if counts:
                print(f"  [{sid}] word pairs: train {counts['train']}, dev {counts['dev']}, test {counts['test']}")
    print(f"  align/train.tsv: {n_train} pairs")
    return 0
