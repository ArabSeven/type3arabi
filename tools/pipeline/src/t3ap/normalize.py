"""normalize stage: raw Arabic text -> norm/<group>/*.txt (docs/04 §3)."""
import re
from pathlib import Path

from .arabic import base_form, marked_form

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
RAW_DIR = PIPELINE_DATA / "raw"
NORM_DIR = PIPELINE_DATA / "norm"

GROUPS = ["msa", "lev", "egy", "glf", "irq", "mag"]
ARABIC_CHAR_RE = re.compile(r"[\u0621-\u064A\u0671\u067E\u0686\u06A4\u06A9\u06AF\u06CC]")


def is_valid_sentence(line: str) -> bool:
    line = line.strip()
    if not line:
        return False
    arabic_count = len(ARABIC_CHAR_RE.findall(line))
    # Filter sentences with < 50% Arabic-letter characters (docs/04 §3)
    return (arabic_count / max(1, len(line))) >= 0.5


def tokenize_words(line: str) -> list[str]:
    # maximal run of Arabic letters + marks
    raw_tokens = re.findall(r"[\u0621-\u065F\u0670-\u06D3]+", line)
    return [t for t in raw_tokens if len(t) <= 20]


def run() -> int:
    NORM_DIR.mkdir(parents=True, exist_ok=True)
    print("=== Stage 2: normalize (docs/04 §3) ===")

    for grp in GROUPS:
        grp_dir = NORM_DIR / grp
        grp_dir.mkdir(parents=True, exist_ok=True)
        out_base_file = grp_dir / "sentences.base.txt"
        out_marked_file = grp_dir / "sentences.marked.txt"

        # Search for raw files for this group in RAW_DIR
        input_files = list(RAW_DIR.glob(f"**/*{grp}*.txt"))
        if not input_files:
            continue

        base_lines = []
        marked_lines = []

        for in_file in input_files:
            text = in_file.read_text(encoding="utf-8")
            for line in text.splitlines():
                if is_valid_sentence(line):
                    tokens = tokenize_words(line)
                    if tokens:
                        b_tokens = [base_form(t) for t in tokens]
                        m_tokens = [marked_form(t) for t in tokens]
                        base_lines.append(" ".join(b_tokens))
                        marked_lines.append(" ".join(m_tokens))

        out_base_file.write_text("\n".join(base_lines) + "\n", encoding="utf-8")
        out_marked_file.write_text("\n".join(marked_lines) + "\n", encoding="utf-8")
        print(f"  [{grp.upper()}] {len(base_lines)} normalized sentences written to {grp_dir}")

    return 0
