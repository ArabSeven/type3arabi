# App quirks (docs/02 §16)

| App | Symptom | Root cause | Workaround in TIP | Test |
|---|---|---|---|---|
| Windows Start menu search (SearchHost) | After committing «مرحباً»‎, the box shows a whole sentence («مرحباً كيف حالك يا صديقي الغالي»‎); not in the Settings app's search, not noticed in English | Start's own search completion of the query (Owner confirmed 2026-09-26 by pasting «مرحباً»‎ with Type3arabi off). Type3arabi writes only the chosen candidate + the typed key (STATUS D60) | None needed; documented in README "Good to know" | Manual: paste test in Start |
| Subtitle Edit 5.2 (Avalonia 11; any IMM32-only app: Qt 5, Java, SDL…) | First letter appears, then the app freezes (100% CPU); the candidate list stays black; the tray shows the "IME disabled" icon | Our `OnLayoutChange` queued a `Relayout`, whose `GetTextExt` CUAS reports as another layout change, endlessly inside TSF's queue (D65) | Echoes ignored: not during our sessions, one queued, settled after a no-move relayout until the app's message loop runs, budget 8 per word (docs/02 §9) | `imm32_harness` |
