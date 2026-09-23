"""lexicon stage (docs/04 §5): counts -> out/lexicon.tsv, out/bigrams.tsv, out/charlm.tsv.

lexicon.tsv  word, lp_msa..lp_mag (ln P(w|d), add-0.5 smoothing over the kept vocabulary; words unseen in a
             group get -99 = "unseen", quantized to 255), flags (1 TANWEEN_FATH, 2 NO_COMPLETE, 4 SACRED).
             Selection: total count >= MIN_COUNT, junk filtered, ranked by max_d(lp_d + ln prior_d), cap.
bigrams.tsv  prev, next, ln P(next | prev) (absolute discounting D=0.75 over all groups, MSA weight 0.5);
             count >= 3, both words in the lexicon, top 64 successors per word, global cap.
charlm.tsv   ngram, order, ln P(last char | preceding chars) for orders 1..5 over base forms with ^/$
             boundaries, weighted by token frequency; entries with count < CHR_MIN dropped, cap by count.
"""
import math
from collections import Counter, defaultdict
from pathlib import Path

from .arabic import base_form
from .seedlists import no_complete

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
COUNTS_DIR = PIPELINE_DATA / "counts"
OUT_DIR = PIPELINE_DATA / "out"

GROUPS = ["msa", "lev", "egy", "glf", "irq", "mag"]
PRIORS = [0.35, 0.20, 0.20, 0.12, 0.05, 0.08]
MIN_COUNT = 5
MAX_WORDS = 600_000
UNSEEN_LP = -99.0
MAX_BIGRAMS = 3_000_000
SUCCESSORS = 64
CHR_ORDER = 5
CHR_MIN = 3
CHR_CAP = 600_000

# Must equal crates/t3a-engine/src/display.rs SACRED (tested in tests/test_lexicon.py).
SACRED_WORDS = {"الله", "والله", "بالله", "تالله", "فالله", "لله", "ولله", "فلله", "اللهم", "وبالله", "فبالله"}
ADVERBIAL_TANWEEN = {
    "شكرا", "جدا", "أهلا", "اهلا", "طبعا", "عفوا", "أبدا", "ابدا", "مثلا", "تقريبا", "حالا", "دائما", "معا",
    "جميعا", "تماما", "قليلا", "كثيرا", "أيضا", "ايضا", "غدا", "أحيانا", "احيانا", "فورا", "سابقا", "لاحقا",
    "حاليا", "خصوصا", "عموما", "أولا", "اولا", "ثانيا", "أخيرا", "اخيرا", "مجانا", "صباحا", "مساء", "يوميا",
}
# Single letters that are real words (or common clitic spellings) — every other 1-letter token is noise.
ONE_LETTER_OK = {"و", "ع"}


def is_junk(w: str) -> bool:
    if len(w) == 1:
        return w not in ONE_LETTER_OK
    run = 1
    for a, b in zip(w, w[1:]):
        run = run + 1 if a == b else 1
        if run >= 3:  # elongations / laughter (هههه) are handled by the engine, not the lexicon
            return True
    return False


def load_unigrams():
    counts = {g: Counter() for g in GROUPS}
    for g in GROUPS:
        f = COUNTS_DIR / f"{g}.uni.tsv"
        if f.exists():
            with f.open(encoding="utf-8") as fh:
                for line in fh:
                    w, c = line.rstrip("\n").split("\t")
                    counts[g][w] = int(c)
    return counts


