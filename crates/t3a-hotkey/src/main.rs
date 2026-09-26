//! t3a-hotkey — global activation hotkey companion (docs/02 §11) and small installer helper.
//!
//!   t3a-hotkey.exe                    at sign-in; stays running: keeps the keyboard list tidy and
//!                                     runs the hotkey (if enabled)
//!   t3a-hotkey.exe --tidy             unload Arabic layouts the user did not choose (see profile.rs)
//!   t3a-hotkey.exe --enable-profile   enable the Type3arabi keyboard for the signed-in user
//!   t3a-hotkey.exe --disable-profile  remove it from the user's keyboards
//!   t3a-hotkey.exe --restart-warning  explain what may not work until the next restart
//!   t3a-hotkey.exe --list-profiles    print the user's input profiles (what Win+Space lists)
//!
//! Hotkey loop: a message-only window registers the hotkey (MOD_NOREPEAT); WM_HOTKEY switches the
//! foreground window between the Arabic (Type3arabi) keyboard and the last non-Arabic one via
//! WM_INPUTLANGCHANGEREQUEST. No hooks (R5). The Settings app posts `WM_APP_RELOAD` to the window class
//! `Type3arabi_Hotkey` after changing the hotkey. Tidy-up: a 10 s timer compares the session's loaded
//! layouts (one user32 call) and runs `profile::tidy` only when they changed, so an Arabic 101 that
//! Windows loads at any time (sign-in, unlock, an app) is gone within seconds.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(not(windows))]
    eprintln!("t3a-hotkey runs on Windows only.");
    #[cfg(windows)]
    std::process::exit(win::main());
}

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
        GetWindowThreadProcessId, MessageBoxW, PostMessageW, RegisterClassW, SetTimer,
        HWND_MESSAGE, MB_ICONINFORMATION, MB_OK, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
        WM_HOTKEY, WM_INPUTLANGCHANGEREQUEST, WM_TIMER, WNDCLASSW,
    };

    /// Sent by the Settings app after it rewrote `config.toml`.
    pub const WM_APP_RELOAD: u32 = WM_APP + 1;
    const HOTKEY_ID: i32 = 1;
    const TIDY_TIMER: usize = 1;
    const TIDY_EVERY_MS: u32 = 10_000;
    const LANG_ARABIC: u16 = 0x01;

    pub fn main() -> i32 {
        let arg = std::env::args().nth(1).unwrap_or_default();
        match arg.as_str() {
            "--enable-profile" => t3a_hotkey::profile::enable(),
            "--disable-profile" => t3a_hotkey::profile::disable(),
            "--list-profiles" => {
                list_profiles();
                0
            }
            "--restart-warning" => {
                restart_warning();
                0
            }
            "--tidy" => {
                let _ = t3a_hotkey::profile::tidy();
                0
            }
            _ => run_companion(),
        }
    }

    /// Loaded layouts at the last tidy-up that left nothing behind (message thread only). Only the
    /// list is compared, so a user's own Arabic 101 costs one tidy-up per change, not one per tick.
    static TIDY_SEEN: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

    /// Tidy up if the session's loaded layouts changed since the last clean check. Windows loads
    /// layouts during sign-in, on unlock and when apps ask, so this runs at start and on the timer.
    fn tidy_if_changed() {
        let now = t3a_hotkey::profile::loaded_layouts();
        let mut seen = TIDY_SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if *seen == now {
            return;
        }
        let done = t3a_hotkey::profile::tidy();
        // Remember the list only when nothing was left behind, so a failed unload is retried.
        *seen = if done.failed == 0 {
            t3a_hotkey::profile::loaded_layouts()
        } else {
            Vec::new()
        };
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
        for p in t3a_hotkey::profile::list_profiles(0) {
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
                t3a_paths::write_hotkey_status("off", "");
                return false;
            }
            if t3a_engine::config::reserved_shortcut(&config.global_hotkey).is_some() {
                // A system-wide copy of e.g. Ctrl+C would break copying in every app.
                t3a_paths::log_error(
                    "global_hotkey: reserved by Windows or common apps; not registered",
                );
                t3a_paths::write_hotkey_status("reserved", &config.global_hotkey);
                return false;
            }
            let Ok(spec) = parse(&config.global_hotkey) else {
                t3a_paths::log_error("global_hotkey: invalid value in config.toml");
                t3a_paths::write_hotkey_status("invalid", &config.global_hotkey);
                return false;
            };
            if RegisterHotKey(Some(hwnd), HOTKEY_ID, modifiers(&spec), spec.vk as u32).is_err() {
                t3a_paths::log_error("global_hotkey: already used by another program");
                t3a_paths::write_hotkey_status("taken", &config.global_hotkey);
                return false;
            }
            t3a_paths::write_hotkey_status("ok", &config.global_hotkey);
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
                t3a_hotkey::arabic_hotkey_target(
                    &loaded.iter().map(|h| h.0 as usize).collect::<Vec<_>>(),
                )
                .map(|h| HKL(h as *mut _))
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
            WM_TIMER if w.0 == TIDY_TIMER => {
                tidy_if_changed();
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, w, l),
        }
    }

    /// The resident companion: hotkey (if enabled) and the keyboard-list tidy-up. Idle cost: blocked
    /// in `GetMessageW` between 10 s timer ticks that make one user32 call.
    fn run_companion() -> i32 {
        // SAFETY: single-instance mutex, message-only window and a standard message loop.
        unsafe {
            let _mutex = CreateMutexW(None, true, w!("Local\\Type3arabi.Hotkey"));
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = t3a_hotkey::profile::tidy();
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
            tidy_if_changed();
            SetTimer(Some(hwnd), TIDY_TIMER, TIDY_EVERY_MS, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        0
    }
}
