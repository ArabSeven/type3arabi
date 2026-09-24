"""Registry invariants (AGENTS.md R14, ADR-0010)."""
from t3ap import datasets, sources


def test_registry_is_valid():
    rows = sources.load()
    assert rows


def test_exclude_nc_drops_every_nc_source():
    kept = sources.allowed("rules", "release", exclude_nc=True)
    assert all(r.get("license_family") != "nc" for r in kept)
    assert any(r.get("license_family") == "nc" for r in sources.allowed("rules", "release"))


def test_release_mode_never_uses_internal_sources():
    assert all(r["status"] == "approved" for r in sources.allowed("rules", "release"))


def test_datasets_md_is_current():
    # Regenerate with `uv run t3ap datasets-md` after editing data/sources.toml.
    assert datasets.is_current()
