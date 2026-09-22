"""t3ap — Type3arabi pipeline CLI. Stages per docs/04 §2. Each stage writes under pipeline_data/ (gitignored)."""
import argparse
import sys

from . import sources

STAGES = {
    "fetch": "Download approved/eval-only sources into pipeline_data/raw/<id>/ and write manifest.lock.json (M2).",
    "normalize": "Arabic normalization + sentence filtering → norm/<group>/*.txt.zst (docs/04 §3) (M2).",
    "count": "Unigrams per dialect group, bigrams, char 5-grams, marked-token counts (docs/04 §5) (M2).",
    "lexicon": "Select ≤600k forms, per-dialect lp, flags TANWEEN_FATH/NO_COMPLETE/SACRED, bigrams, char LM (M2).",
    "diac": "Vocalized variants per base word from marked tokens (docs/04 §5) (M2).",
    "align": "EM chunk alignment on approved parallel pairs with the seed prior (docs/04 §6) (M6).",
    "tune": "Coordinate ascent of EngineParams on dev splits (docs/04 §7) (M6).",
}


def cmd_sources(_args):
    rows = sources.load()
    width = max(len(r["id"]) for r in rows)
    for r in rows:
        print(f"{r['id']:<{width}}  {r['status']:<15} roles={','.join(r['roles']) or '-'}")
    print(f"\n{len(rows)} sources; shipped-data eligible: {sorted({r['id'] for r in rows if r['status'] == 'approved'})}")
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(prog="t3ap", description=__doc__)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("sources", help="validate and list data/sources.toml")
    for name, help_ in STAGES.items():
        sub.add_parser(name, help=help_)
    sub.add_parser("all", help="run stages fetch..tune")
    args = p.parse_args(argv)
    if args.cmd == "sources":
        return cmd_sources(args)
    print(f"stage '{args.cmd}' is not implemented yet: {STAGES.get(args.cmd, 'runs every stage')}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
