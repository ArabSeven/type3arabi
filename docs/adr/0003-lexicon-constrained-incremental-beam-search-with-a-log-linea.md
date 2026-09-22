# ADR-0003: Lexicon-constrained incremental beam search with a log-linear model

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
Candidates must appear within one frame, offline, inside arbitrary host processes, and must be explainable and adaptable. Published neural seq2seq systems reach ~80% word accuracy but need heavy runtimes; lexicon + LM noisy-channel systems (Yamli, Maren, Google IME, 3arrib) are fast, adaptive and competitive.

## Decision
Search an Arabic trie with a Latin→Arabic chunk transliteration model, scored log-linearly with a dialect-mixture unigram LM, bigram context, a user model, and kind penalties; plus an unconstrained char-LM OOV path. Parameters tuned offline. No neural model at runtime in v1.

## Consequences
+ Deterministic, fast (≤3 ms p99), explainable (`t3a-cli explain`), trivially adaptive (sticky choices).
+ Data-driven improvements without code changes (better lexicon/rules/weights = better product).
− Unseen clitic combinations rely on OOV path until the clitic lattice (M9).

## Alternatives considered
Char seq2seq/transformer at runtime (latency, size, opacity); rules-only (ArabiziKit shows rules alone plateau).
