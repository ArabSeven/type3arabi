# ADR-0006: Overlay (hook) mode only as fallback

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
The Owner allowed a taskbar app with an activation shortcut if the language-integration path is 'highly resistant'.

## Decision
Do not build the overlay unless Gate G1 (end of M1) fails. Keep engine and UI front-end-agnostic so the overlay (docs/11) can reuse them.

## Consequences
+ Focus on the higher-quality path; fallback cost ~8–10 days if needed.
− None significant.

## Alternatives considered
Building both in parallel (doubles QA surface).
