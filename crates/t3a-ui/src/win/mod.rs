//! Windows candidate popup and tashkeel editor window (docs/02 §9, docs/05 §3–4).
//!
//! Ownership: `PopupWindow` owns the HWND and a heap-allocated `RefCell<PopupState>` whose address
//! is stable for the window's whole life; the window procedure reaches the state only through that
//! pointer and only with `try_borrow*`, so it can never observe freed or aliased memory.
//!
//! Mouse: the window never activates (WM_MOUSEACTIVATE → MA_NOACTIVATE), so clicking it keeps the
//! host's focus and caret. Paint records the clickable rectangles (`targets`); clicks and the wheel
//! become `PopupEvent`s delivered to the TIP's handler with no borrow held (the handler usually
//! re-renders this window).

use crate::{
    metrics, Footer, ListModel, PopupEvent, PopupModel, RowMarker, TashkeelModel, Theme,
    CLEAR_ALL_AR, LIST_HINTS, MARK_PALETTE, TASHKEEL_HINTS,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    GetMonitorInfoW, GetStockObject, GetTextExtentPoint32W, InvalidateRect, MonitorFromPoint,
    ReleaseDC, RoundRect, SelectObject, SetBkMode, SetTextColor, DRAW_TEXT_FORMAT, DT_CENTER,
    DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_RTLREADING, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK,
    FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, FW_NORMAL, FW_SEMIBOLD,
    HDC, HFONT, MONITORINFO, MONITOR_DEFAULTTONEAREST, NULL_BRUSH, NULL_PEN, PAINTSTRUCT, PS_SOLID,
    SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, LoadCursorW,
    RegisterClassExW, SetWindowLongPtrW, SetWindowPos, ShowWindow, UnregisterClassW, CS_DROPSHADOW,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WNDCLASSEXW, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
const WINDOW_CLASS: PCWSTR = w!("Type3arabi_CandidateWindow");

const MK_LBUTTON: usize = 0x0001;
const MK_SHIFT: usize = 0x0004;
const MK_CONTROL: usize = 0x0008;
const ZWJ: char = '\u{200D}';
const DOTTED_CIRCLE: char = '\u{25CC}';

/// Tashkeel editor geometry at 96 DPI.
mod tk {
    pub const WIDTH: f32 = 560.0;
    pub const TOP_H: f32 = 40.0;
    pub const WORD_H: f32 = 84.0;
    pub const WORD_FONT: f32 = 42.0;
    pub const PALETTE_H: f32 = 100.0;
    pub const FOOTER_H: f32 = 24.0;
    pub const CELL_GAP: f32 = 4.0;
}

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

/// Receiver of mouse input on the popup (the TIP).
pub type Handler = Rc<dyn Fn(PopupEvent)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Row(usize),
    Letter(usize),
    Mark(usize),
    ClearAll,
    Pick(usize),
}

struct PopupState {
    model: PopupModel,
    theme: Theme,
    dpi: u32,
    /// Clickable rectangles recorded by the last paint.
    targets: Vec<(RECT, Hit)>,
    handler: Option<Handler>,
    /// Letter under the pointer while dragging a selection.
    drag_letter: Option<usize>,
}

pub struct PopupWindow {
    hwnd: HWND,
    state: Box<RefCell<PopupState>>,
}

// Convert 0xRRGGBB to GDI COLORREF (0x00BBGGRR)
fn to_colorref(rgb: u32) -> COLORREF {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    COLORREF((b << 16) | (g << 8) | r)
}

