"""t3ap — Type3arabi pipeline CLI. Stages per docs/04 §2. Each stage writes under pipeline_data/ (gitignored)."""
import argparse
import sys

from . import count, datasets, diac, fetch, lexicon, normalize, pairs, sources

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
    print(
        f"\n{len(rows)} sources; shipped-data eligible: {sorted({r['id'] for r in rows if r['status'] == 'approved'})}"
    )
    return 0


def cmd_all(args):
    code = fetch.run(mode=args.mode)
    if code != 0:
        return code
    code = normalize.run()
    if code != 0:
        return code
    code = count.run()
    if code != 0:
        return code
    code = lexicon.run()
    if code != 0:
        return code
    code = diac.run()
    if code != 0:
        return code
    code = pairs.run(mode=args.mode, exclude_nc=args.exclude_nc)
    if code != 0:
        return code
    print("=== Python stages done. Next (repo root):")
    print("  cargo run --release -p t3a-cli -- train-rules")
    print("  cargo run --release -p t3a-cli -- build-data --in pipeline_data/out --seed data/seed --out target/type3arabi.dat")
    print("  cargo run --release -p t3a-cli -- tune pipeline_data/eval/*.dev.tsv --data target/type3arabi.dat")
    print("  (then build-data again to embed params.toml)")
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(prog="t3ap", description=__doc__)
    p.add_argument(
        "--mode",
        choices=["internal", "release"],
        default="internal",
        help="build mode: internal (approved + internal) or release (approved only)",
    )
    p.add_argument(
        "--exclude-nc",
        action="store_true",
        help="leave out non-commercial (license_family = nc) sources: the ADR-0010 exit path",
    )
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("sources", help="validate and list data/sources.toml")
    sub.add_parser("datasets-md", help="regenerate DATASETS.md from data/sources.toml")

    p_fetch = sub.add_parser("fetch", help=STAGES["fetch"])
    p_fetch.add_argument("--only", help="fetch only this source ID")

    sub.add_parser("normalize", help=STAGES["normalize"])
    sub.add_parser("count", help=STAGES["count"])
    sub.add_parser("lexicon", help=STAGES["lexicon"])
    sub.add_parser("diac", help=STAGES["diac"])
    sub.add_parser("pairs", help="word pairs for rule training (align/train.tsv) + held-out eval sets (eval/*.tsv)")
    sub.add_parser("align", help=STAGES["align"])
    sub.add_parser("tune", help=STAGES["tune"])
    sub.add_parser("all", help="run stages fetch..diac (and align/tune in M6)")

    args = p.parse_args(argv)
    if args.cmd == "sources":
        return cmd_sources(args)
    elif args.cmd == "fetch":
        return fetch.run(mode=args.mode, only=args.only)
    elif args.cmd == "normalize":
        return normalize.run()
    elif args.cmd == "count":
        return count.run()
    elif args.cmd == "lexicon":
        return lexicon.run()
    elif args.cmd == "diac":
        return diac.run()
    elif args.cmd == "pairs":
        return pairs.run(mode=args.mode, exclude_nc=args.exclude_nc)
    elif args.cmd == "datasets-md":
        return datasets.run()
    elif args.cmd == "all":
        return cmd_all(args)

    print(
        f"stage '{args.cmd}' is not implemented yet: {STAGES.get(args.cmd, 'runs every stage')}",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
