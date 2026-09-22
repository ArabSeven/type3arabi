//! The main TSF TextService implementing ITfTextInputProcessorEx and all sinks (docs/02 §3, §4).

use crate::ids::GUID_PRESERVED_TOGGLE;
use crate::keyrouter::{classify, Action, Popup, RouterState, Toggle};
use crate::win::compose::EditSession;
use crate::win::context::evaluate_context_mode;
use crate::win::dll::{add_object, release_object, CLSID_TYPE3ARABI_TIP};
use crate::win::guard::guard;
use crate::win::keys::translate_key;
use std::cell::RefCell;
use std::sync::{Arc, OnceLock};
use t3a_engine::normalize::InputChar;
use t3a_engine::session::{CommitHow, Session};
use t3a_engine::{Config, Engine, UserStore};
use t3a_ui::{Footer, ListModel, PopupModel, Row, RowMarker};
use windows::core::{implement, Interface, BOOL, BSTR, GUID};
use windows::Win32::Foundation::{E_POINTER, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, ITfCompartmentEventSink, ITfCompartmentEventSink_Impl,
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfContextView,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfDocumentMgr, ITfFnConfigure, ITfFnConfigure_Impl, ITfFunction, ITfFunctionProvider,
    ITfFunctionProvider_Impl, ITfFunction_Impl, ITfInsertAtSelection, ITfKeyEventSink,
    ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfRange, ITfSource, ITfTextInputProcessor,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl, ITfTextInputProcessor_Impl,
    ITfTextLayoutSink, ITfTextLayoutSink_Impl, ITfThreadFocusSink, ITfThreadFocusSink_Impl,
    ITfThreadMgr, ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl, TfLayoutCode, TF_ANCHOR_END,
    TF_ES_ASYNCDONTCARE, TF_ES_READWRITE, TF_IAS_NOQUERY, TF_SELECTION, TF_SELECTIONSTYLE,
    TF_TMAE_SECUREMODE,
};
use windows::Win32::UI::WindowsAndMessaging::GetMessageExtraInfo;
use windows_core::IUnknownImpl;

const T3A_REINJECT_MAGIC: usize = 0x54334152; // "T3AR"

static ENGINE: OnceLock<Engine> = OnceLock::new();
fn get_engine() -> &'static Engine {
    ENGINE.get_or_init(|| {
        let candidates = [
            t3a_paths::data_file_path(),
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .unwrap_or_else(|| std::path::Path::new(""))
                .join(t3a_paths::DATA_FILE_NAME),
            std::path::PathBuf::from("target").join(t3a_paths::DATA_FILE_NAME),
            std::path::PathBuf::from(t3a_paths::DATA_FILE_NAME),
        ];
        for path in &candidates {
            if path.exists() {
                if let Ok(file) = t3a_data::DataFile::open(path) {
                    let boxed: &'static t3a_data::DataFile = Box::leak(Box::new(file));
                    if let Ok(view) = boxed.view() {
                        if let Ok(eng) = Engine::new(view) {
                            return eng;
                        }
                    }
                }
            }
        }
        Engine::builtin()
    })
}

static USER_STORE: OnceLock<Arc<UserStore>> = OnceLock::new();
fn get_user_store(secure_mode: bool) -> Arc<UserStore> {
    USER_STORE
        .get_or_init(|| {
            let is_ac = t3a_paths::is_app_container();
            let read_only = is_ac || secure_mode;
            let store_dir = t3a_paths::user_store_dir();
            match UserStore::open(&store_dir, read_only) {
                Ok(store) => Arc::new(store),
                Err(_) => Arc::new(
                    UserStore::open(std::path::Path::new(""), true).unwrap_or_else(|_| {
                        let tmp = std::env::temp_dir().join("type3arabi_tmp_store");
                        UserStore::open(&tmp, true).unwrap()
                    }),
                ),
            }
        })
        .clone()
}

fn get_config() -> Config {
    let p = t3a_paths::config_path();
    if let Ok(s) = std::fs::read_to_string(&p) {
        let (cfg, _warns) = Config::parse(&s);
        cfg
    } else {
        Config::default()
    }
}

fn reinject_key(wparam: WPARAM, lparam: LPARAM) {
    let vk = (wparam.0 & 0xFF) as u16;
    let scan = ((lparam.0 >> 16) & 0xFF) as u16;
    let ext = ((lparam.0 >> 24) & 1) != 0;
    let flags_down = if ext { 0x0001 } else { 0 }; // KEYEVENTF_EXTENDEDKEY
    let flags_up = flags_down | 0x0002; // KEYEVENTF_KEYUP

    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: KEYBD_EVENT_FLAGS(flags_down),
                    time: 0,
                    dwExtraInfo: T3A_REINJECT_MAGIC,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: KEYBD_EVENT_FLAGS(flags_up),
                    time: 0,
                    dwExtraInfo: T3A_REINJECT_MAGIC,
                },
            },
        },
    ];
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[derive(Clone, Debug)]
pub struct ReEditState {
    pub latin: String,
    pub text: String,
    pub had_trailing_space: bool,
    pub candidate_index: usize,
}

