# 02 — TSF integration (the Windows side), precisely

Audience: the agent implementing `t3a-tip`, `t3a-ui` hosting, `t3a-hotkey`, and the installer's
registration step. Everything here is decided; spikes (S1–S5) confirm details and record results in
`docs/spikes/`.

Primary references (read the code, don't copy blindly):
- Microsoft **SampleIME** (C++, MIT): `microsoft/Windows-classic-samples/Samples/IME/cpp/SampleIME` — canonical structure.
- **saschanaz/ime-rs** — incremental Rust port of SampleIME, in working state.
- **mkpoli/ainuKey** — minimal working Rust (`windows-rs`) TIP with preedit + commit.
- **nathancorvussolis/corvusskk** and **rime/weasel** — mature C++ TIPs; mine them for app quirks.
- Docs: "Custom IME requirements" (learn.microsoft.com/windows/apps/develop/input/input-method-editor-requirements),
  TSF reference (msctf.h), "64-bit considerations for text services", "UILess mode overview".
Code may be adapted only from MIT/Apache/BSD sources, with attribution in `NOTICE.md`.

---

## 1. Identity (do not change once shipped — changing them orphans user installs)

| Item | Value |
|---|---|
| TIP CLSID | `{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}` |
| Profile GUID | `{90D49398-54D3-4F08-9C15-0B38D0820A87}` |
| Display attribute: composing (preview) | `{3F94CF8A-3689-49C9-9597-18F7435E0FE8}` |
| Display attribute: tashkeel editing | `{5AD3E153-E0D0-41BA-8D1E-E8B312230016}` |
| Private compartment: engine mode (Arabic/Latin) | `{62E8BEAD-7142-47C3-9555-656FC8A9E8FA}` |
| Preserved key: mode toggle | `{35746905-7855-46BE-A409-AF7CC35C8FF7}` |
| MSI UpgradeCode | `{C71F4AAE-43FF-47DA-B6F0-582B0611BAF6}` |
| Spare (reserve for reconversion function provider) | `{51D93195-3C9E-4C04-BDE1-419D15F1015C}` |
| Profile description | `Type3arabi` (Windows shows **AR – Type3arabi** / "Arabic – Type3arabi") |
| COM ThreadingModel | `Apartment` |

All GUIDs live in `crates/t3a-tip/src/ids.rs` as `windows_core::GUID::from_u128(0x…)`.

## 2. Registration (`DllRegisterServer` / `DllUnregisterServer`)

Order (mirror in unregister, reversed):
1. **COM**: `HKLM\Software\Classes\CLSID\{CLSID}` default = `Type3arabi`,
   `\InprocServer32` default = full DLL path, `ThreadingModel = Apartment`. (Standard COM keys are
   allowed — R6.) On 64-bit Windows the x86 DLL registers into the WOW64 view automatically when
   `regsvr32` (32-bit) or the MSI 32-bit component runs it. ARM64 DLL registers in the native view.
   All three use the **same CLSID** so Windows presents one logical input method.
2. **Profiles**: `CoCreateInstance(CLSID_TF_InputProcessorProfiles)` → `ITfInputProcessorProfileMgr::RegisterProfile(
   CLSID, langid, PROFILE_GUID, "Type3arabi", iconFile = <this DLL path>, uIconIndex = -IDI_BRAND (resource id, negative),
   hklSubstitute = see §6.3, dwPreferredLayout = 0, bEnabledByDefault = TRUE, dwFlags = 0)` for **each** LANGID in §2.1.
3. **Categories** (`ITfCategoryMgr::RegisterCategory(CLSID, cat, CLSID)`):
   `GUID_TFCAT_TIP_KEYBOARD`, `GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER`, `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`,
   `GUID_TFCAT_TIPCAP_SECUREMODE`, `GUID_TFCAT_TIPCAP_COMLESS`, `GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT`,
   `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT`, `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT`.
4. The **installer** (not the DLL) then calls `InstallLayoutOrTip("<langid>:{CLSID}{PROFILE}", flags)` once for
   the user's chosen Arabic locale (default: an Arabic language already in the user's list; else `0401`).
   Flag `ILOT_DEFPROFILE` only if the user ticked "Make Type3arabi the default Arabic keyboard".
   Never write `HKCU\Keyboard Layout\Preload` or similar directly.

