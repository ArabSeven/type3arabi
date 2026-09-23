//! Windows candidate popup window implementation (docs/02 §9).
//!
//! Ownership: `PopupWindow` owns the HWND and a heap-allocated `RefCell<PopupState>` whose address
//! is stable for the window's whole life; the window procedure reaches the state only through that
//! pointer and only with `try_borrow`, so it can never observe freed or aliased memory.

use crate::{metrics, Footer, PopupModel, RowMarker, Theme};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GetMonitorInfoW,
    InvalidateRect, MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, DT_CALCRECT,
    DT_CENTER, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, FONT_CHARSET,
    FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, FW_NORMAL, FW_SEMIBOLD, HDC,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW,
    RegisterClassExW, SetWindowLongPtrW, SetWindowPos, ShowWindow, UnregisterClassW, CS_DROPSHADOW,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, MA_NOACTIVATE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSEXW,
    WS_CLIPSIBLINGS, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
const WINDOW_CLASS: PCWSTR = w!("Type3arabi_CandidateWindow");

/// The module (DLL) this code lives in — NOT the host exe. Window classes must be registered
/// against the module that contains the window procedure.
fn this_module() -> HINSTANCE {
    let mut hmod = HMODULE::default();
    // SAFETY: FROM_ADDRESS with the address of a function in this module; UNCHANGED_REFCOUNT means
    // no reference is taken, so nothing needs releasing.
    unsafe {
        let _ = GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(wndproc as *const () as *const u16),
            &mut hmod,
        );
    }
    HINSTANCE(hmod.0)
}

/// Unregister the popup window class. Call when the DLL is about to unload (`DllCanUnloadNow`),
/// after every `PopupWindow` has been dropped.
pub fn unregister_class() {
    if CLASS_REGISTERED.swap(false, Ordering::AcqRel) {
        // SAFETY: class name is a static wide string; failure (a window still alive) is harmless.
        unsafe {
            let _ = UnregisterClassW(WINDOW_CLASS, Some(this_module()));
        }
    }
}

struct PopupState {
    model: PopupModel,
    theme: Theme,
    dpi: u32,
}

pub struct PopupWindow {
    hwnd: HWND,
    state: Box<RefCell<PopupState>>,
}

/// `DrawTextW` that is safe for empty strings: an empty Rust slice has a dangling pointer, and
/// user32 dereferences it even with a zero count (host crash 0xC000041D, found by tsf_harness).
unsafe fn draw_text(
    hdc: HDC,
    text: &mut [u16],
    rect: *mut RECT,
    format: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) -> i32 {
    if text.is_empty() {
        return 0;
    }
    DrawTextW(hdc, text, rect, format)
}

// Convert 0xRRGGBB to GDI COLORREF (0x00BBGGRR)
fn to_colorref(rgb: u32) -> COLORREF {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    COLORREF((b << 16) | (g << 8) | r)
}

impl PopupWindow {
    pub fn new() -> windows::core::Result<Self> {
        // SAFETY: plain Win32 window creation on the calling (UI) thread; the state pointer stored
        // in GWLP_USERDATA points into a Box that outlives the window (cleared in Drop first).
        unsafe {
            let hinstance = this_module();
            if !CLASS_REGISTERED.load(Ordering::Acquire) {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
                    lpfnWndProc: Some(wndproc),
                    hInstance: hinstance,
                    lpszClassName: WINDOW_CLASS,
                    ..Default::default()
                };
                // Fails harmlessly with ERROR_CLASS_ALREADY_EXISTS if another thread won the race.
                let _ = RegisterClassExW(&wc);
                CLASS_REGISTERED.store(true, Ordering::Release);
            }

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                WINDOW_CLASS,
                w!("Type3arabi Candidates"),
                WS_POPUP | WS_CLIPSIBLINGS,
                0,
                0,
                200,
                100,
                None,
                None,
                Some(hinstance),
                None,
            )?;

