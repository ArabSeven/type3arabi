"""End-to-end run of stages 2–5 + pairs on the mini fixture, in a temporary directory.

Never touches the real pipeline_data/ and never downloads anything: every module's directory
constants are redirected to tmp_path, and the fixture stands in for fetched FineWeb-2 text.
"""
import shutil
from pathlib import Path

from t3ap import count, diac, lexicon, normalize, pairs

REPO = Path(__file__).resolve().parents[3]
FIXTURE = REPO / "data" / "fixtures" / "mini" / "raw"


def redirect(monkeypatch, root: Path):
    for mod in (normalize, count, lexicon, diac, pairs):
        monkeypatch.setattr(mod, "PIPELINE_DATA", root, raising=False)
    monkeypatch.setattr(normalize, "RAW_DIR", root / "raw")
    monkeypatch.setattr(normalize, "NORM_DIR", root / "norm")
    monkeypatch.setattr(count, "NORM_DIR", root / "norm")
    monkeypatch.setattr(count, "COUNTS_DIR", root / "counts")
    monkeypatch.setattr(lexicon, "COUNTS_DIR", root / "counts")
    monkeypatch.setattr(lexicon, "OUT_DIR", root / "out")
    monkeypatch.setattr(diac, "COUNTS_DIR", root / "counts")
    monkeypatch.setattr(diac, "OUT_DIR", root / "out")
    monkeypatch.setattr(pairs, "RAW_DIR", root / "raw")


def test_stages_end_to_end_on_fixture(tmp_path, monkeypatch):
    redirect(monkeypatch, tmp_path)
    fw = tmp_path / "raw" / "fineweb2"
    fw.mkdir(parents=True)
    for f in FIXTURE.glob("*.txt"):
        shutil.copy(f, fw / f"{f.stem}.fixture.txt")
    parallel = tmp_path / "raw" / "talafha-jordanian"
    parallel.mkdir(parents=True)
    (parallel / "pairs.tsv").write_text(
        "mar7aba\tمرحبا\tLEV\ttrain\n$ofti\tشفتي\tLEV\ttrain\nkifak\tكيفك\tLEV\ttest\n", encoding="utf-8"
    )

    assert normalize.run() == 0
    assert (tmp_path / "norm" / "lev" / "base.txt").stat().st_size > 0

    assert count.run() == 0
    assert (tmp_path / "counts" / "lev.uni.tsv").exists()
    assert (tmp_path / "counts" / "lev.bi.tsv").exists()

    monkeypatch.setattr(lexicon, "MIN_COUNT", 1)
    assert lexicon.run() == 0
    lex = (tmp_path / "out" / "lexicon.tsv").read_text(encoding="utf-8")
    assert "\nالله\t" in lex  # sacred words are always present
    assert (tmp_path / "out" / "charlm.tsv").exists()

    assert diac.run() == 0
    assert (tmp_path / "out" / "diac.tsv").exists()

    assert pairs.run() == 0
    train = (tmp_path / "align" / "train.tsv").read_text(encoding="utf-8")
    assert "mar7aba\tمرحبا" in train
    assert "شفتي" not in train  # `$ofti` uses a symbol the engine cannot type: skipped
    assert "kifak" in (tmp_path / "eval" / "talafha-jordanian.test.tsv").read_text(encoding="utf-8")


def test_junk_filter():
    assert lexicon.is_junk("ههههه")
    assert lexicon.is_junk("ا")
    assert not lexicon.is_junk("و")
    assert not lexicon.is_junk("كتب")
