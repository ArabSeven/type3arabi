# t3ap — Type3arabi data pipeline

Spec: `docs/04-data-pipeline.md`. Licensing gate: `data/sources.toml` (AGENTS.md R14).

```bash
cd tools/pipeline
uv sync --extra dev
uv run pytest            # normalization vectors (shared with Rust) + registry validation
uv run t3ap sources      # list datasets and their status
uv run t3ap all          # M2+: fetch → normalize → count → lexicon → diac → align → tune
```
Outputs go to `<repo>/pipeline_data/` (gitignored); `cargo run -p t3a-cli -- build-data` compiles them into `type3arabi.dat`.
