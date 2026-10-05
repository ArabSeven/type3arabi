//! The TIP inside an IMM32 host, end to end with real keystrokes.
//!
//! Many apps are not TSF-aware: they talk to input methods through WM_IME_* messages and Imm* calls —
//! Avalonia (Subtitle Edit 5), Qt 5, Java, SDL, wxWidgets, many games. Windows bridges them to TSF
//! (CUAS): the TIP gets a "transitory" context whose layout queries (GetTextExt) are answered by asking
//! the app's IME window, and CUAS reports layout changes for them. `tsf_harness` (RichEdit, a native
//! TSF control) never takes that path; this harness does. The freeze of 2026-10-05 (Subtitle Edit
//! 5.2: first letter, then a hung app with a black candidate list) lived there: docs/02 §9.
//!
//! The host copies Avalonia 11's Win32 backend (`Imm32InputMethod`, `WindowImpl.AppWndProc`):
//! - WM_IME_SETCONTEXT: DefWindowProc without ISC_SHOWUICOMPOSITIONWINDOW (the app draws the preedit);
//! - WM_IME_STARTCOMPOSITION / WM_IME_ENDCOMPOSITION handled, not passed on;
//! - WM_IME_COMPOSITION: read GCS_COMPSTR / GCS_RESULTSTR, then DefWindowProc;
//! - every preedit change moves the caret, posted to the dispatcher, which then calls SetCaretPos +
//!   ImmSetCandidateWindow(CFS_EXCLUDE);
//! - an input-language change drops the window's input context and associates a new one.
//!
//! CUAS only bridges the thread's *active* TIP, so the harness activates the registered Type3arabi
//! profile (needs an installed or dev-installed Type3arabi; exit code 4 = skipped without it) and makes
//! TSF load **this build's** DLL for it: a per-user COM entry
//! `HKCU\Software\Classes\CLSID\{TIP}\InprocServer32` → a temp copy of `t3a_tip.dll`, written just
//! before the activation and deleted right after it (milliseconds). It refuses to run when such an
//! entry already exists, and removes one left by a crashed run of itself. Keys are real (SendInput),
//! sent only while the harness window is the foreground window (it is visible for that reason).
//!
//!   cargo build --release -p t3a-tip --target x86_64-pc-windows-msvc        (builds t3a_tip.dll)
//!   cargo run --release -p t3a-tip --example imm32_harness --target x86_64-pc-windows-msvc
//!   T3A_INSTALLED=1 …   the installed DLL instead of this build (reproduce a released version)
//!   T3A_SYMPATH=<dir>   PDB folder for the installed DLL's frames in a freeze report
//!
//! Exit code 0 = every scenario produced the expected text, the app's message queue drained after
//! every key and the candidate list was painted. A freeze prints the UI thread's stack (x64).

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    std::process::exit(harness::run());
}