pub struct Inner {
    pub thread_mgr: Option<ITfThreadMgr>,
    pub client_id: u32,
    pub session: Session<'static>,
    pub user_store: Arc<UserStore>,
    pub config: Config,
    pub arabic_mode: bool,
    pub secure_mode: bool,
    pub active_composition: Option<ITfComposition>,
    pub composition_range: Option<ITfRange>,
    pub popup: Option<t3a_ui::PopupWindow>,
    pub eaten_keys: [bool; 256],
    pub cookie_thread_mgr: u32,
    pub cookie_thread_focus: u32,
    pub cookie_key_sink: bool,
    pub cookie_text_layout: u32,
    pub selected_index: usize,
    pub current_page: usize,
    pub last_commit: Option<ReEditState>,
    pub recent_words: Vec<String>,
    pub last_raw_key: (WPARAM, LPARAM),
}

#[implement(
    ITfTextInputProcessor,
    ITfTextInputProcessorEx,
    ITfThreadMgrEventSink,
    ITfThreadFocusSink,
    ITfTextLayoutSink,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfDisplayAttributeProvider,
    ITfCompartmentEventSink,
    ITfFunctionProvider,
    ITfFunction,
    ITfFnConfigure
)]
pub struct TextService {
    pub inner: RefCell<Inner>,
}

