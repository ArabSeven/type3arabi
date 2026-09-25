//! The TSF text service (docs/02 §3–§12).
//!
//! Re-entrancy rule (docs/02 §3): all state lives in one `RefCell<State>` that is borrowed only for
//! short stretches containing no TSF calls. Every borrow is a `try_borrow*`, so an unexpected
//! re-entry degrades to "key not eaten" instead of a panic. Document changes happen only inside
//! edit sessions (`DocOp` + `apply`), which read and write state the same way.

use crate::ids::GUID_PRESERVED_TOGGLE;
use crate::keyrouter::{classify, Action, Decision, Key, KeyMap, Popup, RouterState, Toggle};
use crate::win::compose::EditSession;
use crate::win::context::evaluate_context_mode;
use crate::win::display::{
    DisplayAttributeInfo, EnumDisplayAttributeInfo, GUID_ATTR_INPUT, GUID_ATTR_TASHKEEL,
};
use crate::win::dll::{self, add_object, release_object};
use crate::win::guard::guard;
use crate::win::keys::translate_key;
use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::sync::{Arc, OnceLock};
use t3a_engine::normalize::InputChar;
use t3a_engine::session::{CandidateKind, CommitHow, Session, Trailing};
use t3a_engine::{
    Config, Engine, NoUser, SelectMode, TashkeelAction, TashkeelCmd, TashkeelEditor, UserScorer,
    UserStore,
};
use t3a_ui::{
    Footer, ListModel, PopupEvent, PopupModel, PopupWindow, Row, RowMarker, TashkeelModel,
};
use windows::core::{implement, Interface, BOOL, GUID};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_SPACE;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, IEnumTfDisplayAttributeInfo, ITfCategoryMgr, ITfComposition,
    ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfContextComposition,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfDocumentMgr, ITfEditSession, ITfFnConfigure, ITfFnConfigure_Impl, ITfFunction_Impl,
    ITfInsertAtSelection, ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfRange,
    ITfSource, ITfTextInputProcessor, ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfTextInputProcessor_Impl, ITfThreadFocusSink, ITfThreadFocusSink_Impl, ITfThreadMgr,
    ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl, GUID_PROP_ATTRIBUTE, TF_AE_NONE,
    TF_ANCHOR_END, TF_ES_ASYNC, TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY, TF_INVALID_COOKIE,
    TF_MOD_CONTROL, TF_PRESERVEDKEY, TF_SELECTION, TF_SELECTIONSTYLE, TF_TMAE_SECUREMODE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetGUIThreadInfo, GetMessageExtraInfo, GUITHREADINFO,
};
use windows_core::IUnknownImpl;

const PASS: Decision = Decision {
    eat: false,
    action: Action::Pass,
};

// ---------------------------------------------------------------------------------------------
// Process-wide resources

static ENGINE: OnceLock<Engine> = OnceLock::new();

fn engine() -> &'static Engine {
    ENGINE.get_or_init(|| {
        // Next to the DLL (dev install: <dir>\x64\t3a_tip.dll + <dir>\type3arabi.dat), then the
        // installed location.
        let mut candidates = Vec::new();
        if let Some(dir) = dll::dll_path().as_deref().and_then(|p| p.parent()) {
            candidates.push(dir.join(t3a_paths::DATA_FILE_NAME));
            if let Some(up) = dir.parent() {
                candidates.push(up.join(t3a_paths::DATA_FILE_NAME));
            }
        }
        candidates.push(t3a_paths::data_file_path());
        for path in &candidates {
            if !path.exists() {
                continue;
            }
            if let Ok(file) = t3a_data::DataFile::open(path) {
                let file: &'static t3a_data::DataFile = Box::leak(Box::new(file));
                if let Ok(view) = file.view() {
                    if let Ok(eng) = Engine::new(view) {
                        return eng;
                    }
                }
            }
        }
        t3a_paths::log_error("type3arabi.dat missing or invalid; using built-in seed tables");
        Engine::builtin()
    })
}

static USER_STORE: OnceLock<Option<Arc<UserStore>>> = OnceLock::new();

/// Opened on first use (first keystroke), not at activation (R4).
fn user_store() -> Option<Arc<UserStore>> {
    USER_STORE
        .get_or_init(|| {
            let dir = t3a_paths::user_store_dir();
            let read_only = t3a_paths::is_app_container();
            UserStore::open(&dir, read_only)
                .or_else(|_| UserStore::open(&dir, true))
                .ok()
                .map(Arc::new)
        })
        .clone()
}

fn with_scorer<R>(f: impl FnOnce(&dyn UserScorer) -> R) -> R {
    match user_store() {
        Some(store) => f(store.as_ref()),
        None => f(&NoUser),
    }
}

/// The user config and its modification time (docs/13). Missing or unreadable ⇒ defaults.
fn load_config() -> (Config, Option<std::time::SystemTime>) {
    let path = t3a_paths::config_path();
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    match std::fs::read_to_string(&path) {
        Ok(text) => (Config::parse(&text).0, mtime),
        Err(_) => (Config::default(), mtime),
    }
}