#[cfg(windows)]
mod harness {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};
    use windows::core::{w, GUID, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Input::Ime::{
        ImmAssociateContext, ImmCreateContext, ImmGetCompositionStringW, ImmGetContext,
        ImmNotifyIME, ImmReleaseContext, ImmSetCandidateWindow, CANDIDATEFORM, CFS_EXCLUDE,
        CPS_COMPLETE, GCS_COMPSTR, GCS_RESULTSTR, HIMC, IME_COMPOSITION_STRING, NI_COMPOSITIONSTR,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MapVirtualKeyW, SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD,
        KEYBDINPUT, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
    };
    use windows::Win32::UI::TextServices::{
        CLSID_TF_InputProcessorProfiles, CLSID_TF_ThreadMgr, ITfInputProcessorProfileMgr,
        ITfThreadMgr, TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_PROFILETYPE_INPUTPROCESSOR,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_APP_CURSOR_RECT: u32 = WM_APP + 1; // Avalonia: Dispatcher.Post(MoveImeWindow)
    const WM_APP_RESET: u32 = WM_APP + 2; // Avalonia: Dispatcher.Post(ImmNotifyIME CPS_COMPLETE)
    const WM_APP_ENABLE: u32 = WM_APP + 3; // Avalonia: SetClient → Dispatcher.Post(EnableImm)
    const ISC_SHOWUICOMPOSITIONWINDOW: isize = 0x8000_0000u32 as i32 as isize;

    /// What the emulated app shows: committed text + preedit (Avalonia's TextBox).
    #[derive(Default)]
    struct App {
        hwnd: HWND,
        text: String,
        preedit: String,
        composing: bool,
        himc: HIMC,
        langid: u16,
        caret: bool,
        /// Avalonia's `_ignoreWmChar`: set when a result string was taken, so the WM_CHAR that
        /// DefWindowProc makes of it (WM_IME_CHAR) is not inserted twice; cleared by WM_KEYUP.
        ignore_char: bool,
        /// Messages dispatched since the last key (by message id): the shape of a busy queue.
        counts: BTreeMap<u32, u32>,
    }

    thread_local! {
        static APP: RefCell<App> = RefCell::new(App::default());
    }

    /// Milliseconds since start at which the message queue was last empty (watchdog).
    static LAST_IDLE: AtomicU64 = AtomicU64::new(0);
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

    fn now_ms() -> u64 {
        START.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    fn comp_string(himc: HIMC, kind: IME_COMPOSITION_STRING) -> String {
        unsafe {
            let bytes = ImmGetCompositionStringW(himc, kind, None, 0);
            if bytes <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; bytes as usize / 2];
            let got =
                ImmGetCompositionStringW(himc, kind, Some(buf.as_mut_ptr().cast()), bytes as u32);
            String::from_utf16_lossy(&buf[..(got.max(0) as usize / 2).min(buf.len())])
        }
    }

    /// Avalonia: the preedit changed → layout → CursorRectangleChanged → SetCursorRect, which posts.
    fn cursor_moved(h: HWND) {
        unsafe {
            let _ = PostMessageW(Some(h), WM_APP_CURSOR_RECT, WPARAM(0), LPARAM(0));
        }
    }

    /// Avalonia's EnableImm: the window's context, or a new one, associated with the window.
    unsafe fn enable_imm(h: HWND) {
        let mut himc = ImmGetContext(h);
        if himc.is_invalid() {
            himc = ImmCreateContext();
        }
        let current = APP.with(|a| a.borrow().himc);
        if himc != current {
            if !current.is_invalid() {
                disable_imm(h);
            }
            let _ = ImmAssociateContext(h, himc);
            let _ = ImmReleaseContext(h, himc);
            APP.with(|a| {
                let mut a = a.borrow_mut();
                a.himc = himc;
                if !a.caret {
                    a.caret = CreateCaret(h, None, 2, 2).is_ok();
                }
            });
        }
    }

    /// Avalonia's DisableImm: Reset (posted CPS_COMPLETE), no context on the window.
    unsafe fn disable_imm(h: HWND) {
        APP.with(|a| {
            let mut a = a.borrow_mut();
            if a.caret {
                let _ = DestroyCaret();
                a.caret = false;
            }
            a.himc = HIMC::default();
        });
        let _ = PostMessageW(Some(h), WM_APP_RESET, WPARAM(0), LPARAM(0));
        let _ = ImmAssociateContext(h, HIMC::default());
    }

    /// Avalonia's UpdateInputMethod: another primary language recreates the input context.
    unsafe fn update_input_method(h: HWND, hkl: isize) {
        let langid = (hkl & 0xFFFF) as u16;
        let old = APP.with(|a| std::mem::replace(&mut a.borrow_mut().langid, langid));
        if old != 0 && (old & 0x3FF) != (langid & 0x3FF) {
            disable_imm(h);
            enable_imm(h);
        }
    }

    unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match m {
            WM_IME_SETCONTEXT => {
                let _ = DefWindowProcW(h, m, wp, LPARAM(lp.0 & !ISC_SHOWUICOMPOSITIONWINDOW));
                update_input_method(h, GetKeyboardLayout(0).0 as isize);
                LRESULT(0)
            }
            WM_INPUTLANGCHANGE => {
                update_input_method(h, lp.0);
                DefWindowProcW(h, m, wp, lp)
            }
            WM_IME_STARTCOMPOSITION => {
                APP.with(|a| {
                    let mut a = a.borrow_mut();
                    a.preedit.clear();
                    a.composing = true;
                });
                LRESULT(0)
            }
            WM_IME_COMPOSITION => {
                let flags = lp.0 as u32;
                let himc = ImmGetContext(h);
                if flags & GCS_RESULTSTR.0 != 0 {
                    let r = comp_string(himc, GCS_RESULTSTR);
                    APP.with(|a| {
                        let mut a = a.borrow_mut();
                        a.preedit.clear();
                        a.text.push_str(&r);
                        a.ignore_char = !r.is_empty();
                    });
                }
                if flags & GCS_COMPSTR.0 != 0 {
                    let c = comp_string(himc, GCS_COMPSTR);
                    APP.with(|a| a.borrow_mut().preedit = c);
                }
                let _ = ImmReleaseContext(h, himc);
                cursor_moved(h);
                DefWindowProcW(h, m, wp, lp)
            }
            WM_IME_ENDCOMPOSITION => {
                APP.with(|a| {
                    let mut a = a.borrow_mut();
                    a.composing = false;
                    a.preedit.clear();
                });
                cursor_moved(h);
                LRESULT(0)
            }
            WM_CHAR => {
                let c = wp.0 as u32;
                APP.with(|a| {
                    let mut a = a.borrow_mut();
                    if !a.composing && !a.ignore_char && c >= 32 {
                        if let Some(ch) = char::from_u32(c) {
                            a.text.push(ch);
                        }
                    }
                });
                LRESULT(0)
            }
            WM_KEYUP => {
                APP.with(|a| a.borrow_mut().ignore_char = false);
                DefWindowProcW(h, m, wp, lp)
            }
            WM_APP_CURSOR_RECT => {
                // Avalonia's MoveImeWindow: caret at the end of the preedit, candidate form excluding it.
                let himc = ImmGetContext(h);
                if !himc.is_invalid() {
                    let n = APP.with(|a| {
                        let a = a.borrow();
                        a.text.chars().count() + a.preedit.chars().count()
                    }) as i32;
                    let (x1, y1) = (20 + 9 * n, 30);
                    let (x2, y2) = (x1 + 2, y1 + 20);
                    let _ = SetCaretPos(x2, y2);
                    let form = CANDIDATEFORM {
                        dwIndex: 0,
                        dwStyle: CFS_EXCLUDE,
                        ptCurrentPos: POINT { x: x1, y: y1 },
                        rcArea: RECT {
                            left: x1,
                            top: y1,
                            right: x2,
                            bottom: y2 + 1,
                        },
                    };
                    let _ = ImmSetCandidateWindow(himc, &form);
                    let _ = ImmReleaseContext(h, himc);
                }
                LRESULT(0)
            }
            WM_APP_RESET => {
                let himc = ImmGetContext(h);
                if !himc.is_invalid() {
                    let _ = ImmNotifyIME(himc, NI_COMPOSITIONSTR, CPS_COMPLETE, 0);
                    let _ = ImmReleaseContext(h, himc);
                }
                LRESULT(0)
            }
            WM_APP_ENABLE => {
                enable_imm(h);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }

    /// Pump until the queue is empty (an idle app) or `limit` passes (a frozen one: false).
    fn pump_idle(limit: Duration) -> bool {
        let t0 = Instant::now();
        unsafe {
            let mut msg = MSG::default();
            loop {
                if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    LAST_IDLE.store(now_ms(), Ordering::Relaxed);
                    return true;
                }
                APP.with(|a| *a.borrow_mut().counts.entry(msg.message).or_default() += 1);
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if t0.elapsed() > limit {
                    return false;
                }
            }
        }
    }

    /// Let posted work, timers and input run, like an app between two keystrokes.
    fn settle() -> bool {
        for _ in 0..5 {
            if !pump_idle(Duration::from_secs(3)) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        pump_idle(Duration::from_secs(3))
    }

    fn vk_for(c: char) -> u16 {
        match c {
            'a'..='z' => c.to_ascii_uppercase() as u16,
            '0'..='9' => c as u16,
            ' ' => 0x20,
            '\n' => 0x0D,
            '\x08' => 0x08,
            '\x1B' => 0x1B,
            _ => panic!("no vk for {c:?}"),
        }
    }

    /// A real keystroke (ImmProcessKey → CUAS → the TIP, as in an app). Only while our window is the
    /// foreground window, so no key can ever land in another app.
    fn send_key(app: HWND, vk: u16) -> bool {
        unsafe {
            if GetForegroundWindow() != app {
                return false;
            }
            let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16;
            let ki = |up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: scan,
                        dwFlags: if up {
                            KEYEVENTF_KEYUP
                        } else {
                            Default::default()
                        },
                        ..Default::default()
                    },
                },
            };
            SendInput(&[ki(false), ki(true)], std::mem::size_of::<INPUT>() as i32) == 2
        }
    }

    /// Our candidate list, if this process has one on screen.
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
                if pid == me && IsWindowVisible(h).as_bool() {
                    return Some(h);
                }
                after = Some(h);
            }
        }
    }

    /// A visible list with an unpainted region is the "black list" of a frozen app.
    fn popup_unpainted() -> bool {
        popup().is_some_and(|p| unsafe {
            windows::Win32::Graphics::Gdi::GetUpdateRect(p, None, false).as_bool()
        })
    }

    // -----------------------------------------------------------------------------------------
    // This build's DLL behind the registered profile (per-user COM entry for milliseconds)

    const TIP_KEY: &str = "Software\\Classes\\CLSID\\{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}";
    const OWNER_MARK: &str = "T3aImm32Harness";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// Our entry exists (with our marker value): a crashed run left it; anyone else's: None.
    fn hkcu_entry() -> Option<bool> {
        use windows::Win32::System::Registry::*;
        unsafe {
            let mut k = HKEY::default();
            let key = wide(TIP_KEY);
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                None,
                KEY_READ,
                &mut k,
            )
            .is_err()
            {
                return Some(false);
            }
            let mark = wide(OWNER_MARK);
            let ours = RegQueryValueExW(k, PCWSTR(mark.as_ptr()), None, None, None, None).is_ok();
            let _ = RegCloseKey(k);
            ours.then_some(true)
        }
    }

    fn remove_hkcu_entry() {
        use windows::Win32::System::Registry::*;
        let key = wide(TIP_KEY);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
        }
    }

    /// Points the TIP's CLSID at a DLL for this user until dropped. TSF loads the TIP as soon as the
    /// window gets a context (display attributes), so this spans window creation → profile activation.
    struct DllOverride;

    impl Drop for DllOverride {
        fn drop(&mut self) {
            remove_hkcu_entry();
        }
    }

    fn dll_override(dll: &Path) -> Result<DllOverride, String> {
        use windows::Win32::System::Registry::*;
        match hkcu_entry() {
            Some(false) => {}
            Some(true) => remove_hkcu_entry(), // left by a crashed run of this harness
            None => {
                return Err(format!(
                    "HKCU\\{TIP_KEY} exists and is not ours; not touching it"
                ))
            }
        }
        unsafe {
            let mut k = HKEY::default();
            let sub = wide(&format!("{TIP_KEY}\\InprocServer32"));
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_VOLATILE, // gone at sign-out even if everything else fails
                KEY_WRITE,
                None,
                &mut k,
                None,
            )
            .ok()
            .map_err(|e| format!("RegCreateKeyExW: {e}"))?;
            let set = |name: &str, value: &str| {
                let n = wide(name);
                let v = wide(value);
                let bytes = std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len() * 2);
                let _ = RegSetValueExW(
                    k,
                    if name.is_empty() {
                        PCWSTR::null()
                    } else {
                        PCWSTR(n.as_ptr())
                    },
                    None,
                    REG_SZ,
                    Some(bytes),
                );
            };
            set("", &dll.display().to_string());
            set("ThreadingModel", "Apartment");
            let _ = RegCloseKey(k);
            let mut parent = HKEY::default();
            let key = wide(TIP_KEY);
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                None,
                KEY_WRITE,
                &mut parent,
            )
            .is_ok()
            {
                let mark = wide(OWNER_MARK);
                let _ = RegSetValueExW(parent, PCWSTR(mark.as_ptr()), None, REG_SZ, Some(&[0, 0]));
                let _ = RegCloseKey(parent);
            }
        }
        Ok(DllOverride)
    }

    /// Path of the loaded `t3a_tip.dll` in this process.
    fn loaded_tip() -> Option<PathBuf> {
        use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
        unsafe {
            let m = GetModuleHandleW(w!("t3a_tip.dll")).ok()?;
            let mut buf = [0u16; 520];
            let n = GetModuleFileNameW(Some(m), &mut buf) as usize;
            Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
        }
    }

    // -----------------------------------------------------------------------------------------

    pub fn run() -> i32 {
        let installed = std::env::var_os("T3A_INSTALLED").is_some();
        let sandbox = std::env::temp_dir().join(format!("t3a-imm32-{}", std::process::id()));
        let _ = std::fs::create_dir_all(sandbox.join("Type3arabi"));
        let mut dll = None;
        if !installed {
            // This build's DLL + the repo's data next to it (the TIP reads type3arabi.dat beside itself).
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            let built = exe_dir.parent().map(|d| d.join("t3a_tip.dll"));
            let Some(built) = built.filter(|p| p.exists()) else {
                println!("FAIL: t3a_tip.dll not built next to the examples folder (cargo build -p t3a-tip first)");
                return 2;
            };
            let copy = sandbox.join("t3a_tip.dll");
            if std::fs::copy(&built, &copy).is_err() {
                println!("FAIL: could not copy {}", built.display());
                return 2;
            }
            let repo_dat =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/type3arabi.dat");
            if repo_dat.exists() {
                let _ = std::fs::copy(&repo_dat, sandbox.join("type3arabi.dat"));
            }
            // Learning off, fixed dialect, never the user's own store (as tsf_harness).
            let _ = std::fs::write(
                sandbox.join("Type3arabi").join("config.toml"),
                "[learning]\nenabled = false\n\n[dialect]\nprofile = \"LEV\"\n",
            );
            std::env::set_var("LOCALAPPDATA", &sandbox);
            dll = Some(copy);
        }
        // Watchdog: a key that never returns (the TIP blocked inside the host) is a frozen app too.
        // It prints the UI thread's stack and whether it burns CPU (a loop) or waits (a deadlock).
        LAST_IDLE.store(now_ms(), Ordering::Relaxed);
        let ui = stack::current_thread();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(500));
            if now_ms().saturating_sub(LAST_IDLE.load(Ordering::Relaxed)) > 8_000 {
                println!("FAIL: the host's UI thread has been blocked for 8 s (frozen app)");
                stack::report(ui);
                remove_hkcu_entry_if_ours();
                std::process::exit(20);
            }
        });
        let code = run_host(dll.as_deref());
        let _ = std::fs::remove_dir_all(&sandbox); // the DLL copy stays locked until exit: harmless
        code
    }

    fn remove_hkcu_entry_if_ours() {
        if hkcu_entry() == Some(true) {
            remove_hkcu_entry();
        }
    }

    fn run_host(dll: Option<&Path>) -> i32 {
        let over = match dll.map(dll_override).transpose() {
            Ok(o) => o,
            Err(e) => {
                println!("FAIL: {e}");
                return 2;
            }
        };
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let class = w!("T3aImm32Harness");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            // Visible and active: real keys go to the foreground window only.
            let h = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Type3arabi IMM32 harness"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                120,
                120,
                600,
                300,
                None,
                None,
                None,
                None,
            )
            .expect("window");
            let _ = SetForegroundWindow(h);
            APP.with(|a| a.borrow_mut().hwnd = h);
            let _ = SetFocus(Some(h));
            // Avalonia: a focused TextBox becomes the input client → EnableImm (posted).
            let _ = PostMessageW(Some(h), WM_APP_ENABLE, WPARAM(0), LPARAM(0));
            settle();

            let tm: ITfThreadMgr =
                CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)
                    .expect("thread mgr");
            let mgr: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
                    .expect("profile mgr");
            let clsid = GUID::from_u128(t3a_tip::ids::CLSID_TIP);
            let profile = GUID::from_u128(t3a_tip::ids::GUID_PROFILE);
            let activate = || {
                mgr.ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    t3a_tip::ids::LANGID,
                    &clsid,
                    &profile,
                    Default::default(),
                    TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
                )
            };
            let activated = activate();
            drop(over); // the per-user entry lives only until this build is loaded
            if let Err(e) = activated {
                println!("SKIP: the Type3arabi profile is not registered on this PC ({e})");
                return 4;
            }
            let loaded = loaded_tip();
            println!(
                "TIP loaded: {}",
                loaded
                    .as_ref()
                    .map_or("none".into(), |p| p.display().to_string())
            );
            if let Some(dll) = dll {
                let same = loaded
                    .as_ref()
                    .and_then(|p| p.canonicalize().ok())
                    .zip(dll.canonicalize().ok())
                    .is_some_and(|(a, b)| a == b);
                if !same {
                    println!("FAIL: TSF did not load this build's DLL");
                    return 2;
                }
            }
            // The language switch the user makes (Avalonia drops and recreates its input context).
            let lang = t3a_tip::ids::LANGID as isize;
            let _ = SendMessageW(
                h,
                WM_INPUTLANGCHANGE,
                Some(WPARAM(0)),
                Some(LPARAM(lang | (lang << 16))),
            );
            if !settle() {
                println!("FAIL: the host never got idle after the language switch");
                return report_busy();
            }
            if tm.GetFocus().ok().and_then(|dm| dm.GetTop().ok()).is_none() {
                println!("FAIL: the IMM32 window has no TSF context (CUAS off?)");
                return 2;
            }

            // (keys, expected app text). "\x08" = Backspace, "\x1B" = Esc, "\n" = Enter.
            let scenarios: &[(&str, &str)] = &[
                ("mar7aba ", "مرحباً "), // U+0645 U+0631 U+062D U+0628 U+0627 U+064B U+0020
                ("shukran ", "شكراً "),  // U+0634 U+0643 U+0631 U+0627 U+064B U+0020
                ("mar7\x08\x08\x08\x08", ""),
                ("hello\x1B", "hello"),
                ("3arabi\n", "عربي"), // U+0639 U+0631 U+0628 U+064A
            ];
            let rounds: usize = std::env::var("T3A_ROUNDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3);
            let trace = std::env::var_os("T3A_TRACE").is_some();
            let mut failures = 0;
            for round in 0..rounds {
                for (keys, expected) in scenarios {
                    APP.with(|a| a.borrow_mut().text.clear());
                    for (i, c) in keys.chars().enumerate() {
                        APP.with(|a| a.borrow_mut().counts.clear());
                        if !send_key(h, vk_for(c)) {
                            println!("ABORT: the harness window is not the foreground window; no key sent");
                            return 30;
                        }
                        if !settle() {
                            println!(
                                "FAIL round {round}: key {c:?} of {keys:?}: the app's message queue never \
                                 drained (frozen app)"
                            );
                            return report_busy();
                        }
                        if popup_unpainted() {
                            println!("FAIL round {round}: key {c:?} of {keys:?}: the list was never painted");
                            failures += 1;
                        }
                        if i == 0 && round == 0 && c == 'm' {
                            println!(
                                "PASS first letter: the app stays responsive, the list is painted"
                            );
                        }
                        if trace {
                            APP.with(|a| {
                                let a = a.borrow();
                                eprintln!(
                                    "  key {c:?}: text {:?} preedit {:?} msgs {:x?}",
                                    a.text, a.preedit, a.counts
                                );
                            });
                        }
                    }
                    let got = APP.with(|a| {
                        let a = a.borrow();
                        format!("{}{}", a.text, a.preedit)
                    });
                    let pass = got == *expected;
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
            }
            println!("{failures} scenario(s) failed");
            i32::from(failures != 0)
        }
    }

    /// Which messages kept the queue busy (the shape of the loop).
    fn report_busy() -> i32 {
        APP.with(|a| {
            let a = a.borrow();
            let mut top: Vec<_> = a.counts.iter().collect();
            top.sort_by(|x, y| y.1.cmp(x.1));
            for (m, n) in top.into_iter().take(12) {
                println!("  message {m:#06X}: {n}×");
            }
        });
        1
    }

    /// Stack of a stuck thread, without a debugger (x64): suspend, unwind, symbolize with dbghelp.
    mod stack {
        use windows::Win32::Foundation::{
            DuplicateHandle, DUPLICATE_SAME_ACCESS, FILETIME, HANDLE,
        };
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetCurrentThread, GetThreadTimes,
        };
        #[cfg(target_arch = "x86_64")]
        use windows::Win32::System::Threading::{ResumeThread, SuspendThread};

        #[derive(Clone, Copy)]
        pub struct Thread(isize);

        pub fn current_thread() -> Thread {
            let mut h = HANDLE::default();
            // SAFETY: duplicating the pseudo-handle into a real one usable from another thread.
            unsafe {
                let p = GetCurrentProcess();
                let _ = DuplicateHandle(
                    p,
                    GetCurrentThread(),
                    p,
                    &mut h,
                    0,
                    false,
                    DUPLICATE_SAME_ACCESS,
                );
            }
            Thread(h.0 as isize)
        }

        fn cpu_ms(h: HANDLE) -> u64 {
            let (mut a, mut b, mut k, mut u) = Default::default();
            // SAFETY: plain query on a valid thread handle.
            unsafe {
                let _ = GetThreadTimes(h, &mut a, &mut b, &mut k, &mut u);
            }
            let t =
                |f: FILETIME| (((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64) / 10_000;
            t(k) + t(u)
        }

        pub fn report(t: Thread) {
            let h = HANDLE(t.0 as *mut _);
            let c0 = cpu_ms(h);
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let c1 = cpu_ms(h);
            println!(
                "  UI thread CPU over 1 s: {} ms ({})",
                c1 - c0,
                if c1 - c0 > 500 {
                    "busy loop"
                } else {
                    "waiting: deadlock / blocked call"
                }
            );
            #[cfg(target_arch = "x86_64")]
            for sample in 0..3 {
                println!("  -- stack sample {sample}");
                for (i, pc) in frames(h).into_iter().enumerate() {
                    println!("  #{i:02} {}", symbol(pc));
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }

        #[cfg(target_arch = "x86_64")]
        fn frames(h: HANDLE) -> Vec<u64> {
            use windows::Win32::System::Diagnostics::Debug::*;
            let mut out = Vec::new();
            // SAFETY: the thread is suspended while its context is read and unwound; unwinding only
            // reads its stack memory, which stays mapped. Resumed before symbolization.
            unsafe {
                SuspendThread(h);
                let mut ctx = Box::new(CONTEXT {
                    ContextFlags: CONTEXT_FULL_AMD64,
                    ..Default::default()
                });
                if GetThreadContext(h, &mut *ctx).is_ok() {
                    for _ in 0..80 {
                        let pc = ctx.Rip;
                        if pc == 0 {
                            break;
                        }
                        out.push(pc);
                        let mut base = 0u64;
                        let f = RtlLookupFunctionEntry(pc, &mut base, None);
                        if f.is_null() {
                            ctx.Rip = *(ctx.Rsp as *const u64);
                            ctx.Rsp += 8;
                        } else {
                            let mut data = std::ptr::null_mut();
                            let mut frame = 0u64;
                            RtlVirtualUnwind(
                                UNW_FLAG_NHANDLER,
                                base,
                                pc,
                                f,
                                &mut *ctx,
                                &mut data,
                                &mut frame,
                                None,
                            );
                        }
                    }
                }
                ResumeThread(h);
            }
            out
        }

        #[cfg(target_arch = "x86_64")]
        fn symbol(pc: u64) -> String {
            use std::sync::Once;
            use windows::Win32::System::Diagnostics::Debug::*;
            use windows::Win32::System::LibraryLoader::{
                GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            };
            static INIT: Once = Once::new();
            #[repr(C)]
            struct Buf {
                info: SYMBOL_INFOW,
                name: [u16; 512],
            }
            // SAFETY: dbghelp calls from one thread (the watchdog); buffers sized as dbghelp expects.
            unsafe {
                let p = GetCurrentProcess();
                INIT.call_once(|| {
                    SymSetOptions(0x2 | 0x10 | 0x4); // UNDNAME | LOAD_LINES | DEFERRED_LOADS
                    let path: Vec<u16> = std::env::var("T3A_SYMPATH")
                        .unwrap_or_default()
                        .encode_utf16()
                        .chain([0])
                        .collect();
                    let _ = SymInitializeW(p, windows::core::PCWSTR(path.as_ptr()), true);
                });
                let mut module = windows::Win32::Foundation::HMODULE::default();
                let mut name = String::from("?");
                if GetModuleHandleExW(
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                        | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    windows::core::PCWSTR(pc as usize as *const u16),
                    &mut module,
                )
                .is_ok()
                {
                    let mut buf = [0u16; 260];
                    let n = GetModuleFileNameW(Some(module), &mut buf) as usize;
                    name = String::from_utf16_lossy(&buf[..n])
                        .rsplit('\\')
                        .next()
                        .unwrap_or("?")
                        .to_string();
                }
                let off = pc.wrapping_sub(module.0 as u64);
                let mut b: Box<Buf> = Box::new(std::mem::zeroed());
                b.info.SizeOfStruct = std::mem::size_of::<SYMBOL_INFOW>() as u32;
                b.info.MaxNameLen = 512;
                let mut disp = 0u64;
                let mut s = format!("{name}+{off:#x}");
                if SymFromAddrW(p, pc, Some(&mut disp), &mut b.info).is_ok() {
                    let len = (b.info.NameLen as usize).min(511);
                    let base = std::ptr::addr_of!(b.info.Name) as *const u16;
                    let n = String::from_utf16_lossy(std::slice::from_raw_parts(base, len));
                    s = format!("{name}!{n}+{disp:#x}");
                    let mut line = IMAGEHLP_LINEW64 {
                        SizeOfStruct: std::mem::size_of::<IMAGEHLP_LINEW64>() as u32,
                        ..Default::default()
                    };
                    let mut ld = 0u32;
                    if SymGetLineFromAddrW64(p, pc, &mut ld, &mut line).is_ok()
                        && !line.FileName.is_null()
                    {
                        let f = line.FileName.to_string().unwrap_or_default();
                        let f: Vec<&str> = f.rsplit(['\\', '/']).take(2).collect();
                        s.push_str(&format!(
                            "  ({}/{}:{})",
                            f[f.len() - 1],
                            f[0],
                            line.LineNumber
                        ));
                    }
                }
                s
            }
        }
    }
}