### 2.1 LANGIDs registered
`0x0401` ar-SA, `0x0801` ar-IQ, `0x0C01` ar-EG, `0x1001` ar-LY, `0x1401` ar-DZ, `0x1801` ar-MA,
`0x1C01` ar-TN, `0x2001` ar-OM, `0x2401` ar-YE, `0x2801` ar-SY, `0x2C01` ar-JO, `0x3001` ar-LB,
`0x3401` ar-KW, `0x3801` ar-AE, `0x3C01` ar-BH, `0x4001` ar-QA.
Rationale: the keyboard must appear under whichever Arabic variant the user already has; the chosen
LANGID also seeds the dialect prior (`docs/03 §7.2`).

### 2.2 Resources in the DLL (via `embed-resource` build-dep + `t3a-tip/res/t3a.rc`)
- `IDI_BRAND` (id 101): black glyph **ع** in a white rounded box with 1-px 50%-black stroke; sizes
  16/20/24/32/40/48, 32-bit ARGB. `IDI_MODE_AR` (102): white **ع** with stroke. `IDI_MODE_LATIN` (103): white **A** with stroke.
  (Microsoft guideline: IME icons are black/white glyphic, stored in the DLL, not loose .ico files.)
- `VS_VERSIONINFO` with product/file version = workspace version, CompanyName = Owner's legal name.

## 3. COM object model

```
ClassFactory ──creates──► TextService   (one per thread that activates the TIP)
TextService implements:
  ITfTextInputProcessorEx         Activate/ActivateEx/Deactivate
  ITfThreadMgrEventSink           OnSetFocus (document focus changes)
  ITfThreadFocusSink              OnSetThreadFocus / OnKillThreadFocus (hide/show popup)
  ITfTextEditSink                 OnEndEdit (selection moved by user/app → finalize composition, drop re-edit anchor)
  ITfTextLayoutSink               OnLayoutChange (reposition popup; handles TF_E_NOLAYOUT apps)
  ITfKeyEventSink                 OnTestKeyDown/OnKeyDown/OnTestKeyUp/OnKeyUp/OnPreservedKey
  ITfCompositionSink              OnCompositionTerminated
  ITfDisplayAttributeProvider     EnumDisplayAttributeInfo / GetDisplayAttributeInfo
  ITfCompartmentEventSink         open/close + private mode compartment changes
  ITfFunctionProvider             exposes ITfFnConfigure (Settings) and later ITfFnReconversion (M9)
  ITfFnConfigure                  Show() → launch Settings app (desktop processes only)
Other objects:
  LangBarInputModeItem : ITfLangBarItemButton, ITfSource   (GUID_LBI_INPUTMODE; tray mode icon ع/A, click toggles)
  CandidateUIElement   : ITfCandidateListUIElementBehavior (+ ITfCandidateListUIElement, ITfUIElement)
  EditSession          : ITfEditSession (closure-based: one struct holding a boxed FnOnce(ec) )
  DisplayAttributeInfo : ITfDisplayAttributeInfo (×2)
  EnumDisplayAttributeInfo : IEnumTfDisplayAttributeInfo
```

Implement with `windows::core::implement` (`#[implement(ITfTextInputProcessorEx, ITfKeyEventSink, …)]`).
State inside `TextService` is `RefCell`-based (single-threaded apartment) — never hold a `borrow_mut`
across a call that can re-enter TSF (RequestEditSession, SetText, EndComposition, UIElement calls).
Pattern: take the data you need, drop the borrow, call TSF, re-borrow.

Exports: `DllGetClassObject`, `DllCanUnloadNow` (object + lock counts), `DllRegisterServer`,
`DllUnregisterServer`. `DllMain` does nothing except store the module handle (no loader-lock work).

