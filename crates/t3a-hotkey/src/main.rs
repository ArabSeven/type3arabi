//! t3a-hotkey — optional global activation hotkey companion (docs/02 §11). Implemented in M7 after Spike S3.
//! Behavior contract: read config; exit immediately if `general.global_hotkey_enabled = false`; register the
//! hotkey (MOD_NOREPEAT); on WM_HOTKEY toggle the foreground window between Type3arabi and the last
//! non-Arabic keyboard; never poll, never hook.

fn main() {
    #[cfg(not(windows))]
    eprintln!("t3a-hotkey runs on Windows only.");
    #[cfg(windows)]
    {
        // M7: message-only window + RegisterHotKey + GetMessageW loop.
    }
}
