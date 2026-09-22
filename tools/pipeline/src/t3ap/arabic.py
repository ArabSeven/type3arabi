"""Arabic normalization — MUST match crates/t3a-engine/src/arabic.rs::normalize_word (docs/04 §3).
Shared test vectors: data/eval/normalization.tsv (tested in both languages)."""
import unicodedata

MARKS = {chr(c) for c in range(0x064B, 0x0653)} | {"ٰ"}
DROP = {"ـ", "‌", "‍", "‎", "‏"} \
    | {chr(c) for c in range(0x202A, 0x202F)} | {chr(c) for c in range(0x2066, 0x206A)} \
    | {chr(c) for c in range(0x06D6, 0x06EE)} | {chr(c) for c in range(0x0615, 0x061B)}
MAP = {"ٱ": "ا", "ی": "ي", "ک": "ك", "ہ": "ه"}


def base_form(s: str) -> str:
    """NFC, drop tatweel/joiners/bidi/Quranic marks, map Farsi/Quranic letter variants, strip harakat."""
    s = unicodedata.normalize("NFC", s)
    out = []
    for c in s:
        if c in DROP or c in MARKS:
            continue
        out.append(MAP.get(c, c))
    return "".join(out)


def marked_form(s: str) -> str:
    """Like base_form but keeps harakat, reordered to typing order: shadda, vowel, dagger alif (docs/03 §10.1)."""
    s = unicodedata.normalize("NFC", s)
    out, shadda, vowel, dagger = [], False, None, False

    def flush():
        nonlocal shadda, vowel, dagger
        if shadda:
            out.append("ّ")
        if vowel:
            out.append(vowel)
        if dagger:
            out.append("ٰ")
        shadda, vowel, dagger = False, None, False

    for c in s:
        if c in DROP:
            continue
        if c == "ّ":
            shadda = True
        elif c == "ٰ":
            dagger = True
        elif c in MARKS:
            vowel = c
        else:
            flush()
            out.append(MAP.get(c, c))
    flush()
    return "".join(out)
