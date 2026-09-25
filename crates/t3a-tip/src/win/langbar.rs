//! The input-indicator button (docs/02 §10): the mode icon Windows shows in the taskbar next to the
//! language, while Type3arabi is the active keyboard. Left click switches Arabic ⇄ Latin; right click opens
//! a small menu (Arabic, Latin, Settings…).
//!
//! Documented TSF mechanism only (no Shell_NotifyIcon, no hooks): one `ITfLangBarItemButton` with
//! `GUID_LBI_INPUTMODE` — since Windows 8 the input indicator ignores every other item — added through
//! `ITfLangBarItemMgr` on activation and removed on deactivation, with the `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT`
//! category registered (dll.rs). Right clicks arrive as `OnClick(TF_LBI_CLK_RIGHT)` and the button shows its
//! own popup menu, as Microsoft's SampleIME and CorvusSKK do. Icons: `res/` (black-and-white IME icon style).

use crate::win::dll;
use crate::win::dll::CLSID_TYPE3ARABI_TIP;
use crate::win::guard::guard;
use crate::win::service::TextService;
use std::cell::{Cell, RefCell};
use windows::core::{implement, w, IUnknown, Interface, Ref, BOOL, BSTR, GUID, PCWSTR};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, HWND, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::TextServices::{
    ITfLangBarItem, ITfLangBarItemButton, ITfLangBarItemButton_Impl, ITfLangBarItemSink,
    ITfLangBarItem_Impl, ITfMenu, ITfSource, ITfSource_Impl, TfLBIClick, GUID_LBI_INPUTMODE,
    TF_LANGBARITEMINFO, TF_LBI_CLK_LEFT, TF_LBI_CLK_RIGHT, TF_LBI_ICON, TF_LBI_STYLE_BTN_BUTTON,
    TF_LBI_STYLE_SHOWNINTRAY, TF_LBI_TEXT, TF_LBI_TOOLTIP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DestroyMenu, DestroyWindow, GetSystemMetrics,
    LoadImageW, TrackPopupMenuEx, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, MF_CHECKED, MF_GRAYED,
    MF_SEPARATOR, MF_STRING, SM_CXSMICON, TPMPARAMS, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TPM_TOPALIGN, TPM_VERTICAL, WINDOW_EX_STYLE, WS_POPUP,
};

/// Icon resources embedded by build.rs.
const IDI_MODE_ARABIC: u16 = 102;
const IDI_MODE_LATIN: u16 = 103;
/// Menu command ids.
const CMD_ARABIC: u32 = 1;
const CMD_LATIN: u32 = 2;
const CMD_SETTINGS: u32 = 3;
/// The one sink cookie this item hands out.
const SINK_COOKIE: u32 = 0x7433;

/// What the button needs from the text service; implemented by `TextService_Impl` (service.rs).
pub trait TrayOwner {
    fn tray_is_arabic(&self) -> bool;
    fn tray_set_arabic(&self, arabic: bool);
    /// The Arabic ⇄ Latin shortcut as shown to the user ("Ctrl+Space"), or None.
    fn tray_toggle_hint(&self) -> Option<String>;
    /// False in sandboxed apps and on the secure desktop (no process may be started there).
    fn tray_can_open_settings(&self) -> bool;
    fn tray_open_settings(&self);
    fn tray_secure(&self) -> bool;
}

#[implement(ITfLangBarItemButton, ITfLangBarItem, ITfSource)]
pub struct ModeButton {
    /// The text service (cleared by `detach` on deactivation, which breaks the reference cycle).
    owner: RefCell<Option<IUnknown>>,
    sink: RefCell<Option<ITfLangBarItemSink>>,
    shown: Cell<bool>,
}

impl ModeButton {
    pub fn new(owner: IUnknown) -> Self {
        Self {
            owner: RefCell::new(Some(owner)),
            sink: RefCell::new(None),
            shown: Cell::new(true),
        }
    }

    /// Run `f` with the text service, if still attached.
    fn with_owner<R>(&self, f: impl FnOnce(&dyn TrayOwner) -> R) -> Option<R> {
        let owner = self.owner.borrow().clone()?;
        let service = owner.cast_object_ref::<TextService>().ok()?;
        Some(f(service))
    }
}

impl ModeButton_Impl {
    /// Forget the text service and the sink (deactivation).
    pub fn detach(&self) {
        self.owner.borrow_mut().take();
        self.sink.borrow_mut().take();
    }