            let state = Box::new(RefCell::new(PopupState {
                model: PopupModel::Hidden,
                theme: Theme::LIGHT,
                dpi: GetDpiForWindow(hwnd).max(96),
            }));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const _ as _);
            Ok(Self { hwnd, state })
        }
    }

    pub fn is_visible(&self) -> bool {
        self.state
            .try_borrow()
            .map(|s| !matches!(s.model, PopupModel::Hidden))
            .unwrap_or(false)
    }

    pub fn hide(&mut self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.model = PopupModel::Hidden;
        }
        // SAFETY: hwnd is owned by self and alive until Drop.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Show `model` next to `anchor` = (left, top, right, bottom) of the composition, screen coords.
    pub fn show(&mut self, model: PopupModel, anchor: (i32, i32, i32, i32)) {
        if let PopupModel::Hidden = model {
            self.hide();
            return;
        }
        let (width, height) = {
            let Ok(mut s) = self.state.try_borrow_mut() else {
                return;
            };
            s.model = model;
            s.calculate_size()
        };
        let (_anchor_left, anchor_top, anchor_right, anchor_bottom) = anchor;

        // SAFETY: Win32 calls on our own window; no borrow of `state` is held across them.
        unsafe {
            let pt = POINT {
                x: anchor_right,
                y: anchor_bottom,
            };
            let hmonitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(hmonitor, &mut mi);
            let work = mi.rcWork;

            // RTL alignment: right edge of popup aligns with right edge of composition
            let mut x = anchor_right - width;
            if x + width > work.right {
                x = work.right - width;
            }
            if x < work.left {
                x = work.left;
            }

            // Place below composition by default; flip above if not enough room
            let mut y = anchor_bottom + 2;
            if y + height > work.bottom {
                let y_above = anchor_top - height - 2;
                y = if y_above >= work.top {
                    y_above
                } else {
                    work.bottom - height
                };
            }

            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }
}

impl PopupState {
    fn calculate_size(&self) -> (i32, i32) {
        let dpi = self.dpi;
        let scale = |v: f32| metrics::scale(v, dpi) as i32;

        match &self.model {
            PopupModel::List(list) => {
                let width = scale(metrics::MIN_WIDTH)
                    .max(240)
                    .min(scale(metrics::MAX_WIDTH));
                let header_h = scale(metrics::HEADER_H);
                let rows_h = list.rows.len() as i32 * scale(metrics::ROW_H);
                let footer_h = match list.footer {
                    Footer::Hidden => 0,
                    _ => scale(metrics::FOOTER_H),
                };
                (width, header_h + rows_h + footer_h + 2)
            }
            PopupModel::Tashkeel(_) => (scale(380.0), scale(158.0)),
            PopupModel::Hidden => (0, 0),
        }
    }

