# ADR-0004: Single memory-mapped zero-copy data file

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
Many processes load the TIP simultaneously; each must not duplicate tens of MB. AppContainer processes can read Program Files.

## Decision
All runtime linguistic data lives in `%ProgramFiles%\Type3arabi\type3arabi.dat`, a versioned little-endian format read via read-only file mapping and `bytemuck` casts (docs/12).

## Consequences
+ Physical pages shared across processes; near-zero load time; only touched pages resident.
+ One writer/one reader in `t3a-data` keeps the format honest.
− Format changes need version bumps and migration of the builder.

## Alternatives considered
Per-process deserialization (memory ×N, slow start); SQLite (heavier, locking, not zero-copy).