    /// Ask the input indicator to re-read the icon and tooltip (after an Arabic ⇄ Latin switch).
    pub fn refresh(&self) {
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            // SAFETY: plain COM call; no borrow is held (the indicator calls GetIcon back).
            unsafe {
                let _ = sink.OnUpdate(TF_LBI_ICON | TF_LBI_TOOLTIP | TF_LBI_TEXT);
            }
        }
    }

    fn arabic(&self) -> bool {
        self.with_owner(|o| o.tray_is_arabic()).unwrap_or(true)
    }

    fn label(&self) -> String {
        if self.arabic() {
            "Type3arabi · Arabic (ع)".into()
        } else {
            "Type3arabi · Latin (A)".into()
        }
    }

    fn select(&self, cmd: u32) {
        match cmd {
            CMD_ARABIC => {
                let _ = self.with_owner(|o| o.tray_set_arabic(true));
            }
            CMD_LATIN => {
                let _ = self.with_owner(|o| o.tray_set_arabic(false));
            }
            CMD_SETTINGS => {
                let _ = self.with_owner(|o| {
                    if o.tray_can_open_settings() {
                        o.tray_open_settings();
                    }
                });
            }
            _ => {}
        }
    }

    /// The right-click menu. Owner window: the focused window of this thread, else a temporary
    /// invisible popup of our own (never a window of another thread).
    fn show_menu(&self, pt: &POINT, area: Option<RECT>) {
        let Some((arabic, hint, can_settings)) = self.with_owner(|o| {
            (
                o.tray_is_arabic(),
                o.tray_toggle_hint(),
                o.tray_can_open_settings(),
            )
        }) else {
            return;
        };
        let latin_label = match &hint {
            Some(k) => format!("Latin · لاتيني\t{k}"),
            None => "Latin · لاتيني".into(),
        };
        let items: [(u32, String, bool, bool); 3] = [
            (CMD_ARABIC, "Arabic · عربي".into(), arabic, true),
            (CMD_LATIN, latin_label, !arabic, true),
            (
                CMD_SETTINGS,
                "Type3arabi Settings… · الإعدادات".into(),
                false,
                can_settings,
            ),
        ];
        // SAFETY: a menu and (maybe) a window created and destroyed on this thread; NUL-terminated strings.
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            for (i, (id, text, checked, enabled)) in items.iter().enumerate() {
                if i == 2 {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let mut flags = MF_STRING;
                if *checked {
                    flags |= MF_CHECKED;
                }
                if !*enabled {
                    flags |= MF_GRAYED;
                }
                let _ = AppendMenuW(menu, flags, *id as usize, PCWSTR(wide.as_ptr()));
            }
            let mut temp: Option<HWND> = None;
            let mut owner = GetFocus();
            if owner.is_invalid() {
                if let Ok(h) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    PCWSTR::null(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(dll::module_handle()),
                    None,
                ) {
                    owner = h;
                    temp = Some(h);
                }
            }
            if !owner.is_invalid() {
                let params = area.map(|r| TPMPARAMS {
                    cbSize: std::mem::size_of::<TPMPARAMS>() as u32,
                    rcExclude: r,
                });
                let cmd = TrackPopupMenuEx(
                    menu,
                    (TPM_LEFTALIGN
                        | TPM_TOPALIGN
                        | TPM_VERTICAL
                        | TPM_NONOTIFY
                        | TPM_RETURNCMD
                        | TPM_RIGHTBUTTON)
                        .0,
                    pt.x,
                    pt.y,
                    owner,
                    params.as_ref().map(|p| p as *const TPMPARAMS),
                );
                if cmd.as_bool() {
                    self.select(cmd.0 as u32);
                }
            }
            let _ = DestroyMenu(menu);
            if let Some(h) = temp {
                let _ = DestroyWindow(h);
            }
        }
    }
}

