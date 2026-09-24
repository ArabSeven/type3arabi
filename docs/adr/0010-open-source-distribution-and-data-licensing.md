# ADR-0010: Free and open source: Apache-2.0 code, CC BY-NC-SA 4.0 model, GitHub + Microsoft Store

- **Status:** Accepted (2026-09-24) — Owner decision (STATUS.md → Owner decisions, O12)
- **Deciders:** Owner, implementing agent
- **Supersedes:** the "all rights reserved" placeholder license; AGENTS.md R14 "non-commercial ⇒ forbidden"
  for *data*; the "internal-only" framing of the 2026-09-22 R14 amendment as the only way to use
  unlicensed data (it remains, for sources still awaiting clearance)
- **Amends:** ADR-0005 (update check reads GitHub Releases), ADR-0008 (release channels, asset names)

## Context
The Owner wants Type3arabi to be a public contribution that is free forever and costs no more to run than a
domain name. The best Arabizi ↔ Arabic data available is mostly licensed for non-commercial use (DODa,
NileChat: CC BY-NC 4.0; TArC: CC BY-NC-SA 4.0), which the original rules ruled out. A free product with no
revenue can honour those licenses, provided code and data are licensed separately and the project never
earns money from the data.

## Decision
**1. Two licenses, two artifacts.**

| Artifact | License | Why |
|---|---|---|
| Source code, binaries, installer scripts, docs, hand-written seed data (`data/seed/`, `data/eval/`) | **Apache-2.0** | Permissive and attribution-preserving (NOTICE), with a patent grant. The Owner's authorship stays credited. |
| The language model: `type3arabi.dat` and any file derived from third-party datasets | **CC BY-NC-SA 4.0** | The narrowest license that satisfies every data source we use: CC BY, CC BY-NC (both may be adapted under BY-NC-SA), CC BY-NC-SA (requires it), ODC-By, MIT/Apache/CC0 (attribution only). |

The model's provenance is published in `DATASETS.md` (generated from `data/sources.toml`) and embedded in the
data file's `META` section.

**2. Data license policy (replaces the data half of R11/R14).**
- *Allowed* for the shipped model: public domain / CC0, MIT, BSD, Apache-2.0, ODC-By, ODC-PDDL, CC BY, CC BY-NC,
  CC BY-NC-SA (4.0 or earlier 3.0/2.0 versions whose terms are compatible), plus written permission from the
  rights holder compatible with CC BY-NC-SA 4.0.
- *Not allowed*: CC BY-SA and ODbL (they require share-alike under *their* license, which cannot carry NC;
  CC BY-NC-SA ← BY-SA is one-way incompatible), any NoDerivatives license, research-only / custom
  restrictive terms, paid licenses (LDC). Such sources may still be `eval-only`: eval reports ship nothing.
- *Unstated license*: `internal` until the rights holder answers (`--mode internal` builds only); a public
  release is built with `--mode release`.

**3. Distribution: no servers of our own.**
- Source code: public GitHub repository, `ArabSeven/type3arabi`. `.gitignore` excludes every downloaded dataset,
  pipeline output, build output and third-party dependency. Nothing derived from third-party data is
  committed. The model reaches users only inside releases.
- Binaries: **GitHub Releases** (the only download the project hosts), plus the **Microsoft Store** (a signed copy).
  Each release also carries version-free asset names (e.g. `Type3arabi-x64.msi`), so the website can link to
  `https://github.com/ArabSeven/type3arabi/releases/latest/download/Type3arabi-x64.msi` for a one-click download.
- Website: `type3arabi.com` on Cloudflare Pages (static; no backend, no analytics scripts, no cookies).
- No accounts, no telemetry (R10 unchanged), no backend. The app's only network use remains ADR-0005's
  optional update check, which reads the public GitHub Releases API.

**4. Staying non-commercial (conditions of the NC data).**
- The app, the model and every release are free of charge. No ads, no paid tier, no paid features, no paid
  bundling or placement, no selling of the model or of access to it.
- An optional donation link may exist on the website and the GitHub page. It is never a condition of
  download, never unlocks anything, and never appears inside the installer or the typing UI.
- Any future change to these conditions needs a new ADR *and* a data rebuild without the NC sources.
  The pipeline therefore records each source's license family, and `build-data` can exclude NC sources
  (`--exclude-nc`), so that rebuild is one command.

## Consequences
+ The strongest available Maghrebi, Tunisian and Egyptian data becomes usable: DODa, TArC, NileChat.
+ Running cost ≈ one domain per year. GitHub and Cloudflare Pages carry all traffic.
+ Clear, auditable provenance (DATASETS.md, META) for users, store reviewers and dataset authors.
− The *bundle* is not OSI "open source" as a whole: the code is, but the model is NC. The README and the Store
  listing must say so precisely ("open-source app; model licensed CC BY-NC-SA 4.0").
− Others may fork the code commercially under Apache-2.0, but they cannot ship our model commercially.
  They would have to train their own from permissively licensed data.
− CC BY-SA resources (Wikipedia word counts, Maknuune, the NArabizi treebank) cannot enter the model.
− Microsoft Store: an MSI/EXE submission requires an installer signed by a CA in the Microsoft Trusted Root
  Program (this replaces the self-signed plan in O2). Free options for open-source projects (e.g. SignPath
  Foundation) need checking before paying for a certificate. Whether an MSIX package can register an
  in-process TSF TIP is unverified, so MSI submission is the default path.

## Alternatives considered
- **Permissive-only data** (the old rules): Maghrebi and Egyptian stay weak, and there is no Tunisian at all.
- **Model under CC BY-NC 4.0** (not SA): it would forbid TArC and the NC-SA Darija corpora.
- **Everything under one copyleft license (GPL)**: it does not solve the data NC terms, and it deters contributors.
- **Hosting downloads on our own server / R2**: this costs money and adds a backend. GitHub Releases already does
  the job for free.
