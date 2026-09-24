# Type3arabi — اكتب عربي

Type Arabic in any Windows app by writing the way you chat: `mar7aba ya 7abibi` → **مرحبا يا حبيبي**.

Type3arabi is a native Windows keyboard (**Arabic · Type3arabi**) in the tradition of Microsoft Maren and
Yamli. It offers:
- a right-to-left candidate list with the most likely word pre-selected;
- awareness of dialects (Levantine, Egyptian, Gulf, Iraqi, Maghrebi, MSA);
- automatic critical diacritics (**اللّه**, **شكراً**);
- a built-in tashkeel editor.

It works 100% offline, sends nothing anywhere, has no accounts and no telemetry, and is free.

**Status:** in development (pre-release). See [`STATUS.md`](STATUS.md).

## Download
Releases are published on [GitHub Releases](https://github.com/ArabSeven/type3arabi/releases) only, and later
on the Microsoft Store. The latest installer is always at
`https://github.com/ArabSeven/type3arabi/releases/latest/download/Type3arabi-x64.msi`.

## License
- **Code:** [Apache-2.0](LICENSE).
- **Language model** (`type3arabi.dat`, shipped inside releases):
  [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/). It is learned from datasets, some of
  which allow non-commercial use only. See [`DATASETS.md`](DATASETS.md) for full provenance and
  [`NOTICE.md`](NOTICE.md) for attributions.
- No dataset is stored in this repository. `tools/pipeline` downloads them from their publishers
  ([`docs/04-data-pipeline.md`](docs/04-data-pipeline.md)).

## For contributors

| Start here | |
|---|---|
| Rules for anyone (or any agent) working here | [`AGENTS.md`](AGENTS.md) |
| Where the project stands | [`STATUS.md`](STATUS.md) |
| Trying a build on your PC | [`TESTING.md`](TESTING.md) |
| Product brief | [`docs/00-vision.md`](docs/00-vision.md) |
| Architecture | [`docs/01-architecture.md`](docs/01-architecture.md) |
| The prediction engine | [`docs/03-engine-algorithm.md`](docs/03-engine-algorithm.md) |
| UX | [`docs/05-ux-spec.md`](docs/05-ux-spec.md) |
| Roadmap | [`docs/09-roadmap.md`](docs/09-roadmap.md) |
| Licensing and distribution decision | [`docs/adr/0010-open-source-distribution-and-data-licensing.md`](docs/adr/0010-open-source-distribution-and-data-licensing.md) |

Try the engine (seed rules only, or with a built data file):
```bash
cargo run -p t3a-cli -- repl --dialect LEV
cargo run -p t3a-cli -- explain 3allam --dialect MSA
```
