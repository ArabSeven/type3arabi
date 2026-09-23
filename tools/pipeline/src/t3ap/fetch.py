"""fetch stage (docs/04 §2 stage 1): download sources into pipeline_data/raw/<id>/.

- FineWeb-2 (monolingual Arabic): streams parquet row groups over HTTP (no full-shard download) until
  the per-group token cap is reached; writes raw/fineweb2/<group>.<config>.txt (one text line per line).
- Parallel Arabizi corpora: downloads and converts to raw/<id>/pairs.tsv with columns
  `latin \t arabic \t dialect \t split` (split = deterministic 80/10/10 by row hash, sources.split_row).

Only sources whose status allows fetching in the chosen mode are touched (AGENTS.md R14).
"""
import hashlib
import io
import json
import time
import urllib.request
from pathlib import Path

from . import sources

REPO = Path(__file__).resolve().parents[4]
PIPELINE_DATA = REPO / "pipeline_data"
RAW_DIR = PIPELINE_DATA / "raw"

# Dialect group -> FineWeb-2 configs (docs/04 §4; only configs that exist on the hub).
FINEWEB_CONFIGS = {
    "msa": ["arb_Arab"],
    "lev": ["apc_Arab"],
    "egy": ["arz_Arab"],
    "glf": ["ars_Arab"],
    "irq": ["acm_Arab"],
    "mag": ["ary_Arab", "aeb_Arab", "arq_Arab"],
}
# Whitespace-token caps per group. Dialect subsets are small; take everything up to the cap.
TOKEN_CAPS = {"msa": 60_000_000, "lev": 60_000_000, "egy": 50_000_000, "glf": 30_000_000,
              "irq": 30_000_000, "mag": 40_000_000}

HF = "https://huggingface.co"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def http_get(url: str, retries: int = 4) -> bytes:
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "t3ap/0.1"})
            with urllib.request.urlopen(req, timeout=120) as r:
                return r.read()
        except Exception:  # noqa: BLE001 - network hiccups: retry with backoff
            if attempt == retries - 1:
                raise
            time.sleep(2 ** attempt)
    raise RuntimeError("unreachable")


# --------------------------------------------------------------------------------------------
# FineWeb-2

def fetch_fineweb(manifest: dict) -> None:
    import pyarrow.parquet as pq
    from huggingface_hub import HfFileSystem

    fs = HfFileSystem()
    out_dir = RAW_DIR / "fineweb2"
    out_dir.mkdir(parents=True, exist_ok=True)
    per_group = {}
    for group, configs in FINEWEB_CONFIGS.items():
        cap = TOKEN_CAPS[group]
        tokens = 0
        for cfg in configs:
            if tokens >= cap:
                break
            out = out_dir / f"{group}.{cfg}.txt"
            done_marker = out.with_suffix(".done")
            if done_marker.exists():
                tokens += int(done_marker.read_text())
                print(f"  [fineweb2 {group}/{cfg}] cached ({int(done_marker.read_text()):,} tokens)")
                continue
            files = sorted(fs.glob(f"datasets/HuggingFaceFW/fineweb-2/data/{cfg}/train/*.parquet"))
            cfg_tokens = 0
            with out.open("w", encoding="utf-8", newline="\n") as w:
                for path in files:
                    if tokens + cfg_tokens >= cap:
                        break
                    with fs.open(path, "rb", block_size=16 << 20) as fh:
                        pf = pq.ParquetFile(fh)
                        for rg in range(pf.num_row_groups):
                            if tokens + cfg_tokens >= cap:
                                break
                            texts = pf.read_row_group(rg, columns=["text"]).column("text").to_pylist()
                            for t in texts:
                                for line in t.splitlines():
                                    line = line.strip()
                                    if line:
                                        w.write(line + "\n")
                                        cfg_tokens += line.count(" ") + 1
                            print(f"  [fineweb2 {group}/{cfg}] row group {rg + 1}/{pf.num_row_groups}: "
                                  f"{tokens + cfg_tokens:,} tokens", flush=True)
            done_marker.write_text(str(cfg_tokens))
            tokens += cfg_tokens
        per_group[group] = tokens
    manifest["sources"]["fineweb2"] = {
        "revision": "main",
        "token_caps": TOKEN_CAPS,
        "tokens_per_group": per_group,
        "files": {p.name: {"sha256": sha256_file(p), "bytes": p.stat().st_size}
                  for p in sorted(out_dir.glob("*.txt"))},
    }


# --------------------------------------------------------------------------------------------
# Parallel corpora -> pairs.tsv