fn keymap(c: &Config) -> KeyMap {
    KeyMap::from_config(
        &c.key_commit_latin,
        &c.key_open_tashkeel,
        &c.key_commit_harakat,
    )
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

// ---------------------------------------------------------------------------------------------
// State

/// What Backspace can undo right after a commit (docs/02 §12.2).
#[derive(Clone, Debug)]
struct ReEditAnchor {
    latin: String,
    word: String,
    trailing_space: bool,
    rank: usize,
}

pub struct State {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    secure_mode: bool,
    /// The popup's Settings button may start the Settings app: not on the secure desktop, not in
    /// AppContainer apps (docs/02 §14). Set at activation.
    settings_allowed: bool,
    cookie_thread_mgr: u32,
    cookie_thread_focus: u32,
    key_sink: bool,
    preserved_toggle: bool,
    attr_input: i32,
    attr_tashkeel: i32,
    session: Session<'static>,
    config: Config,
    config_mtime: Option<std::time::SystemTime>,
    keys: KeyMap,
    arabic_mode: bool,
    composition: Option<ITfComposition>,
    /// Context of the current composition: mouse clicks on the popup arrive outside any key event.
    context: Option<ITfContext>,
    selected: usize,
    page: usize,
    tashkeel: Option<TashkeelEditor>,
    anchor: Option<ReEditAnchor>,
    recent_words: Vec<String>,
    eaten: [bool; 256],
    popup: Option<PopupWindow>,
    text_rect: RECT,
    /// Edit sessions queued asynchronously and not yet run. While > 0, new sessions are queued too,
    /// so document edits always apply in request order (docs/02 §8).
    pending_async: u32,
}

impl State {
    fn composing(&self) -> bool {
        !self.session.is_empty() || self.tashkeel.is_some()
    }

    fn page_size(&self) -> usize {
        self.config.candidates_per_page.clamp(5, 9) as usize
    }

    fn preview_text(&self) -> String {
        if let Some(ed) = &self.tashkeel {
            return ed.render();
        }
        let list = self.session.candidates();
        if self.config.inline_preview == "latin" {
            return list.raw.clone();
        }
        list.items
            .get(self.selected)
            .or_else(|| list.items.first())
            .map_or_else(|| list.raw.clone(), |c| c.text.clone())
    }

    fn preview_op(&self) -> DocOp {
        DocOp::Preview
    }

    fn popup_model(&self) -> PopupModel {
        if let Some(ed) = &self.tashkeel {
            return PopupModel::Tashkeel(TashkeelModel {
                picks: ed.picks.clone(),
                from_typing: ed.from_typing,
                highlighted_pick: ed.highlighted_pick,
                word: ed.render(),
                focused_letter: ed.focused_letter,
                selected: ed.selection(),
            });
        }
        let list = self.session.candidates();
        let ps = self.page_size();
        let total = list.items.len();
        let pages = total.div_ceil(ps).max(1);
        let page = self.page.min(pages - 1);
        let start = page * ps;
        let end = (start + ps).min(total);
        let rows = list.items[start..end]
            .iter()
            .map(|c| Row {
                text: c.text.clone(),
                marker: match c.kind {
                    CandidateKind::Completion => RowMarker::Completion,
                    CandidateKind::Custom => RowMarker::Custom,
                    CandidateKind::RawLatin => RowMarker::RawLatin,
                    _ => RowMarker::None,
                },
                rtl: c.kind != CandidateKind::RawLatin,
            })
            .collect();
        let footer = if pages > 1 {
            Footer::Paging {
                page: (page + 1).min(255) as u8,
                pages: pages.min(255) as u8,
            }
        } else if self.config.footer_hints == "never" {
            Footer::Hidden
        } else {
            Footer::Hints
        };
        let badge = self.config.show_dialect_badge.then(|| {
            t3a_engine::dialect::argmax(&self.session.dialect())
                .arabic_name()
                .to_string()
        });
        PopupModel::List(ListModel {
            latin: list.raw.clone(),
            badge,
            rows,
            highlighted: self.selected.saturating_sub(start),
            footer,
            settings: self.settings_allowed,
        })
    }
}

/// A document change, executed inside an edit session by `apply`.
enum DocOp {
    /// Show the *current* composing word (starts the composition if needed) and place the popup.
    /// The text is read from state when the session runs, never captured at request time: an edit
    /// session may run asynchronously after a later commit, and a captured preview would then
    /// reopen a composition with stale text (docs/02 §8).
    Preview,
    /// Replace the composition (if any) with `text`, end it, then insert `suffix` after it.
    Commit { text: Vec<u16>, suffix: Vec<u16> },
    /// End the composition keeping whatever it shows (focus change, deactivation).
    Finalize,
    /// Insert text at the caret (no composition).
    Insert(Vec<u16>),
    /// If the text before the caret is `expect`, turn it back into a composition showing the
    /// restored preview; otherwise behave like a plain Backspace.
    ReEdit { expect: Vec<u16> },
}

// ---------------------------------------------------------------------------------------------
// COM object

#[implement(
    ITfTextInputProcessor,
    ITfTextInputProcessorEx,
    ITfThreadMgrEventSink,
    ITfThreadFocusSink,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfDisplayAttributeProvider,
    ITfFnConfigure
)]
pub struct TextService {
    state: RefCell<State>,
}

impl TextService {
    pub fn new() -> windows::core::Result<Self> {
        let (config, config_mtime) = load_config();
        let keys = keymap(&config);
        let session = Session::new(engine(), config.to_engine_settings());
        add_object();
        Ok(Self {
            state: RefCell::new(State {
                thread_mgr: None,
                client_id: 0,
                secure_mode: false,
                settings_allowed: false,
                cookie_thread_mgr: TF_INVALID_COOKIE,
                cookie_thread_focus: TF_INVALID_COOKIE,
                key_sink: false,
                preserved_toggle: false,
                attr_input: 0,
                attr_tashkeel: 0,
                session,
                config,
                config_mtime,
                keys,
                arabic_mode: true,
                composition: None,
                context: None,
                selected: 0,
                page: 0,
                tashkeel: None,
                anchor: None,
                recent_words: Vec::new(),
                eaten: [false; 256],
                popup: None,
                text_rect: RECT::default(),
                pending_async: 0,
            }),
        })
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        release_object();
    }
}

