# 13 — `config.toml` schema

Location: `%LOCALAPPDATA%\Type3arabi\config.toml`. Written only by the Settings app (atomic replace).
Read by the TIP (live reload, `docs/01 §6`) and `t3a-hotkey`. Unknown keys are ignored; invalid values fall
back to defaults with one error-log line. The canonical defaults file is `config/config.default.toml`
(tests assert the parser's defaults equal that file).

| Key | Type | Default | Meaning |
|---|---|---|---|
| `schema` | int | 1 | schema version |
| **[general]** | | | |
| `mode_toggle` | string | `"Ctrl+Space"` | `Ctrl+Space`, `Shift+Space`, `Ctrl+Shift+Space`, `ShiftTap`, `none` |
| `mode_scope` | string | `"global"` | `global` (one Arabic/Latin state everywhere) or `per_app` |
| `global_hotkey_enabled` | bool | `true` | run `t3a-hotkey` |
| `global_hotkey` | string | `"Ctrl+Alt+A"` | modifiers `Ctrl`, `Alt`, `Shift`, `Win` + key name |
| **[typing]** | | | |
| `latin_layout` | string | `"auto"` | `auto` or KLID like `"0000040C"` |
| `inline_preview` | string | `"arabic"` | `arabic` or `latin` |
| `candidates_per_page` | int | 7 | 5–9 |
| `predictive_completions` | bool | `true` | |
| `article_joining` | bool | `true` | `docs/03 §6.4` |
| `reedit_backspace` | bool | `true` | `docs/02 §12.2` |
| `commit_on_focus_loss` | string | `"preview"` | `preview` or `latin` |
| `latin_in_url_email` | bool | `true` | `docs/02 §7` |
| **[dialect]** | | | |
| `profile` | string | `"auto"` | `auto`, `MSA`, `LEV`, `EGY`, `GLF`, `IRQ`, `MAG` |
| `seed_from_region` | bool | `true` | use Windows region for the auto prior |
| **[style]** | | | |
| `allah_form` | string | `"shadda"` | `plain`, `shadda`, `shadda_fatha`, `shadda_dagger` |
| `adverbial_tanween` | bool | `true` | |
| `tanween_style` | string | `"on_alif"` | `on_alif`, `before_alif`, `off` |
| `hamza` | string | `"standard"` | `standard`, `relaxed` |
| `vowel_harakat` | string | `"light"` | `light`, `full` |
| `arabic_punctuation` | bool | `true` | `، ؛ ؟` |
| `numerals` | string | `"western"` | `western`, `eastern` |
| **[learning]** | | | |
| `enabled` | bool | `true` | |
| `sticky_last_choice` | bool | `true` | |
| **[privacy]** | | | |
| `use_surrounding_text` | bool | `true` | read ≤ 64 chars before the caret for context (never stored) |
| **[appearance]** | | | |
| `theme` | string | `"system"` | `system`, `light`, `dark` |
| `font_family` | string | `"Segoe UI"` | |
| `font_size` | int | 18 | DIPs, 14–28 |
| `footer_hints` | string | `"auto"` | `auto`, `always`, `never` |
| `show_dialect_badge` | bool | `true` | |
| **[apps]** | | | |
| `excluded` | array of string | `[]` | exe names where the TIP stays passive |
| `rtl_assist` | bool | `false` | `docs/02 §13` |
| `rtl_assist_classes` | array of string | `["Edit", "RichEdit20W", "RICHEDIT50W"]` | |

`state.toml` (same folder, written by `t3a-hotkey`/TIP, read by Settings): `hotkey_conflict`, `last_error`, `data_version_seen`.