impl TextService {
    pub fn new() -> windows::core::Result<Self> {
        add_object();
        let popup = t3a_ui::PopupWindow::new().ok();
        let engine = get_engine();
        let config = get_config();
        let user_store = get_user_store(false);
        let session = Session::new(engine, config.to_engine_settings());

        Ok(Self {
            inner: RefCell::new(Inner {
                thread_mgr: None,
                client_id: 0,
                session,
                user_store,
                config,
                arabic_mode: true,
                secure_mode: false,
                active_composition: None,
                composition_range: None,
                popup,
                eaten_keys: [false; 256],
                cookie_thread_mgr: 0,
                cookie_thread_focus: 0,
                cookie_key_sink: false,
                cookie_text_layout: 0,
                selected_index: 0,
                current_page: 0,
                last_commit: None,
                recent_words: Vec::new(),
                last_raw_key: (WPARAM(0), LPARAM(0)),
            }),
        })
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        release_object();
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(
        &self,
        ptim: windows_core::Ref<'_, ITfThreadMgr>,
        tid: u32,
    ) -> windows::core::Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> windows::core::Result<()> {
        guard(Ok(()), || {
            let mut inner = self.inner.borrow_mut();
            if let Some(mut popup) = inner.popup.take() {
                popup.hide();
                inner.popup = Some(popup);
            }
            inner.session.reset();
            inner.active_composition = None;
            inner.composition_range = None;
            inner.last_commit = None;

            if let Some(tm) = inner.thread_mgr.take() {
                if let Ok(source) = tm.cast::<ITfSource>() {
                    if inner.cookie_thread_mgr != 0 {
                        let _ = unsafe { source.UnadviseSink(inner.cookie_thread_mgr) };
                        inner.cookie_thread_mgr = 0;
                    }
                    if inner.cookie_thread_focus != 0 {
                        let _ = unsafe { source.UnadviseSink(inner.cookie_thread_focus) };
                        inner.cookie_thread_focus = 0;
                    }
                }
                if inner.cookie_key_sink {
                    if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                        let _ = unsafe { km.UnadviseKeyEventSink(inner.client_id) };
                    }
                    inner.cookie_key_sink = false;
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
            let tm = ptim.ok()?;
            let mut inner = self.inner.borrow_mut();
            inner.thread_mgr = Some(tm.clone());
            inner.client_id = tid;
            inner.secure_mode = (dwflags & TF_TMAE_SECUREMODE) != 0;

            // Advise ThreadMgr sinks
            if let Ok(source) = tm.cast::<ITfSource>() {
                let this_unk: windows::core::IUnknown = self.to_interface();
                if let Ok(cookie) = unsafe {
                    source.AdviseSink(
                        &windows::Win32::UI::TextServices::ITfThreadMgrEventSink::IID,
                        &this_unk,
                    )
                } {
                    inner.cookie_thread_mgr = cookie;
                }

                if let Ok(cookie_focus) = unsafe {
                    source.AdviseSink(
                        &windows::Win32::UI::TextServices::ITfThreadFocusSink::IID,
                        &this_unk,
                    )
                } {
                    inner.cookie_thread_focus = cookie_focus;
                }
            }

            // Advise KeyEventSink
            if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                let this_key_sink: ITfKeyEventSink = self.to_interface();
                if unsafe { km.AdviseKeyEventSink(tid, &this_key_sink, true) }.is_ok() {
                    inner.cookie_key_sink = true;
                }

                // Register preserved key (Ctrl+Space toggle)
                let preserved_guid = GUID::from_u128(GUID_PRESERVED_TOGGLE);
                let preserved_key = windows::Win32::UI::TextServices::TF_PRESERVEDKEY {
                    uVKey: windows::Win32::UI::Input::KeyboardAndMouse::VK_SPACE.0 as u32,
                    uModifiers: windows::Win32::UI::TextServices::TF_MOD_CONTROL,
                };
                let desc: Vec<u16> = "Arabic/Latin Toggle"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let _ = unsafe { km.PreserveKey(tid, &preserved_guid, &preserved_key, &desc) };
            }

            Ok(())
        })
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(
        &self,
        _pdim: windows_core::Ref<'_, ITfDocumentMgr>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnUninitDocumentMgr(
        &self,
        _pdim: windows_core::Ref<'_, ITfDocumentMgr>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnSetFocus(
        &self,
        _pdimfocus: windows_core::Ref<'_, ITfDocumentMgr>,
        _pdimprevfocus: windows_core::Ref<'_, ITfDocumentMgr>,
    ) -> windows::core::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if let Some(popup) = &mut inner.popup {
            popup.hide();
        }
        inner.session.reset();
        inner.active_composition = None;
        inner.composition_range = None;
        inner.last_commit = None;
        Ok(())
    }
    fn OnPushContext(&self, _pic: windows_core::Ref<'_, ITfContext>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPopContext(&self, _pic: windows_core::Ref<'_, ITfContext>) -> windows::core::Result<()> {
        Ok(())
    }
}

impl ITfThreadFocusSink_Impl for TextService_Impl {
    fn OnSetThreadFocus(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnKillThreadFocus(&self) -> windows::core::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if let Some(popup) = &mut inner.popup {
            popup.hide();
        }
        inner.last_commit = None;
        Ok(())
    }
}

impl ITfTextLayoutSink_Impl for TextService_Impl {
    fn OnLayoutChange(
        &self,
        _pic: windows_core::Ref<'_, ITfContext>,
        _lcode: TfLayoutCode,
        _pview: windows_core::Ref<'_, ITfContextView>,
    ) -> windows::core::Result<()> {
        Ok(())
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
            unsafe {
                if GetMessageExtraInfo().0 as usize == T3A_REINJECT_MAGIC {
                    return Ok(BOOL::from(false));
                }
            }

            let inner = self.inner.borrow();
            let (key, mods) = translate_key(wparam, lparam);
            let ctx_mode =
                evaluate_context_mode(pic.as_ref(), inner.arabic_mode, inner.secure_mode);

            let raw = inner.session.candidates().raw.to_ascii_lowercase();
            let buffer_is_article =
                inner.config.article_joining && matches!(raw.as_str(), "el" | "al" | "il" | "l");

            let reedit_anchor = inner.last_commit.is_some() && inner.config.reedit_backspace;

            let state = RouterState {
                context: ctx_mode,
                composing: !inner.session.is_empty(),
                popup: if inner.popup.as_ref().is_some_and(|p| p.is_visible()) {
                    Popup::List
                } else {
                    Popup::Hidden
                },
                reedit_anchor,
                buffer_is_article,
                toggle: Toggle::parse(&inner.config.mode_toggle),
            };

            let decision = classify(&state, key, mods);
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
            unsafe {
                if GetMessageExtraInfo().0 as usize == T3A_REINJECT_MAGIC {
                    return Ok(BOOL::from(false));
                }
            }

            let (key, mods) = translate_key(wparam, lparam);

            let action = {
                let inner = self.inner.borrow();
                inner.user_store.sync();

                let ctx_mode =
                    evaluate_context_mode(pic.as_ref(), inner.arabic_mode, inner.secure_mode);

                let raw = inner.session.candidates().raw.to_ascii_lowercase();
                let buffer_is_article = inner.config.article_joining
                    && matches!(raw.as_str(), "el" | "al" | "il" | "l");

                let reedit_anchor = inner.last_commit.is_some() && inner.config.reedit_backspace;

                let state = RouterState {
                    context: ctx_mode,
                    composing: !inner.session.is_empty(),
                    popup: if inner.popup.as_ref().is_some_and(|p| p.is_visible()) {
                        Popup::List
                    } else {
                        Popup::Hidden
                    },
                    reedit_anchor,
                    buffer_is_article,
                    toggle: Toggle::parse(&inner.config.mode_toggle),
                };
                let decision = classify(&state, key, mods);
                if !decision.eat {
                    return Ok(BOOL::from(false));
                }
                decision.action
            };

            {
                let mut inner = self.inner.borrow_mut();
                let vk_idx = wparam.0 & 0xFF;
                inner.eaten_keys[vk_idx] = true;
                inner.last_raw_key = (wparam, lparam);
                if action != Action::ReEdit && inner.session.is_empty() {
                    inner.last_commit = None;
                }
            }

            self.execute_action(pic.as_ref(), action)?;
            Ok(BOOL::from(true))
        })
    }

    fn OnTestKeyUp(
        &self,
        _pic: windows_core::Ref<'_, ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        let inner = self.inner.borrow();
        let vk_idx = wparam.0 & 0xFF;
        Ok(BOOL::from(inner.eaten_keys[vk_idx]))
    }

    fn OnKeyUp(
        &self,
        _pic: windows_core::Ref<'_, ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> windows::core::Result<BOOL> {
        let mut inner = self.inner.borrow_mut();
        let vk_idx = wparam.0 & 0xFF;
        let eaten = inner.eaten_keys[vk_idx];
        inner.eaten_keys[vk_idx] = false;
        Ok(BOOL::from(eaten))
    }

    fn OnPreservedKey(
        &self,
        _pic: windows_core::Ref<'_, ITfContext>,
        rguid: *const GUID,
    ) -> windows::core::Result<BOOL> {
        if rguid.is_null() {
            return Err(windows::core::Error::from(E_POINTER));
        }
        let preserved_guid = GUID::from_u128(GUID_PRESERVED_TOGGLE);
        // SAFETY: rguid is verified non-null above
        if unsafe { *rguid } == preserved_guid {
            let mut inner = self.inner.borrow_mut();
            inner.arabic_mode = !inner.arabic_mode;
            Ok(BOOL::from(true))
        } else {
            Ok(BOOL::from(false))
        }
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _pcomposition: windows_core::Ref<'_, ITfComposition>,
    ) -> windows::core::Result<()> {
        let mut inner = self.inner.borrow_mut();
        inner.active_composition = None;
        inner.composition_range = None;
        inner.session.reset();
        if let Some(popup) = &mut inner.popup {
            popup.hide();
        }
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> windows::core::Result<IEnumTfDisplayAttributeInfo> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }

    fn GetDisplayAttributeInfo(
        &self,
        _guid: *const GUID,
    ) -> windows::core::Result<ITfDisplayAttributeInfo> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }
}

