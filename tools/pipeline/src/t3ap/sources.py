"""Dataset registry access (data/sources.toml). Enforces AGENTS.md R14 in the pipeline."""
import hashlib
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
SOURCES = REPO / "data" / "sources.toml"
VALID_STATUS = {"approved", "provisional", "internal", "eval-only", "owner-decision", "blocked"}
VALID_ROLES = {"lexicon", "lm", "charlm", "diac", "rules", "tuning", "eval", "reference"}
VALID_FAMILY = {"permissive", "nc"}


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
        if r["status"] == "approved":
            # ADR-0010: every approved source declares whether it is non-commercial.
            assert r.get("license_family") in VALID_FAMILY, f"{r['id']}: license_family missing"
        if r["status"] == "provisional":
            # ADR-0011: used in releases while the rights holder's reply is pending; the model is
            # licensed conservatively (license family "unknown" => CC BY-NC-SA) and the source must
            # say when clearance was requested.
            assert r.get("license_family") == "unknown", f"{r['id']}: provisional => license_family unknown"
            assert "requested" in r.get("notes", ""), f"{r['id']}: provisional => notes say when clearance was requested"
    return rows


def shipped_statuses(mode: str, exclude_provisional: bool = False) -> set:
    """Statuses whose data may shape SHIPPED artifacts in `mode` (AGENTS.md R14, ADR-0011)."""
    valid = {"approved", "provisional"}
    if mode == "internal":
        valid.add("internal")
    if exclude_provisional:
        valid.discard("provisional")
    return valid


def allowed(role: str, mode: str = "internal", exclude_nc: bool = False, exclude_provisional: bool = False):
    """Sources that may feed `role` into SHIPPED artifacts. `exclude_nc` drops non-commercial ones (ADR-0010);
    `exclude_provisional` drops sources still awaiting permission (ADR-0011: the exit path if one is declined)."""
    valid = shipped_statuses(mode, exclude_provisional)
    return [
        r
        for r in load()
        if r["status"] in valid and role in r["roles"] and not (exclude_nc and r.get("license_family") == "nc")
    ]


def fetchable(mode: str = "internal"):
    """Sources the fetch stage may download."""
    valid = shipped_statuses(mode) | {"eval-only", "owner-decision"}
    return [r for r in load() if r["status"] in valid and r["url"] != "internal"]


def split_row(key: str) -> str:
    """Deterministic 80/10/10 train/dev/test split by stable row hash."""
    h = int.from_bytes(hashlib.sha256(key.strip().encode("utf-8")).digest()[:4], "little") % 100
    if h < 80:
        return "train"
    elif h < 90:
        return "dev"
    else:
        return "test"