def write_pairs(sid: str, rows, manifest: dict) -> None:
    out_dir = RAW_DIR / sid
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / "pairs.tsv"
    n = {"train": 0, "dev": 0, "test": 0}
    with out.open("w", encoding="utf-8", newline="\n") as w:
        for latin, arabic, dialect in rows:
            latin = " ".join(str(latin or "").split())
            arabic = " ".join(str(arabic or "").split())
            if not latin or not arabic or latin.lower() == "nan" or arabic.lower() == "nan":
                continue
            split = sources.split_row(latin + "\t" + arabic)
            n[split] += 1
            w.write(f"{latin}\t{arabic}\t{dialect}\t{split}\n")
    manifest["sources"][sid] = {"sha256": sha256_file(out), "pairs": n}
    print(f"  [{sid}] {sum(n.values()):,} pairs (train {n['train']}, dev {n['dev']}, test {n['test']})")


def hf_parquet_rows(dataset: str):
    import pyarrow.parquet as pq

    meta = json.loads(http_get(f"https://datasets-server.huggingface.co/parquet?dataset={dataset}"))
    for f in meta.get("parquet_files", []):
        table = pq.read_table(io.BytesIO(http_get(f["url"])))
        yield from table.to_pylist()


def fetch_parallel(src: dict, manifest: dict) -> bool:
    sid = src["id"]
    if sid == "akhanafer-levantine":
        rows = ((r["arabizi"], r["arabic"], "LEV") for r in hf_parquet_rows("akhanafer/arabic-to-arabizi"))
    elif sid == "elkababi-darija":
        rows = ((r["darija_Latn"], r["darija_Arab_new"], "MAG")
                for r in hf_parquet_rows("elkababi2/Darija-Text-Ar-Arabizi"))
    elif sid == "arabizikit-corpus":
        dmap = {"egyptian": "EGY", "levantine": "LEV", "gulf": "GLF", "maghrebi": "MAG", "msa": "MSA"}

        def kit_rows():
            for r in hf_parquet_rows("rabeeeehh/arabizi-kit-corpus"):
                for e in r.get("entries") or []:
                    yield e.get("arabizi"), e.get("reference"), dmap.get(str(e.get("dialect")).lower(), "MSA")
        rows = kit_rows()
    elif sid == "talafha-jordanian":
        import openpyxl

        data = http_get("https://raw.githubusercontent.com/bashartalafha/Arabizi-Transliteration/master/"
                        "Arabizi-Arabic%20Parallel%20corpora.xlsx")
        wb = openpyxl.load_workbook(io.BytesIO(data), read_only=True)

        def xlsx_rows():
            for ws in wb.worksheets:
                for i, row in enumerate(ws.iter_rows(values_only=True)):
                    if i == 0 or not row or len(row) < 2:
                        continue
                    a, b = row[0], row[1]
                    # Column order differs between sheets: detect which side is Latin.
                    if isinstance(a, str) and any("؀" <= c <= "ۿ" for c in a):
                        a, b = b, a
                    yield a, b, "LEV"
        rows = xlsx_rows()
    else:
        return False
    write_pairs(sid, rows, manifest)
    return True


# --------------------------------------------------------------------------------------------

def run(mode: str = "internal", only: str | None = None) -> int:
    RAW_DIR.mkdir(parents=True, exist_ok=True)
    lock = PIPELINE_DATA / "manifest.lock.json"
    manifest = {"mode": mode, "sources": {}}
    if lock.exists():
        old = json.loads(lock.read_text(encoding="utf-8"))
        if old.get("mode") == mode:
            manifest["sources"] = {k: v for k, v in old.get("sources", {}).items() if ":" not in k}
    todo = sources.fetchable(mode=mode)
    if only:
        todo = [s for s in todo if s["id"] == only]
    print(f"=== Stage 1: fetch ({mode} mode, {len(todo)} candidate sources) ===")
    for s in todo:
        sid = s["id"]
        try:
            if sid == "fineweb2":
                fetch_fineweb(manifest)
            elif s["kind"] == "parallel" and fetch_parallel(s, manifest):
                pass
            else:
                print(f"  [{sid}] no automated fetcher (kind={s['kind']}); skipped")
        except Exception as e:  # noqa: BLE001 - one unavailable source must not stop the build
            print(f"  [{sid}] FAILED: {e}")
            manifest["sources"][sid] = {"error": str(e)[:300]}
    manifest["fetched_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    lock.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"  Wrote {lock}")
    return 0
