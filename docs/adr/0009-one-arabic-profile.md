# ADR-0009: One Arabic profile (ar-SA); dialects are never a user choice

- **Status:** Accepted (2026-09-23) — Owner-directed (STATUS.md → Owner decisions, O10)
- **Deciders:** Owner, implementing agent
- **Amends:** ADR-0001 ("registered under all common Arabic LANGIDs")

## Context
The first real-machine test registered the profile under 16 Arabic LANGIDs with `bEnabledByDefault`.
The Windows input switcher filled with "Type3arabi" under Jordan, Iraq, Algeria, … — confusing, and it
suggested the user must pick a dialect by picking a country. The Owner's direction: users see literally
one entry, "Arabic – Type3arabi"; dialects are something the engine is trained on, not a selectable
modifier.

## Decision
Register the TSF profile under exactly one LANGID, `0x0401` (ar-SA). The installer adds ar-SA to the
user's language list with the Type3arabi keyboard as its only input method. The LANGID carries no dialect
meaning; the dialect posterior comes from the region prior and online updates (docs/03 §7).
`DllUnregisterServer` still removes the profile from all 16 Arabic LANGIDs to clean older installs.

## Consequences
+ One entry in the switcher; no dialect choice exposed.
+ Simpler registration, uninstall and support.
− Windows has no country-neutral Arabic user language, so the entry reads "Arabic (Saudi Arabia)".
− A user who already has another Arabic variant (e.g. ar-JO with the Arabic 101 keyboard) keeps that
  entry too; the installer does not remove the user's own languages.

## Alternatives considered
- All 16 LANGIDs (ADR-0001 original): the flood above.
- The user's existing Arabic variant, else ar-SA (docs/02 §2 step 4 original): the switcher entry name would
  vary per machine and the DLL would still need every LANGID registered.
