# 04 — Data pipeline (`tools/pipeline`, `t3a-cli build-data`)

Goal: turn open data into one file, `type3arabi.dat`, reproducibly, with every shipped byte traceable to
an approved source (R14). The pipeline is offline tooling; nothing here runs on users' machines.

## 1. Stack
- Python 3.12 managed by **uv** (`tools/pipeline/pyproject.toml`). Libraries: `datasets`, `huggingface_hub`,
  `pyarrow`, `regex`, `numpy`, `tqdm`. (Tooling only — not shipped; R11 applies to shipped binaries.)
- Heavy counting uses streaming (`datasets` streaming mode) and writes sharded TSVs; merge with `sort`/`numpy`.
- Final compilation is Rust: `t3a-cli build-data` (so the binary format has exactly one writer and one reader,
  both in `t3a-data`).
- Working directory `pipeline_data/` (gitignored). Every run writes `pipeline_data/manifest.lock.json` with
  source ids, revisions/commit hashes, file SHA-256s and dates.

## 2. Stages and commands

| # | Command | Input | Output |
|---|---|---|---|
| 1 | `uv run t3ap fetch [--only id]` | `data/sources.toml` (status `approved` or `eval-only`) | `pipeline_data/raw/<id>/…`, manifest |
| 2 | `uv run t3ap normalize` | raw Arabic text | `norm/<group>/*.txt.zst` (one sentence per line; base form + marked form) |
| 3 | `uv run t3ap count` | norm | `counts/<group>.uni.tsv`, `counts/all.bi.tsv`, `counts/chr5.tsv`, `counts/marked.tsv` |
| 4 | `uv run t3ap lexicon` | counts, `data/seed/*` | `out/lexicon.tsv` (word, lp×6, flags), `out/bigrams.tsv`, `out/charlm.tsv` |
| 5 | `uv run t3ap diac` | `counts/marked.tsv` | `out/diac.tsv` (base word → vocalized variants + lp) |
| 6 | `uv run t3ap align` | approved parallel pairs + `data/seed/mappings.tsv` | `out/rules.tsv` (trained transliteration rules) |
| 7 | `uv run t3ap tune` | dev splits, built data | `out/params.toml` (λ's, beams, penalties) |
| 8 | `cargo run -p t3a-cli -- build-data --in pipeline_data/out --seed data/seed --out target/type3arabi.dat` | out/*, seed | `type3arabi.dat` |
| 9 | `cargo run -p t3a-cli -- eval --data target/type3arabi.dat --sets data/eval/*.tsv,pipeline_data/test/*.tsv --report reports/` | | Markdown + JSON report |

`uv run t3ap all` runs 1–7; CI runs 8–9 on a small fixture (`data/fixtures/mini/`) to keep the format honest.

## 3. Arabic normalization (must match `t3a-engine::arabic::normalize_word` exactly — shared test vectors in `data/eval/normalization.tsv`)
1. Unicode NFC first (input hygiene), then:
2. Remove tatweel U+0640, ZWJ/ZWNJ, bidi controls, Quranic annotation marks U+06D6–U+06ED, U+0615–U+061A.
3. Map: alef wasla ٱ U+0671 → ا; Farsi yeh ی U+06CC → ي; keheh ک U+06A9 → ك; Farsi ye final forms → ي; heh goal ہ → ه;
   alef maksura stays ى; ta marbuta stays ة; hamza forms stay as is.
4. **Base form** = strip marks U+064B–U+0652, U+0670. **Marked form** = keep marks, reorder per `docs/03 §10.1`.
5. Token = maximal run of Arabic letters (+ marks). Everything else is a separator. Drop tokens > 20 letters.
6. Sentences with < 50% Arabic-letter characters are skipped (filters code/boilerplate).

## 4. Dialect groups and corpus mapping
| Group | FineWeb-2 subsets (take what exists; the script lists configs matching `*_Arab`) | Other approved |
|---|---|---|
| MSA | `arb_Arab` (cap: 400 M tokens, random sample of documents) | — |
| LEV | `apc_Arab`, `ajp_Arab` (if present) | golden LEV |
| EGY | `arz_Arab`, `apd_Arab` (Sudanese, folded in) | golden EGY |
| GLF | `afb_Arab`, `ars_Arab`, `acq_Arab`, `acw_Arab`, `ayh_Arab` (those present) | golden GLF |
| IRQ | `acm_Arab` | golden IRQ |
| MAG | `ary_Arab`, `arq_Arab`, `aeb_Arab`, `ayl_Arab` | golden MAG |
FineWeb-2's language ID confuses dialects with MSA; that is acceptable because every word also gets a
mixture score. Record the per-group token counts in the manifest.

## 5. Lexicon, LM and diacritics (stages 3–5)
- **Unigrams** per group with add-0.5 smoothing over the union vocabulary → `lp[d]`. Unseen in group ⇒ 255.
- **Selection**: keep words with total count ≥ 5 and `max_d lp[d] ≥ −18`; rank by `max_d (lp[d] + ln prior_d)`
  with prior `MSA .35, LEV .2, EGY .2, GLF .12, IRQ .05, MAG .08`; cap **600 000** forms.
- **Flags**: `TANWEEN_FATH` (`docs/03 §10.3` rule), `NO_COMPLETE` (in `data/seed/no_complete.tsv`: offensive or
  sensitive words never offered as *completions*; still valid exact matches), `SACRED` (in the SACRED set).
- **Bigrams**: counts over all groups (MSA downweighted ×0.5), interpolated Kneser-Ney, keep `count ≥ 3`, top 64
  successors per word, both words in the lexicon. Target ≤ 3 M pairs.
- **Char 5-gram** over base forms with boundaries; stupid backoff; prune to 600 k entries by count.
- **Diacritics (DIAC)**: from `counts/marked.tsv` (tokens that carry ≥ 1 mark in approved sources) collect, per
  base word, vocalized variants with count ≥ 3; keep ≤ 8 by count; also the tanween ratio for stage 4.
  Sources: FineWeb-2 marked tokens (always), plus diacritized corpora only if approved (`tashkeela` is
  `owner-decision`).

## 6. Transliteration rule training (stage 6)
1. **Pairs**: word pairs from approved parallel data. Sentence pairs are tokenized on whitespace; equal
   token counts ⇒ 1:1; otherwise a DP aligner using the seed model with 1:2 / 2:1 merges (article split
   `el bet ↔ البيت`, `3al ↔ على ال`). Discard sentence pairs whose best alignment cost is in the worst 10%.
2. **Chunk alignment** (m2m-style EM): Latin chunks 1–4 symbols ↔ Arabic chunks 0–3 letters. Candidate
   chunk pairs = all seed rules + any (Latin chunk ≤ 2, single Arabic letter) pair with a small prior (lets
   EM discover unseen mappings). Forward-backward over the segmentation lattice; M-step with Dirichlet
   prior `κ · P_seed(α|ℓ)` (`κ = 5`), per position class and per dialect (dialect from the source tag;
   backoff to `*` when a dialect has < 200 occurrences of the chunk). 10 iterations or Δ log-lik < 0.1%.
3. **Pruning**: drop rules with `P < 0.005` unless in the seed file.
4. **Self-training (optional, M6)**: run the engine on approved monolingual Arabizi; keep outputs whose
   top-1 beats top-2 by ≥ 3.0 score; add as pairs with weight 0.3; one more EM round. Never on eval data.
5. Output `out/rules.tsv`: same columns as `data/seed/mappings.tsv` plus `count` and per-dialect lp.

## 7. Tuning (stage 7)
Coordinate ascent over `λ_tm, λ_lm, λ_ctx, λ_chr, oov_penalty, gamma_completion, p_gem` on the **dev** split
(approved parallel dev + golden dev), objective = `top1 + 0.25 · hit@5 − 0.5 · max(0, p99_ms − 3)`.
`λ_usr` is not tuned offline (no user data); it is set by the simulated-user test (`docs/08 §4`).
Emit `out/params.toml`; `build-data` embeds it as the `PARM` section.

## 8. Golden set protocol (the data we own — highest value, create in M2 and grow forever)
- Collect Arabic sentences (news headlines, chat-like prompts, everyday phrases) per dialect: 300 per group.
- Native speakers (≥ 3 per dialect; the Owner recruits) are shown the **Arabic** sentence and asked to
  type it "the way you'd write it in WhatsApp in Latin letters", with no examples (avoid priming).
  Collect via a simple form; contributors agree to release their typing as **CC0**.
- Every sentence is typed by ≥ 2 people ⇒ natural spelling variation.
- Store as `data/eval/golden/<group>.tsv` (`arabizi \t arabic \t dialect \t contributor_id`). Split 50/50
  **dev** (tuning allowed) / **test** (never used for anything but reporting). Test files are named `*.test.tsv`.

## 9. Synthetic augmentation (optional, Owner approval + budget)
Offline LLM generation of Arabizi spellings for the top 30 000 words per dialect group (prompted per
dialect with 20 golden examples, temperature 0.8, 5 spellings each). Human spot-check 200 random rows per
group (accept ≥ 90% plausible or discard the batch). Tag `synthetic`, weight 0.3 in EM, never in eval.

## 10. Quality gates for a data build (CI on the full build machine)
- Header/section validation passes; file ≤ **60 MB**; lexicon 400–650 k forms; bigrams ≤ 3.5 M.
- Normalization vectors pass in both Python and Rust.
- Eval report meets the current milestone's gates (`docs/09`), and no golden-test regression > 1 point vs
  the previous released data build.
- `data/sources.toml` ids in the manifest are all `approved` for the roles used.
