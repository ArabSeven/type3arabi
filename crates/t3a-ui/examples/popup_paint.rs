//! Paint-path smoke test: shows the popup with several models, pumps WM_PAINT and, with
//! `--out <dir>`, saves each rendering as a .bmp (visual review of docs/05 layouts).
//!   cargo run -p t3a-ui --example popup_paint -- --out target/popup-shots
#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use t3a_ui::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::*;

    let out_dir = std::env::args()
        .skip_while(|a| a != "--out")
        .nth(1)
        .map(std::path::PathBuf::from);
    let rows = |v: &[(&str, bool)]| -> Vec<Row> {
        v.iter()
            .map(|(t, rtl)| Row {
                text: t.to_string(),
                marker: if *rtl {
                    RowMarker::None
                } else {
                    RowMarker::RawLatin
                },
                rtl: *rtl,
            })
            .collect()
    };
    let list = |r: &[(&str, bool)], hl: usize| {
        PopupModel::List(ListModel {
            latin: "mar7aba".into(),
            badge: Some("شامي".into()),
            rows: rows(r),
            highlighted: hl,
            footer: Footer::Hints,
        })
    };
    let models = [
        (
            "list",
            list(
                &[
                    ("مرحباً", true),
                    ("مرحبة", true),
                    ("مرحبه", true),
                    ("mar7aba", false),
                ],
                0,
            ),
        ),
        // Regression: an empty row crashed DrawTextW (0xC000041D in the host).
        (
            "list-empty-row",
            list(&[("", true), ("هلو", true), ("hello", false)], 1),
        ),
        (
            "tashkeel",
            PopupModel::Tashkeel(TashkeelModel {
                picks: vec![
                    "شُكْراً".into(), // U+0634 U+064F U+0643 U+0652 U+0631 U+0627 U+064B
                    "شَكَراً".into(),
                ],
                from_typing: true,
                highlighted_pick: None,
                word: "شكراً".into(), // U+0634 U+0643 U+0631 U+0627 U+064B
                focused_letter: 1,
                selected: vec![0, 1],
            }),
        ),
    ];
    let mut p = PopupWindow::new().expect("popup");
    // `--dark` renders the dark theme (the popup otherwise follows Windows' app mode).
    let dark = std::env::args().any(|a| a == "--dark");
    if dark {
        p.set_theme(Theme::DARK);
    }
    for (name, model) in models {
        let name = if dark {
            format!("{name}-dark")
        } else {
            name.to_string()
        };
        p.show(model, (700, 300, 720, 320));
        // SAFETY: plain GDI capture of our own window into a DIB, then written as a BMP file.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            if let Some(dir) = &out_dir {
                let hwnd = p.hwnd();
                let mut rc = windows::Win32::Foundation::RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let (w, h) = (rc.right, rc.bottom);
                let screen = GetDC(None);
                let dc = CreateCompatibleDC(Some(screen));
                let bmp = CreateCompatibleBitmap(screen, w, h);
                let old = SelectObject(dc, bmp.into());
                let _ = PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(1)); // PW_CLIENTONLY
                let mut bi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: h,
                        biPlanes: 1,
                        biBitCount: 24,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let stride = ((w * 3 + 3) & !3) as usize;
                let mut pixels = vec![0u8; stride * h as usize];
                GetDIBits(
                    dc,
                    bmp,
                    0,
                    h as u32,
                    Some(pixels.as_mut_ptr() as *mut _),
                    &mut bi,
                    DIB_RGB_COLORS,
                );
                SelectObject(dc, old);
                let _ = DeleteObject(bmp.into());
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                let mut file = Vec::with_capacity(54 + pixels.len());
                let size = (54 + pixels.len()) as u32;
                file.extend_from_slice(b"BM");
                file.extend_from_slice(&size.to_le_bytes());
                file.extend_from_slice(&0u32.to_le_bytes());
                file.extend_from_slice(&54u32.to_le_bytes());
                file.extend_from_slice(&40u32.to_le_bytes());
                file.extend_from_slice(&w.to_le_bytes());
                file.extend_from_slice(&h.to_le_bytes());
                file.extend_from_slice(&1u16.to_le_bytes());
                file.extend_from_slice(&24u16.to_le_bytes());
                file.extend_from_slice(&[0u8; 24]);
                file.extend_from_slice(&pixels);
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{name}.bmp")), file);
            }
        }
        eprintln!("model {name} painted");
    }
}
