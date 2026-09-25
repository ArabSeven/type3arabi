//! End-to-end smoke test of the TIP inside a real TSF host, without registering anything.
//!
//! Creates a window with a TSF-enabled RichEdit control, activates a thread manager, activates
//! `TextService` on it in-process, feeds key events through the TIP's key sink, and prints the
//! control's text. Exit code 0 = every scenario produced the expected text and nothing crashed.
//!
//!   cargo run -p t3a-tip --example tsf_harness --target x86_64-pc-windows-msvc

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    std::process::exit(harness::run());
}

#[cfg(windows)]
mod harness {
    use t3a_tip::service::TextService;
    use windows::core::{w, Interface, PCWSTR};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::LibraryLoader::LoadLibraryW;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardState, MapVirtualKeyW, SetFocus, SetKeyboardState, MAPVK_VK_TO_VSC,
    };
    use windows::Win32::UI::TextServices::TF_LBI_CLK_RIGHT;
    use windows::Win32::UI::TextServices::{
        CLSID_TF_ThreadMgr, ITfContext, ITfKeyEventSink, ITfLangBarItemButton, ITfLangBarItemMgr,
        ITfTextInputProcessor, ITfThreadMgr, GUID_LBI_INPUTMODE, TF_LANGBARITEMINFO,
        TF_LBI_CLK_LEFT, TF_LBI_STYLE_SHOWNINTRAY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetWindowTextLengthW, GetWindowTextW,
        PeekMessageW, RegisterClassW, SendMessageW, SetWindowTextW, ShowWindow, TranslateMessage,
        MSG, PM_REMOVE, SW_SHOW, WINDOW_EX_STYLE, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW,
        WS_VISIBLE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, EndMenu, GetMenuItemCount, GetMenuState, GetMenuStringW, KillTimer, SetTimer,
        HMENU, MF_BYPOSITION, MF_CHECKED, MF_GRAYED, MN_GETHMENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetClientRect, GetWindowThreadProcessId, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_MOUSEWHEEL,
    };

    const EM_SETEDITSTYLE: u32 = 0x0400 + 204;
    const SES_USECTF: usize = 0x0001_0000;
    const ES_MULTILINE: u32 = 0x0004;

    unsafe extern "system" fn wndproc(
        h: HWND,
        m: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        DefWindowProcW(h, m, w, l)
    }

    fn pump() {
        unsafe {
            let mut msg = MSG::default();
            for _ in 0..200 {
                if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn text(edit: HWND) -> String {
        unsafe {
            let n = GetWindowTextLengthW(edit) as usize;
            let mut buf = vec![0u16; n + 1];
            let got = GetWindowTextW(edit, &mut buf) as usize;
            String::from_utf16_lossy(&buf[..got])
        }
    }

    /// Virtual key + lParam for a key down.
    fn key(vk: u16) -> (WPARAM, LPARAM) {
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as isize;
        (WPARAM(vk as usize), LPARAM(1 | (scan << 16)))
    }

    fn vk_for(c: char) -> u16 {
        match c {
            'a'..='z' => c.to_ascii_uppercase() as u16,
            '0'..='9' => c as u16,
            ' ' => 0x20,
            '\n' => 0x0D,
            '\x08' => 0x08,
            '\x1B' => 0x1B,
            ',' => 0xBC,
            '\t' => 0x09,
            '\u{E006}' => 0x28, // VK_DOWN
            _ => panic!("no vk for {c:?}"),
        }
    }

    // Pseudo-keys for scenarios (Private Use Area characters):
    const CLICK_ROW_1: char = '\u{E001}'; // click the 2nd candidate row
    const WHEEL_DOWN: char = '\u{E002}'; // mouse wheel down over the list
    const SHIFT_SPACE: char = '\u{E003}'; // Shift+Space
    const CLICK_CLEAR_ALL: char = '\u{E004}'; // tashkeel editor: "clear all" button
    const CLICK_DAMMA: char = '\u{E005}'; // tashkeel editor: 2nd palette cell (damma)
    const TRAY_CLICK: char = '\u{E007}'; // left click on the input-indicator button (Arabic <-> Latin)
    const TRAY_MENU_LATIN: char = '\u{E008}'; // input-indicator menu: "Latin"
    const TRAY_MENU_ARABIC: char = '\u{E009}'; // input-indicator menu: "Arabic"

    /// The TIP's input-indicator button, found the way Windows finds it: by GUID_LBI_INPUTMODE.
    fn tray_button(tm: &ITfThreadMgr) -> Option<ITfLangBarItemButton> {
        unsafe {
            let mgr: ITfLangBarItemMgr = tm.cast().ok()?;
            mgr.GetItem(&GUID_LBI_INPUTMODE).ok()?.cast().ok()
        }
    }

    /// Items of the open right-click menu, read from the live menu window: (text, checked, grayed).
    static MENU_SEEN: std::sync::Mutex<Vec<(String, bool, bool)>> =
        std::sync::Mutex::new(Vec::new());

    /// Timer callback running inside the menu's modal loop: record the items, then close the menu.
    unsafe extern "system" fn read_and_close_menu(_: HWND, _: u32, id: usize, _: u32) {
        let _ = KillTimer(None, id);
        let me = std::process::id();
        let mut after: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, after, w!("#32768"), PCWSTR::null()) {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == me {
                let menu = HMENU(SendMessageW(h, MN_GETHMENU, None, None).0 as *mut _);
                let mut seen = MENU_SEEN.lock().unwrap();
                for i in 0..GetMenuItemCount(Some(menu)).max(0) {
                    let mut buf = [0u16; 128];
                    let n = GetMenuStringW(menu, i as u32, Some(&mut buf), MF_BYPOSITION) as usize;
                    let state = GetMenuState(menu, i as u32, MF_BYPOSITION);
                    seen.push((
                        String::from_utf16_lossy(&buf[..n]),
                        state & MF_CHECKED.0 != 0,
                        state & MF_GRAYED.0 != 0,
                    ));
                }
                break;
            }
            after = Some(h);
        }
        // T3A_MENU_HOLD_MS keeps the menu open (for a screenshot) before closing it.
        if let Some(ms) = std::env::var("T3A_MENU_HOLD_MS")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        let _ = EndMenu();
    }

    /// Right click: the menu must open with Arabic checked, Latin, and Settings, then close cleanly.
    fn check_tray_menu(tm: &ITfThreadMgr) -> Result<(), String> {
        let b = tray_button(tm).ok_or("no tray button")?;
        MENU_SEEN.lock().unwrap().clear();
        unsafe {
            SetTimer(None, 0, 300, Some(read_and_close_menu));
            let area = RECT {
                left: 600,
                top: 400,
                right: 624,
                bottom: 424,
            };
            b.OnClick(TF_LBI_CLK_RIGHT, POINT { x: 612, y: 400 }, &area)
                .map_err(|e| format!("OnClick(right): {e}"))?;
        }
        pump();
        let seen = MENU_SEEN.lock().unwrap().clone();
        println!("  tray menu: {seen:?}");
        let texts: Vec<&str> = seen.iter().map(|(t, _, _)| t.as_str()).collect();
        let ok = seen.len() == 4
            && texts[0].starts_with("Arabic")
            && seen[0].1
            && texts[1].starts_with("Latin")
            && !seen[1].1
            && texts[3].starts_with("Type3arabi Settings")
            && !seen[3].2;
        if ok {
            Ok(())
        } else {
            Err("unexpected menu items".into())
        }
    }

    /// Checks of the tray button itself (docs/02 §10). Returns a failure message.
    fn check_tray(tm: &ITfThreadMgr) -> Result<(), String> {
        let b = tray_button(tm).ok_or("no GUID_LBI_INPUTMODE item")?;
        unsafe {
            let mut info = TF_LANGBARITEMINFO::default();
            b.GetInfo(&mut info).map_err(|e| format!("GetInfo: {e}"))?;
            if info.guidItem != GUID_LBI_INPUTMODE || info.dwStyle & TF_LBI_STYLE_SHOWNINTRAY == 0 {
                return Err("GetInfo: wrong item/style".into());
            }
            let icon = b.GetIcon().map_err(|e| format!("GetIcon: {e}"))?;
            if icon.is_invalid() {
                return Err("GetIcon: no icon".into());
            }
            let _ = DestroyIcon(icon); // the caller owns it
            let tip = b.GetTooltipString().map_err(|e| format!("tooltip: {e}"))?;
            if !tip.to_string().contains("Arabic") {
                return Err(format!("tooltip: {tip}"));
            }
        }
        Ok(())
    }

    /// Our own popup: another process (e.g. an app using the installed Type3arabi) may have a
    /// window of the same class, so match the owning process too.
    fn popup() -> Option<HWND> {
        unsafe {
            let me = std::process::id();
            let mut after: Option<HWND> = None;
            loop {
                let h = FindWindowExW(
                    None,
                    after,
                    w!("Type3arabi_CandidateWindow"),
                    PCWSTR::null(),
                )
                .ok()?;
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, Some(&mut pid));
                if pid == me {
                    return Some(h);
                }
                after = Some(h);
            }
        }
    }

    /// Send a mouse message to the popup at client coordinates computed from its painted layout.
    fn mouse(msg: u32, wparam: usize, at: impl Fn(i32, i32, f32) -> (i32, i32)) -> bool {
        let Some(h) = popup() else {
            println!("  popup window not found");
            return false;
        };
        unsafe {
            let mut rc = RECT::default();
            let _ = GetClientRect(h, &mut rc);
            let scale = GetDpiForWindow(h).max(96) as f32 / 96.0;
            let (x, y) = at(rc.right, rc.bottom, scale);
            let lp = LPARAM((((y as u32 & 0xFFFF) << 16) | (x as u32 & 0xFFFF)) as isize);
            SendMessageW(h, msg, Some(WPARAM(wparam)), Some(lp));
            if msg == WM_LBUTTONDOWN {
                SendMessageW(h, WM_LBUTTONUP, Some(WPARAM(0)), Some(lp));
            }
        }
        pump();
        true
    }

    fn type_keys(tm: &ITfThreadMgr, sink: &ITfKeyEventSink, ctx: &ITfContext, keys: &str) -> bool {
        for c in keys.chars() {
            let px = |v: f32, s: f32| (v * s).round() as i32;
            match c {
                TRAY_CLICK | TRAY_MENU_LATIN | TRAY_MENU_ARABIC => {
                    let Some(b) = tray_button(tm) else {
                        println!("  tray button not found");
                        return false;
                    };
                    let r = unsafe {
                        match c {
                            TRAY_CLICK => {
                                b.OnClick(TF_LBI_CLK_LEFT, POINT { x: 0, y: 0 }, &RECT::default())
                            }
                            TRAY_MENU_LATIN => b.OnMenuSelect(2),
                            _ => b.OnMenuSelect(1),
                        }
                    };
                    if r.is_err() {
                        println!("  tray call failed: {r:?}");
                        return false;
                    }
                    pump();
                    continue;
                }
                CLICK_ROW_1 => {
                    // header 22 + one row of 34, then the middle of the 2nd row
                    if !mouse(WM_LBUTTONDOWN, 0x1, |w, _, s| {
                        (w / 2, px(22.0 + 34.0 + 17.0, s))
                    }) {
                        return false;
                    }
                    continue;
                }
                WHEEL_DOWN => {
                    let delta = (-120i16 as u16 as usize) << 16;
                    if !mouse(WM_MOUSEWHEEL, delta, |w, _, s| (w / 2, px(40.0, s))) {
                        return false;
                    }
                    continue;
                }
                CLICK_CLEAR_ALL => {
                    if !mouse(WM_LBUTTONDOWN, 0x1, |_, _, s| (px(24.0, s), px(20.0, s))) {
                        return false;
                    }
                    continue;
                }
                CLICK_DAMMA => {
                    // palette cells run right-to-left: cell 1 is the second from the right edge
                    let ok = mouse(WM_LBUTTONDOWN, 0x1, |w, _, s| {
                        let pad = px(12.0, s);
                        let gap = px(4.0, s);
                        let cell = (w - 2 * pad - 9 * gap) / 10;
                        (
                            w - pad - cell - gap - cell / 2,
                            px(40.0 + 84.0 + 30.0, s) + 1,
                        )
                    });
                    if !ok {
                        return false;
                    }
                    continue;
                }
                _ => {}
            }
            let shift = c == SHIFT_SPACE;
            let (w, l) = key(if shift { 0x20 } else { vk_for(c) });
            let mut saved = [0u8; 256];
            if shift {
                unsafe {
                    let _ = GetKeyboardState(&mut saved);
                    let mut st = saved;
                    st[0x10] = 0x80; // VK_SHIFT down
                    st[0xA0] = 0x80; // VK_LSHIFT
                    let _ = SetKeyboardState(&st);
                }
            }
            let trace = std::env::var_os("T3A_TRACE").is_some();
            unsafe {
                if trace {
                    eprintln!("  test {c:?}");
                }
                let test = sink.OnTestKeyDown(ctx, w, l);
                if trace {
                    eprintln!("  down {c:?}");
                }
                let down = sink.OnKeyDown(ctx, w, l);
                if trace {
                    let old =
                        windows::Win32::System::LibraryLoader::GetModuleHandleW(w!("t3a_tip.dll"))
                            .is_ok();
                    eprintln!("  pump {c:?} (registered t3a_tip.dll loaded: {old})");
                }
                pump();
                let up_l = LPARAM(l.0 | (0xC000_0000u32 as i32 as isize));
                let _ = sink.OnTestKeyUp(ctx, w, up_l);
                let _ = sink.OnKeyUp(ctx, w, up_l);
                if test.is_err() || down.is_err() {
                    println!("  key {c:?}: sink returned an error");
                    return false;
                }
            }
            if shift {
                unsafe {
                    let _ = SetKeyboardState(&saved);
                }
            }
            pump();
        }
        true
    }

    /// The TIP's config inside the harness sandbox. Learning off: scenarios must not depend on each
    /// other's commits (sticky choices). Fixed dialect: with `auto`, earlier scenarios would shift the
    /// dialect estimate and reorder later lists.
    const HARNESS_CONFIG: &str = "[learning]\nenabled = false\n\n[dialect]\nprofile = \"LEV\"\n";

    /// Text of the 2nd candidate for `word` under HARNESS_CONFIG, as the TIP would commit it.
    fn second_candidate(word: &str, dat: &std::path::Path) -> String {
        let engine = match t3a_data::DataFile::open(dat) {
            Ok(file) => {
                let file: &'static t3a_data::DataFile = Box::leak(Box::new(file));
                t3a_engine::Engine::new(file.view().expect("data view")).expect("engine")
            }
            Err(_) => t3a_engine::Engine::builtin(),
        };
        let settings = t3a_engine::Config::parse(HARNESS_CONFIG)
            .0
            .to_engine_settings();
        let mut s = t3a_engine::session::Session::new(&engine, settings);
        for ch in word.chars() {
            s.push(
                t3a_engine::normalize::InputChar::new(ch),
                &t3a_engine::NoUser,
            );
        }
        s.candidates()
            .items
            .get(1)
            .map(|c| c.text.clone())
            .unwrap_or_default()
    }

    unsafe extern "system" fn on_exception(
        info: *mut windows::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
    ) -> i32 {
        let rec = &*(*info).ExceptionRecord;
        let code = rec.ExceptionCode.0 as u32;
        // Only real faults (access violation, stack overflow, heap corruption…), not C++/RPC noise.
        if matches!(code, 0xC0000005 | 0xC00000FD | 0xC0000374 | 0xC0000409) {
            eprintln!(
                "EXCEPTION {code:#010X} at {:?}
{}",
                rec.ExceptionAddress,
                std::backtrace::Backtrace::force_capture()
            );
        }
        0 // EXCEPTION_CONTINUE_SEARCH
    }

    pub fn run() -> i32 {
        // Use this checkout's data build: the TIP looks for type3arabi.dat next to its module (here,
        // this exe) before the installed copy.
        let repo_dat =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/type3arabi.dat");
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            if repo_dat.exists() {
                let _ = std::fs::copy(&repo_dat, dir.join("type3arabi.dat"));
            }
        }
        // Never touch the real user's learning store: point %LOCALAPPDATA% at a fresh temp dir.
        let sandbox = std::env::temp_dir().join(format!("t3a-harness-{}", std::process::id()));
        let _ = std::fs::create_dir_all(sandbox.join("Type3arabi"));
        // Learning off: scenarios must not depend on each other's commits (sticky choices).
        let _ = std::fs::write(
            sandbox.join("Type3arabi").join("config.toml"),
            HARNESS_CONFIG,
        );
        std::env::set_var("LOCALAPPDATA", &sandbox);
        let code = run_in_sandbox();
        let _ = std::fs::remove_dir_all(&sandbox);
        code
    }

    fn run_in_sandbox() -> i32 {
        unsafe {
            if std::env::var_os("T3A_TRACE").is_some() {
                windows::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler(
                    1,
                    Some(on_exception),
                );
            }
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let _ = LoadLibraryW(w!("msftedit.dll"));

            let class = w!("T3aHarness");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let top = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Type3arabi TSF harness"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                600,
                300,
                None,
                None,
                None,
                None,
            )
            .expect("top window");
            let _ = ShowWindow(top, SW_SHOW);

            let tm: ITfThreadMgr =
                CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)
                    .expect("thread mgr");
            let tid = tm.Activate().expect("activate thread mgr");

            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("RICHEDIT50W"),
                PCWSTR::null(),
                windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                    WS_CHILD.0 | WS_VISIBLE.0 | ES_MULTILINE,
                ),
                10,
                10,
                560,
                240,
                Some(top),
                None,
                None,
                None,
            )
            .expect("richedit");
            SendMessageW(
                edit,
                EM_SETEDITSTYLE,
                Some(WPARAM(SES_USECTF)),
                Some(LPARAM(SES_USECTF as isize)),
            );
            let _ = SetFocus(Some(edit));
            pump();

            let Some(ctx) = tm.GetFocus().ok().and_then(|dm| dm.GetTop().ok()) else {
                println!("FAIL: RichEdit did not expose a TSF context");
                return 2;
            };

            let tip: ITfTextInputProcessor = TextService::new().expect("service").into();
            if let Err(e) = tip.Activate(&tm, tid) {
                println!("FAIL: Activate: {e}");
                return 3;
            }
            let sink: ITfKeyEventSink = tip.cast().expect("key sink");
            // Windows' keyboard options open our Settings through ITfFnConfigure (not called here:
            // it would start the Settings app).
            match tip
                .cast::<windows::Win32::UI::TextServices::ITfFnConfigure>()
                .and_then(|f| f.GetDisplayName())
            {
                Ok(name) if name == "Type3arabi Settings" => {}
                other => {
                    println!("FAIL: ITfFnConfigure: {other:?}");
                    return 4;
                }
            }

            // The input-indicator (tray) button, as Windows sees it.
            if let Err(e) = check_tray(&tm).and_then(|_| check_tray_menu(&tm)) {
                println!("FAIL: tray button: {e}");
                return 5;
            }

            // The 2nd row depends on the data build (e.g. مرحبة vs مرحبه), not on the UI under test:
            // ask the engine for it, with the same data file and default settings as the TIP.
            let dat = std::env::current_exe()
                .map(|p| p.with_file_name("type3arabi.dat"))
                .unwrap_or_default();
            let row2 = second_candidate("mar7aba", &dat) + " ";
            // (typed keys, expected text). "\x08" = Backspace, "\x1B" = Esc, "\n" = Enter.
            let scenarios: &[(&str, &str)] = &[
                ("mar7aba ", "مرحباً "), // U+0645 U+0631 U+062D U+0628 U+0627 U+064B
                ("shukran ", "شكراً "),  // U+0634 U+0643 U+0631 U+0627 U+064B
                ("allah ", "اللّه "),    // U+0627 U+0644 U+0644 U+0651 U+0647
                ("mar7\x08\x08\x08\x08", ""),
                ("hello\x1B", "hello"),
                ("3arabi\n", "عربي"), // U+0639 U+0631 U+0628 U+064A
                ("kifak,", "كيفك،"),
                // Re-edit: Backspace right after a commit reopens the word as a composition;
                // Esc then commits the Latin buffer in its place.
                ("mar7aba\n\x08\x1B", "mar7aba"),
                // Tab opens the tashkeel editor; 'a' puts fatha on the first letter; Enter commits.
                // (Not mar7aba: the Esc above taught "raw Latin first" for it — sticky choice.)
                ("shukran\ta\n", "شَكراً"), // U+0634 U+064E U+0643 U+0631 U+0627 U+064B
                // Owner requests 2026-09-23: Shift+Space commits the Latin word; mouse selection.
                ("hello\u{E003}", "hello "),
                // 2nd candidate three ways: ↓ + Space, click on row 2, wheel down + Space.
                ("mar7aba\u{E006} ", &row2),
                ("mar7aba\u{E001}", &row2),
                ("mar7aba\u{E002} ", &row2),
                ("shukran\t\u{E004}\n", "شكرا"), // clear all diacritics (drops the tanween)
                ("shukran\t\u{E005}\n", "شُكراً"), // U+0634 U+064F ...: damma on the 1st letter
                // Tray button (2026-09-25): a click switches to Latin (letters go in as typed), a
                // second click back to Arabic; the menu's Latin / Arabic items do the same.
                ("\u{E007}hello\u{E007}mar7aba ", "helloمرحباً "),
                ("\u{E008}hello\u{E009}shukran ", "helloشكراً "),
            ];
            let mut failures = 0;
            // Every scenario runs several times in one process: edit-session ordering bugs are
            // intermittent (T3A_ROUNDS overrides; default 3).
            let rounds: usize = std::env::var("T3A_ROUNDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3);
            let all: Vec<&(&str, &str)> = (0..rounds).flat_map(|_| scenarios.iter()).collect();
            for (keys, expected) in all {
                let _ = SetWindowTextW(edit, w!(""));
                pump();
                let ok = type_keys(&tm, &sink, &ctx, keys);
                // Close any still-open composition so the control text is final.
                let (esc_w, esc_l) = key(0x1B);
                let _ = sink.OnKeyDown(&ctx, esc_w, esc_l);
                pump();
                let got = text(edit);
                let pass = ok && got == *expected;
                println!(
                    "{} {:<22} -> {:?} (expected {:?})",
                    if pass { "PASS" } else { "FAIL" },
                    format!("{keys:?}"),
                    got,
                    expected
                );
                if !pass {
                    failures += 1;
                }
            }

            let _ = tip.Deactivate();
            pump();
            let _ = tm.Deactivate();
            println!("{} scenario(s) failed", failures);
            if failures == 0 {
                0
            } else {
                1
            }
        }
    }
}