/// `a` blended over `b` with weight `t` (0..=1), both 0xRRGGBB.
fn blend(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let x = ((a >> shift) & 0xFF) as f32;
        let y = ((b >> shift) & 0xFF) as f32;
        ((x * t + y * (1.0 - t)).round() as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
}

fn contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
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
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
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
                targets: Vec::new(),
                handler: None,
                drag_letter: None,
            }));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const _ as _);
            Ok(Self { hwnd, state })
        }
    }

    /// Install the receiver of mouse events.
    pub fn set_handler(&mut self, handler: Handler) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.handler = Some(handler);
        }
    }

    /// The window handle (tests drive it with synthetic mouse messages).
    pub fn hwnd(&self) -> HWND {
        self.hwnd
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
            s.targets.clear();
            s.drag_letter = None;
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
            s.size()
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

// ---------------------------------------------------------------------------------------------
// GDI helpers (all callers hold a valid DC)

unsafe fn font(px: i32, weight: i32) -> HFONT {
    CreateFontW(
        -px,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        // ARABIC_CHARSET: GDI honours DT_RTLREADING (right-to-left paragraph order for mixed
        // Arabic/Latin labels) only for fonts created with a Hebrew or Arabic charset.
        FONT_CHARSET(178),
        FONT_OUTPUT_PRECISION(0),
        FONT_CLIP_PRECISION(0),
        FONT_QUALITY(5), // CLEARTYPE_QUALITY
        0u32,
        w!("Segoe UI"),
    )
}

/// `DrawTextW` that is safe for empty strings: an empty Rust slice has a dangling pointer, and
/// user32 dereferences it even with a zero count (host crash 0xC000041D, found by tsf_harness).
unsafe fn draw(dc: HDC, text: &str, rect: RECT, format: DRAW_TEXT_FORMAT) {
    let mut w: Vec<u16> = text.encode_utf16().collect();
    if w.is_empty() {
        return;
    }
    let mut r = rect;
    DrawTextW(dc, &mut w, &mut r, format | DT_NOPREFIX | DT_SINGLELINE);
}

/// Centered, word-wrapped text (palette names that need two lines).
unsafe fn draw_wrapped(dc: HDC, text: &str, rect: RECT) {
    let mut w: Vec<u16> = text.encode_utf16().collect();
    if w.is_empty() {
        return;
    }
    let mut r = rect;
    DrawTextW(
        dc,
        &mut w,
        &mut r,
        DT_CENTER | DT_WORDBREAK | DT_NOPREFIX | DT_RTLREADING,
    );
}

/// Width of `text` in the DC's current font (shaped: Arabic joining applies).
unsafe fn extent(dc: HDC, text: &str) -> i32 {
    let w: Vec<u16> = text.encode_utf16().collect();
    if w.is_empty() {
        return 0;
    }
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &w, &mut size);
    size.cx
}

unsafe fn fill(dc: HDC, r: RECT, rgb: u32) {
    let b = CreateSolidBrush(to_colorref(rgb));
    FillRect(dc, &r, b);
    let _ = DeleteObject(b.into());
}

/// Rounded rectangle: `fill_rgb` interior (None = hollow), 1-px `line_rgb` border (None = none).
unsafe fn round(dc: HDC, r: RECT, radius: i32, fill_rgb: Option<u32>, line_rgb: Option<u32>) {
    let brush = fill_rgb.map(|c| CreateSolidBrush(to_colorref(c)));
    let pen = line_rgb.map(|c| CreatePen(PS_SOLID, 1, to_colorref(c)));
    let old_brush = SelectObject(dc, brush.map_or(GetStockObject(NULL_BRUSH), |b| b.into()));
    let old_pen = SelectObject(dc, pen.map_or(GetStockObject(NULL_PEN), |p| p.into()));
    let _ = RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, old_brush);
    SelectObject(dc, old_pen);
    if let Some(b) = brush {
        let _ = DeleteObject(b.into());
    }
    if let Some(p) = pen {
        let _ = DeleteObject(p.into());
    }
}

/// Letter spans of an Arabic word with marks: (char index of the letter, char index after its marks).
fn letter_spans(word: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = word.chars().collect();
    let is_mark = |c: char| ('\u{064B}'..='\u{0652}').contains(&c) || c == '\u{0670}';
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        i += 1;
        while i < chars.len() && is_mark(chars[i]) {
            i += 1;
        }
        spans.push((start, i));
    }
    spans
}

impl PopupState {
    fn px(&self, v: f32) -> i32 {
        metrics::scale(v, self.dpi).round() as i32
    }