def run() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    print("=== Stage 4: lexicon (docs/04 §5) ===")
    counts = load_unigrams()
    total = Counter()
    for g in GROUPS:
        total.update(counts[g])
    vocab = [w for w, c in total.items() if c >= MIN_COUNT and not is_junk(w)]
    V = len(vocab)
    N = {g: sum(counts[g].values()) for g in GROUPS}
    print(f"  {len(total):,} types, {V:,} with count >= {MIN_COUNT} after junk filter")

    no_comp, _, no_comp_prefixes = no_complete()
    tanween = Counter()
    marked_f = COUNTS_DIR / "marked.tsv"
    if marked_f.exists():
        with marked_f.open(encoding="utf-8") as fh:
            for line in fh:
                m, c = line.rstrip("\n").split("\t")
                if "ً" in m:
                    tanween[base_form(m)] += int(c)

    ranked = []
    for w in vocab:
        lps = []
        for g in GROUPS:
            c = counts[g].get(w, 0)
            lps.append(math.log((c + 0.5) / (N[g] + 0.5 * V)) if c > 0 and N[g] > 0 else UNSEEN_LP)
        score = max(lp + math.log(p) for lp, p in zip(lps, PRIORS) if lp > UNSEEN_LP)
        flags = 0
        if w.endswith("ا") and (w in ADVERBIAL_TANWEEN
                                or tanween[w] >= 50 or tanween[w] >= 0.25 * total[w] > 0):
            flags |= 1
        if w in no_comp or (no_comp_prefixes and w.startswith(no_comp_prefixes)):
            flags |= 2
        if w in SACRED_WORDS:
            flags |= 4
        ranked.append((score, w, lps, flags))
    for w in SACRED_WORDS:  # always present even if rare in the sample
        if w not in total or total[w] < MIN_COUNT:
            ranked.append((0.0, w, [-12.0] * 6, 4))
    ranked.sort(key=lambda x: (-x[0], x[1]))
    ranked = ranked[:MAX_WORDS]
    lexicon = {w for _, w, _, _ in ranked}

    with (OUT_DIR / "lexicon.tsv").open("w", encoding="utf-8", newline="\n") as f:
        f.write("# word\tlp_msa\tlp_lev\tlp_egy\tlp_glf\tlp_irq\tlp_mag\tflags\n")
        for _, w, lps, flags in ranked:
            f.write(w + "\t" + "\t".join(f"{lp:.4f}" for lp in lps) + f"\t{flags}\n")
    print(f"  [LEX] {len(ranked):,} words")

    # ---- bigrams: absolute discounting, conditional on prev word
    pair = Counter()
    for g in GROUPS:
        wgt = 0.5 if g == "msa" else 1.0
        f = COUNTS_DIR / f"{g}.bi.tsv"
        if not f.exists():
            continue
        with f.open(encoding="utf-8") as fh:
            for line in fh:
                a, b, c = line.rstrip("\n").split("\t")
                if a in lexicon and b in lexicon:
                    pair[(a, b)] += wgt * int(c)
    prev_total = Counter()
    for g in GROUPS:
        wgt = 0.5 if g == "msa" else 1.0
        for w, c in counts[g].items():
            if w in lexicon:
                prev_total[w] += wgt * c
    succ = defaultdict(list)
    for (a, b), c in pair.items():
        if c >= 3:
            succ[a].append((c, b))
    rows = []
    for a, lst in succ.items():
        lst.sort(reverse=True)
        for c, b in lst[:SUCCESSORS]:
            rows.append((c, a, b, math.log(max(c - 0.75, 0.25) / max(prev_total[a], c))))
    rows.sort(key=lambda r: -r[0])
    rows = rows[:MAX_BIGRAMS]
    with (OUT_DIR / "bigrams.tsv").open("w", encoding="utf-8", newline="\n") as f:
        f.write("# prev\tnext\tlp\n")
        for _, a, b, lp in sorted(rows, key=lambda r: (r[1], -r[0])):
            f.write(f"{a}\t{b}\t{lp:.4f}\n")
    print(f"  [BIGR] {len(rows):,} pairs ({len(succ):,} prev words)")

    # ---- char LM: conditional log-probs for orders 1..5 over base forms with boundaries
    ng = [Counter() for _ in range(CHR_ORDER + 1)]
    for w, c in total.items():
        if c < 2 or is_junk(w):
            continue
        s = "^" + w + "$"
        for n in range(1, CHR_ORDER + 1):
            for i in range(len(s) - n + 1):
                gram = s[i:i + n]
                if n == 1 and gram == "^":
                    continue
                ng[n][gram] += c
    uni_total = sum(ng[1].values())
    entries = []
    for n in range(1, CHR_ORDER + 1):
        for gram, c in ng[n].items():
            if c < CHR_MIN:
                continue
            if n == 1:
                lp = math.log(c / uni_total)
            else:
                ctx = gram[:-1]
                ctx_c = ng[n - 1].get(ctx, 0) if ctx != "^" else sum(v for k, v in ng[2].items() if k[0] == "^")
                if ctx == "^":
                    ctx_c = ctx_c or 1
                if ctx_c == 0:
                    continue
                # context counts exclude the final '$' position; clamp to a probability
                lp = min(0.0, math.log(c / ctx_c))
            entries.append((c, n, gram, lp))
    entries.sort(key=lambda e: (-e[0], e[2]))
    entries = entries[:CHR_CAP]
    with (OUT_DIR / "charlm.tsv").open("w", encoding="utf-8", newline="\n") as f:
        f.write("# ngram\torder\tlp\n")
        for _, n, gram, lp in entries:
            f.write(f"{gram}\t{n}\t{lp:.4f}\n")
    print(f"  [CHLM] {len(entries):,} n-grams (orders 1..{CHR_ORDER})")
    return 0
