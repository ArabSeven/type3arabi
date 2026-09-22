"""Dataset registry access (data/sources.toml). Enforces AGENTS.md R14 in the pipeline."""
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
SOURCES = REPO / "data" / "sources.toml"
VALID_STATUS = {"approved", "eval-only", "owner-decision", "blocked"}
VALID_ROLES = {"lexicon", "lm", "charlm", "diac", "rules", "tuning", "eval", "reference"}


def load():
    data = tomllib.loads(SOURCES.read_text(encoding="utf-8"))
    rows = data["source"]
    ids = set()
    for r in rows:
        assert r["id"] not in ids, f"duplicate id {r['id']}"
        ids.add(r["id"])
        assert r["status"] in VALID_STATUS, (r["id"], r["status"])
        assert set(r["roles"]) <= VALID_ROLES, (r["id"], r["roles"])
        if r["status"] == "blocked":
            assert not r["roles"], f"{r['id']}: blocked sources have no roles"
    return rows


def allowed(role: str):
    """Sources that may feed `role` into SHIPPED artifacts."""
    return [r for r in load() if r["status"] == "approved" and role in r["roles"]]


def fetchable():
    """Sources the fetch stage may download (approved or eval-only; owner-decision counts as eval-only)."""
    return [r for r in load() if r["status"] in {"approved", "eval-only", "owner-decision"} and r["url"] != "internal"]