    /// Window size for the current model.
    fn size(&self) -> (i32, i32) {
        match &self.model {
            PopupModel::List(list) => {
                let header = self.px(metrics::HEADER_H);
                let rows = list.rows.len() as i32 * self.px(metrics::ROW_H);
                let footer = match list.footer {
                    Footer::Hidden => 0,
                    _ => self.px(metrics::FOOTER_H),
                };
                // Widest row decides the width (measured with the row font on a screen DC).
                // SAFETY: screen DC acquired and released here; fonts deleted.
                let widest = unsafe {
                    let dc = GetDC(None);
                    let f = font(self.px(18.0), FW_SEMIBOLD.0 as i32);
                    let old = SelectObject(dc, f.into());
                    let w = list
                        .rows
                        .iter()
                        .map(|r| extent(dc, &r.text))
                        .max()
                        .unwrap_or(0);
                    SelectObject(dc, old);
                    let _ = DeleteObject(f.into());
                    ReleaseDC(None, dc);
                    w
                };
                let width = (widest + self.px(2.0 * metrics::PAD_X + 40.0))
                    .max(self.px(300.0)) // fits the keycap hint row
                    .min(self.px(metrics::MAX_WIDTH));
                (width, header + rows + footer + 2)
            }
            PopupModel::Tashkeel(_) => (
                self.px(tk::WIDTH),
                self.px(tk::TOP_H + tk::WORD_H + tk::PALETTE_H + tk::FOOTER_H) + 4,
            ),
            PopupModel::Hidden => (0, 0),
        }
    }

    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        self.targets
            .iter()
            .find(|(r, _)| contains(r, x, y))
            .map(|&(_, h)| h)
    }

    unsafe fn paint(&mut self, hwnd: HWND, hdc: HDC) {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (width, height) = (rc.right - rc.left, rc.bottom - rc.top);
        if width <= 0 || height <= 0 {
            return;
        }
        // Double buffer
        let dc = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, width, height);
        let old_bmp = SelectObject(dc, bmp.into());
        fill(dc, rc, self.theme.bg);
        round(dc, rc, 0, None, Some(self.theme.border));
        SetBkMode(dc, TRANSPARENT);
        self.targets.clear();

        let model = self.model.clone();
        match &model {
            PopupModel::List(list) => self.paint_list(dc, width, list),
            PopupModel::Tashkeel(t) => self.paint_tashkeel(dc, width, t),
            PopupModel::Hidden => {}
        }

        let _ = BitBlt(hdc, 0, 0, width, height, Some(dc), 0, 0, SRCCOPY);
        SelectObject(dc, old_bmp);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
    }

    /// A centered row of hints laid out right-to-left: each hint is a Latin keycap followed (to its
    /// left) by its Arabic label. Each piece is single-direction, so no bidi reordering is needed.
    unsafe fn draw_hints(
        &self,
        dc: HDC,
        hints: &[(&str, &str)],
        area: RECT,
        key_font: HFONT,
        label_font: HFONT,
    ) {
        let th = self.theme;
        let (cap_pad, inner, gap) = (self.px(5.0), self.px(4.0), self.px(12.0));
        let mut sizes = Vec::with_capacity(hints.len());
        for (key, label) in hints {
            SelectObject(dc, key_font.into());
            let kw = extent(dc, key) + 2 * cap_pad;
            SelectObject(dc, label_font.into());
            sizes.push((kw, extent(dc, label)));
        }
        let total: i32 = sizes.iter().map(|(k, l)| k + inner + l).sum::<i32>()
            + gap * (hints.len() as i32 - 1).max(0);
        let mut x = (area.left + area.right + total) / 2;
        let cap_top = area.top + self.px(4.0);
        let cap_bottom = area.bottom - self.px(4.0);
        for ((key, label), (kw, lw)) in hints.iter().zip(sizes) {
            let cap = RECT {
                left: x - kw,
                top: cap_top,
                right: x,
                bottom: cap_bottom,
            };
            round(
                dc,
                cap,
                self.px(4.0),
                Some(blend(th.border, th.bg, 0.35)),
                Some(th.border),
            );
            SelectObject(dc, key_font.into());
            SetTextColor(dc, to_colorref(th.text));
            draw(dc, key, cap, DT_CENTER | DT_VCENTER);
            x -= kw + inner;
            SelectObject(dc, label_font.into());
            SetTextColor(dc, to_colorref(th.secondary));
            draw(
                dc,
                label,
                RECT {
                    left: x - lw,
                    right: x,
                    ..area
                },
                DT_RIGHT | DT_VCENTER | DT_RTLREADING,
            );
            x -= lw + gap;
        }
    }

    unsafe fn paint_list(&mut self, dc: HDC, width: i32, list: &ListModel) {
        let th = self.theme;
        let pad = self.px(metrics::PAD_X);
        let f_small = font(self.px(12.0), FW_NORMAL.0 as i32);
        let f_row = font(self.px(18.0), FW_SEMIBOLD.0 as i32);
        let f_mark = font(self.px(11.0), FW_NORMAL.0 as i32);
        let old_font = SelectObject(dc, f_small.into());

        // Header: Latin buffer (left), dialect badge (right)
        let header_h = self.px(metrics::HEADER_H);
        SetTextColor(dc, to_colorref(th.secondary));
        draw(
            dc,
            &list.latin,
            RECT {
                left: pad,
                top: 0,
                right: width / 2,
                bottom: header_h,
            },
            DT_LEFT | DT_VCENTER,
        );
        if let Some(badge) = &list.badge {
            draw(
                dc,
                &format!("[ {badge} ]"),
                RECT {
                    left: width / 2,
                    top: 0,
                    right: width - pad,
                    bottom: header_h,
                },
                DT_RIGHT | DT_VCENTER | DT_RTLREADING,
            );
        }

        // Rows
        let row_h = self.px(metrics::ROW_H);
        let mut y = header_h;
        for (idx, row) in list.rows.iter().enumerate() {
            let row_rc = RECT {
                left: 1,
                top: y,
                right: width - 1,
                bottom: y + row_h,
            };
            if idx == list.highlighted {
                fill(dc, row_rc, blend(th.accent, th.bg, 0.12));
                let bar = self.px(metrics::ACCENT_BAR);
                fill(
                    dc,
                    RECT {
                        left: width - 1 - bar,
                        ..row_rc
                    },
                    th.accent,
                );
            }
            let marker = match row.marker {
                RowMarker::Completion => "\u{22EF}",
                RowMarker::RawLatin => "EN",
                RowMarker::Custom => "\u{2605}",
                RowMarker::None => "",
            };
            SelectObject(dc, f_mark.into());
            SetTextColor(dc, to_colorref(th.secondary));
            draw(
                dc,
                marker,
                RECT {
                    left: pad,
                    right: pad + self.px(24.0),
                    ..row_rc
                },
                DT_LEFT | DT_VCENTER,
            );
            SelectObject(dc, f_row.into());
            SetTextColor(dc, to_colorref(th.text));
            let text_rc = RECT {
                left: pad + self.px(24.0),
                right: width - pad - self.px(metrics::ACCENT_BAR),
                ..row_rc
            };
            let flags = if row.rtl {
                DT_RIGHT | DT_VCENTER | DT_RTLREADING
            } else {
                DT_LEFT | DT_VCENTER
            };
            draw(dc, &row.text, text_rc, flags);
            self.targets.push((row_rc, Hit::Row(idx)));
            y += row_h;
        }

        // Footer
        let footer_rc = RECT {
            left: pad,
            top: y,
            right: width - pad,
            bottom: y + self.px(metrics::FOOTER_H),
        };
        SelectObject(dc, f_mark.into());
        SetTextColor(dc, to_colorref(th.secondary));
        match list.footer {
            Footer::Hints => self.draw_hints(dc, &LIST_HINTS, footer_rc, f_mark, f_mark),
            Footer::Paging { page, pages } => draw(
                dc,
                &format!("{page}/{pages} \u{25BE}"),
                footer_rc,
                DT_CENTER | DT_VCENTER,
            ),
            Footer::Hidden => {}
        }

        SelectObject(dc, old_font);
        for f in [f_small, f_row, f_mark] {
            let _ = DeleteObject(f.into());
        }
    }

    unsafe fn paint_tashkeel(&mut self, dc: HDC, width: i32, t: &TashkeelModel) {
        let th = self.theme;
        let pad = self.px(metrics::PAD_X);
        let f_chip = font(self.px(13.0), FW_NORMAL.0 as i32);
        let f_word = font(self.px(tk::WORD_FONT), FW_SEMIBOLD.0 as i32);
        let f_mark = font(self.px(30.0), FW_NORMAL.0 as i32);
        let f_name = font(self.px(11.0), FW_NORMAL.0 as i32);
        let f_key = font(self.px(11.0), FW_SEMIBOLD.0 as i32);
        let old_font = SelectObject(dc, f_chip.into());

        // 1. Top bar: "clear all" button (left), quick-pick chips (right, RTL)
        let top_h = self.px(tk::TOP_H);
        let chip_top = self.px(7.0);
        let chip_bottom = top_h - self.px(7.0);
        let clear_w = extent(dc, CLEAR_ALL_AR) + extent(dc, "\u{2715}") + self.px(26.0);
        let clear_rc = RECT {
            left: pad,
            top: chip_top,
            right: pad + clear_w,
            bottom: chip_bottom,
        };
        round(
            dc,
            clear_rc,
            self.px(8.0),
            Some(blend(0xC42B1C, th.bg, 0.08)),
            Some(blend(0xC42B1C, th.bg, 0.55)),
        );
        SetTextColor(dc, to_colorref(0xC42B1C));
        let icon_w = extent(dc, "\u{2715}");
        draw(
            dc,
            "\u{2715}",
            RECT {
                left: clear_rc.right - self.px(8.0) - icon_w,
                right: clear_rc.right - self.px(8.0),
                ..clear_rc
            },
            DT_LEFT | DT_VCENTER,
        );
        draw(
            dc,
            CLEAR_ALL_AR,
            RECT {
                left: clear_rc.left + self.px(8.0),
                right: clear_rc.right - self.px(14.0) - icon_w,
                ..clear_rc
            },
            DT_RIGHT | DT_VCENTER | DT_RTLREADING,
        );
        self.targets.push((clear_rc, Hit::ClearAll));

        let circled = ['①', '②', '③', '④', '⑤', '⑥', '⑦', '⑧'];
        let mut chip_x = width - pad;
        for (idx, pick) in t.picks.iter().take(8).enumerate() {
            let badge = if idx == 0 && t.from_typing {
                " \u{2726}من كتابتك"
            } else {
                ""
            };
            let num = circled[idx].to_string();
            let label = format!("{pick}{badge}");
            let num_w = extent(dc, &num);
            let chip_w = num_w + extent(dc, &label) + self.px(22.0);
            let left = chip_x - chip_w;
            if left < clear_rc.right + self.px(8.0) {
                break;
            }
            let chip_rc = RECT {
                left,
                top: chip_top,
                right: chip_x,
                bottom: chip_bottom,
            };
            let on = t.highlighted_pick == Some(idx);
            round(
                dc,
                chip_rc,
                self.px(8.0),
                Some(if on { th.accent } else { th.bg }),
                Some(if on { th.accent } else { th.border }),
            );
            SetTextColor(dc, to_colorref(if on { 0xFFFFFF } else { th.text }));
            draw(
                dc,
                &num,
                RECT {
                    left: chip_rc.right - self.px(8.0) - num_w,
                    right: chip_rc.right - self.px(8.0),
                    ..chip_rc
                },
                DT_LEFT | DT_VCENTER,
            );
            draw(
                dc,
                &label,
                RECT {
                    left: chip_rc.left + self.px(6.0),
                    right: chip_rc.right - self.px(12.0) - num_w,
                    ..chip_rc
                },
                DT_RIGHT | DT_VCENTER | DT_RTLREADING,
            );
            self.targets.push((chip_rc, Hit::Pick(idx)));
            chip_x = left - self.px(6.0);
        }
        fill(
            dc,
            RECT {
                left: 1,
                top: top_h,
                right: width - 1,
                bottom: top_h + 1,
            },
            th.border,
        );

        // 2. The word, with the selected letters highlighted and the focused one underlined
        let word_top = top_h + 1;
        let word_h = self.px(tk::WORD_H);
        SelectObject(dc, f_word.into());
        let chars: Vec<char> = t.word.chars().collect();
        let spans = letter_spans(&t.word);
        let total = extent(dc, &t.word);
        let x_right = width / 2 + total / 2;
        // Right edge offset of each letter = shaped width of the word up to (and incl.) it. A ZWJ
        // keeps the prefix's last letter in its joining form so widths match the whole word.
        let prefix_width = |end: usize, joins: bool| {
            let mut s: String = chars[..end].iter().collect();
            if joins {
                s.push(ZWJ);
            }
            extent(dc, &s)
        };
        let mut edges = vec![0];
        for (k, &(_, end)) in spans.iter().enumerate() {
            edges.push(prefix_width(end, k + 1 < spans.len()));
        }
        let hl_top = word_top + self.px(8.0);
        let hl_bottom = word_top + word_h - self.px(8.0);
        for k in 0..spans.len() {
            let r = RECT {
                left: x_right - edges[k + 1],
                top: hl_top,
                right: x_right - edges[k],
                bottom: hl_bottom,
            };
            if t.selected.contains(&k) {
                round(
                    dc,
                    RECT {
                        left: r.left - self.px(2.0),
                        right: r.right + self.px(2.0),
                        ..r
                    },
                    self.px(6.0),
                    Some(blend(th.accent, th.bg, 0.18)),
                    Some(blend(th.accent, th.bg, 0.45)),
                );
            }
            if k == t.focused_letter {
                fill(
                    dc,
                    RECT {
                        left: r.left,
                        top: hl_bottom - self.px(4.0),
                        right: r.right,
                        bottom: hl_bottom - self.px(1.0),
                    },
                    th.accent,
                );
            }
            // generous click targets: full height of the word area
            self.targets.push((
                RECT {
                    top: word_top,
                    bottom: word_top + word_h,
                    ..r
                },
                Hit::Letter(k),
            ));
        }
        SetTextColor(dc, to_colorref(th.text));
        draw(
            dc,
            &t.word,
            RECT {
                left: x_right - total - self.px(4.0),
                top: word_top,
                right: x_right + self.px(4.0),
                bottom: word_top + word_h,
            },
            DT_CENTER | DT_VCENTER | DT_RTLREADING,
        );
        let pal_top = word_top + word_h;
        fill(
            dc,
            RECT {
                left: 1,
                top: pal_top,
                right: width - 1,
                bottom: pal_top + 1,
            },
            th.border,
        );

        // 3. Palette: one cell per mark, three rows each — mark on ◌, Arabic name, key
        let gap = self.px(tk::CELL_GAP);
        let n = MARK_PALETTE.len() as i32;
        let cell_w = (width - 2 * pad - (n - 1) * gap) / n;
        let cell_top = pal_top + self.px(6.0);
        let cell_bottom = pal_top + self.px(tk::PALETTE_H) - self.px(4.0);
        let row1 = cell_top + self.px(38.0);
        let row2 = row1 + self.px(30.0); // room for a two-line name
        for (i, &(mark, key, name)) in MARK_PALETTE.iter().enumerate() {
            // RTL order: the first mark sits at the right edge
            let right = width - pad - i as i32 * (cell_w + gap);
            let cell = RECT {
                left: right - cell_w,
                top: cell_top,
                right,
                bottom: cell_bottom,
            };
            round(dc, cell, self.px(8.0), Some(th.bg), Some(th.border));
            let glyph = if mark == '\u{2715}' {
                mark.to_string()
            } else {
                format!("{DOTTED_CIRCLE}{mark}")
            };
            SelectObject(dc, f_mark.into());
            SetTextColor(dc, to_colorref(th.text));
            draw(
                dc,
                &glyph,
                RECT {
                    bottom: row1,
                    ..cell
                },
                DT_CENTER | DT_VCENTER,
            );
            SelectObject(dc, f_name.into());
            SetTextColor(dc, to_colorref(th.secondary));
            draw_wrapped(
                dc,
                name,
                RECT {
                    left: cell.left + self.px(2.0),
                    top: row1,
                    right: cell.right - self.px(2.0),
                    bottom: row2,
                },
            );
            SelectObject(dc, f_key.into());
            let key_w = extent(dc, key).max(self.px(10.0)) + self.px(10.0);
            let cx = (cell.left + cell.right) / 2;
            let key_rc = RECT {
                left: cx - key_w / 2,
                top: row2 + self.px(1.0),
                right: cx + key_w / 2,
                bottom: cell.bottom - self.px(4.0),
            };
            round(
                dc,
                key_rc,
                self.px(4.0),
                Some(blend(th.border, th.bg, 0.35)),
                Some(th.border),
            );
            SetTextColor(dc, to_colorref(th.text));
            draw(dc, key, key_rc, DT_CENTER | DT_VCENTER);
            self.targets.push((cell, Hit::Mark(i)));
        }

        // 4. Footer hints
        let foot_top = pal_top + self.px(tk::PALETTE_H);
        fill(
            dc,
            RECT {
                left: 1,
                top: foot_top,
                right: width - 1,
                bottom: foot_top + 1,
            },
            th.border,
        );
        self.draw_hints(
            dc,
            &TASHKEEL_HINTS,
            RECT {
                left: pad,
                top: foot_top + 1,
                right: width - pad,
                bottom: foot_top + self.px(tk::FOOTER_H),
            },
            f_key,
            f_name,
        );

        SelectObject(dc, old_font);
        for f in [f_chip, f_word, f_mark, f_name, f_key] {
            let _ = DeleteObject(f.into());
        }
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

/// Translate a mouse message into a popup event (under a short borrow of the state).
fn mouse_event(
    state: &RefCell<PopupState>,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<(Handler, PopupEvent)> {
    let mut s = state.try_borrow_mut().ok()?;
    let handler = s.handler.clone()?;
    let x = (lparam.0 & 0xFFFF) as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let event = match msg {
        WM_MOUSEWHEEL => {
            if !matches!(s.model, PopupModel::List(_)) {
                return None;
            }
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            if delta > 0 {
                PopupEvent::WheelUp
            } else {
                PopupEvent::WheelDown
            }
        }
        WM_LBUTTONDOWN => match s.hit(x, y)? {
            Hit::Row(i) => PopupEvent::Row(i),
            Hit::Mark(i) => PopupEvent::Mark(i),
            Hit::ClearAll => PopupEvent::ClearAll,
            Hit::Pick(i) => PopupEvent::Pick(i),
            Hit::Letter(i) => {
                s.drag_letter = Some(i);
                PopupEvent::Letter {
                    index: i,
                    toggle: wparam.0 & MK_CONTROL != 0,
                    range: wparam.0 & MK_SHIFT != 0,
                }
            }
        },
        WM_MOUSEMOVE => {
            let last = s.drag_letter?;
            if wparam.0 & MK_LBUTTON == 0 {
                s.drag_letter = None;
                return None;
            }
            match s.hit(x, y)? {
                Hit::Letter(i) if i != last => {
                    s.drag_letter = Some(i);
                    PopupEvent::Letter {
                        index: i,
                        toggle: false,
                        range: true,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    Some((handler, event))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<PopupState>;
    // SAFETY (all arms): `ptr` is 0 or points into the owning PopupWindow's Box, and it is cleared
    // before that Box is freed (see Drop). Panics must not unwind into user32.
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !ptr.is_null() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Ok(mut state) = (*ptr).try_borrow_mut() {
                        state.paint(hwnd, hdc);
                    }
                }));
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_MOUSEWHEEL => {
            if !ptr.is_null() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if msg == WM_LBUTTONDOWN {
                        SetCapture(hwnd);
                    }
                    // The borrow ends inside `mouse_event`; the handler may re-render this window.
                    if let Some((handler, event)) = mouse_event(&*ptr, msg, wparam, lparam) {
                        handler(event);
                    }
                }));
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            if !ptr.is_null() {
                if let Ok(mut s) = (*ptr).try_borrow_mut() {
                    s.drag_letter = None;
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_spans_keep_marks_with_their_letter() {
        // عَلَّمَ = ع َ ل ّ َ م َ
        let word = "\u{0639}\u{064E}\u{0644}\u{0651}\u{064E}\u{0645}\u{064E}";
        assert_eq!(letter_spans(word), vec![(0, 2), (2, 5), (5, 7)]);
        assert!(letter_spans("").is_empty());
    }

    #[test]
    fn blend_endpoints() {
        assert_eq!(blend(0xFF0000, 0x0000FF, 1.0), 0xFF0000);
        assert_eq!(blend(0xFF0000, 0x0000FF, 0.0), 0x0000FF);
    }
}
