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

### Option A — the installer (what users will get)
Build it (no admin needed), then run the MSI:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1 -Data target\type3arabi-rc2.dat
```

`target\installer\Type3arabi-<version>-x64.msi` installs to `C:\Program Files\Type3arabi`, adds **Arabic (Saudi
Arabia) · Type3arabi**, starts the global hotkey (Ctrl+Alt+A) and adds **Type3arabi Settings** to the Start
menu. The page after the license lists what it adds. The last page offers **Restart now (recommended)**. If you
untick it, a message explains what may not work until you restart. If a dev build is installed, run
`scripts\dev-uninstall.ps1` first.

Automated install checks (one UAC prompt; nothing is typed and no app is closed):

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\validate-msi.ps1 target\installer\Type3arabi-1.0.0-rc.2-x64.msi   # static, no install
powershell -ExecutionPolicy Bypass -File .\scripts\test-installer.ps1 -Msi target\installer\Type3arabi-1.0.0-rc.2-x64.msi   # silent upgrade
powershell -ExecutionPolicy Bypass -File .\scripts\test-installer.ps1 -Msi target\installer\Type3arabi-1.0.0-rc.2-x64.msi -Uninstall
```

`test-installer.ps1` upgrades silently (`msiexec /qn`, like the Microsoft Store) while apps and a stand-in process
hold the keyboard DLL, and checks that none of them is closed, that no restart is started, the installed state
(version, files, x64/x86 COM, one TSF profile, startup entry, shortcuts, your keyboard list) and, with `-Uninstall`,
that uninstalling removes everything but your settings and learned words before it reinstalls. Report:
`%TEMP%\t3a-installer-test\report.txt`.

### Option B — the dev scripts

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
| `mar7aba` | Space | While typing, the word appears **dotted-underlined** and a candidate list opens under it. Space inserts `مرحباً ` |
| `shukran` | Space | `شكراً ` (tanween) |
| `allah` | Space | `اللّه ` (shadda); `الله` is offered as row 2 |
| `3arabi` | Enter | `عربي` with no newline; press Enter again for a newline |
| `kifak` | `,` | `كيفك،` (Arabic comma) |
| `hello` | Esc | `hello` stays Latin. Next time you type `hello`, the Latin form is ranked first (the list learns) |
| `mar7a` | ↓ ↓ then Space | Inserts the highlighted row |
| `mar7aba` | Enter, then Backspace | The word turns back into an editable `mar7aba` composition with its candidate list |
| `shukran` | Tab, `a`, Enter | Tashkeel editor; the first letter is highlighted; `a` puts a fatha on it → `شَكراً` |
| `shukran` | Tab, Shift+← ←, `a` | Shift+arrows select several letters; one key marks them all |
| (editor) | click a letter / Ctrl+click / drag | select one / add / a range; click a palette cell to apply it; **مسح الكل** clears all |
| `hello` | Shift+Space | `hello ` stays Latin (no scrolling to the Latin row) |
| `mar7aba` | mouse wheel / click a row | wheel moves the highlight; a click inserts that row |
| `oktob`, `ekhtibar` | Space | `أكتب`, `اختبار` |
| `3allam` | Ctrl+Enter | Harakat from your vowels, e.g. `عَلَّم` |
| Ctrl+Space | | Toggles Arabic ↔ plain Latin typing |
| While composing | ← / Ctrl+S / Home | Commits the word, then the key does its normal job |

Please report: the app, what you typed, what you saw, and whether anything froze or closed.

## 2b. Settings
Start menu → **Type3arabi Settings** (or `target\settings\release\type3arabi-settings.exe`). Every shortcut
is on the **الاختصارات / Keyboard** page: click a field and press the new keys; **Esc** (or clicking elsewhere)
cancels and keeps the old shortcut. A shortcut that would get in the way of typing (a bare letter, plain Space,
Esc) or that is already used is refused with a reason, and the field keeps waiting for another try. Changes
apply from the next word in every app. Diacritics style (Allah form, tanween, hamza), dialect and learning are
on the other pages. Windows' own keyboard options for Type3arabi (Settings → Time & language → Language &
region → Arabic → Language options → Type3arabi → Options, or the classic Text Services dialog → Properties)
also open this app.

**Moving to another PC**: Settings → التعلّم / Learning → **Export** saves a `.t3learn` file in Downloads (tick
"Include my settings" to take your shortcuts too). On the other PC: **Import…**, choose Merge or Replace, and
optionally restore the settings. Apps already open use the imported words from the next word you type.

**One entry only**: after installing, `Win+Space` should list your languages plus exactly one
**Arabic (Saudi Arabia) · Type3arabi**, no Arabic (101). To see what Windows lists:
`& "C:\Program Files\Type3arabi\t3a-hotkey.exe" --list-profiles | Out-String` (lines with `True`).

## 3. Known limitations of this build

- **Password fields** (to verify, spike S2): where Windows disables input methods, keys go to the layout under the
  keyboard, which since D14/D33 is Windows' hidden US layout, so Latin letters are expected. Fields an app marks as
  password/PIN/number (input scopes) get Latin letters from Type3arabi itself. Please check a browser password field.
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
pass on both architectures before any TIP change is committed (AGENTS.md §5). It runs quietly by default (its
window is transparent and never activated, the popup is made transparent after a warm-up word, so only that first
popup may flash once); `T3A_HARNESS_VISIBLE=1` shows everything and uses real focus changes.
