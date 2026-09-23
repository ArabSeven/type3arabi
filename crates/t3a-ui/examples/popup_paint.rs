//! Paint-path smoke test: shows the popup with several models and pumps WM_PAINT.
//!   cargo run -p t3a-ui --example popup_paint
#[cfg(not(windows))]
fn main() {}
#[cfg(windows)]
fn main() {
    use t3a_ui::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
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
    let models = [
        vec![("هل", true), ("هيل", true), ("هال", true), ("hell", false)],
        vec![("هله", true), ("هلو", true), ("hello", false)],
        // Regression: an empty row crashed DrawTextW (0xC000041D in the host).
        vec![("", true), ("هلو", true), ("hello", false)],
    ];
    let mut p = PopupWindow::new().expect("popup");
    for (i, m) in models.iter().enumerate() {
        let model = PopupModel::List(ListModel {
            latin: "x".into(),
            badge: Some("شامي".into()),
            rows: rows(m),
            highlighted: 0,
            footer: Footer::Hints,
        });
        p.show(model, (400, 300, 420, 320));
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        eprintln!("model {i} painted");
    }
}
