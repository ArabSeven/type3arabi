from pathlib import Path

from t3ap.arabic import base_form, marked_form

VECTORS = Path(__file__).resolve().parents[3] / "data" / "eval" / "normalization.tsv"


def test_vectors_match_rust():
    lines = [l for l in VECTORS.read_text(encoding="utf-8").splitlines() if l and not l.startswith("#")][1:]
    assert len(lines) >= 10
    for line in lines:
        src, want, note = line.split("\t")
        assert base_form(src) == want, note


def test_marked_form_order():
    # NFC puts fatha (ccc 30) before shadda (ccc 33); we store shadda first.
    assert marked_form("اللَّه") == "اللَّه"
