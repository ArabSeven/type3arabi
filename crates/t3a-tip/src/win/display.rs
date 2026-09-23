//! Display attributes: how the host draws the composing word (docs/02 §8).

use crate::ids::{GUID_DISPLAY_ATTR_INPUT, GUID_DISPLAY_ATTR_TASHKEEL};
use crate::win::guard::guard;
use std::cell::Cell;
use windows::core::{implement, BSTR, GUID};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, S_FALSE};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl, ITfDisplayAttributeInfo,
    ITfDisplayAttributeInfo_Impl, TF_ATTR_INPUT, TF_ATTR_TARGET_CONVERTED, TF_CT_NONE, TF_DA_COLOR,
    TF_DISPLAYATTRIBUTE, TF_LS_DOT, TF_LS_SOLID,
};

pub const GUID_ATTR_INPUT: GUID = GUID::from_u128(GUID_DISPLAY_ATTR_INPUT);
pub const GUID_ATTR_TASHKEEL: GUID = GUID::from_u128(GUID_DISPLAY_ATTR_TASHKEEL);
const ALL: [GUID; 2] = [GUID_ATTR_INPUT, GUID_ATTR_TASHKEEL];

fn attribute(guid: &GUID) -> Option<(TF_DISPLAYATTRIBUTE, &'static str)> {
    let none = TF_DA_COLOR {
        r#type: TF_CT_NONE,
        ..Default::default()
    };
    if *guid == GUID_ATTR_INPUT {
        Some((
            TF_DISPLAYATTRIBUTE {
                crText: none,
                crBk: none,
                lsStyle: TF_LS_DOT,
                fBoldLine: false.into(),
                crLine: none,
                bAttr: TF_ATTR_INPUT,
            },
            "Type3arabi composing",
        ))
    } else if *guid == GUID_ATTR_TASHKEEL {
        Some((
            TF_DISPLAYATTRIBUTE {
                crText: none,
                crBk: none,
                lsStyle: TF_LS_SOLID,
                fBoldLine: true.into(),
                crLine: none,
                bAttr: TF_ATTR_TARGET_CONVERTED,
            },
            "Type3arabi tashkeel",
        ))
    } else {
        None
    }
}

#[implement(ITfDisplayAttributeInfo)]
pub struct DisplayAttributeInfo {
    guid: GUID,
}

impl DisplayAttributeInfo {
    pub fn lookup(guid: &GUID) -> Option<ITfDisplayAttributeInfo> {
        attribute(guid).map(|_| DisplayAttributeInfo { guid: *guid }.into())
    }
}

impl ITfDisplayAttributeInfo_Impl for DisplayAttributeInfo_Impl {
    fn GetGUID(&self) -> windows::core::Result<GUID> {
        Ok(self.guid)
    }
    fn GetDescription(&self) -> windows::core::Result<BSTR> {
        guard(Err(E_FAIL.into()), || {
            attribute(&self.guid)
                .map(|(_, d)| BSTR::from(d))
                .ok_or_else(|| E_FAIL.into())
        })
    }
    fn GetAttributeInfo(&self, pda: *mut TF_DISPLAYATTRIBUTE) -> windows::core::Result<()> {
        if pda.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let (a, _) = attribute(&self.guid).ok_or(windows::core::Error::from(E_FAIL))?;
        // SAFETY: pda checked non-null; caller provides a TF_DISPLAYATTRIBUTE-sized buffer.
        unsafe { *pda = a };
        Ok(())
    }
    fn SetAttributeInfo(&self, _pda: *const TF_DISPLAYATTRIBUTE) -> windows::core::Result<()> {
        Ok(()) // attributes are fixed
    }
    fn Reset(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

#[implement(IEnumTfDisplayAttributeInfo)]
pub struct EnumDisplayAttributeInfo {
    pos: Cell<usize>,
}

impl EnumDisplayAttributeInfo {
    pub fn create() -> IEnumTfDisplayAttributeInfo {
        EnumDisplayAttributeInfo { pos: Cell::new(0) }.into()
    }
}

impl IEnumTfDisplayAttributeInfo_Impl for EnumDisplayAttributeInfo_Impl {
    fn Clone(&self) -> windows::core::Result<IEnumTfDisplayAttributeInfo> {
        Ok(EnumDisplayAttributeInfo {
            pos: Cell::new(self.pos.get()),
        }
        .into())
    }

    fn Next(
        &self,
        ulcount: u32,
        rginfo: *mut Option<ITfDisplayAttributeInfo>,
        pcfetched: *mut u32,
    ) -> windows::core::Result<()> {
        if rginfo.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let mut fetched = 0u32;
        while fetched < ulcount && self.pos.get() < ALL.len() {
            let info = DisplayAttributeInfo::lookup(&ALL[self.pos.get()]);
            // SAFETY: caller provides an array of at least `ulcount` slots.
            unsafe { rginfo.add(fetched as usize).write(info) };
            fetched += 1;
            self.pos.set(self.pos.get() + 1);
        }
        if !pcfetched.is_null() {
            // SAFETY: checked non-null.
            unsafe { *pcfetched = fetched };
        }
        if fetched == ulcount {
            Ok(())
        } else {
            Err(S_FALSE.into())
        }
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.pos.set(0);
        Ok(())
    }

    fn Skip(&self, ulcount: u32) -> windows::core::Result<()> {
        let next = self.pos.get() + ulcount as usize;
        self.pos.set(next.min(ALL.len()));
        if next <= ALL.len() {
            Ok(())
        } else {
            Err(S_FALSE.into())
        }
    }
}
