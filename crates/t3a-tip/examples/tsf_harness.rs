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
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::LibraryLoader::LoadLibraryW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, SetFocus, MAPVK_VK_TO_VSC};
    use windows::Win32::UI::TextServices::{
        CLSID_TF_ThreadMgr, ITfContext, ITfKeyEventSink, ITfTextInputProcessor, ITfThreadMgr,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetWindowTextLengthW, GetWindowTextW,
        PeekMessageW, RegisterClassW, SendMessageW, SetWindowTextW, ShowWindow, TranslateMessage,
        MSG, PM_REMOVE, SW_SHOW, WINDOW_EX_STYLE, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW,
        WS_VISIBLE,
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
            _ => panic!("no vk for {c:?}"),
        }
    }

    fn type_keys(sink: &ITfKeyEventSink, ctx: &ITfContext, keys: &str) -> bool {
        for c in keys.chars() {
            let (w, l) = key(vk_for(c));
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
            pump();
        }
        true
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
        let _ = std::fs::create_dir_all(&sandbox);
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
            ];
            let mut failures = 0;
            for (keys, expected) in scenarios {
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
