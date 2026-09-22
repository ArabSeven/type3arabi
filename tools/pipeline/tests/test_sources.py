from t3ap import sources


def test_registry_is_valid():
    rows = sources.load()
    assert any(r["id"] == "fineweb2" and r["status"] == "approved" for r in rows)
    for r in sources.allowed("lexicon"):
        assert r["status"] == "approved"