Every exported function and COM method body: `guard(|| { … })` (R1). `guard` returns the method's
failure value (usually `E_FAIL`; for `OnTestKeyDown` it returns `S_OK` with `*pfEaten = FALSE`).

## 4. Activation lifecycle

`ActivateEx(ptim, tid, flags)`:
1. Store `ITfThreadMgr`, client id `tid`, flags. Record `secure_mode = flags & TF_TMAE_SECUREMODE`,
   `uiless_only = flags & TF_TMAE_UIELEMENTENABLEDONLY`.
2. `ITfThreadMgrEx::GetActiveFlags` → `immersive = flags & TF_TMF_IMMERSIVEMODE`.
3. `app_container = token_is_appcontainer()` (`GetTokenInformation(TokenIsAppContainer)`).
4. `DATA.get_or_init(load_data)` — mmap `%ProgramFiles%\Type3arabi\type3arabi.dat`, validate header
   (magic, version, section table bounds). Failure ⇒ `disabled = true` (safe passthrough) + error log.
5. Load config snapshot (§6 of `docs/01`). Excluded process (config `apps.excluded`, match exe file
   name case-insensitively) ⇒ `disabled = true`.
6. Advise sinks: `ITfThreadMgrEventSink`, `ITfThreadFocusSink` (via `ITfSource` on thread mgr),
   `ITfKeystrokeMgr::AdviseKeyEventSink(tid, self, fForeground = TRUE)`, preserved key(s) (§10),
   compartment sinks (open/close global compartment + private mode compartment), text-edit + layout sinks
   on the currently focused context (re-advised in `OnSetFocus`).
7. Create the lang-bar input-mode item and add it (`ITfLangBarItemMgr::AddItem`).
8. Set `GUID_COMPARTMENT_KEYBOARD_OPENCLOSE` = open (Microsoft: "set the default IME mode to on").
9. Post (not call) a warm-up message to the popup's message-only window: creates D2D/DWrite factories
   off the critical path. Must return in ≤ 30 ms total (budget in `docs/06`).

`Deactivate()`: finalize any composition (commit the current inline preview as-is, no learning),
hide/destroy popup, unadvise everything in reverse order, release interfaces, flush user journal
channel (non-blocking send of a Flush message). Never block.

`OnSetFocus(new, prev)`: if composing in `prev` → finalize (as above). Hide popup. Re-advise
edit/layout sinks on `new`'s top context. Clear re-edit anchor.

## 5. Key routing

### 5.1 Principles
1. `OnTestKeyDown` must be **pure** (no state change, no edit sessions): it calls
   `KeyRouter::classify(&state, key) -> Decision { eat: bool, action: KeyAction }` and returns `eat`.
   `OnKeyDown` calls the same function and then executes `action`. (Identical inputs ⇒ identical decision.)
2. We **eat every printable key** while the TIP is enabled and a writable context has focus — in both
   Arabic and Latin modes — because the underlying HKL may be the Arabic 101 layout (see §6.3).
3. Keys with Ctrl or Alt (without being AltGr producing a character) or Win are never eaten, except
   our preserved keys and the in-composition keys listed below. If composing, they first trigger
   **commit-and-reinject** (§5.3).
4. Track keys eaten on key-down in a 256-bit set; eat the matching key-up; pass all other key-ups.
5. Any injected key carrying `dwExtraInfo == T3A_REINJECT_MAGIC (0x54334152, "T3AR")` is passed untouched.
   Read it with `GetMessageExtraInfo()` inside the sink.

### 5.2 Key table (Arabic mode). "Popup" = candidate list visible; "Tashkeel" = tashkeel editor open.

