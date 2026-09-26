//! End-to-end smoke test of the TIP inside a real TSF host, without registering anything.
//!
//! Creates a window with a TSF-enabled RichEdit control, activates a thread manager, activates
//! `TextService` on it in-process, feeds key events through the TIP's key sink, and prints the
//! control's text. Exit code 0 = every scenario produced the expected text and nothing crashed.
//!
//!   cargo run -p t3a-tip --example tsf_harness --target x86_64-pc-windows-msvc
//!
//! Quiet by default: the host window is transparent, never activated and not on the taskbar, and the
//! candidate popup is made transparent after a warm-up word, so a run does not disturb the desktop
//! (it still needs a desktop session). `T3A_HARNESS_VISIBLE=1` shows everything, as before.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    // A copy of this exe stands in for the Settings app (see `run`): when the popup's Settings
    // button starts it, it only leaves a marker for the harness and exits.
    let exe = std::env::current_exe().unwrap_or_default();
    if exe
        .file_name()
        .is_some_and(|n| n == "Type3arabi Settings.exe")
    {
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            let _ = std::fs::write(harness::settings_marker(dir.as_ref()), "");
        }
        return;
    }
    std::process::exit(harness::run());
}

#[cfg(windows)]
mod harness {
    use t3a_tip::service::TextService;
    use windows::core::{w, Interface, PCWSTR};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::LibraryLoader::LoadLibraryW;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetFocus, GetKeyboardState, MapVirtualKeyW, SetFocus, SetKeyboardState, MAPVK_VK_TO_VSC,
    };
    use windows::Win32::UI::TextServices::{
        CLSID_TF_ThreadMgr, ITfContext, ITfKeyEventSink, ITfTextInputProcessor, ITfThreadMgr,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetAncestor, GetWindowTextLengthW,
        GetWindowTextW, PeekMessageW, RegisterClassW, SendMessageW, SetForegroundWindow,
        SetWindowTextW, ShowWindow, TranslateMessage, GA_ROOT, MSG, PM_REMOVE, SW_SHOW,
        WINDOW_EX_STYLE, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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
    const CLICK_SETTINGS: char = '\u{E007}'; // the Settings tab in the list header's left corner

    // The host changes things behind the TIP's back (chat apps, browsers, autocorrect):
    const HOST_CLEAR: char = '\u{E008}'; // the app replaces its text (SetWindowText) mid-word
    const FOCUS_AWAY: char = '\u{E009}'; // focus leaves the field and comes back

    thread_local! {
        /// The RichEdit under test (for HOST_CLEAR / FOCUS_AWAY).
        static EDIT: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    }
    /// `SetWindowLongPtrW` takes a LONG_PTR: i32 on x86, isize on x64.
    #[cfg(target_pointer_width = "64")]
    type LongPtr = isize;
    #[cfg(target_pointer_width = "32")]
    type LongPtr = i32;

    fn visible() -> bool {
        std::env::var_os("T3A_HARNESS_VISIBLE").is_some()
    }

    /// Quiet mode: make our own popup fully transparent (it is non-activating already), so
    /// scenarios do not flash windows on the user's screen. Messages still reach it.
    fn hide_popup() {
        if visible() {
            return;
        }
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE,
            LWA_ALPHA, WS_EX_LAYERED,
        };
        if let Some(h) = popup() {
            unsafe {
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                let _ = SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as LongPtr);
                let _ = SetLayeredWindowAttributes(
                    h,
                    windows::Win32::Foundation::COLORREF(0),
                    0,
                    LWA_ALPHA,
                );
            }
        }
    }

    /// File the stand-in Settings app writes when it is started.
    pub fn settings_marker(local_app_data: &std::path::Path) -> std::path::PathBuf {
        local_app_data.join("Type3arabi").join("settings-opened")
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

    fn type_keys(sink: &ITfKeyEventSink, ctx: &ITfContext, keys: &str) -> bool {
        for c in keys.chars() {
            let px = |v: f32, s: f32| (v * s).round() as i32;
            match c {
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
                CLICK_SETTINGS => {
                    let marker = settings_marker(&std::path::PathBuf::from(
                        std::env::var_os("LOCALAPPDATA").unwrap_or_default(),
                    ));
                    let _ = std::fs::remove_file(&marker);
                    let focus = unsafe { GetFocus() };
                    // tab: x 6..36, y 0..19 at 96 DPI
                    if !mouse(WM_LBUTTONDOWN, 0x1, |_, _, s| (px(21.0, s), px(9.0, s))) {
                        return false;
                    }
                    // Keep pumping like a real app's UI thread (ShellExecute finishes on it).
                    let t0 = std::time::Instant::now();
                    let started = (0..500).any(|_| {
                        pump();
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        marker.exists()
                    });
                    println!(
                        "  settings stand-in started: {started} after {:?}",
                        t0.elapsed()
                    );
                    if !started {
                        println!("  Settings app was not started by the popup's Settings button");
                        return false;
                    }
                    // The new window took the focus (the word was finalized, as on any focus
                    // change); give it back to the test control for the next scenarios.
                    // Quiet mode: the harness is never the active window, so the stand-in cannot
                    // take the focus from it; move the focus off the field ourselves, as the real
                    // Settings window does.
                    unsafe {
                        if !visible() {
                            let _ = SetFocus(Some(GetAncestor(focus, GA_ROOT)));
                            for _ in 0..10 {
                                pump();
                                std::thread::sleep(std::time::Duration::from_millis(10));
                            }
                        }
                        if visible() {
                            let _ = SetForegroundWindow(GetAncestor(focus, GA_ROOT));
                        }
                        let _ = SetFocus(Some(focus));
                    }
                    // Let the activation/focus messages settle before the next scenario types.
                    for _ in 0..25 {
                        pump();
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    continue;
                }
                HOST_CLEAR => {
                    let edit = HWND(EDIT.with(|e| e.get()) as *mut _);
                    unsafe {
                        let _ = SetWindowTextW(edit, w!(""));
                    }
                    pump();
                    continue;
                }
                FOCUS_AWAY => {
                    let edit = HWND(EDIT.with(|e| e.get()) as *mut _);
                    unsafe {
                        let _ = SetFocus(Some(GetAncestor(edit, GA_ROOT)));
                        pump();
                        let _ = SetFocus(Some(edit));
                    }
                    pump();
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
        // The TIP starts "Type3arabi Settings.exe" found next to its module (here, this exe).
        let stand_in = std::env::current_exe()
            .map(|p| p.with_file_name("Type3arabi Settings.exe"))
            .unwrap_or_default();
        let _ = std::fs::copy(std::env::current_exe().unwrap_or_default(), &stand_in);
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
            use windows::Win32::UI::WindowsAndMessaging::{
                SetLayeredWindowAttributes, LWA_ALPHA, SW_SHOWNOACTIVATE, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            };
            let quiet_ex = if visible() {
                WINDOW_EX_STYLE(0)
            } else {
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
            };
            let top = CreateWindowExW(
                quiet_ex,
                class,
                w!("Type3arabi TSF harness"),
                WS_OVERLAPPEDWINDOW
                    | if visible() {
                        WS_VISIBLE
                    } else {
                        Default::default()
                    },
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
            if visible() {
                let _ = ShowWindow(top, SW_SHOW);
            } else {
                // Fully transparent and never activated: nothing to see, no focus taken.
                let _ = SetLayeredWindowAttributes(
                    top,
                    windows::Win32::Foundation::COLORREF(0),
                    0,
                    LWA_ALPHA,
                );
                let _ = ShowWindow(top, SW_SHOWNOACTIVATE);
            }

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
            EDIT.with(|e| e.set(edit.0 as isize));
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
                // Owner request 2026-09-25: the header's Settings tab starts the Settings app; its
                // window takes the focus, which finalizes the word as shown (U+0645 ... U+064B).
                ("mar7aba\u{E007}", "مرحباً"),
                // The app clears its text while the diacritics editor (or the list) is open: that word
                // is gone with it, and typing goes on normally with a fresh word.
                ("shukran\t\u{E008}mar7aba ", "مرحباً "), // U+0645 U+0631 U+062D U+0628 U+0627 U+064B
                ("mar7\u{E008}shukran ", "شكراً "),       // U+0634 U+0643 U+0631 U+0627 U+064B
                // A letter that is not an editor command inserts the edited word and starts a new
                // one (it used to be swallowed; Esc then keeps the new word in Latin).
                ("shukran\tb", "شكراًb"),
                // Focus leaves and comes back mid-word: the word stays as shown, typing goes on.
                ("shukran\t\u{E009}mar7aba ", "شكراًمرحباً "),
                // Every word start also reads the field's input scopes (docs/02 §7, R9). RichEdit does
                // not report them (GetValue fails), which must mean "no scope": normal typing above.
            ];
            // Quiet mode: create the popup with a warm-up word, then make it transparent.
            let _ = type_keys(&sink, &ctx, "mar7");
            // docs/02 §9: the popup is owned by the field's top-level window, which keeps it in that
            // window's z-order band (Windows Search / Start draw above unowned windows).
            {
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetAncestor, GetWindow, GA_ROOT, GW_OWNER,
                };
                let root = GetAncestor(edit, GA_ROOT);
                let owner = popup().and_then(|h| GetWindow(h, GW_OWNER).ok());
                if owner != Some(root) {
                    println!("FAIL: popup owner {owner:?}, expected the host window {root:?}");
                    return 5;
                }
                println!("PASS popup is owned by the host window");

                // docs/02 §9: the popup follows the composition when the host reports that the field
                // moved (ITfTextLayoutSink).
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
                };
                let rect = |h: HWND| {
                    let mut r = windows::Win32::Foundation::RECT::default();
                    let _ = GetWindowRect(h, &mut r);
                    r
                };
                if let Some(p) = popup() {
                    let (host0, pop0) = (rect(root), rect(p));
                    let _ = SetWindowPos(
                        root,
                        None,
                        host0.left,
                        host0.top + 60,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    pump();
                    // Hosts like Start report a moved field through ITfTextLayoutSink (RichEdit only does
                    // for text changes), so report this move the way such a host does.
                    if let (Ok(sink), Ok(view)) = (
                        tip.cast::<windows::Win32::UI::TextServices::ITfTextLayoutSink>(),
                        ctx.GetActiveView(),
                    ) {
                        let _ = sink.OnLayoutChange(
                            &ctx,
                            windows::Win32::UI::TextServices::TF_LC_CHANGE,
                            &view,
                        );
                    }
                    pump();
                    let pop1 = rect(p);
                    let (dx, dy) = (pop1.left - pop0.left, pop1.top - pop0.top);
                    let _ = SetWindowPos(
                        root,
                        None,
                        host0.left,
                        host0.top,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    pump();
                    // Vertical only: the list is right-aligned to the word and clamped at the screen's left
                    // edge, so a horizontal move near that edge need not move it.
                    if (dx, dy) != (0, 60) {
                        println!(
                            "FAIL: popup moved by ({dx}, {dy}) when the host moved by (0, 60)"
                        );
                        return 6;
                    }
                    println!("PASS popup follows the field when its window moves");
                }
            }
            hide_popup();
            let (esc_w, esc_l) = key(0x1B);
            let _ = sink.OnKeyDown(&ctx, esc_w, esc_l);
            pump();
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
                let ok = type_keys(&sink, &ctx, keys);
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
