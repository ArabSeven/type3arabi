# NOTICE

Type3arabi
Copyright 2026 Hassan Obaida and Type3arabi contributors.

This product includes software developed by the Type3arabi contributors, licensed under the Apache
License, Version 2.0 (see `LICENSE`).

## The language model (`type3arabi.dat`)

The word and spelling statistics in `type3arabi.dat` are licensed under the Creative Commons
Attribution-NonCommercial-ShareAlike 4.0 International License (CC BY-NC-SA 4.0,
https://creativecommons.org/licenses/by-nc-sa/4.0/). They were learned from the datasets below. No dataset
text is redistributed. The full provenance, including sources that are not used, is in `DATASETS.md`. Each
data file lists the sources it was built from in its `META` section.

- **FineWeb-2** (HuggingFaceFW/fineweb-2): Open Data Commons Attribution License (ODC-By) v1.0, also subject
  to Common Crawl's Terms of Use. Word, word-pair and character statistics.
- **Darija Open Dataset (DODa)**, https://github.com/darija-open-dataset/dataset: CC BY-NC 4.0. Moroccan
  Arabizi ↔ Arabic spelling rules. Changes: sentences tokenized and aligned word by word; only statistics kept.
- **TArC, Tunisian Arabish Corpus**, Elisa Gugliotta and Marco Dinarelli (LREC 2020),
  https://github.com/eligugliotta/tarc: CC BY-NC-SA 4.0. Tunisian Arabizi ↔ Arabic spelling rules.
  Changes as above.
- **ArabiziKit corpus** (rabeeeehh/arabizi-kit-corpus): MIT. Spelling rules.
- **UBC-NLP NileChat Arabizi** (nilechat-arabizi-egy / -mor), when present in a build's META: CC BY-NC 4.0.

Development builds may also contain statistics from datasets whose authors have been asked for permission.
Those builds are marked `"distribution": "internal-only"` and are never published:

- Bashar Talafha, Analle Abu Ammar and Mahmoud Al-Ayyoub. Atar: Attention-based LSTM for Arabizi
  Transliteration. IJECE, 2021. https://github.com/bashartalafha/Arabizi-Transliteration
- Ali Khanafer. arabic-to-arabizi. https://huggingface.co/datasets/akhanafer/arabic-to-arabizi

Transliteration conventions documented publicly for Microsoft Maren, and in ArabiziKit (MIT), informed the
hand-written seed rules. No code or data was copied.

## Third-party software

The licenses of the Rust crates compiled into the binaries are listed in `THIRD-PARTY-LICENSES.html`,
generated for each release by `cargo about generate`. All of them are permissive (AGENTS.md R11, enforced by
`cargo deny`).

- Microsoft Windows-classic-samples (SampleIME), MIT: structural reference for the TSF implementation.
- WiX Toolset v5 (© .NET Foundation and contributors, MS-RL): the installer is built with WiX, and the MSI embeds
  two unmodified WiX custom-action DLLs (`WixUiCa`, `Wix4UtilCA`, signed by "WiX Toolset (.NET Foundation)") that
  run only during setup. Source: https://github.com/wixtoolset/wix

## Fonts and brand

- The Settings app bundles **Kufam** (© 2019 The Kufam Project Authors) and **Manrope** (© 2019 The Manrope
  Project Authors), both under the SIL Open Font License 1.1; the license texts ship next to the fonts
  (`fonts/OFL-Kufam.txt`, `fonts/OFL-Manrope.txt` in the Settings app's resources).
- The Type3arabi / «اكتب عربي» name and the t3 logo are the project's brand (Hassan Obaida). The code license
  (Apache-2.0) does not grant trademark rights (Apache-2.0 §6): forks should use their own name and logo.
