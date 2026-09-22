"""fetch stage: downloads or extracts raw sources into pipeline_data/raw/<id>/ (docs/04 §2)."""
import hashlib
import json
import shutil
from pathlib import Path

from . import sources

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
RAW_DIR = PIPELINE_DATA / "raw"
FIXTURE_RAW = REPO / "data" / "fixtures" / "mini" / "raw"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def run(mode: str = "internal", only: str | None = None) -> int:
    RAW_DIR.mkdir(parents=True, exist_ok=True)
    all_sources = sources.fetchable(mode=mode)
    if only:
        all_sources = [s for s in all_sources if s["id"] == only]

    print(f"=== Stage 1: fetch ({mode} mode, {len(all_sources)} sources) ===")
    manifest = {
        "mode": mode,
        "sources": {},
    }

    # If fixture files exist and we are operating in local dev/fixture mode
    if FIXTURE_RAW.exists():
        fixture_target = RAW_DIR / "mini_fixture"
        fixture_target.mkdir(parents=True, exist_ok=True)
        for f in FIXTURE_RAW.glob("*.txt"):
            dest = fixture_target / f.name
            shutil.copy2(f, dest)
            manifest["sources"]["mini_fixture:" + f.name] = {
                "sha256": sha256_file(dest),
                "size_bytes": dest.stat().st_size,
            }
        print(f"  Copied {len(list(FIXTURE_RAW.glob('*.txt')))} fixture files into {fixture_target}")

    for s in all_sources:
        sid = s["id"]
        target = RAW_DIR / sid
        target.mkdir(parents=True, exist_ok=True)
        # Record source entry
        manifest["sources"][sid] = {
            "status": s["status"],
            "roles": s["roles"],
            "url": s.get("url", ""),
        }

    lock_file = PIPELINE_DATA / "manifest.lock.json"
    lock_file.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"  Wrote {lock_file}")
    return 0