    unsafe fn paint(&self, hwnd: HWND, hdc: HDC) {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let width = rc.right - rc.left;
        let height = rc.bottom - rc.top;

        if width <= 0 || height <= 0 {
            return;
        }

        // Double buffer
        let mem_dc = CreateCompatibleDC(Some(hdc));
        let mem_bmp = CreateCompatibleBitmap(hdc, width, height);
        let old_bmp = SelectObject(mem_dc, mem_bmp.into());

        let bg_brush = CreateSolidBrush(to_colorref(self.theme.bg));
        let border_brush = CreateSolidBrush(to_colorref(self.theme.border));
        let accent_brush = CreateSolidBrush(to_colorref(self.theme.accent));
        let _ = FillRect(mem_dc, &rc, bg_brush);
        let _ = FrameRect(mem_dc, &rc, border_brush);

        let dpi = self.dpi;
        let scale = |v: f32| metrics::scale(v, dpi) as i32;

        SetBkMode(mem_dc, TRANSPARENT);

        if let PopupModel::List(list) = &self.model {
            let font_header = CreateFontW(
                -scale(12.0),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(0),
                0u32,
                w!("Segoe UI"),
            );
            let font_row = CreateFontW(
                -scale(18.0),
                0,
                0,
                0,
                FW_SEMIBOLD.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(0),
                0u32,
                w!("Segoe UI"),
            );
            let font_marker = CreateFontW(
                -scale(11.0),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(0),
                0u32,
                w!("Segoe UI"),
            );

            // 1. Header: Latin buffer on left, dialect badge on right
            let header_h = scale(metrics::HEADER_H);
            let pad_x = scale(metrics::PAD_X);
            let mut latin_rc = RECT {
                left: pad_x,
                top: 0,
                right: width / 2,
                bottom: header_h,
            };
            SetTextColor(mem_dc, to_colorref(self.theme.secondary));
            let old_font = SelectObject(mem_dc, font_header.into());

            let mut latin_utf16: Vec<u16> = list.latin.encode_utf16().collect();
            let _ = draw_text(
                mem_dc,
                &mut latin_utf16,
                &mut latin_rc,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );

            if let Some(badge) = &list.badge {
                let mut badge_rc = RECT {
                    left: width / 2,
                    top: 0,
                    right: width - pad_x,
                    bottom: header_h,
                };
                let mut badge_utf16: Vec<u16> = format!("[ {badge} ]").encode_utf16().collect();
                let _ = draw_text(
                    mem_dc,
                    &mut badge_utf16,
                    &mut badge_rc,
                    DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }

            // 2. Rows
            let row_h = scale(metrics::ROW_H);
            let mut y = header_h;

            for (idx, row) in list.rows.iter().enumerate() {
                let row_rc = RECT {
                    left: 1,
                    top: y,
                    right: width - 1,
                    bottom: y + row_h,
                };

                // Highlighted row
                if idx == list.highlighted {
                    // Tinted background
                    let tint_bg = CreateSolidBrush(to_colorref(self.theme.bg));
                    let _ = FillRect(mem_dc, &row_rc, tint_bg);
                    let _ = DeleteObject(tint_bg.into());

                    // Accent bar on the RIGHT edge (RTL)
                    let bar_w = scale(metrics::ACCENT_BAR);
                    let bar_rc = RECT {
                        left: width - 1 - bar_w,
                        top: y,
                        right: width - 1,
                        bottom: y + row_h,
                    };
                    let _ = FillRect(mem_dc, &bar_rc, accent_brush);
                }

                // Row marker at the left end
                match row.marker {
                    RowMarker::Completion => {
                        let _ = SelectObject(mem_dc, font_marker.into());
                        SetTextColor(mem_dc, to_colorref(self.theme.secondary));
                        let mut marker_rc = RECT {
                            left: pad_x,
                            top: y,
                            right: pad_x + scale(20.0),
                            bottom: y + row_h,
                        };
                        let mut m_utf16 = vec![0x22EF]; // ⋯
                        let _ = draw_text(
                            mem_dc,
                            &mut m_utf16,
                            &mut marker_rc,
                            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                        );
                    }
                    RowMarker::RawLatin => {
                        let _ = SelectObject(mem_dc, font_marker.into());
                        SetTextColor(mem_dc, to_colorref(self.theme.secondary));
                        let mut marker_rc = RECT {
                            left: pad_x,
                            top: y,
                            right: pad_x + scale(24.0),
                            bottom: y + row_h,
                        };
                        let mut m_utf16: Vec<u16> = "EN".encode_utf16().collect();
                        let _ = draw_text(
                            mem_dc,
                            &mut m_utf16,
                            &mut marker_rc,
                            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                        );
                    }
                    _ => {}
                }

                // Row text
                let _ = SelectObject(mem_dc, font_row.into());
                SetTextColor(mem_dc, to_colorref(self.theme.text));

                let mut text_rc = RECT {
                    left: pad_x + scale(24.0),
                    top: y,
                    right: width - pad_x - scale(metrics::ACCENT_BAR),
                    bottom: y + row_h,
                };
                let mut text_utf16: Vec<u16> = row.text.encode_utf16().collect();
                let flags = if row.rtl {
                    DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX
                } else {
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX
                };
                let _ = draw_text(mem_dc, &mut text_utf16, &mut text_rc, flags);

                y += row_h;
            }

            // 3. Footer
            if let Footer::Hints = list.footer {
                let _ = SelectObject(mem_dc, font_marker.into());
                SetTextColor(mem_dc, to_colorref(self.theme.secondary));
                let mut footer_rc = RECT {
                    left: pad_x,
                    top: y,
                    right: width - pad_x,
                    bottom: y + scale(metrics::FOOTER_H),
                };
                let mut f_utf16: Vec<u16> = crate::FOOTER_HINTS_AR.encode_utf16().collect();
                let _ = draw_text(
                    mem_dc,
                    &mut f_utf16,
                    &mut footer_rc,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }

            let _ = SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font_header.into());
            let _ = DeleteObject(font_row.into());
            let _ = DeleteObject(font_marker.into());
        }

        if let PopupModel::Tashkeel(tashkeel) = &self.model {
            let font_chip = CreateFontW(
                -scale(12.0),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(0),
                0u32,
                w!("Segoe UI"),
            );
            let font_word = CreateFontW(
                -scale(34.0),
                0,
                0,
                0,
                FW_SEMIBOLD.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(0),
                0u32,
                w!("Segoe UI"),
            );
            let font_palette = CreateFontW(
                -scale(13.0),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(0),
                0u32,
                w!("Segoe UI"),
            );
            let font_footer = CreateFontW(
                -scale(11.0),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(0),
                0u32,
                w!("Segoe UI"),
            );

            let pad_x = scale(metrics::PAD_X);
            let chip_bar_h = scale(32.0);
            let word_h = scale(64.0);
            let palette_h = scale(36.0);

            // 1. Quick picks bar (top, RTL chips)
            let mut chip_x = width - pad_x;
            let old_font = SelectObject(mem_dc, font_chip.into());

            for (idx, pick) in tashkeel.picks.iter().take(8).enumerate() {
                let badge_suffix = if idx == 0 && tashkeel.from_typing {
                    " ✦من كتابتك"
                } else {
                    ""
                };
                let num_symbol = match idx {
                    0 => "①",
                    1 => "②",
                    2 => "③",
                    3 => "④",
                    4 => "⑤",
                    5 => "⑥",
                    6 => "⑦",
                    _ => "⑧",
                };
                let chip_text = format!("{num_symbol} {pick}{badge_suffix}");
                let mut chip_utf16: Vec<u16> = chip_text.encode_utf16().collect();

                let mut calc_rc = RECT::default();
                let _ = draw_text(
                    mem_dc,
                    &mut chip_utf16,
                    &mut calc_rc,
                    DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
                );
                let chip_w = (calc_rc.right - calc_rc.left) + scale(12.0);
                let chip_left = chip_x - chip_w;
                if chip_left < pad_x {
                    break;
                }

                let chip_rc = RECT {
                    left: chip_left,
                    top: scale(4.0),
                    right: chip_x,
                    bottom: chip_bar_h - scale(4.0),
                };

                if tashkeel.highlighted_pick == Some(idx) {
                    let _ = FillRect(mem_dc, &chip_rc, accent_brush);
                    SetTextColor(mem_dc, COLORREF(0x00FFFFFF));
                } else {
                    let _ = FrameRect(mem_dc, &chip_rc, border_brush);
                    SetTextColor(mem_dc, to_colorref(self.theme.text));
                }

                let mut text_rc = chip_rc;
                let _ = draw_text(
                    mem_dc,
                    &mut chip_utf16,
                    &mut text_rc,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );

                chip_x = chip_left - scale(6.0);
            }

            // Divider below chips
            let div1_rc = RECT {
                left: 1,
                top: chip_bar_h,
                right: width - 1,
                bottom: chip_bar_h + 1,
            };
            let _ = FillRect(mem_dc, &div1_rc, border_brush);

            // 2. Main word display
            let word_y = chip_bar_h;
            let mut word_rc = RECT {
                left: pad_x,
                top: word_y,
                right: width - pad_x,
                bottom: word_y + word_h,
            };
            let _ = SelectObject(mem_dc, font_word.into());
            SetTextColor(mem_dc, to_colorref(self.theme.text));
            let mut word_utf16: Vec<u16> = tashkeel.word.encode_utf16().collect();
            let _ = draw_text(
                mem_dc,
                &mut word_utf16,
                &mut word_rc,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );

            // Divider below word
            let div2_y = word_y + word_h;
            let div2_rc = RECT {
                left: 1,
                top: div2_y,
                right: width - 1,
                bottom: div2_y + 1,
            };
            let _ = FillRect(mem_dc, &div2_rc, border_brush);

            // 3. Mark palette (10 items)
            let palette_y = div2_y + 1;
            let num_items = crate::MARK_PALETTE.len();
            let item_w = (width - 2 * pad_x) / num_items as i32;
            let _ = SelectObject(mem_dc, font_palette.into());

            for (idx, &(mark, key_hint, _name)) in crate::MARK_PALETTE.iter().enumerate() {
                let item_left = pad_x + idx as i32 * item_w;
                let item_rc = RECT {
                    left: item_left + scale(2.0),
                    top: palette_y + scale(4.0),
                    right: item_left + item_w - scale(2.0),
                    bottom: palette_y + palette_h - scale(4.0),
                };

                let _ = FrameRect(mem_dc, &item_rc, border_brush);

                let mark_str = if mark == '\u{2715}' {
                    format!("✕ {key_hint}")
                } else {
                    format!("◌{mark} {key_hint}")
                };
                let mut mark_utf16: Vec<u16> = mark_str.encode_utf16().collect();
                SetTextColor(mem_dc, to_colorref(self.theme.text));
                let mut text_rc = item_rc;
                let _ = draw_text(
                    mem_dc,
                    &mut mark_utf16,
                    &mut text_rc,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }

            // Divider below palette
            let div3_y = palette_y + palette_h;
            let div3_rc = RECT {
                left: 1,
                top: div3_y,
                right: width - 1,
                bottom: div3_y + 1,
            };
            let _ = FillRect(mem_dc, &div3_rc, border_brush);

            // 4. Footer hints
            let footer_y = div3_y + 1;
            let mut footer_rc = RECT {
                left: pad_x,
                top: footer_y,
                right: width - pad_x,
                bottom: height - 1,
            };
            let _ = SelectObject(mem_dc, font_footer.into());
            SetTextColor(mem_dc, to_colorref(self.theme.secondary));
            let footer_text = "Enter: إدراج · Esc: رجوع · ←→: حرف";
            let mut f_utf16: Vec<u16> = footer_text.encode_utf16().collect();
            let _ = draw_text(
                mem_dc,
                &mut f_utf16,
                &mut footer_rc,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );

            let _ = SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font_chip.into());
            let _ = DeleteObject(font_word.into());
            let _ = DeleteObject(font_palette.into());
            let _ = DeleteObject(font_footer.into());
        }

        // Copy memory DC to screen DC
        let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY);

        let _ = SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(mem_bmp.into());
        let _ = DeleteDC(mem_dc);
        let _ = DeleteObject(bg_brush.into());
        let _ = DeleteObject(border_brush.into());
        let _ = DeleteObject(accent_brush.into());
    }
}

impl Drop for PopupWindow {
    fn drop(&mut self) {
        // SAFETY: we own the window; clear the state pointer first so no late message can read it.
        unsafe {
            if !self.hwnd.is_invalid() {
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<PopupState>;
            // SAFETY: the pointer is 0 or points into the owning PopupWindow's Box, and it is
            // cleared before that Box is freed (see Drop). Panics must not unwind into user32.
            if !ptr.is_null() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Ok(state) = (*ptr).try_borrow() {
                        state.paint(hwnd, hdc);
                    }
                }));
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