| Key | Idle (not composing) | Composing (Popup) | Tashkeel editor |
|---|---|---|---|
| Latin letter, top-row digit, `'` | Eat → start composition, append | Eat → append | Eat → mark command (`docs/05 §4.3`) |
| Numpad digit | Eat → start a number token (literal digit) | Eat → append as *literal digit* | Eat → ignore |
| `-` directly after `el/al/il/l` article | — | Eat → article joiner (no-op char, `docs/03 §6.4`) | — |
| Backspace | Re-edit check (§12.2); else Pass | Eat → remove last Latin char; empty ⇒ cancel composition | Eat → clear marks on focused letter; if none ⇒ back to list |
| Space | Pass | Eat → commit highlighted + space (unless article-join) | Eat → commit vocalized + space |
| Enter | Pass | Eat → commit highlighted, **no** newline | Eat → commit vocalized, no newline |
| Ctrl+Enter | Pass | Eat → commit highlighted with vowel-derived harakat | Eat → commit vocalized |
| Tab | Pass | Eat → open tashkeel editor on highlighted | Eat → next quick-pick |
| Shift+Tab | Pass | Eat → move highlight up | Eat → previous quick-pick |
| ↓ / ↑ | Pass | Eat → move highlight (wraps) | Eat → move in quick-picks |
| PageDown / PageUp | Pass | Eat → next/prev page (if > page size) | — |
| ← / → / Home / End / Delete | Pass | Eat → commit highlighted (no space) + **reinject** key | Eat → move focused letter (visual direction; `docs/05 §4.2`) |
| Esc | Pass | Eat → commit the **raw Latin** buffer as typed | Eat → close editor, back to list |
| Punctuation `, ; ? . ! : ( ) " …` | Eat → insert mapped punctuation (`docs/03 §6.6`) | Eat → commit highlighted, then insert mapped punctuation | Eat → commit vocalized, then punctuation |
| Shift+letter | as letter (uppercase kept for normalization) | as letter | Shift variants of mark commands |
| Ctrl/Alt/Win combos | Pass | Commit highlighted (no space) + **reinject** | Commit vocalized + reinject |
| Mode toggle (default Ctrl+Space) | Preserved key → toggle Arabic/Latin | Commit highlighted, then toggle | Commit vocalized, then toggle |

Latin mode: every printable key ⇒ Eat → insert the Latin-layout character directly
(`ITfInsertAtSelection::InsertTextAtSelection`, `TF_IAS_NOQUERY`), no composition, no popup. All
other keys pass. Mode toggle works as above.

### 5.3 Commit-and-reinject
Used when a key must reach the app *after* we finalize the composition. In `OnKeyDown`: eat the
key, run a sync edit session that commits, then `SendInput` the same key (down + up) with
`dwExtraInfo = T3A_REINJECT_MAGIC` and the original modifier state. The reinjected key bypasses the
router (§5.1-5). If `SendInput` fails (UIPI) the key is lost — acceptable only for navigation
keys; log nothing (not user text, but noise).

## 6. Latin layout translation (key → character)

### 6.1 Algorithm (in `t3a-tip::keymap`)
```
scan = (lParam >> 16) & 0xFF;  ext = (lParam >> 24) & 1
hkl  = config.latin_layout_hkl()          // resolved once per config load, see 6.2
vk_l = MapVirtualKeyExW(scan | (ext ? 0xE000 : 0), MAPVK_VSC_TO_VK_EX, hkl)
state = GetKeyboardState()                // copy; clear VK_CONTROL/VK_MENU unless AltGr combo
n = ToUnicodeEx(vk_l, scan, state, buf, 4, wFlags = 0x4 /* don't change kernel kbd state */, hkl)
n == 1 && !is_control(buf[0])  ⇒ printable char buf[0]
otherwise ⇒ not printable (pass unless in the key table)
```
Why scan codes: the thread's active HKL is Arabic, so the `vk` TSF hands us is the Arabic layout's
VK. Re-deriving the VK from the **scan code** through the user's Latin layout makes AZERTY/QWERTZ
users get exactly the characters printed on their keys.

Numpad: detect by `vk` in `VK_NUMPAD0..=VK_NUMPAD9` (before translation) ⇒ literal digit.