impl ITfCompartmentEventSink_Impl for TextService_Impl {
    fn OnChange(&self, _rguid: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }
}

impl ITfFunctionProvider_Impl for TextService_Impl {
    fn GetType(&self) -> windows::core::Result<GUID> {
        Ok(CLSID_TYPE3ARABI_TIP)
    }

    fn GetDescription(&self) -> windows::core::Result<BSTR> {
        Ok(BSTR::from("Type3arabi Function Provider"))
    }

    fn GetFunction(
        &self,
        _rguid: *const GUID,
        riid: *const GUID,
    ) -> windows::core::Result<windows::core::IUnknown> {
        if riid.is_null() {
            return Err(windows::core::Error::from(E_POINTER));
        }
        let unk: windows::core::IUnknown = self.to_interface();
        Ok(unk)
    }
}

impl ITfFunction_Impl for TextService_Impl {
    fn GetDisplayName(&self) -> windows::core::Result<BSTR> {
        Ok(BSTR::from("Type3arabi"))
    }
}

impl ITfFnConfigure_Impl for TextService_Impl {
    fn Show(
        &self,
        _hwndparent: HWND,
        _langid: u16,
        _rguidprofile: *const GUID,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

impl TextService_Impl {
    fn execute_action(
        &self,
        pic: Option<&ITfContext>,
        action: Action,
    ) -> windows::core::Result<()> {
        let Some(ctx) = pic else {
            return Ok(());
        };

        match action {
            Action::AppendChar(ch) => {
                let mut inner = self.inner.borrow_mut();
                if inner.session.is_empty() {
                    let prev_words = inner.recent_words.clone();
                    let prev_refs: Vec<&str> = prev_words.iter().map(|s| s.as_str()).collect();
                    inner.session.set_context(&prev_refs);
                }
                let store = Arc::clone(&inner.user_store);
                inner.session.push(InputChar::new(ch), store.as_ref());
                inner.selected_index = 0;
                inner.current_page = 0;
                self.sync_composition_and_ui(ctx, &mut inner)?;
            }
            Action::AppendLiteralDigit(digit) => {
                let mut inner = self.inner.borrow_mut();
                if inner.session.is_empty() {
                    let prev_words = inner.recent_words.clone();
                    let prev_refs: Vec<&str> = prev_words.iter().map(|s| s.as_str()).collect();
                    inner.session.set_context(&prev_refs);
                }
                let store = Arc::clone(&inner.user_store);
                inner.session.push(InputChar::numpad(digit), store.as_ref());
                inner.selected_index = 0;
                inner.current_page = 0;
                self.sync_composition_and_ui(ctx, &mut inner)?;
            }
            Action::ArticleHyphen => {
                // Article hyphen '-' after el/al/il/l is swallowed
            }
            Action::Backspace => {
                let mut inner = self.inner.borrow_mut();
                let store = Arc::clone(&inner.user_store);
                inner.session.pop(store.as_ref());
                inner.selected_index = 0;
                inner.current_page = 0;
                if inner.session.is_empty() {
                    self.end_composition_internal(ctx, &mut inner, None)?;
                } else {
                    self.sync_composition_and_ui(ctx, &mut inner)?;
                }
            }
            Action::ReEdit => {
                let mut inner = self.inner.borrow_mut();
                if let Some(mut anchor) = inner.last_commit.take() {
                    if anchor.had_trailing_space {
                        anchor.had_trailing_space = false;
                        inner.last_commit = Some(anchor);
                        let tid = inner.client_id;
                        let ctx_clone = ctx.clone();
                        let edit_session = EditSession::new(move |ec| unsafe {
                            let insert_at_sel: ITfInsertAtSelection = ctx_clone.cast()?;
                            let range =
                                insert_at_sel.InsertTextAtSelection(ec, TF_IAS_NOQUERY, &[])?;
                            let mut shifted = 0i32;
                            let _ = range.ShiftStart(ec, -1, &mut shifted, std::ptr::null());
                            let _ = range.SetText(ec, 0, &[]);
                            let _ = range.Collapse(ec, TF_ANCHOR_END);
                            let sel = TF_SELECTION {
                                range: std::mem::ManuallyDrop::new(Some(range)),
                                style: TF_SELECTIONSTYLE {
                                    ase: windows::Win32::UI::TextServices::TF_AE_NONE,
                                    fInterimChar: BOOL::from(false),
                                },
                            };
                            let _ = ctx_clone.SetSelection(ec, &[sel]);
                            Ok(())
                        });
                        unsafe {
                            let session_inst: windows::Win32::UI::TextServices::ITfEditSession =
                                edit_session.into();
                            let _ = ctx.RequestEditSession(
                                tid,
                                &session_inst,
                                TF_ES_READWRITE | TF_ES_ASYNCDONTCARE,
                            );
                        }
                    } else {
                        let delete_len = anchor.text.encode_utf16().count() as i32;
                        let tid = inner.client_id;
                        let ctx_clone = ctx.clone();
                        let edit_session = EditSession::new(move |ec| unsafe {
                            let insert_at_sel: ITfInsertAtSelection = ctx_clone.cast()?;
                            let range =
                                insert_at_sel.InsertTextAtSelection(ec, TF_IAS_NOQUERY, &[])?;
                            let mut shifted = 0i32;
                            let _ =
                                range.ShiftStart(ec, -delete_len, &mut shifted, std::ptr::null());
                            let _ = range.SetText(ec, 0, &[]);
                            let _ = range.Collapse(ec, TF_ANCHOR_END);
                            let sel = TF_SELECTION {
                                range: std::mem::ManuallyDrop::new(Some(range)),
                                style: TF_SELECTIONSTYLE {
                                    ase: windows::Win32::UI::TextServices::TF_AE_NONE,
                                    fInterimChar: BOOL::from(false),
                                },
                            };
                            let _ = ctx_clone.SetSelection(ec, &[sel]);
                            Ok(())
                        });
                        unsafe {
                            let session_inst: windows::Win32::UI::TextServices::ITfEditSession =
                                edit_session.into();
                            let _ = ctx.RequestEditSession(
                                tid,
                                &session_inst,
                                TF_ES_READWRITE | TF_ES_ASYNCDONTCARE,
                            );
                        }

                        let store = Arc::clone(&inner.user_store);
                        inner.session.restore(&anchor.latin, store.as_ref());
                        inner.selected_index = anchor
                            .candidate_index
                            .min(inner.session.candidates().items.len());
                        let page_size = inner.config.candidates_per_page.clamp(5, 9) as usize;
                        inner.current_page =
                            if inner.selected_index < inner.session.candidates().items.len() {
                                inner.selected_index / page_size
                            } else {
                                0
                            };
                        self.sync_composition_and_ui(ctx, &mut inner)?;
                    }
                }
            }
            Action::NextCandidate => {
                let mut inner = self.inner.borrow_mut();
                let total = inner.session.candidates().items.len() + 1; // +1 for raw Latin
                if total > 0 {
                    inner.selected_index = (inner.selected_index + 1) % total;
                    let page_size = inner.config.candidates_per_page.clamp(5, 9) as usize;
                    if inner.selected_index < inner.session.candidates().items.len() {
                        inner.current_page = inner.selected_index / page_size;
                    }
                    self.sync_composition_and_ui(ctx, &mut inner)?;
                }
            }
            Action::PrevCandidate => {
                let mut inner = self.inner.borrow_mut();
                let total = inner.session.candidates().items.len() + 1;
                if total > 0 {
                    if inner.selected_index == 0 {
                        inner.selected_index = total - 1;
                    } else {
                        inner.selected_index -= 1;
                    }
                    let page_size = inner.config.candidates_per_page.clamp(5, 9) as usize;
                    if inner.selected_index < inner.session.candidates().items.len() {
                        inner.current_page = inner.selected_index / page_size;
                    }
                    self.sync_composition_and_ui(ctx, &mut inner)?;
                }
            }
            Action::NextPage => {
                let mut inner = self.inner.borrow_mut();
                let page_size = inner.config.candidates_per_page.clamp(5, 9) as usize;
                let total_pages = inner
                    .session
                    .candidates()
                    .items
                    .len()
                    .div_ceil(page_size)
                    .max(1);
                if inner.current_page + 1 < total_pages {
                    inner.current_page += 1;
                    inner.selected_index = inner.current_page * page_size;
                    self.sync_composition_and_ui(ctx, &mut inner)?;
                }
            }
            Action::PrevPage => {
                let mut inner = self.inner.borrow_mut();
                let page_size = inner.config.candidates_per_page.clamp(5, 9) as usize;
                if inner.current_page > 0 {
                    inner.current_page -= 1;
                    inner.selected_index = inner.current_page * page_size;
                    self.sync_composition_and_ui(ctx, &mut inner)?;
                }
            }
            Action::CommitSpace => {
                let mut inner = self.inner.borrow_mut();
                let selected = inner.selected_index;
                self.commit_candidate_internal(ctx, &mut inner, selected, CommitHow::Space)?;
            }
            Action::CommitNoSpace => {
                let mut inner = self.inner.borrow_mut();
                let selected = inner.selected_index;
                self.commit_candidate_internal(ctx, &mut inner, selected, CommitHow::Enter)?;
            }
            Action::CommitWithHarakat => {
                let mut inner = self.inner.borrow_mut();
                let selected = inner.selected_index;
                self.commit_candidate_internal(ctx, &mut inner, selected, CommitHow::WithHarakat)?;
            }
            Action::CommitRaw => {
                let mut inner = self.inner.borrow_mut();
                self.commit_candidate_internal(ctx, &mut inner, usize::MAX, CommitHow::Enter)?;
            }
            Action::CommitAndReinject => {
                let (wparam, lparam) = {
                    let mut inner = self.inner.borrow_mut();
                    let selected = inner.selected_index;
                    let raw = inner.last_raw_key;
                    self.commit_candidate_internal(ctx, &mut inner, selected, CommitHow::Enter)?;
                    raw
                };
                reinject_key(wparam, lparam);
            }
            Action::CommitThenPunctuation(c) => {
                let (punct, tid) = {
                    let mut inner = self.inner.borrow_mut();
                    let punct =
                        t3a_engine::display::punctuation(c, inner.config.arabic_punctuation);
                    let selected = inner.selected_index;
                    let tid = inner.client_id;
                    self.commit_candidate_internal(ctx, &mut inner, selected, CommitHow::Enter)?;
                    (punct, tid)
                };
                let punct_utf16: Vec<u16> = [punct as u16].to_vec();
                let ctx_clone = ctx.clone();
                let edit_session = EditSession::new(move |ec| unsafe {
                    let insert_at_sel: ITfInsertAtSelection = ctx_clone.cast()?;
                    let range =
                        insert_at_sel.InsertTextAtSelection(ec, TF_IAS_NOQUERY, &punct_utf16)?;
                    range.Collapse(ec, TF_ANCHOR_END)?;
                    let sel = TF_SELECTION {
                        range: std::mem::ManuallyDrop::new(Some(range)),
                        style: TF_SELECTIONSTYLE {
                            ase: windows::Win32::UI::TextServices::TF_AE_NONE,
                            fInterimChar: BOOL::from(false),
                        },
                    };
                    ctx_clone.SetSelection(ec, &[sel])?;
                    Ok(())
                });
                unsafe {
                    let session_inst: windows::Win32::UI::TextServices::ITfEditSession =
                        edit_session.into();
                    let _ = ctx.RequestEditSession(
                        tid,
                        &session_inst,
                        TF_ES_READWRITE | TF_ES_ASYNCDONTCARE,
                    );
                }
            }
            Action::InsertLatin(c) => {
                let (latin_utf16, tid) = {
                    let inner = self.inner.borrow();
                    ([c as u16].to_vec(), inner.client_id)
                };
                let ctx_clone = ctx.clone();
                let edit_session = EditSession::new(move |ec| unsafe {
                    let insert_at_sel: ITfInsertAtSelection = ctx_clone.cast()?;
                    let range =
                        insert_at_sel.InsertTextAtSelection(ec, TF_IAS_NOQUERY, &latin_utf16)?;
                    range.Collapse(ec, TF_ANCHOR_END)?;
                    let sel = TF_SELECTION {
                        range: std::mem::ManuallyDrop::new(Some(range)),
                        style: TF_SELECTIONSTYLE {
                            ase: windows::Win32::UI::TextServices::TF_AE_NONE,
                            fInterimChar: BOOL::from(false),
                        },
                    };
                    ctx_clone.SetSelection(ec, &[sel])?;
                    Ok(())
                });
                unsafe {
                    let session_inst: windows::Win32::UI::TextServices::ITfEditSession =
                        edit_session.into();
                    let _ = ctx.RequestEditSession(
                        tid,
                        &session_inst,
                        TF_ES_READWRITE | TF_ES_ASYNCDONTCARE,
                    );
                }
            }
            Action::InsertPunctuation(c) => {
                let (punct_utf16, tid) = {
                    let inner = self.inner.borrow();
                    let punct =
                        t3a_engine::display::punctuation(c, inner.config.arabic_punctuation);
                    ([punct as u16].to_vec(), inner.client_id)
                };
                let ctx_clone = ctx.clone();
                let edit_session = EditSession::new(move |ec| unsafe {
                    let insert_at_sel: ITfInsertAtSelection = ctx_clone.cast()?;
                    let range =
                        insert_at_sel.InsertTextAtSelection(ec, TF_IAS_NOQUERY, &punct_utf16)?;
                    range.Collapse(ec, TF_ANCHOR_END)?;
                    let sel = TF_SELECTION {
                        range: std::mem::ManuallyDrop::new(Some(range)),
                        style: TF_SELECTIONSTYLE {
                            ase: windows::Win32::UI::TextServices::TF_AE_NONE,
                            fInterimChar: BOOL::from(false),
                        },
                    };
                    ctx_clone.SetSelection(ec, &[sel])?;
                    Ok(())
                });
                unsafe {
                    let session_inst: windows::Win32::UI::TextServices::ITfEditSession =
                        edit_session.into();
                    let _ = ctx.RequestEditSession(
                        tid,
                        &session_inst,
                        TF_ES_READWRITE | TF_ES_ASYNCDONTCARE,
                    );
                }
            }
            Action::ToggleMode => {
                let mut inner = self.inner.borrow_mut();
                inner.arabic_mode = !inner.arabic_mode;
            }
            Action::CommitThenToggle => {
                let mut inner = self.inner.borrow_mut();
                let selected = inner.selected_index;
                self.commit_candidate_internal(ctx, &mut inner, selected, CommitHow::Enter)?;
                inner.arabic_mode = !inner.arabic_mode;
            }
            Action::OpenTashkeel | Action::Tashkeel(_) => {
                // Handled in M5
            }
            _ => {}
        }
        Ok(())
    }

    fn commit_candidate_internal(
        &self,
        ctx: &ITfContext,
        inner: &mut Inner,
        index: usize,
        how: CommitHow,
    ) -> windows::core::Result<()> {
        let latin = inner.session.candidates().raw.clone();
        let total_cands = inner.session.candidates().items.len();

        let (commit_text, trailing_space, had_rank) = if index < total_cands {
            let top_word = inner
                .session
                .candidates()
                .items
                .first()
                .map(|c| c.base.clone());
            let navigated = index > 0;
            let commit = inner.session.commit(index, how);
            if commit.learnable
                && inner.config.learning_enabled
                && !inner.secure_mode
                && !t3a_paths::is_app_container()
            {
                inner.user_store.record(
                    &latin,
                    &commit.base,
                    index,
                    navigated,
                    top_word.as_deref(),
                );
            }
            let has_space = commit.trailing == t3a_engine::session::Trailing::Space;
            (commit.text, has_space, index)
        } else {
            // Raw Latin selected
            let raw = inner.session.candidates().raw.clone();
            inner.session.reset();
            (raw, how == CommitHow::Space, total_cands)
        };

        let text_to_insert = if trailing_space {
            format!("{commit_text} ")
        } else {
            commit_text.clone()
        };

        inner.last_commit = Some(ReEditState {
            latin,
            text: commit_text.clone(),
            had_trailing_space: trailing_space,
            candidate_index: had_rank,
        });

        inner
            .recent_words
            .push(t3a_engine::arabic::strip_marks(&commit_text));
        if inner.recent_words.len() > 2 {
            inner.recent_words.remove(0);
        }

        self.end_composition_internal(ctx, inner, Some(&text_to_insert))
    }

    fn sync_composition_and_ui(
        &self,
        ctx: &ITfContext,
        inner: &mut Inner,
    ) -> windows::core::Result<()> {
        let candidates = inner.session.candidates();
        let default_text = if inner.selected_index < candidates.items.len() {
            candidates.items[inner.selected_index].text.as_str()
        } else if let Some(cand) = candidates.items.first() {
            cand.text.as_str()
        } else {
            candidates.raw.as_str()
        };
        let preview_text = if inner.config.inline_preview == "latin" {
            candidates.raw.as_str()
        } else {
            default_text
        };
        let preview_utf16: Vec<u16> = preview_text.encode_utf16().collect();
        let tid = inner.client_id;

        let ctx_clone = ctx.clone();
        let edit_session = EditSession::new(move |ec| unsafe {
            let insert_at_sel: ITfInsertAtSelection = ctx_clone.cast()?;
            let range = insert_at_sel.InsertTextAtSelection(ec, TF_IAS_NOQUERY, &preview_utf16)?;

            range.Collapse(ec, TF_ANCHOR_END)?;
            let sel = TF_SELECTION {
                range: std::mem::ManuallyDrop::new(Some(range)),
                style: TF_SELECTIONSTYLE {
                    ase: windows::Win32::UI::TextServices::TF_AE_NONE,
                    fInterimChar: BOOL::from(false),
                },
            };
            ctx_clone.SetSelection(ec, &[sel])?;
            Ok(())
        });

        unsafe {
            let session_inst: windows::Win32::UI::TextServices::ITfEditSession =
                edit_session.into();
            let _ =
                ctx.RequestEditSession(tid, &session_inst, TF_ES_READWRITE | TF_ES_ASYNCDONTCARE);
        }

        // Show popup
        if let Some(popup) = &mut inner.popup {
            let page_size = inner.config.candidates_per_page.clamp(5, 9) as usize;
            let total_items = candidates.items.len();
            let total_pages = total_items.div_ceil(page_size).max(1);
            if inner.current_page >= total_pages {
                inner.current_page = total_pages.saturating_sub(1);
            }
            let start = inner.current_page * page_size;
            let end = (start + page_size).min(total_items);

            let mut rows = Vec::new();
            for c in &candidates.items[start..end] {
                let marker = match c.kind {
                    t3a_engine::session::CandidateKind::Completion => RowMarker::Completion,
                    t3a_engine::session::CandidateKind::Custom => RowMarker::Custom,
                    _ => RowMarker::None,
                };
                rows.push(Row {
                    text: c.text.clone(),
                    marker,
                    rtl: true,
                });
            }
            rows.push(Row {
                text: candidates.raw.clone(),
                marker: RowMarker::RawLatin,
                rtl: false,
            });

            let highlighted = if inner.selected_index >= start && inner.selected_index < end {
                inner.selected_index - start
            } else if inner.selected_index >= total_items {
                rows.len().saturating_sub(1)
            } else {
                0
            };

            let footer = if total_pages > 1 {
                Footer::Paging {
                    page: (inner.current_page + 1) as u8,
                    pages: total_pages as u8,
                }
            } else {
                match inner.config.footer_hints.as_str() {
                    "always" => Footer::Hints,
                    "never" => Footer::Hidden,
                    _ => Footer::Hints,
                }
            };

            let badge = if inner.config.show_dialect_badge {
                let dominant = t3a_engine::dialect::argmax(&inner.session.dialect());
                Some(dominant.arabic_name().to_string())
            } else {
                None
            };

            let list_model = ListModel {
                latin: candidates.raw.clone(),
                badge,
                rows,
                highlighted,
                footer,
            };

            let mut pt = windows::Win32::Foundation::POINT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
            }
            let anchor = (pt.x, pt.y, pt.x + 20, pt.y + 20);
            popup.show(PopupModel::List(list_model), anchor);
        }

        Ok(())
    }

    fn end_composition_internal(
        &self,
        ctx: &ITfContext,
        inner: &mut Inner,
        commit_text: Option<&str>,
    ) -> windows::core::Result<()> {
        let tid = inner.client_id;
        if let Some(text) = commit_text {
            let text_utf16: Vec<u16> = text.encode_utf16().collect();
            let ctx_clone = ctx.clone();
            let edit_session = EditSession::new(move |ec| unsafe {
                let insert_at_sel: ITfInsertAtSelection = ctx_clone.cast()?;
                let range = insert_at_sel.InsertTextAtSelection(ec, TF_IAS_NOQUERY, &text_utf16)?;
                range.Collapse(ec, TF_ANCHOR_END)?;
                let sel = TF_SELECTION {
                    range: std::mem::ManuallyDrop::new(Some(range)),
                    style: TF_SELECTIONSTYLE {
                        ase: windows::Win32::UI::TextServices::TF_AE_NONE,
                        fInterimChar: BOOL::from(false),
                    },
                };
                ctx_clone.SetSelection(ec, &[sel])?;
                Ok(())
            });
            unsafe {
                let session_inst: windows::Win32::UI::TextServices::ITfEditSession =
                    edit_session.into();
                let _ = ctx.RequestEditSession(
                    tid,
                    &session_inst,
                    TF_ES_READWRITE | TF_ES_ASYNCDONTCARE,
                );
            }
        }

        if let Some(popup) = &mut inner.popup {
            popup.hide();
        }
        inner.session.reset();
        inner.active_composition = None;
        inner.composition_range = None;

        Ok(())
    }
}
