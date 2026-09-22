# ADR-0007: Settings app with Tauri 2

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
Settings is rarely opened, needs good Arabic text rendering/RTL forms, and must not affect typing performance.

## Decision
Tauri 2 (WebView2) with a vanilla TypeScript UI, bilingual; it depends on `t3a-engine` for live previews and `t3a-paths` for file locations.

## Consequences
+ Excellent Arabic shaping/RTL via the browser engine; small bundle; Rust backend.
− Requires WebView2 (present on supported Windows versions).

## Alternatives considered
Win32 dialogs (weak RTL/typography); WinUI 3 (.NET/Windows App SDK runtime); egui (no complex Arabic shaping).
