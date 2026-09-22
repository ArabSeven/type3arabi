//! Windows candidate popup window implementation.

use crate::{metrics, Footer, PopupModel, RowMarker, Theme};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GetMonitorInfoW,
    MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_LEFT, DT_NOPREFIX,
    DT_RIGHT, DT_SINGLELINE, DT_VCENTER, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION,
    FONT_QUALITY, FW_NORMAL, FW_SEMIBOLD, HDC, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
    SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW,
    RegisterClassExW, SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_DROPSHADOW, CS_HREDRAW,
    CS_VREDRAW, GWLP_USERDATA, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOWNOACTIVATE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSEXW,
    WS_CLIPSIBLINGS, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
const WINDOW_CLASS: PCWSTR = w!("Type3arabi_CandidateWindow");

pub struct PopupWindow {
    hwnd: HWND,
    model: PopupModel,
    theme: Theme,
    dpi: u32,
    on_candidate_clicked: Option<Box<dyn Fn(usize) + 'static>>,
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
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            let hinstance = HINSTANCE(hinst.0);
            if !CLASS_REGISTERED.load(Ordering::Acquire) {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
                    lpfnWndProc: Some(wndproc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance,
                    hIcon: Default::default(),
                    hCursor: Default::default(),
                    hbrBackground: Default::default(),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: WINDOW_CLASS,
                    hIconSm: Default::default(),
                };
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

            let mut popup = Self {
                hwnd,
                model: PopupModel::Hidden,
                theme: Theme::LIGHT,
                dpi: 96,
                on_candidate_clicked: None,
            };

            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut popup as *mut _ as usize as isize) as _,
            );
            Ok(popup)
        }
    }

    pub fn set_on_click<F: Fn(usize) + 'static>(&mut self, f: F) {
        self.on_candidate_clicked = Some(Box::new(f));
    }

    pub fn is_visible(&self) -> bool {
        !matches!(self.model, PopupModel::Hidden)
    }

    pub fn hide(&mut self) {
        self.model = PopupModel::Hidden;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn show(&mut self, model: PopupModel, anchor: (i32, i32, i32, i32)) {
        if let PopupModel::Hidden = model {
            self.hide();
            return;
        }

        self.model = model;
        unsafe {
            // Update userdata pointer before painting
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                (self as *mut _ as usize as isize) as _,
            );

            let (_anchor_left, anchor_top, anchor_right, anchor_bottom) = anchor;
            let (width, height) = self.calculate_size();

            // Screen/Monitor bounds
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
            if x < work.left {
                x = work.left;
            }
            if x + width > work.right {
                x = work.right - width;
            }

            // Place below composition by default; flip above if not enough room
            let mut y = anchor_bottom + 4;
            if y + height > work.bottom {
                let y_above = anchor_top - height - 4;
                if y_above >= work.top {
                    y = y_above;
                } else {
                    y = work.bottom - height;
                }
            }

            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

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
            PopupModel::Tashkeel(_) => (scale(320.0), scale(180.0)),
            PopupModel::Hidden => (0, 0),
        }
    }

    unsafe fn paint(&self, hdc: HDC) {
        let mut rc = RECT::default();
        let _ = GetClientRect(self.hwnd, &mut rc);
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
            let _ = DrawTextW(
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
                let _ = DrawTextW(
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
                        let _ = DrawTextW(
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
                        let _ = DrawTextW(
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
                let _ = DrawTextW(mem_dc, &mut text_utf16, &mut text_rc, flags);

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
                let _ = DrawTextW(
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
        unsafe {
            if !self.hwnd.is_invalid() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PopupWindow;

    match msg {
        WM_MOUSEACTIVATE => LRESULT((MA_NOACTIVATE as isize) as _),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !ptr.is_null() {
                (*ptr).paint(hdc);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if !ptr.is_null() {
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let dpi = (*ptr).dpi;
                let scale = |v: f32| metrics::scale(v, dpi) as i32;
                let header_h = scale(metrics::HEADER_H);
                let row_h = scale(metrics::ROW_H);
                if y >= header_h {
                    let row_idx = ((y - header_h) / row_h) as usize;
                    if let Some(cb) = &(*ptr).on_candidate_clicked {
                        cb(row_idx);
                    }
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
