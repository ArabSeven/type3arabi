from t3ap import sources


def test_registry_is_valid():
    rows = sources.load()
    assert any(r["id"] == "fineweb2" and r["status"] == "approved" for r in rows)
    assert any(r["id"] == "talafha-jordanian" and r["status"] == "internal" for r in rows)
    
    # Internal mode allows approved + internal
    allowed_internal = sources.allowed("rules", mode="internal")
    assert any(r["id"] == "talafha-jordanian" for r in allowed_internal)
    
    # Release mode strictly requires approved
    allowed_release = sources.allowed("rules", mode="release")
    assert not any(r["id"] == "talafha-jordanian" for r in allowed_release)
    assert all(r["status"] == "approved" for r in allowed_release)


def test_deterministic_split():
    # Stable hashes for known keys
    splits = [sources.split_row(f"word_{i}\tarabic_{i}") for i in range(1000)]
    train_count = splits.count("train")
    dev_count = splits.count("dev")
    test_count = splits.count("test")
    
    # Should roughly follow 80/10/10 (+- 5%)
    assert 750 <= train_count <= 850
    assert 70 <= dev_count <= 130
    assert 70 <= test_count <= 130
    
    # Deterministic: identical key produces identical split
    assert sources.split_row("mar7aba\tمرحبا") == sources.split_row("mar7aba\tمرحبا")
