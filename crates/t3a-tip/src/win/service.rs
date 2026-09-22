//! The main TSF TextService implementing ITfTextInputProcessorEx and all sinks (docs/02 §3, §4).

use crate::ids::GUID_PRESERVED_TOGGLE;
use crate::keyrouter::{classify, Action, Popup, RouterState, Toggle};
use crate::win::compose::EditSession;
use crate::win::context::evaluate_context_mode;
use crate::win::dll::{add_object, release_object, CLSID_TYPE3ARABI_TIP};
use crate::win::guard::guard;
use crate::win::keys::translate_key;
use std::cell::RefCell;
use std::sync::OnceLock;
use t3a_engine::normalize::InputChar;
use t3a_engine::session::Session;
use t3a_engine::user::NoUser;
use t3a_engine::Engine;
use t3a_ui::{Footer, ListModel, PopupModel, Row, RowMarker};
use windows::core::{implement, Interface, BOOL, BSTR, GUID};
use windows::Win32::Foundation::{E_POINTER, HWND, LPARAM, WPARAM};
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
use windows_core::IUnknownImpl;

static ENGINE: OnceLock<Engine> = OnceLock::new();
fn get_engine() -> &'static Engine {
    ENGINE.get_or_init(Engine::builtin)
}

pub struct Inner {
    pub thread_mgr: Option<ITfThreadMgr>,
    pub client_id: u32,
    pub session: Session<'static>,
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
        Ok(Self {
            inner: RefCell::new(Inner {
                thread_mgr: None,
                client_id: 0,
                session: Session::new(engine, t3a_engine::EngineSettings::default()),
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
            let inner = self.inner.borrow();
            let (key, mods) = translate_key(wparam, lparam);
            let ctx_mode =
                evaluate_context_mode(pic.as_ref(), inner.arabic_mode, inner.secure_mode);

            let state = RouterState {
                context: ctx_mode,
                composing: !inner.session.is_empty(),
                popup: if inner.popup.as_ref().is_some_and(|p| p.is_visible()) {
                    Popup::List
                } else {
                    Popup::Hidden
                },
                reedit_anchor: false,
                buffer_is_article: false,
                toggle: Toggle::CtrlSpace,
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
            let (key, mods) = translate_key(wparam, lparam);

            let action = {
                let inner = self.inner.borrow();
                let ctx_mode =
                    evaluate_context_mode(pic.as_ref(), inner.arabic_mode, inner.secure_mode);
                let state = RouterState {
                    context: ctx_mode,
                    composing: !inner.session.is_empty(),
                    popup: if inner.popup.as_ref().is_some_and(|p| p.is_visible()) {
                        Popup::List
                    } else {
                        Popup::Hidden
                    },
                    reedit_anchor: false,
                    buffer_is_article: false,
                    toggle: Toggle::CtrlSpace,
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

        let user = NoUser;
        match action {
            Action::AppendChar(ch) => {
                let mut inner = self.inner.borrow_mut();
                inner.session.push(InputChar::new(ch), &user);
                self.sync_composition_and_ui(ctx, &mut inner)?;
            }
            Action::AppendLiteralDigit(digit) => {
                let mut inner = self.inner.borrow_mut();
                inner.session.push(InputChar::numpad(digit), &user);
                self.sync_composition_and_ui(ctx, &mut inner)?;
            }
            Action::Backspace => {
                let mut inner = self.inner.borrow_mut();
                inner.session.pop(&user);
                if inner.session.is_empty() {
                    self.end_composition_internal(ctx, &mut inner, None)?;
                } else {
                    self.sync_composition_and_ui(ctx, &mut inner)?;
                }
            }
            Action::CommitSpace => {
                let mut inner = self.inner.borrow_mut();
                let commit_text = if let Some(cand) = inner.session.candidates().items.first() {
                    format!("{} ", cand.text)
                } else {
                    format!("{} ", inner.session.candidates().raw)
                };
                self.end_composition_internal(ctx, &mut inner, Some(&commit_text))?;
            }
            Action::CommitNoSpace => {
                let mut inner = self.inner.borrow_mut();
                let commit_text = if let Some(cand) = inner.session.candidates().items.first() {
                    cand.text.clone()
                } else {
                    inner.session.candidates().raw.clone()
                };
                self.end_composition_internal(ctx, &mut inner, Some(&commit_text))?;
            }
            Action::CommitRaw => {
                let mut inner = self.inner.borrow_mut();
                let raw = inner.session.candidates().raw.clone();
                self.end_composition_internal(ctx, &mut inner, Some(&raw))?;
            }
            Action::CommitWithHarakat => {
                let mut inner = self.inner.borrow_mut();
                let commit = inner
                    .session
                    .commit(0, t3a_engine::session::CommitHow::WithHarakat);
                let commit_text = format!("{} ", commit.text);
                self.end_composition_internal(ctx, &mut inner, Some(&commit_text))?;
            }
            Action::NextCandidate => {}
            Action::PrevCandidate => {}
            Action::ToggleMode => {
                let mut inner = self.inner.borrow_mut();
                inner.arabic_mode = !inner.arabic_mode;
            }
            Action::CommitThenToggle => {
                let mut inner = self.inner.borrow_mut();
                let commit_text = inner
                    .session
                    .candidates()
                    .items
                    .first()
                    .map(|c| c.text.clone())
                    .unwrap_or_default();
                self.end_composition_internal(ctx, &mut inner, Some(&commit_text))?;
                inner.arabic_mode = !inner.arabic_mode;
            }
            _ => {}
        }
        Ok(())
    }

    fn sync_composition_and_ui(
        &self,
        ctx: &ITfContext,
        inner: &mut Inner,
    ) -> windows::core::Result<()> {
        let candidates = inner.session.candidates();
        let default_text = candidates
            .items
            .first()
            .map(|c| c.text.as_str())
            .unwrap_or("");
        let preview_utf16: Vec<u16> = default_text.encode_utf16().collect();
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
            let mut rows = Vec::new();
            for c in candidates.items.iter().take(7) {
                rows.push(Row {
                    text: c.text.clone(),
                    marker: RowMarker::None,
                    rtl: true,
                });
            }
            rows.push(Row {
                text: candidates.raw.clone(),
                marker: RowMarker::RawLatin,
                rtl: false,
            });

            let list_model = ListModel {
                latin: candidates.raw.clone(),
                badge: Some("شامي".to_string()),
                rows,
                highlighted: 0,
                footer: Footer::Hints,
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
