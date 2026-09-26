# App quirks (docs/02 §16)

| App | Symptom | Root cause | Workaround in TIP | Test |
|---|---|---|---|---|
| Windows Start menu search (SearchHost) | After committing «مرحباً»‎, the box shows a whole sentence («مرحباً كيف حالك يا صديقي الغالي»‎); not in the Settings app's search, not noticed in English | Start's own search completion of the query (Owner confirmed 2026-09-26 by pasting «مرحباً»‎ with Type3arabi off). Type3arabi writes only the chosen candidate + the typed key (STATUS D60) | None needed; documented in README "Good to know" | Manual: paste test in Start |
