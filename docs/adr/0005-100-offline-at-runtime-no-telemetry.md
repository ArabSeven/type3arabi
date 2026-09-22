# ADR-0005: 100% offline at runtime, no telemetry

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
Typed text is highly sensitive; AppContainer processes may have no network; users in the region often have metered connections.

## Decision
No shipped binary performs network I/O, except an optional, off-by-default update check in the Settings app (M7+). No telemetry or crash upload.

## Consequences
+ Privacy by construction; predictable latency; works on air-gapped machines.
− Improvement feedback comes only from voluntary channels (golden set, beta testers, GitHub issues).

## Alternatives considered
Cloud-assisted prediction (latency, privacy, AppContainer failures).
