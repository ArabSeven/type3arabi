//! t3a-hotkey — global activation hotkey companion (docs/02 §11) and small installer helper.
//!
//!   t3a-hotkey.exe                    run the hotkey loop (exits at once if disabled in config)
//!   t3a-hotkey.exe --enable-profile   enable the Type3arabi keyboard for the signed-in user
//!   t3a-hotkey.exe --disable-profile  remove it from the user's keyboards
//!   t3a-hotkey.exe --restart-warning  explain what may not work until the next restart
//!   t3a-hotkey.exe --list-profiles    print the user's input profiles (what Win+Space lists)
//!
//! Hotkey loop: a message-only window registers the hotkey (MOD_NOREPEAT); WM_HOTKEY switches the
//! foreground window between the Arabic (Type3arabi) keyboard and the last non-Arabic one via
//! WM_INPUTLANGCHANGEREQUEST. No polling, no hooks (R5). The Settings app posts `WM_APP_RELOAD` to the
//! window class `Type3arabi_Hotkey` after changing the hotkey.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(not(windows))]
    eprintln!("t3a-hotkey runs on Windows only.");
    #[cfg(windows)]
    std::process::exit(win::main());
}

#[cfg(windows)]
mod profile;

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use t3a_engine::Config;
    use t3a_hotkey::{parse, HotkeySpec};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{
        GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
    };
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, GetKeyboardLayoutList, RegisterHotKey, UnregisterHotKey, HKL,
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW,
        GetWindowThreadProcessId, MessageBoxW, PostMessageW, RegisterClassW, HWND_MESSAGE,
        MB_ICONINFORMATION, MB_OK, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_HOTKEY,
        WM_INPUTLANGCHANGEREQUEST, WNDCLASSW,
    };

    /// Sent by the Settings app after it rewrote `config.toml`.
    pub const WM_APP_RELOAD: u32 = WM_APP + 1;
    const HOTKEY_ID: i32 = 1;
    const LANG_ARABIC: u16 = 0x01;

    pub fn main() -> i32 {
        let arg = std::env::args().nth(1).unwrap_or_default();
        match arg.as_str() {
            "--enable-profile" => crate::profile::enable(),
            "--disable-profile" => crate::profile::disable(),
            "--list-profiles" => {
                list_profiles();
                0
            }
            "--restart-warning" => {
                restart_warning();
                0
            }
            _ => run_hotkey_loop(),
        }
    }

    fn load_config() -> Config {
        std::fs::read_to_string(t3a_paths::config_path())
            .map(|t| Config::parse(&t).0)
            .unwrap_or_default()
    }

    /// Print every input profile (to the calling console; this is a GUI-subsystem exe).
    fn list_profiles() {
        use std::io::Write;
        let mut out = String::from(
            "lang  kind    enabled  name
",
        );
        for p in crate::profile::list_profiles(0) {
            out.push_str(&format!(
                "{:04X}  {:<6}  {:<7}  {}
",
                p.lang,
                if p.tip { "tip" } else { "layout" },
                p.enabled,
                p.name
            ));
        }
        // Redirected output (pipe/file) works as is; a bare GUI-subsystem exe has to attach to the
        // console it was started from.
        if std::io::stdout().write_all(out.as_bytes()).is_err() {
            // SAFETY: attaching to the parent console has no preconditions; failure = no console.
            if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok() {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .open("CONOUT$")
                    .and_then(|mut f| f.write_all(out.as_bytes()));
            }
        }
    }

    fn restart_warning() {
        // The message box lays text out left to right: a trailing RLM (U+200F) after each Arabic line keeps
        // its final period at the end of the Arabic sentence instead of its start (Owner, 2026-09-25).
        let text = "Type3arabi is installed and works in apps you open from now on.\n\n\
            Until you restart (or sign out and back in), apps that were already open may not \
            list Type3arabi, may keep an older version, or may need to be reopened. If typing \
            behaves oddly, restart first.\n\n\
            تم تثبيت «اكتب عربي» ويعمل في البرامج التي تفتحها من الآن.\u{200F}\n\
            إلى أن تعيد تشغيل الجهاز (أو تسجّل الخروج ثم الدخول)، قد لا تظهر لوحة «اكتب عربي» في البرامج \
            المفتوحة مسبقاً أو قد تستخدم نسخة أقدم. إذا لاحظت سلوكاً غريباً فأعد التشغيل أولاً.\u{200F}";
        let t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: plain modal message box with NUL-terminated strings.
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(t.as_ptr()),
                w!("Type3arabi — restart recommended"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }

    fn modifiers(h: &HotkeySpec) -> HOT_KEY_MODIFIERS {
        let mut m = MOD_NOREPEAT;
        if h.ctrl {
            m |= MOD_CONTROL;
        }
        if h.alt {
            m |= MOD_ALT;
        }
        if h.shift {
            m |= MOD_SHIFT;
        }
        if h.win {
            m |= MOD_WIN;
        }
        m
    }

    /// (Re)register the configured hotkey on `hwnd`. Returns false if disabled or taken.
    fn register(hwnd: HWND) -> bool {
        // SAFETY: plain Win32 calls on our own window.
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
            let config = load_config();
            if !config.global_hotkey_enabled {
                return false;
            }
            let Ok(spec) = parse(&config.global_hotkey) else {
                t3a_paths::log_error("global_hotkey: invalid value in config.toml");
                return false;
            };
            if RegisterHotKey(Some(hwnd), HOTKEY_ID, modifiers(&spec), spec.vk as u32).is_err() {
                t3a_paths::log_error("global_hotkey: already used by another program");
                return false;
            }
            true
        }
    }

    fn primary_lang(hkl: HKL) -> u16 {
        (hkl.0 as usize as u16) & 0x3FF
    }

    /// Last non-Arabic keyboard of the foreground window (message thread only).
    static LAST_LATIN: AtomicIsize = AtomicIsize::new(0);

    /// Switch the foreground window between Arabic (Type3arabi) and the last non-Arabic keyboard.
    fn toggle() {
        // SAFETY: plain Win32 queries on the foreground window.
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_invalid() {
                return;
            }
            let tid = GetWindowThreadProcessId(fg, None);
            let cur = GetKeyboardLayout(tid);
            let mut list = [HKL::default(); 32];
            let n = (GetKeyboardLayoutList(Some(&mut list)) as usize).min(32);
            let loaded = &list[..n];
            let target = if primary_lang(cur) == LANG_ARABIC {
                let last = HKL(LAST_LATIN.load(Ordering::Relaxed) as *mut _);
                if loaded.contains(&last) {
                    Some(last)
                } else {
                    loaded
                        .iter()
                        .copied()
                        .find(|h| primary_lang(*h) != LANG_ARABIC)
                }
            } else {
                LAST_LATIN.store(cur.0 as isize, Ordering::Relaxed);
                loaded
                    .iter()
                    .copied()
                    .find(|h| primary_lang(*h) == LANG_ARABIC)
            };
            if let Some(hkl) = target {
                let _ = PostMessageW(
                    Some(fg),
                    WM_INPUTLANGCHANGEREQUEST,
                    WPARAM(0),
                    LPARAM(hkl.0 as isize),
                );
            }
        }
    }

    unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_HOTKEY if w.0 as i32 == HOTKEY_ID => {
                toggle();
                LRESULT(0)
            }
            WM_APP_RELOAD => {
                register(h);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, w, l),
        }
    }

    fn run_hotkey_loop() -> i32 {
        if !load_config().global_hotkey_enabled {
            return 0;
        }
        // SAFETY: single-instance mutex, message-only window and a standard message loop.
        unsafe {
            let _mutex = CreateMutexW(None, true, w!("Local\\Type3arabi.Hotkey"));
            if GetLastError() == ERROR_ALREADY_EXISTS {
                return 0;
            }
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("Type3arabi_Hotkey");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                class,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinst.into()),
                None,
            ) else {
                return 1;
            };
            register(hwnd);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        0
    }
}
