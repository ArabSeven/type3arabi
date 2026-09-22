# Type3arabi — اكتب عربي

Type Arabic on any Windows app by writing the way you chat: `mar7aba ya 7abibi` → **مرحبا يا حبيبي**.

Type3arabi is a native Windows input method (**AR – Type3arabi**) in the tradition of Microsoft Maren and
Yamli: a smart RTL candidate popup with the most likely word pre-selected, dialect awareness (Levantine,
Egyptian, Gulf, Iraqi, Maghrebi, MSA), automatic critical diacritics (**اللّه**, **شكراً**), and a built-in
tashkeel editor. It is offline, private and costs nothing while idle.

**Status:** pre-bootstrap. The architecture and specs are complete; implementation follows `docs/09-roadmap.md`.

| Start here | |
|---|---|
| Rules for anyone (or any agent) working here | [`AGENTS.md`](AGENTS.md) |
| Where the project stands | [`STATUS.md`](STATUS.md) |
| Product brief | [`docs/00-vision.md`](docs/00-vision.md) |
| Architecture | [`docs/01-architecture.md`](docs/01-architecture.md) |
| The prediction engine | [`docs/03-engine-algorithm.md`](docs/03-engine-algorithm.md) |
| UX | [`docs/05-ux-spec.md`](docs/05-ux-spec.md) |
| Roadmap | [`docs/09-roadmap.md`](docs/09-roadmap.md) |

Try the engine today (seed rules only, no dictionary yet):
```bash
cargo run -p t3a-cli -- repl --dialect LEV
cargo run -p t3a-cli -- explain 3allam --dialect MSA
```
