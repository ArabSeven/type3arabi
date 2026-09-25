# ADR-0011: Provisional data sources — release with openly published, unlicensed data while permission is pending

- **Status:** Accepted (2026-09-25) — Owner decision (STATUS.md → Owner decisions, O14)
- **Deciders:** Owner, implementing agent
- **Amends:** AGENTS.md R14 (adds the `provisional` status), ADR-0010 §2 (data license policy), `docs/09` v1.0 note

## Context
The two most useful Levantine parallel corpora — Talafha et al. (≈25k Jordanian Arabizi ↔ Arabic word pairs,
GitHub) and Khanafer (433 pairs, Hugging Face) — were published openly for others to use (the Talafha README
says the corpus is "publicly released"; both are public repositories), but neither carries a license text.
Permission was requested on 2026-09-24/25. The authors may not answer for a long time. Without them the
release model is measurably weaker for Levantine (held-out LEV top-1 61.4% vs 65.1%; `oktob`, `ahlan`
regress). Only statistics learned from these pairs (spelling-rule probabilities and tuning) reach the model;
no pair, word list or text from them is shipped (their `lexicon` role is empty).

The Owner reviewed the risk (release audit, 2026-09-25: default copyright, possible takedown, possible effect on a
third-party signing certificate) and decided to use both datasets in releases until the rights holders answer,
and to retrain without a dataset if its authors decline.

## Decision
1. New registry status **`provisional`** in `data/sources.toml`: license unstated, published for public use,
   permission requested, Owner accepted release use. A provisional source must have
   `license_family = "unknown"` (so the model stays CC BY-NC-SA 4.0), an `asked` field and a `notes` line
   saying when permission was requested. The pipeline enforces this.
2. `--mode release` builds may use `approved` **and** `provisional` sources; never `internal`. The data
   file's META lists them under `provisional_sources`; `DATASETS.md` lists them in their own section with the
   citation and the pending request.
3. **Exit path.** `t3ap --mode release --exclude-provisional pairs` (then `train-rules`, `build-data`) rebuilds
   without every provisional source. On a decline: set the source to `blocked`, rebuild, publish a new release
   with the rebuilt model, remove the affected release assets from GitHub Releases and update the Store
   submission. On a grant: record it in `notes` and set the source to `approved` with its license family.
4. Attribution is given as for approved sources (DATASETS.md, NOTICE.md, citations).

## Consequences
- The first public release ships the stronger Levantine model.
- Until permission arrives, the release model contains statistics derived from data without an explicit
  license. This is disclosed publicly (DATASETS.md, META). A decline means a retrain and a new release; copies
  already installed keep the old model (there is no updater).
- Eligibility for SignPath Foundation signing, whose conditions require an open-source license for all
  components, is affected independently of this decision (the model is CC BY-NC-SA in any case); the
  provisional sources should be mentioned in any SignPath inquiry.

## Alternatives considered
- Keep both `internal` and release the weaker model (the previous default): safer, measurably worse for LEV.
- Drop only Khanafer (433 pairs): negligible accuracy effect either way; the Owner chose to treat both alike.
