# Type3arabi — local testing guide

Everything below runs from a PowerShell prompt in the repository root. The install/uninstall
scripts ask for administrator rights once (UAC); approve it.

## 0. Remove the old (broken) dev build first

Earlier dev builds registered the keyboard under 16 Arabic locales and crashed apps. Remove them:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev-uninstall.ps1
```

Then **sign out and back in** (or restart). Windows keeps a loaded input-method DLL until each app
exits; signing out guarantees nothing still runs the old code. Afterwards, `Win+Space` should list only
your normal keyboards (e.g. English (United States)).

## 1. Install

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev-install.ps1
```

What it does:
1. Builds the 64-bit and 32-bit input-method DLLs (as you, not as admin) into `target\tip`.
2. Copies them and `type3arabi.dat` to `C:\Program Files\Type3arabi\` (so rebuilding never fights
   a DLL that Windows has loaded, and sandboxed apps can read the data).
3. Registers the input method under **one** language: Arabic (Saudi Arabia).
4. Adds that language to your language list with **only** the Type3arabi keyboard.

`Win+Space` then shows exactly one Arabic entry: **Arabic (Saudi Arabia) · Type3arabi** (tray: `ARA`).
Windows has no country-neutral "Arabic" language, so the name always includes a country; dialects are
not chosen here, the engine picks them up from what you type.

To reinstall after code changes, run the same command again, then restart the app you test in.

## 2. What to try (Notepad first, then Word, Chrome, WhatsApp, Teams, Start search)

| Type | Then | Expected |
|---|---|---|
| `mar7aba` | Space | While typing, the word appears **dotted-underlined** and a candidate list opens under it. Space inserts `مرحبا ` |
| `shukran` | Space | `شكراً ` (tanween) |
| `allah` | Space | `اللّه ` (shadda); `الله` is offered as row 2 |
| `3arabi` | Enter | `عربي` with no newline; press Enter again for a newline |
| `kifak` | `,` | `كيفك،` (Arabic comma) |
| `hello` | Esc | `hello` stays Latin. Next time you type `hello`, the Latin form is ranked first (the list learns) |
| `mar7a` | ↓ ↓ then Space | Inserts the highlighted row |
| `mar7aba` | Enter, then Backspace | The word turns back into an editable `mar7aba` composition with its candidate list |
| `shukran` | Tab, `a`, Enter | Tashkeel editor; `a` puts a fatha on the first letter → `شَكراً` |
| `3allam` | Ctrl+Enter | Harakat from your vowels, e.g. `عَلَّم` |
| Ctrl+Space | | Toggles Arabic ↔ plain Latin typing |
| While composing | ← / Ctrl+S / Home | Commits the word, then the key does its normal job |

Please report: the app, what you typed, what you saw, and whether anything froze or closed.

## 3. Known limitations of this build

- **Password fields**: Windows disables input methods there, so keys come from the Arabic 101 layout
  (you will type Arabic letters). Use `Win+Space` to switch to English for passwords. (Spike S2.)
- No tray Arabic/Latin indicator yet; Ctrl+Space toggles silently.
- Clicking elsewhere mid-word keeps the word as shown (no learning); that is intended.
- Candidate list is not yet announced by Narrator; no full-screen-game (UI-less) mode yet.

## 4. Uninstall

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev-uninstall.ps1
```

Add `-RemoveUserData` to also delete what the keyboard has learned (`%LOCALAPPDATA%\Type3arabi`).
Reset only the learning: `powershell -ExecutionPolicy Bypass -File .\scripts\dev-reset-learning.ps1`.

## 5. Automated checks (for developers)

```powershell
cargo test --workspace
cargo run -p t3a-tip --example tsf_harness --target x86_64-pc-windows-msvc   # real TSF host, no install needed
cargo run -p t3a-tip --example tsf_harness --target i686-pc-windows-msvc
cargo run -p t3a-ui --example popup_paint                                     # candidate popup paint path
cargo run -p t3a-cli --release -- eval data/eval/smoke.tsv --data target/type3arabi.dat
cargo run -p t3a-cli --release -- bench --data target/type3arabi.dat
```

`tsf_harness` creates a TSF-enabled RichEdit window, activates the text service in-process against a
private `%LOCALAPPDATA%`, types scenarios through the key sink and checks the resulting text. It must
pass on both architectures before any TIP change is committed (AGENTS.md §5).