fn toggle_key() -> TF_PRESERVEDKEY {
    TF_PRESERVEDKEY {
        uVKey: VK_SPACE.0 as u32,
        uModifiers: TF_MOD_CONTROL,
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(
        &self,
        ptim: windows_core::Ref<'_, ITfThreadMgr>,
        tid: u32,
    ) -> windows::core::Result<()> {
        ITfTextInputProcessorEx_Impl::ActivateEx(self, ptim, tid, 0)
    }

    fn Deactivate(&self) -> windows::core::Result<()> {
        guard(Ok(()), || {
            self.finalize_composition();
            let (tm, tid, c_mgr, c_focus, key_sink, preserved, popup) = {
                let Ok(mut s) = self.state.try_borrow_mut() else {
                    return Ok(());
                };
                s.anchor = None;
                s.recent_words.clear();
                let out = (
                    s.thread_mgr.take(),
                    s.client_id,
                    s.cookie_thread_mgr,
                    s.cookie_thread_focus,
                    s.key_sink,
                    s.preserved_toggle,
                    s.popup.take(),
                );
                s.cookie_thread_mgr = TF_INVALID_COOKIE;
                s.cookie_thread_focus = TF_INVALID_COOKIE;
                s.key_sink = false;
                s.preserved_toggle = false;
                out
            };
            drop(popup); // destroys the window on this (its owning) thread
            if let Some(tm) = tm {
                // SAFETY: plain COM calls on the thread manager we were activated with.
                unsafe {
                    if let Ok(source) = tm.cast::<ITfSource>() {
                        for cookie in [c_mgr, c_focus] {
                            if cookie != TF_INVALID_COOKIE {
                                let _ = source.UnadviseSink(cookie);
                            }
                        }
                    }
                    if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                        if preserved {
                            let guid = GUID::from_u128(GUID_PRESERVED_TOGGLE);
                            let _ = km.UnpreserveKey(&guid, &toggle_key());
                        }
                        if key_sink {
                            let _ = km.UnadviseKeyEventSink(tid);
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(
        &self,
        ptim: windows_core::Ref<'_, ITfThreadMgr>,
        tid: u32,
        dwflags: u32,
    ) -> windows::core::Result<()> {
        guard(Ok(()), || {
            let tm = ptim.ok()?.clone();
            let use_ctrl_space = {
                let Ok(mut s) = self.state.try_borrow_mut() else {
                    return Err(E_FAIL.into());
                };
                s.thread_mgr = Some(tm.clone());
                s.client_id = tid;
                s.secure_mode = (dwflags & TF_TMAE_SECUREMODE) != 0;
                s.settings_allowed = !s.secure_mode && !t3a_paths::is_app_container();
                s.arabic_mode = true;
                Toggle::parse(&s.config.mode_toggle) == Toggle::CtrlSpace
            };

            let mut cookies = (TF_INVALID_COOKIE, TF_INVALID_COOKIE);
            let mut key_sink = false;
            let mut preserved = false;
            let mut atoms = (0i32, 0i32);
            // SAFETY: plain COM calls; no state borrow is held.
            unsafe {
                if let Ok(source) = tm.cast::<ITfSource>() {
                    let unk: windows::core::IUnknown = self.to_interface();
                    if let Ok(c) = source.AdviseSink(&ITfThreadMgrEventSink::IID, &unk) {
                        cookies.0 = c;
                    }
                    if let Ok(c) = source.AdviseSink(&ITfThreadFocusSink::IID, &unk) {
                        cookies.1 = c;
                    }
                }
                if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                    let sink: ITfKeyEventSink = self.to_interface();
                    key_sink = km.AdviseKeyEventSink(tid, &sink, true).is_ok();
                    if use_ctrl_space {
                        let guid = GUID::from_u128(GUID_PRESERVED_TOGGLE);
                        let desc = utf16("Type3arabi Arabic/Latin");
                        preserved = km.PreserveKey(tid, &guid, &toggle_key(), &desc).is_ok();
                    }
                }
                if let Ok(cat) = CoCreateInstance::<_, ITfCategoryMgr>(
                    &CLSID_TF_CategoryMgr,
                    None,
                    CLSCTX_INPROC_SERVER,
                ) {
                    atoms.0 = cat.RegisterGUID(&GUID_ATTR_INPUT).unwrap_or(0) as i32;
                    atoms.1 = cat.RegisterGUID(&GUID_ATTR_TASHKEEL).unwrap_or(0) as i32;
                }
            }

            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.cookie_thread_mgr = cookies.0;
                s.cookie_thread_focus = cookies.1;
                s.key_sink = key_sink;
                s.preserved_toggle = preserved;
                s.attr_input = atoms.0;
                s.attr_tashkeel = atoms.1;
            }
            Ok(())
        })
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(
        &self,
        _: windows_core::Ref<'_, ITfDocumentMgr>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnUninitDocumentMgr(
        &self,
        _: windows_core::Ref<'_, ITfDocumentMgr>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnSetFocus(
        &self,
        _focus: windows_core::Ref<'_, ITfDocumentMgr>,
        _prev: windows_core::Ref<'_, ITfDocumentMgr>,
    ) -> windows::core::Result<()> {
        guard(Ok(()), || {
            self.finalize_composition();
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.anchor = None;
                s.recent_words.clear();
            }
            Ok(())
        })
    }
    fn OnPushContext(&self, _: windows_core::Ref<'_, ITfContext>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPopContext(&self, _: windows_core::Ref<'_, ITfContext>) -> windows::core::Result<()> {
        Ok(())
    }
}

impl ITfThreadFocusSink_Impl for TextService_Impl {
    fn OnSetThreadFocus(&self) -> windows::core::Result<()> {
        guard(Ok(()), || {
            self.show_popup();
            Ok(())
        })
    }
    fn OnKillThreadFocus(&self) -> windows::core::Result<()> {
        guard(Ok(()), || {
            self.hide_popup();
            Ok(())
        })
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _fforeground: BOOL) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        pic: windows_core::Ref<'_, ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        guard(Ok(BOOL::from(false)), || {
            let (decision, key) = self.decide(pic.as_ref(), wparam, lparam);
            if !decision.eat && key != Key::Modifier {
                self.drop_anchor();
            }
            Ok(BOOL::from(decision.eat))
        })
    }

    fn OnKeyDown(
        &self,
        pic: windows_core::Ref<'_, ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        guard(Ok(BOOL::from(false)), || {
            let (decision, key) = self.decide(pic.as_ref(), wparam, lparam);
            if decision.action != Action::ReEdit && key != Key::Modifier {
                self.drop_anchor();
            }
            let Some(ctx) = pic.as_ref() else {
                return Ok(BOOL::from(false));
            };
            if !decision.eat {
                return Ok(BOOL::from(false));
            }
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.context = Some(ctx.clone());
            }
            let eaten = self.execute(ctx, decision.action);
            if eaten {
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.eaten[wparam.0 & 0xFF] = true;
                }
            }
            Ok(BOOL::from(eaten))
        })
    }

    fn OnTestKeyUp(
        &self,
        _pic: windows_core::Ref<'_, ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        guard(Ok(BOOL::from(false)), || {
            let eaten = self
                .state
                .try_borrow()
                .map(|s| s.eaten[wparam.0 & 0xFF])
                .unwrap_or(false);
            Ok(BOOL::from(eaten))
        })
    }

    fn OnKeyUp(
        &self,
        _pic: windows_core::Ref<'_, ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        guard(Ok(BOOL::from(false)), || {
            let eaten = match self.state.try_borrow_mut() {
                Ok(mut s) => std::mem::replace(&mut s.eaten[wparam.0 & 0xFF], false),
                Err(_) => false,
            };
            Ok(BOOL::from(eaten))
        })
    }

    fn OnPreservedKey(
        &self,
        pic: windows_core::Ref<'_, ITfContext>,
        rguid: *const GUID,
    ) -> windows::core::Result<BOOL> {
        guard(Ok(BOOL::from(false)), || {
            // SAFETY: TSF passes a valid GUID pointer; checked for null.
            if rguid.is_null() || unsafe { *rguid } != GUID::from_u128(GUID_PRESERVED_TOGGLE) {
                return Ok(BOOL::from(false));
            }
            let composing = self
                .state
                .try_borrow()
                .map(|s| s.composing())
                .unwrap_or(false);
            match (composing, pic.as_ref()) {
                (true, Some(ctx)) => {
                    self.execute(ctx, Action::CommitThenToggle);
                }
                _ => self.toggle_mode(),
            }
            Ok(BOOL::from(true))
        })
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(
        &self,
        ecwrite: u32,
        pcomposition: windows_core::Ref<'_, ITfComposition>,
    ) -> windows::core::Result<()> {
        guard(Ok(()), || {
            // The host ended our composition (click elsewhere, app command…). The text it shows
            // stays; drop our side of it. No learning (docs/02 §8).
            let ctx_attr = pcomposition.as_ref().and_then(|c| {
                // SAFETY: COM calls on the composition being terminated, inside its edit cookie.
                unsafe {
                    let range = c.GetRange().ok()?;
                    let ctx = range.GetContext().ok()?;
                    Some((ctx, range))
                }
            });
            if let Some((ctx, range)) = ctx_attr {
                // SAFETY: ecwrite is the write cookie TSF granted for this callback.
                unsafe { clear_attr(&ctx, ecwrite, &range) };
            }
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.composition = None;
                s.session.reset();
                s.tashkeel = None;
                s.selected = 0;
                s.page = 0;
            }
            self.hide_popup();
            Ok(())
        })
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> windows::core::Result<IEnumTfDisplayAttributeInfo> {
        guard(
            Err(E_FAIL.into()),
            || Ok(EnumDisplayAttributeInfo::create()),
        )
    }

    fn GetDisplayAttributeInfo(
        &self,
        guid: *const GUID,
    ) -> windows::core::Result<ITfDisplayAttributeInfo> {
        guard(Err(E_FAIL.into()), || {
            if guid.is_null() {
                return Err(E_INVALIDARG.into());
            }
            // SAFETY: checked non-null.
            DisplayAttributeInfo::lookup(unsafe { &*guid }).ok_or_else(|| E_INVALIDARG.into())
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Configuration entry point: Windows' keyboard options ("Options"/"Properties" of the keyboard in
// the language settings and the Text Services dialog) call ITfFnConfigure::Show (docs/05 §9).

impl ITfFunction_Impl for TextService_Impl {
    fn GetDisplayName(&self) -> windows::core::Result<windows::core::BSTR> {
        guard(Err(E_FAIL.into()), || {
            Ok(windows::core::BSTR::from("Type3arabi Settings"))
        })
    }
}

impl ITfFnConfigure_Impl for TextService_Impl {
    fn Show(
        &self,
        _hwndparent: windows::Win32::Foundation::HWND,
        _langid: u16,
        _rguidprofile: *const GUID,
    ) -> windows::core::Result<()> {
        guard(Err(E_FAIL.into()), || {
            // docs/02 §14: never start a process from a sandboxed app or the secure desktop.
            let secure = self
                .state
                .try_borrow()
                .map(|s| s.secure_mode)
                .unwrap_or(true);
            if secure || t3a_paths::is_app_container() {
                return Err(windows::Win32::Foundation::E_NOTIMPL.into());
            }
            if open_settings_app() {
                Ok(())
            } else {
                Err(E_FAIL.into())
            }
        })
    }
}

/// Start the Settings app installed next to this DLL (`<install>\x64\t3a_tip.dll` →
/// `<install>\Type3arabi Settings.exe`). Runs in the configuring process, never on the key path.
fn open_settings_app() -> bool {
    let Some(dll) = dll::dll_path() else {
        return false;
    };
    let Some(exe) = dll
        .ancestors()
        .skip(1)
        .take(2)
        .map(|d| d.join("Type3arabi Settings.exe"))
        .find(|p| p.is_file())
    else {
        return false;
    };
    let wide: Vec<u16> = exe
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: NUL-terminated path; ShellExecuteW has no other preconditions.
    let h = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            windows::core::w!("open"),
            windows::core::PCWSTR(wide.as_ptr()),
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    h.0 as usize > 32
}

// ---------------------------------------------------------------------------------------------
// Key handling

impl TextService_Impl {
    /// Pure routing decision (docs/02 §5.1): identical for OnTestKeyDown and OnKeyDown.
    fn decide(&self, ctx: Option<&ITfContext>, wparam: WPARAM, lparam: LPARAM) -> (Decision, Key) {
        // SAFETY: GetMessageExtraInfo has no preconditions.
        if unsafe { GetMessageExtraInfo() }.0 as usize == crate::ids::REINJECT_MAGIC {
            return (PASS, Key::Other);
        }
        let Ok(arabic) = self.state.try_borrow().map(|s| s.arabic_mode) else {
            return (PASS, Key::Other);
        };
        let mode = evaluate_context_mode(ctx, arabic);
        let (key, mods) = translate_key(wparam, lparam);
        let Ok(s) = self.state.try_borrow() else {
            return (PASS, key);
        };
        let raw = s.session.candidates().raw.to_ascii_lowercase();
        let rs = RouterState {
            context: mode,
            composing: s.composing(),
            popup: if s.tashkeel.is_some() {
                Popup::Tashkeel
            } else if s.composing() {
                Popup::List
            } else {
                Popup::Hidden
            },
            reedit_anchor: s.anchor.is_some() && s.config.reedit_backspace,
            buffer_is_article: s.config.article_joining
                && matches!(raw.as_str(), "el" | "al" | "il" | "l"),
            toggle: Toggle::parse(&s.config.mode_toggle),
            keys: s.keys,
        };
        (classify(&rs, key, mods), key)
    }

    fn drop_anchor(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.anchor = None;
        }
    }

    fn toggle_mode(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.arabic_mode = !s.arabic_mode;
        }
    }

    /// Execute an eaten key's action. Returns whether the key stays eaten (false = let the host
    /// also process it, used for commit-then-pass keys like Enter/arrows/Ctrl combos).
    fn execute(&self, ctx: &ITfContext, action: Action) -> bool {
        match action {
            Action::Pass => false,
            Action::AppendChar(c) => {
                self.push_char(ctx, InputChar::new(c));
                true
            }
            Action::AppendLiteralDigit(d) => {
                self.push_char(ctx, InputChar::numpad(d));
                true
            }
            Action::ArticleHyphen => true,
            Action::Backspace => {
                let op = {
                    let Ok(mut s) = self.state.try_borrow_mut() else {
                        return false;
                    };
                    with_scorer(|u| s.session.pop(u).is_some());
                    s.selected = 0;
                    s.page = 0;
                    if s.session.is_empty() {
                        None
                    } else {
                        Some(s.preview_op())
                    }
                };
                match op {
                    Some(op) => self.run(ctx, op),
                    None => {
                        self.hide_popup();
                        self.run(
                            ctx,
                            DocOp::Commit {
                                text: Vec::new(),
                                suffix: Vec::new(),
                            },
                        );
                    }
                }
                true
            }
            Action::ReEdit => self.reedit(ctx),
            Action::NextCandidate | Action::PrevCandidate | Action::NextPage | Action::PrevPage => {
                let op = {
                    let Ok(mut s) = self.state.try_borrow_mut() else {
                        return false;
                    };
                    let total = s.session.candidates().items.len();
                    if total == 0 {
                        return true;
                    }
                    let ps = s.page_size();
                    match action {
                        Action::NextCandidate => s.selected = (s.selected + 1) % total,
                        Action::PrevCandidate => s.selected = (s.selected + total - 1) % total,
                        Action::NextPage if (s.page + 1) * ps < total => {
                            s.selected = (s.page + 1) * ps
                        }
                        Action::PrevPage if s.page > 0 => s.selected = (s.page - 1) * ps,
                        _ => {}
                    }
                    s.page = s.selected / ps;
                    s.preview_op()
                };
                self.run(ctx, op);
                true
            }
            Action::CommitSpace => {
                self.commit(ctx, CommitHow::Space, None);
                true
            }
            Action::CommitNoSpace => {
                self.commit(ctx, CommitHow::Enter, None);
                true
            }
            Action::CommitWithHarakat => {
                self.commit(ctx, CommitHow::WithHarakat, None);
                true
            }
            Action::CommitRaw | Action::CommitRawSpace => {
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.tashkeel = None;
                    let items = &s.session.candidates().items;
                    s.selected = items
                        .iter()
                        .position(|c| c.kind == CandidateKind::RawLatin)
                        .unwrap_or(items.len().saturating_sub(1));
                }
                let how = if action == Action::CommitRawSpace {
                    CommitHow::Space
                } else {
                    CommitHow::Enter
                };
                self.commit(ctx, how, None);
                true
            }
            Action::CommitThenPunctuation(c) => {
                self.commit(ctx, CommitHow::Punctuation, Some(c));
                true
            }
            Action::CommitAndReinject => {
                self.commit(ctx, CommitHow::Enter, None);
                false // the host now receives the key itself (docs/02 §5.3, pass-through variant)
            }
            Action::InsertLatin(c) => {
                self.run(ctx, DocOp::Insert(utf16(c.encode_utf8(&mut [0; 4]))));
                true
            }
            Action::InsertPunctuation(c) => {
                let p = self
                    .state
                    .try_borrow()
                    .map(|s| t3a_engine::display::punctuation(c, s.config.arabic_punctuation))
                    .unwrap_or(c);
                self.run(ctx, DocOp::Insert(utf16(p.encode_utf8(&mut [0; 4]))));
                true
            }
            Action::ToggleMode => {
                self.toggle_mode();
                true
            }
            Action::CommitThenToggle => {
                self.commit(ctx, CommitHow::Enter, None);
                self.toggle_mode();
                true
            }
            Action::OpenTashkeel => {
                let op = {
                    let Ok(mut s) = self.state.try_borrow_mut() else {
                        return false;
                    };
                    let idx = s.selected;
                    let Some(c) = s.session.candidates().items.get(idx) else {
                        return true;
                    };
                    if c.kind == CandidateKind::RawLatin {
                        return true;
                    }
                    let word = c.text.clone();
                    let vh = s.session.vowel_harakat(idx);
                    let picks = s.session.vocalizations(idx);
                    let from_typing = vh.is_some() && picks.first() == vh.as_ref();
                    s.tashkeel = Some(TashkeelEditor::new(&word, picks, from_typing));
                    s.preview_op()
                };
                self.run(ctx, op);
                true
            }
            Action::Tashkeel(cmd) => {
                let result = {
                    let Ok(mut s) = self.state.try_borrow_mut() else {
                        return false;
                    };
                    let Some(ed) = s.tashkeel.as_mut() else {
                        return true;
                    };
                    match ed.apply_cmd(cmd) {
                        TashkeelAction::Continue => Some(s.preview_op()),
                        TashkeelAction::BackToList => {
                            s.tashkeel = None;
                            Some(s.preview_op())
                        }
                        TashkeelAction::Commit(_) => None,
                    }
                };
                match result {
                    Some(op) => self.run(ctx, op),
                    None => self.commit(ctx, CommitHow::Enter, None),
                }
                true
            }
        }
    }

    /// Mouse input on the popup (docs/05 §3.4, §4.4): mapped onto the same actions as the keys.
    fn on_popup_event(&self, ev: PopupEvent) {
        if ev == PopupEvent::Settings {
            // Mouse click, not the key path; the button is drawn only where starting a process is allowed.
            let allowed = self.state.try_borrow().is_ok_and(|s| s.settings_allowed);
            if allowed && !open_settings_app() {
                t3a_paths::log_error("popup: could not start the Settings app");
            }
            return;
        }
        let mut select = None;
        let (ctx, action) = {
            let Ok(s) = self.state.try_borrow() else {
                return;
            };
            let Some(ctx) = s.context.clone() else {
                return;
            };
            let action = match ev {
                PopupEvent::WheelUp => Some(Action::PrevCandidate),
                PopupEvent::WheelDown => Some(Action::NextCandidate),
                PopupEvent::Row(row) => {
                    // Row n of the visible page → candidate index; commit it like Space.
                    let idx = s.page * s.page_size() + row;
                    if idx < s.session.candidates().items.len() {
                        select = Some(idx);
                        Some(Action::CommitSpace)
                    } else {
                        None
                    }
                }
                PopupEvent::Letter {
                    index,
                    toggle,
                    range,
                } => {
                    let mode = if range {
                        SelectMode::Range
                    } else if toggle {
                        SelectMode::Toggle
                    } else {
                        SelectMode::Only
                    };
                    Some(Action::Tashkeel(TashkeelCmd::Select(
                        index.min(u8::MAX as usize) as u8,
                        mode,
                    )))
                }
                PopupEvent::Mark(i) => {
                    let cmd = [
                        TashkeelCmd::Fatha,
                        TashkeelCmd::Damma,
                        TashkeelCmd::Kasra,
                        TashkeelCmd::Sukun,
                        TashkeelCmd::ShaddaToggle,
                        TashkeelCmd::Fathatan,
                        TashkeelCmd::Dammatan,
                        TashkeelCmd::Kasratan,
                        TashkeelCmd::DaggerAlif,
                        TashkeelCmd::Clear,
                    ];
                    cmd.get(i).map(|&c| Action::Tashkeel(c))
                }
                PopupEvent::ClearAll => Some(Action::Tashkeel(TashkeelCmd::ClearAll)),
                PopupEvent::Pick(i) => Some(Action::Tashkeel(TashkeelCmd::QuickPick(
                    (i + 1).min(u8::MAX as usize) as u8,
                ))),
                PopupEvent::Settings => None,
            };
            (ctx, action)
        };
        if let (Some(idx), Ok(mut s)) = (select, self.state.try_borrow_mut()) {
            s.selected = idx;
        }
        if let Some(action) = action {
            self.execute(&ctx, action);
        }
    }

    fn push_char(&self, ctx: &ITfContext, ch: InputChar) {
        let op = {
            let Ok(mut s) = self.state.try_borrow_mut() else {
                return;
            };
            if s.session.is_empty() {
                s.anchor = None;
                // Pick up Settings changes (shortcuts, style) at the start of each word.
                let mtime = std::fs::metadata(t3a_paths::config_path())
                    .and_then(|m| m.modified())
                    .ok();
                if mtime != s.config_mtime {
                    let (config, mtime) = load_config();
                    s.keys = keymap(&config);
                    s.session.set_settings(config.to_engine_settings());
                    s.config = config;
                    s.config_mtime = mtime;
                }
                let prev = s.recent_words.clone();
                let refs: Vec<&str> = prev.iter().map(String::as_str).collect();
                s.session.set_context(&refs);
                if let Some(store) = user_store() {
                    store.sync(); // pick up other processes' learning once per word
                }
            }
            with_scorer(|u| {
                s.session.push(ch, u);
            });
            s.selected = 0;
            s.page = 0;
            if s.session.is_empty() {
                return; // character not accepted by the engine
            }
            s.preview_op()
        };
        self.run(ctx, op);
    }

    /// Commit the highlighted candidate (or the tashkeel editor's word), then `suffix`.
    fn commit(&self, ctx: &ITfContext, how: CommitHow, punct: Option<char>) {
        let op = {
            let Ok(mut s) = self.state.try_borrow_mut() else {
                return;
            };
            if !s.composing() {
                return;
            }
            let idx = s.selected;
            let list = s.session.candidates();
            let latin = list.raw.clone();
            let top = list.items.first().map(|c| c.base.clone());
            let picked_base = list.items.get(idx).map(|c| c.base.clone());
            let list_key = list.key.clone();
            let (text, key, base, trailing_space, learnable) = match s.tashkeel.take() {
                Some(ed) => {
                    let base = picked_base;
                    let key = list_key;
                    s.session.reset();
                    (
                        ed.render(),
                        key,
                        base.clone().unwrap_or_default(),
                        how == CommitHow::Space,
                        base.is_some(),
                    )
                }
                None => {
                    let c = s.session.commit(idx, how);
                    let space = c.trailing == Trailing::Space;
                    (c.text, c.key, c.base, space, c.learnable)
                }
            };
            if learnable && s.config.learning_enabled && !s.secure_mode && !key.is_empty() {
                if let Some(store) = user_store() {
                    if !store.is_read_only() {
                        store.record(&key, &base, idx, idx > 0, top.as_deref());
                    }
                }
            }

            let mut suffix = String::new();
            if trailing_space {
                suffix.push(' ');
            }
            if let Some(c) = punct {
                suffix.push(t3a_engine::display::punctuation(
                    c,
                    s.config.arabic_punctuation,
                ));
            }
            s.anchor = punct.is_none().then(|| ReEditAnchor {
                latin,
                word: text.clone(),
                trailing_space,
                rank: idx,
            });
            if text.chars().any(|c| ('\u{0621}'..='\u{064A}').contains(&c)) {
                s.recent_words.push(t3a_engine::arabic::strip_marks(&text));
                let excess = s.recent_words.len().saturating_sub(2);
                s.recent_words.drain(..excess);
            }
            s.selected = 0;
            s.page = 0;
            DocOp::Commit {
                text: utf16(&text),
                suffix: utf16(&suffix),
            }
        };
        self.hide_popup();
        self.run(ctx, op);
    }

    /// Backspace right after a commit (docs/02 §12.2).
    fn reedit(&self, ctx: &ITfContext) -> bool {
        let op = {
            let Ok(mut s) = self.state.try_borrow_mut() else {
                return false;
            };
            let Some(anchor) = s.anchor.take() else {
                return false;
            };
            if anchor.trailing_space {
                // Step 1: the host deletes the space itself; keep the anchor for the next Backspace.
                s.anchor = Some(ReEditAnchor {
                    trailing_space: false,
                    ..anchor
                });
                return false;
            }
            with_scorer(|u| {
                s.session.restore(&anchor.latin, u);
            });
            let total = s.session.candidates().items.len();
            s.selected = anchor.rank.min(total.saturating_sub(1));
            s.page = s.selected / s.page_size();
            s.tashkeel = None;
            DocOp::ReEdit {
                expect: utf16(&anchor.word),
            }
        };
        self.run(ctx, op);
        true
    }

    /// End the current composition keeping its text (focus change / deactivation). No learning.
    fn finalize_composition(&self) {
        let comp = match self.state.try_borrow_mut() {
            Ok(mut s) => {
                s.session.reset();
                s.tashkeel = None;
                s.selected = 0;
                s.page = 0;
                s.composition.clone()
            }
            Err(_) => None,
        };
        self.hide_popup();
        let ctx = comp.and_then(|c| {
            // SAFETY: COM queries on our own live composition.
            unsafe { c.GetRange().and_then(|r| r.GetContext()).ok() }
        });
        if let Some(ctx) = ctx {
            self.run(&ctx, DocOp::Finalize);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Popup

    fn hide_popup(&self) {
        let popup = self
            .state
            .try_borrow_mut()
            .ok()
            .and_then(|mut s| s.popup.take());
        if let Some(mut p) = popup {
            p.hide();
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.popup = Some(p);
            }
        }
    }

    fn show_popup(&self) {
        let (model, rect, popup) = {
            let Ok(mut s) = self.state.try_borrow_mut() else {
                return;
            };
            if !s.composing() {
                return;
            }
            let model = s.popup_model();
            (model, s.text_rect, s.popup.take())
        };
        let Some(mut popup) = popup.or_else(|| {
            let mut p = PopupWindow::new().ok()?;
            let this = self.to_object();
            p.set_handler(std::rc::Rc::new(move |ev| {
                guard((), || this.on_popup_event(ev));
            }));
            Some(p)
        }) else {
            return;
        };
        popup.show(model, (rect.left, rect.top, rect.right, rect.bottom));
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.popup = Some(popup);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Edit sessions

    fn run(&self, ctx: &ITfContext, op: DocOp) {
        let Ok((tid, pending)) = self
            .state
            .try_borrow()
            .map(|s| (s.client_id, s.pending_async))
        else {
            return;
        };
        let this = self.to_object();
        let ctx2 = ctx.clone();
        let queued = std::rc::Rc::new(std::cell::Cell::new(false));
        let queued2 = queued.clone();
        let session: ITfEditSession = EditSession::new(move |ec| {
            if queued2.get() {
                if let Ok(mut s) = this.state.try_borrow_mut() {
                    s.pending_async = s.pending_async.saturating_sub(1);
                }
            }
            apply(&this, &ctx2, ec, op)
        })
        .into();
        // Order is everything: engine state already changed, and each edit must land in request
        // order (a late session applying after a later commit corrupts text — reproduced under load).
        // So: synchronous when nothing is queued; otherwise, or when TSF refuses a sync lock
        // (TF_E_SYNCHRONOUS, e.g. popup clicks), queue with TF_ES_ASYNC — never ASYNCDONTCARE,
        // which may run at once and overtake the sessions already queued. TSF runs its queue FIFO.
        // SAFETY: plain COM calls; no state borrow is held (the session may run synchronously).
        unsafe {
            if pending == 0 {
                let sync = ctx.RequestEditSession(tid, &session, TF_ES_SYNC | TF_ES_READWRITE);
                if matches!(sync, Ok(hr) if hr.is_ok()) {
                    return;
                }
            }
            queued.set(true);
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.pending_async += 1;
            }
            let hr = ctx.RequestEditSession(tid, &session, TF_ES_ASYNC | TF_ES_READWRITE);
            if !matches!(hr, Ok(h) if h.is_ok()) {
                queued.set(false);
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.pending_async = s.pending_async.saturating_sub(1);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Document operations (run inside an edit session with cookie `ec`)

fn apply(
    this: &TextService_Impl,
    ctx: &ITfContext,
    ec: u32,
    op: DocOp,
) -> windows::core::Result<()> {
    // SAFETY (whole fn): COM calls on live TSF objects inside a granted read/write edit session.
    unsafe {
        match op {
            DocOp::Preview => {
                let (comp, atom, text) = match this.state.try_borrow() {
                    // Nothing to show any more (committed or cancelled since the request).
                    Ok(s) if !s.composing() => return Ok(()),
                    Ok(s) => (
                        s.composition.clone(),
                        if s.tashkeel.is_some() {
                            s.attr_tashkeel
                        } else {
                            s.attr_input
                        },
                        utf16(&s.preview_text()),
                    ),
                    Err(_) => return Ok(()),
                };
                let range = match comp.and_then(|c| c.GetRange().ok()) {
                    Some(r) => Some(r),
                    None => start_composition(this, ctx, ec),
                };
                match range {
                    Some(range) => {
                        range.SetText(ec, 0, &text)?;
                        set_attr(ctx, ec, &range, atom);
                        set_caret_end(ctx, ec, &range);
                        update_text_rect(this, ctx, ec, &range);
                    }
                    None => {
                        // Host refused a composition: still offer the candidate list at the caret.
                        if let Some(sel) = selection_range(ctx, ec) {
                            update_text_rect(this, ctx, ec, &sel);
                        }
                    }
                }
                this.show_popup();
            }
            DocOp::Commit { text, suffix } => {
                let comp = this
                    .state
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut s| s.composition.take());
                match comp {
                    Some(comp) => {
                        if let Ok(range) = comp.GetRange() {
                            let _ = range.SetText(ec, 0, &text);
                            clear_attr(ctx, ec, &range);
                            set_caret_end(ctx, ec, &range);
                        }
                        let _ = comp.EndComposition(ec);
                        if !suffix.is_empty() {
                            insert_at_caret(ctx, ec, &suffix)?;
                        }
                    }
                    None => {
                        let all: Vec<u16> = text.into_iter().chain(suffix).collect();
                        if !all.is_empty() {
                            insert_at_caret(ctx, ec, &all)?;
                        }
                    }
                }
            }
            DocOp::Finalize => {
                let comp = this
                    .state
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut s| s.composition.take());
                if let Some(comp) = comp {
                    if let Ok(range) = comp.GetRange() {
                        clear_attr(ctx, ec, &range);
                    }
                    let _ = comp.EndComposition(ec);
                }
            }
            DocOp::Insert(text) => insert_at_caret(ctx, ec, &text)?,
            DocOp::ReEdit { expect } => {
                let Some(sel) = selection_range(ctx, ec) else {
                    return Ok(());
                };
                let caret = sel.IsEmpty(ec).map(|b| b.as_bool()).unwrap_or(false);
                let mut restored = false;
                if caret && !expect.is_empty() {
                    let r = sel.Clone()?;
                    let mut moved = 0i32;
                    r.ShiftStart(ec, -(expect.len() as i32), &mut moved, std::ptr::null())?;
                    if moved.unsigned_abs() as usize == expect.len() {
                        let mut buf = vec![0u16; expect.len() + 1];
                        let mut got = 0u32;
                        r.GetText(ec, 0, &mut buf, &mut got)?;
                        if buf[..got as usize] == expect[..] {
                            r.SetText(ec, 0, &[])?;
                            restored = true;
                        }
                    }
                }
                if restored {
                    return apply(this, ctx, ec, DocOp::Preview);
                }
                // The text changed since the commit: undo the restore and act as plain Backspace.
                if let Ok(mut s) = this.state.try_borrow_mut() {
                    s.session.reset();
                    s.selected = 0;
                    s.page = 0;
                }
                this.hide_popup();
                if caret {
                    let r = sel.Clone()?;
                    let mut moved = 0i32;
                    r.ShiftStart(ec, -1, &mut moved, std::ptr::null())?;
                    r.SetText(ec, 0, &[])?;
                } else {
                    sel.SetText(ec, 0, &[])?;
                }
            }
        }
    }
    Ok(())
}

unsafe fn selection_range(ctx: &ITfContext, ec: u32) -> Option<ITfRange> {
    let ins: ITfInsertAtSelection = ctx.cast().ok()?;
    ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[]).ok()
}

unsafe fn start_composition(
    this: &TextService_Impl,
    ctx: &ITfContext,
    ec: u32,
) -> Option<ITfRange> {
    let at = selection_range(ctx, ec)?;
    let cc: ITfContextComposition = ctx.cast().ok()?;
    let sink: ITfCompositionSink = this.to_interface();
    let comp = cc.StartComposition(ec, &at, &sink).ok()?;
    let range = comp.GetRange().ok()?;
    match this.state.try_borrow_mut() {
        Ok(mut s) => s.composition = Some(comp),
        Err(_) => {
            let _ = comp.EndComposition(ec);
            return None;
        }
    }
    Some(range)
}

unsafe fn insert_at_caret(ctx: &ITfContext, ec: u32, text: &[u16]) -> windows::core::Result<()> {
    let ins: ITfInsertAtSelection = ctx.cast()?;
    let range = ins.InsertTextAtSelection(ec, Default::default(), text)?;
    set_caret_end(ctx, ec, &range);
    Ok(())
}

unsafe fn set_caret_end(ctx: &ITfContext, ec: u32, range: &ITfRange) {
    let Ok(r) = range.Clone() else {
        return;
    };
    if r.Collapse(ec, TF_ANCHOR_END).is_err() {
        return;
    }
    let sel = [TF_SELECTION {
        range: ManuallyDrop::new(Some(r)),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: false.into(),
        },
    }];
    let _ = ctx.SetSelection(ec, &sel);
    let [s] = sel;
    drop(ManuallyDrop::into_inner(s.range)); // release our reference
}

unsafe fn set_attr(ctx: &ITfContext, ec: u32, range: &ITfRange, atom: i32) {
    if atom == 0 {
        return;
    }
    if let Ok(prop) = ctx.GetProperty(&GUID_PROP_ATTRIBUTE) {
        let v = VARIANT::from(atom);
        let _ = prop.SetValue(ec, range, &v);
    }
}

unsafe fn clear_attr(ctx: &ITfContext, ec: u32, range: &ITfRange) {
    if let Ok(prop) = ctx.GetProperty(&GUID_PROP_ATTRIBUTE) {
        let _ = prop.Clear(ec, range);
    }
}

/// Screen rectangle of `range` for popup placement, with caret / cursor fallbacks (docs/02 §15).
unsafe fn update_text_rect(this: &TextService_Impl, ctx: &ITfContext, ec: u32, range: &ITfRange) {
    let mut rc = RECT::default();
    let mut ok = false;
    if let Ok(view) = ctx.GetActiveView() {
        let mut clipped = BOOL::from(false);
        ok = view.GetTextExt(ec, range, &mut rc, &mut clipped).is_ok()
            && (rc.bottom > rc.top || rc.right > rc.left || rc.left != 0 || rc.top != 0);
    }
    if !ok {
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(0, &mut gti).is_ok() && !gti.hwndCaret.is_invalid() {
            let mut tl = POINT {
                x: gti.rcCaret.left,
                y: gti.rcCaret.top,
            };
            let mut br = POINT {
                x: gti.rcCaret.right,
                y: gti.rcCaret.bottom,
            };
            let _ = ClientToScreen(gti.hwndCaret, &mut tl);
            let _ = ClientToScreen(gti.hwndCaret, &mut br);
            rc = RECT {
                left: tl.x,
                top: tl.y,
                right: br.x,
                bottom: br.y,
            };
        } else {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            rc = RECT {
                left: pt.x,
                top: pt.y,
                right: pt.x + 1,
                bottom: pt.y + 20,
            };
        }
    }
    if let Ok(mut s) = this.state.try_borrow_mut() {
        s.text_rect = rc;
    }
}
