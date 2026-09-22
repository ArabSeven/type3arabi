"""diac stage: vocalized variants per base word from marked tokens (docs/04 §5)."""
from collections import defaultdict
import math
from pathlib import Path

from .arabic import base_form

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
COUNTS_DIR = PIPELINE_DATA / "counts"
OUT_DIR = PIPELINE_DATA / "out"


def run() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    print("=== Stage 5: diac (docs/04 §5) ===")

    marked_f = COUNTS_DIR / "marked.tsv"
    if not marked_f.exists():
        print("  counts/marked.tsv not found; skipping")
        return 0

    base_to_variants = defaultdict(lambda: defaultdict(int))
    base_totals = defaultdict(int)

    for line in marked_f.read_text(encoding="utf-8").splitlines():
        if line.strip():
            parts = line.split("\t")
            if len(parts) >= 2:
                m_word, count = parts[0], int(parts[1])
                b_word = base_form(m_word)
                if b_word and m_word != b_word:
                    base_to_variants[b_word][m_word] += count
                    base_totals[b_word] += count

    out_diac = OUT_DIR / "diac.tsv"
    lines = ["# base\tvariant\tlp\tdialect_mask"]
    total_entries = 0

    for b_word, vars_dict in base_to_variants.items():
        tot = base_totals[b_word]
        # Sort variants by count descending, keep up to 8
        sorted_vars = sorted(vars_dict.items(), key=lambda x: x[1], reverse=True)[:8]
        for m_word, c in sorted_vars:
            prob = c / max(1, tot)
            lp = math.log(prob)
            mask = 0xFF  # valid across all dialects unless constrained
            lines.append(f"{b_word}\t{m_word}\t{lp:.4f}\t{mask}")
            total_entries += 1

    out_diac.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"  [DIAC] {total_entries} vocalization variants for {len(base_to_variants)} base words written to {out_diac.name}")
    return 0