### 6.2 Choosing the Latin layout
`config.typing.latin_layout = "auto" | "<KLID>"` (e.g. `"0000040C"` French). `auto` =
first HKL from `GetKeyboardLayoutList` whose primary language is not Arabic/Farsi/Urdu/Hebrew;
if none, the built-in US mapping table (`keymap::US_FALLBACK`, scan code → (normal, shift) chars).
Never call `LoadKeyboardLayout` (it can add layouts to the user's list); only use HKLs already loaded
in the list, else the built-in table.

Dead keys: v1 ignores dead-key composition (the spacing accent is produced). Documented limitation.

### 6.3 Spike S2 — can the base keyboard under our profile be Latin?
Hypothesis: registering each profile with `hklSubstitute = MAKELONG(langid, 0x0409)` (e.g.
`0x04090401` = "Arabic language, US layout") makes Windows use a Latin physical layout while
Type3arabi is active, so (a) fields where IMEs are disabled (password boxes) receive Latin characters,
and (b) apps still see an **Arabic input language** (keeps Word/Outlook auto-RTL).
Test: register with and without substitute; in Notepad, a Win32 password box, Chrome password field,
Word: check `GetKeyboardLayout(0)`, the characters produced in password fields, and Word's paragraph
direction switching. Record in `docs/spikes/S2-base-layout.md`.
- If S2 **passes**: keep eating printable keys anyway (consistency), ship with the substitute HKL,
  and add Settings → "Latin layout" writes the substitute via `ITfInputProcessorProfiles::SubstituteKeyboardLayout`
  (admin-elevated helper, M7) for AZERTY/QWERTZ users.
- If S2 **fails**: ship with `hklSubstitute = 0`; document "password fields: switch keyboard with Win+Space".

## 7. Context gating (when to be passive)

Compute `ContextMode` on focus change and on composition start, cache per context:

| Condition | Mode |
|---|---|
| No focused context / context read-only (`TF_SD_READONLY` in `ITfContext::GetStatus`) | **Off** (eat nothing) |
| Input scope contains `IS_PASSWORD`, `IS_PIN`, `IS_NUMBER`, `IS_NUMBER_FULLWIDTH`, `IS_DIGITS`, `IS_TELEPHONE_FULLTELEPHONENUMBER`, `IS_TELEPHONE_LOCALNUMBER`, `IS_TELEPHONE_AREACODE`, `IS_TELEPHONE_COUNTRYCODE`, `IS_CURRENCY_AMOUNT`, `IS_DATE_*`, `IS_TIME_*` | **Latin** (insert Latin chars, no transliteration, no learning) |
| Input scope contains `IS_EMAIL_SMTPEMAILADDRESS`, `IS_EMAIL_USERNAME`, `IS_URL`, `IS_LOGINNAME` | **Latin** by default (config `typing.latin_in_url_email = true`) |
| Input scope contains `IS_PRIVATE`, or `secure_mode`, or `app_container` | **Arabic, no learning** (user store read-only) |
| Excluded exe / `disabled` after panic | **Off** |
| else | **Arabic** or **Latin** per mode compartment |

Input scopes: read `GUID_PROP_INPUTSCOPE` property of the context's selection range →
`ITfInputScope::GetInputScopes`. Failure ⇒ treat as no scope.

## 8. Composition & edit sessions

- All document changes happen inside `ITfContext::RequestEditSession(tid, session, TF_ES_READWRITE | TF_ES_ASYNCDONTCARE)`.
  Check the returned `hrSession`; if it indicates async scheduling, the session will run later — the
  engine state is already updated, so the session applies the *latest* state (sessions read state
  when they run, never capture stale text).
- **Start**: `ITfContextComposition::StartComposition(ec, insertionRange, self_as_ITfCompositionSink)`,
  where insertionRange = current selection (via `ITfInsertAtSelection::InsertTextAtSelection(ec, TF_IAS_QUERYONLY, …)`).
- **Update**: `composition.GetRange()` → `SetText(ec, 0, preview)` → set `GUID_PROP_ATTRIBUTE` on the
  range to our display attribute atom (`ITfCategoryMgr::RegisterGUID` once per activation) → collapse
  selection to the end of the range (`SetSelection`).
- Inline preview text = `config.typing.inline_preview`: `"arabic"` (default: the highlighted
  candidate's display string) or `"latin"` (the raw buffer).
- **Commit**: SetText(final string) on the composition range → clear the attribute property →
  collapse selection to end → `EndComposition(ec)` → if a trailing space/punctuation is due, insert
  it with `InsertTextAtSelection(ec, 0, …)` in the same session.
- **Cancel** (buffer emptied by Backspace): SetText(ec, 0, "") then EndComposition.
- `OnCompositionTerminated` (app/TSF ended it, e.g. focus loss, app clicked elsewhere): the text
  already in the range stays (it is the preview). Reset engine session, hide popup, **no learning**.
- `OnEndEdit(ctx, ecReadOnly, editRecord)`: if `editRecord.GetSelectionStatus()` changed and the edit
  was not ours (`self.in_own_session == false`) and the new selection is outside the composition range
  ⇒ finalize composition per `config.typing.commit_on_focus_loss` (`"preview"` default = keep what is
  shown; `"latin"` = replace with raw Latin). Also clear the re-edit anchor.

Display attributes:
- Composing: `TF_DISPLAYATTRIBUTE{ crText: TF_CT_NONE, crBk: TF_CT_NONE, lsStyle: TF_LS_DOT, fBoldLine: FALSE, crLine: TF_CT_NONE, bAttr: TF_ATTR_INPUT }`.
- Tashkeel editing: same with `lsStyle: TF_LS_SOLID, fBoldLine: TRUE, bAttr: TF_ATTR_TARGET_CONVERTED`.

## 9. Hosting the popup

- **Owner window**: `ITfContextView::GetWnd`; if it fails or is null, `GetFocus()` (Microsoft guidance).
- **Window**: class `T3A_Popup`, styles `WS_POPUP`, ex-styles `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST`,
  class style `CS_DROPSHADOW`. Handle `WM_MOUSEACTIVATE → MA_NOACTIVATE`. Windows 11: `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL)`.
  Never call `SetFocus`/`SetForegroundWindow`. Do **not** use `WS_EX_LAYOUTRTL` (we lay out RTL ourselves).
- **Position**: in the same edit session after updating text, `ITfContextView::GetTextExt(ec, compositionRange, &rect, &clipped)`.
  `TF_E_NOLAYOUT` ⇒ keep previous position, wait for `ITfTextLayoutSink::OnLayoutChange`, then re-query.
  Placement rule (screen coords, per-monitor work area from `MonitorFromRect`):
  popup's **right edge aligned to the composition's right edge** (RTL), top = rect.bottom + 2 px;
  if it overflows the bottom → place above (bottom = rect.top − 2); clamp horizontally into the work area.
  Touch keyboard: query `IFrameworkInputPane::Location`; if the popup would intersect it, place above.
- **DPI**: scale all metrics by `GetDpiForWindow(owner) / 96`. Do not declare DPI awareness in the DLL
  (we run at the host's awareness, per Microsoft guidance); for per-monitor-aware hosts, handle
  `WM_DPICHANGED` on our popup.
- **Light dismiss events**: `NotifyWinEvent(EVENT_OBJECT_IME_SHOW / EVENT_OBJECT_IME_HIDE / EVENT_OBJECT_IME_CHANGE, hwnd, OBJID_CLIENT, CHILDID_SELF)`
  on show / hide / move-or-resize.
- **UILess mode**: before showing, `ITfUIElementMgr::BeginUIElement(candidateElement, &show, &id)`.
  If `show == FALSE` (games, full-screen, search pane) do not show our window; keep the UIElement
  updated (`UpdateUIElement(id)`) and implement `ITfCandidateListUIElementBehavior`:
  `GetCount/GetSelection/GetString/GetPageIndex/SetPageIndex/GetCurrentPage/SetSelection/Finalize/Abort`.
  `EndUIElement(id)` when the list closes. Same for the "uiless_only" activation flag.
- **Accessibility**: the popup exposes a UIA provider (`t3a-ui::uia`): list element with
  `UIA_AutomationIdPropertyId = "IME_Candidate_Window"` (conversion list) — use
  `"IME_Prediction_Window"` only for a completions-only list; raise `UIA_MenuOpenedEventId` / `UIA_MenuClosedEventId`;
  each item's `Name` = candidate text; selection change raises `UIA_SelectionItem_ElementSelectedEventId`
  with `IsSelected = TRUE`. `HelpText` = dialect/kind hint (e.g. "Latin", "اقتراح").

## 10. Mode (Arabic ↔ Latin), toggle key, tray mode icon

- State = global compartment `GUID_COMPARTMENT_KEYBOARD_OPENCLOSE` (open = Arabic, closed = Latin),
  obtained via `ITfThreadMgr::GetGlobalCompartment` when `config.general.mode_scope = "global"` (default),
  else the thread compartment. React to changes in `ITfCompartmentEventSink::OnChange`.
- Tray: `LangBarInputModeItem` with `GUID_LBI_INPUTMODE`; icon `IDI_MODE_AR` or `IDI_MODE_LATIN`;
  tooltip "عربي – Arabic" / "Latin"; `OnClick` toggles the compartment; right-click menu:
  Arabic/Latin, Settings…, Help.
- Toggle hotkey: `ITfKeystrokeMgr::PreserveKey(tid, GUID_PRESERVED_TOGGLE, TF_PRESERVEDKEY{uVKey, uModifiers}, "Toggle Arabic/Latin")`.
  Default `Ctrl+Space` (`VK_SPACE`, `TF_MOD_CONTROL`). Config accepts `"Ctrl+Space" | "Shift+Space" | "Ctrl+Shift+Space" | "ShiftTap" | "none"`.
  `ShiftTap` = press and release Shift alone within 300 ms with no other key: implemented in the key-up path, not as a preserved key.

## 11. Global activation hotkey (companion `t3a-hotkey.exe`)

Purpose: from **any** keyboard (e.g. English US), one customizable shortcut switches the foreground app
to Type3arabi, and pressing it again switches back. The TIP itself cannot do this (it is not loaded
while another keyboard is active).

- Process: `t3a-hotkey.exe`, no window except a message-only window, started at logon via
  `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` (installer); on start it reads config and **exits
  immediately** if `general.global_hotkey_enabled = false`. Idle cost: blocked in `GetMessageW` (0 CPU, ~1–2 MB).
- `RegisterHotKey(msgWnd, 1, mods | MOD_NOREPEAT, vk)`; default **Ctrl+Alt+A** (configurable). If
  registration fails, write `hotkey_conflict = true` into `%LOCALAPPDATA%\Type3arabi\state.toml` so Settings can show it.
- On `WM_HOTKEY`: `fg = GetForegroundWindow()`, `cur = GetKeyboardLayout(GetWindowThreadProcessId(fg))`.
  - If `PRIMARYLANGID(LOWORD(cur)) == LANG_ARABIC` ⇒ switch back: `PostMessageW(fg, WM_INPUTLANGCHANGEREQUEST, 0, last_non_arabic_hkl)`.
  - Else remember `last_non_arabic_hkl = cur` and post the Arabic HKL whose language has Type3arabi enabled.
- **Spike S3** validates, in this order, and records which works on Win10 22H2 and Win11 24H2,
  with "Let me use a different input method for each app window" both on and off:
  (a) `WM_INPUTLANGCHANGEREQUEST` (activates the language's *default/last-used* input method — so
  S3 also checks `ILOT_DEFPROFILE`), (b) `ITfInputProcessorProfileMgr::ActivateProfile(TF_PROFILETYPE_INPUTPROCESSOR, langid, CLSID, PROFILE, 0, TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE)`
  from the companion, (c) whether Windows' own *Advanced keyboard settings → Input language hot keys*
  lists Type3arabi (zero-cost alternative; document it in the user guide either way).
- Limitation: UIPI blocks posting to elevated windows unless the companion is elevated; document.

## 12. Surrounding context & re-edit

### 12.1 Context words for the LM
On composition start, in the edit session: clone the selection range, `ShiftStart(ec, -64, …)`,
`GetText` (≤ 64 UTF-16 units). Tokenize from the end: skip whitespace; take up to 2 Arabic words
(runs of U+0621–U+064A, U+0671–U+06D3 plus marks). Strip marks. Pass to `Session::set_context()`.
If reading fails, use the last ≤ 2 words this `TextService` committed in the same context.
Config `privacy.use_surrounding_text = false` ⇒ only our own commits. Never store this text.

### 12.2 Re-edit (Backspace undoes a conversion)
After each commit, store `ReEditAnchor { ctx, range (ITfRange covering the committed word + any trailing
space we inserted), latin, word, rank }`. Clear it on: any key other than Backspace, focus change,
foreign edit or selection change (`OnEndEdit` not ours), composition start.
On Backspace while idle with an anchor (`config.typing.reedit_backspace = true`):
1. If the anchor includes a trailing space and the caret is right after it: delete the space ourselves,
   shrink the anchor, keep it. (Behaves like a normal Backspace.)
2. Else if the caret is right after the word and the range text still equals `word`: replace the range
   with a new composition containing `latin`, restore the engine session for `latin`, open the popup with
   the previously committed candidate highlighted. Next Backspace edits the Latin buffer.
3. Else: pass the key.

## 13. RTL assist (opt-in, default off)
Apps decide paragraph direction. Registering under an Arabic LANGID already makes Office/Outlook and
many RichEdit hosts switch to RTL automatically. For plain `Edit`/`RichEdit` controls where users want
RTL, `apps.rtl_assist = true` makes the TIP send **Ctrl + Right-Shift** (the standard Windows
"right-to-left reading order" shortcut) once per context at the first composition start, only if the
focused window class is in `apps.rtl_assist_classes` (default `["Edit", "RichEdit20W", "RICHEDIT50W"]`)
and the text before the caret on the line is empty. Sent via `SendInput` with the reinject magic.

## 14. Functions
- `ITfFnConfigure::Show(hwndParent, langid, profile)`: if not AppContainer and not secure mode →
  `ShellExecuteExW("open", "%ProgramFiles%\\Type3arabi\\Type3arabi Settings.exe")`; else `E_NOTIMPL`.
- `ITfFnReconversion` — reserved for M9 (reconvert selected Arabic word to its candidates).

## 15. Error handling & safe passthrough
- `guard()` catches panics ⇒ set process-wide `DISABLED: AtomicBool = true`, hide popup, end composition
  if possible (leave text), log `"{timestamp} panic in {method} v{version}"` (no payload, no user text).
- Every `windows::core::Result` error on the key path ⇒ return "not eaten" for that key (never swallow input).
- `DllCanUnloadNow` must be correct (object count + server locks) so apps can unload us.
- Hostile hosts: if `GetTextExt` consistently fails, keep working with the popup positioned at the
  caret from `GetGUIThreadInfo(hwndCaret, rcCaret)` fallback.

## 16. App quirks log
Maintained during M1/M4/M8 in `docs/app-quirks.md` (one row per app: symptom, cause, workaround, test).

## 17. Spikes owned by this doc
| Spike | Question | Budget | Exit |
|---|---|---|---|
| S1 | Scan-code translation gives correct chars for US, UK, French AZERTY, German QWERTZ under the Arabic HKL? | 0.5 day | Table of results + unit tests with recorded scan codes |
| S2 | Does `hklSubstitute` give a Latin base layout while keeping an Arabic input language? | 1 day | §6.3 decision |
| S3 | Global activation hotkey mechanism | 1 day | §11 decision |
| S4 | UIA candidate provider recognized by Narrator | 1 day | Narrator reads candidates |
| S5 | UILess mode in a full-screen DirectX game + Windows search box | 1 day | Candidates visible via host |