impl ITfLangBarItem_Impl for ModeButton_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> windows::core::Result<()> {
        guard(Err(E_FAIL.into()), || {
            if pinfo.is_null() {
                return Err(E_INVALIDARG.into());
            }
            let mut info = TF_LANGBARITEMINFO {
                clsidService: CLSID_TYPE3ARABI_TIP,
                guidItem: GUID_LBI_INPUTMODE,
                dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
                ulSort: 0,
                szDescription: [0; 32],
            };
            for (d, c) in info
                .szDescription
                .iter_mut()
                .zip("Type3arabi".encode_utf16())
            {
                *d = c;
            }
            // SAFETY: the caller passes a writable TF_LANGBARITEMINFO (checked non-null above).
            unsafe { pinfo.write(info) };
            Ok(())
        })
    }
    fn GetStatus(&self) -> windows::core::Result<u32> {
        guard(Ok(0), || Ok(0))
    }
    fn Show(&self, fshow: BOOL) -> windows::core::Result<()> {
        guard(Ok(()), || {
            self.shown.set(fshow.as_bool());
            Ok(())
        })
    }
    fn GetTooltipString(&self) -> windows::core::Result<BSTR> {
        guard(Err(E_FAIL.into()), || Ok(BSTR::from(self.label())))
    }
}

impl ITfLangBarItemButton_Impl for ModeButton_Impl {
    fn OnClick(
        &self,
        click: TfLBIClick,
        pt: &POINT,
        prcarea: *const RECT,
    ) -> windows::core::Result<()> {
        guard(Ok(()), || {
            if click == TF_LBI_CLK_LEFT {
                let arabic = self.arabic();
                let _ = self.with_owner(|o| o.tray_set_arabic(!arabic));
            } else if click == TF_LBI_CLK_RIGHT {
                // SAFETY: the area pointer is null or points to a RECT for the duration of the call.
                let area = unsafe { prcarea.as_ref().copied() };
                self.show_menu(pt, area);
            }
            Ok(())
        })
    }
    fn InitMenu(&self, _pmenu: Ref<'_, ITfMenu>) -> windows::core::Result<()> {
        // TF_LBI_STYLE_BTN_BUTTON: the menu is shown from OnClick (right click), not through ITfMenu.
        Ok(())
    }
    fn OnMenuSelect(&self, wid: u32) -> windows::core::Result<()> {
        guard(Ok(()), || {
            self.select(wid);
            Ok(())
        })
    }
    fn GetIcon(&self) -> windows::core::Result<HICON> {
        guard(Err(E_FAIL.into()), || {
            let id = if self.arabic() {
                IDI_MODE_ARABIC
            } else {
                IDI_MODE_LATIN
            };
            // 16 px at 96 DPI (scaled with the system DPI), 20 px on the secure desktop (IME guidelines).
            let secure = self.with_owner(|o| o.tray_secure()).unwrap_or(false);
            // SAFETY: GetSystemMetrics has no preconditions.
            let small = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
            let size = if secure { small * 5 / 4 } else { small };
            // SAFETY: loads an icon resource of this DLL; a new (non-shared) icon, which the caller
            // destroys, as ITfLangBarItemButton::GetIcon requires.
            let h = unsafe {
                LoadImageW(
                    Some(dll::module_handle()),
                    PCWSTR(id as usize as *const u16),
                    IMAGE_ICON,
                    size,
                    size,
                    LR_DEFAULTCOLOR,
                )
            }?;
            Ok(HICON(h.0))
        })
    }
    fn GetText(&self) -> windows::core::Result<BSTR> {
        guard(Err(E_FAIL.into()), || Ok(BSTR::from(self.label())))
    }
}

impl ITfSource_Impl for ModeButton_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<'_, IUnknown>) -> windows::core::Result<u32> {
        guard(Err(E_FAIL.into()), || {
            // SAFETY: riid is a valid GUID pointer supplied by the caller.
            if riid.is_null() || unsafe { *riid } != ITfLangBarItemSink::IID {
                return Err(windows::Win32::System::Ole::CONNECT_E_CANNOTCONNECT.into());
            }
            let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
            *self.sink.borrow_mut() = Some(sink);
            Ok(SINK_COOKIE)
        })
    }
    fn UnadviseSink(&self, dwcookie: u32) -> windows::core::Result<()> {
        guard(Ok(()), || {
            if dwcookie != SINK_COOKIE {
                return Err(windows::Win32::System::Ole::CONNECT_E_NOCONNECTION.into());
            }
            self.sink.borrow_mut().take();
            Ok(())
        })
    }
}

/// `ITfLangBarItem` for `ITfLangBarItemMgr::AddItem` / `RemoveItem`.
pub fn as_item(button: &ITfLangBarItemButton) -> Option<ITfLangBarItem> {
    button.cast().ok()
}
