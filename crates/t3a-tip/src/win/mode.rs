//! Language bar input mode item and compartment management (docs/02 §10).

use crate::ids::GUID_COMPARTMENT_MODE;
use windows::core::{implement, w, BOOL, BSTR, GUID};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::TextServices::{
    ITfLangBarItem, ITfLangBarItemButton, ITfLangBarItemButton_Impl, ITfLangBarItem_Impl, ITfMenu,
    TfLBIClick, TF_LANGBARITEMINFO, TF_LBI_STYLE_BTN_BUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::HICON;

pub const GUID_MODE_COMPARTMENT: GUID = GUID::from_u128(GUID_COMPARTMENT_MODE);
pub const GUID_LANGBAR_ITEM: GUID = GUID::from_u128(0x73A1B022_4F12_4A99_9B12_71B5DF291A01);

#[implement(ITfLangBarItem, ITfLangBarItemButton)]
pub struct LangBarItemButton {
    arabic_mode: std::cell::Cell<bool>,
}

impl LangBarItemButton {
    pub fn new(arabic_mode: bool) -> Self {
        Self {
            arabic_mode: std::cell::Cell::new(arabic_mode),
        }
    }

    pub fn set_arabic_mode(&self, arabic: bool) {
        self.arabic_mode.set(arabic);
    }
}

impl ITfLangBarItem_Impl for LangBarItemButton_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> windows::core::Result<()> {
        if pinfo.is_null() {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        unsafe {
            let info = &mut *pinfo;
            info.clsidService = crate::ids::CLSID_TIP.into();
            info.guidItem = GUID_LANGBAR_ITEM;
            info.dwStyle = TF_LBI_STYLE_BTN_BUTTON;
            info.ulSort = 0;
            let desc = w!("Type3arabi Input Mode");
            let slice = desc.as_wide();
            let len = slice.len().min(info.szDescription.len() - 1);
            info.szDescription[..len].copy_from_slice(&slice[..len]);
            info.szDescription[len] = 0;
        }
        Ok(())
    }

    fn GetStatus(&self) -> windows::core::Result<u32> {
        Ok(0)
    }

    fn Show(&self, _fshow: BOOL) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> windows::core::Result<BSTR> {
        if self.arabic_mode.get() {
            Ok(BSTR::from("Type3arabi: Arabic (ع)"))
        } else {
            Ok(BSTR::from("Type3arabi: Latin (A)"))
        }
    }
}

impl ITfLangBarItemButton_Impl for LangBarItemButton_Impl {
    fn OnClick(
        &self,
        _click: TfLBIClick,
        _pt: &POINT,
        _prcarea: *const RECT,
    ) -> windows::core::Result<()> {
        self.arabic_mode.set(!self.arabic_mode.get());
        Ok(())
    }

    fn InitMenu(&self, _pmenu: windows_core::Ref<'_, ITfMenu>) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnMenuSelect(&self, _wid: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetIcon(&self) -> windows::core::Result<HICON> {
        Ok(HICON::default())
    }

    fn GetText(&self) -> windows::core::Result<BSTR> {
        if self.arabic_mode.get() {
            Ok(BSTR::from("ع"))
        } else {
            Ok(BSTR::from("A"))
        }
    }
}
