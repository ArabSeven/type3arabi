from pathlib import Path
from t3ap import fetch, normalize, count, lexicon, diac

REPO = Path(__file__).resolve().parents[3]
PIPELINE_DATA = REPO / "pipeline_data"


def test_stages_end_to_end():
    assert fetch.run(mode="internal") == 0
    assert (PIPELINE_DATA / "manifest.lock.json").exists()

    assert normalize.run() == 0
    assert (PIPELINE_DATA / "norm" / "msa" / "sentences.base.txt").exists()

    assert count.run() == 0
    assert (PIPELINE_DATA / "counts" / "msa.uni.tsv").exists()
    assert (PIPELINE_DATA / "counts" / "all.bi.tsv").exists()

    assert lexicon.run() == 0
    assert (PIPELINE_DATA / "out" / "lexicon.tsv").exists()

    assert diac.run() == 0
    assert (PIPELINE_DATA / "out" / "diac.tsv").exists()
