//! Host-process safety guard (AGENTS.md R1, R2; docs/02 §15).
//!
//! Catches panics across COM boundaries, activates process-wide safe-passthrough,
//! and logs the event (without user text).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::HRESULT;
use windows::Win32::Foundation::{E_FAIL, S_OK};

static DISABLED: AtomicBool = AtomicBool::new(false);

/// Returns true if the TIP has entered safe-passthrough mode in this process.
pub fn is_disabled() -> bool {
    DISABLED.load(Ordering::Acquire)
}

/// Disables the TIP for the current process (safe passthrough).
pub fn set_disabled() {
    DISABLED.store(true, Ordering::Release);
}

/// Executes `f` inside `catch_unwind`. If disabled or if `f` panics, returns `fallback`.
pub fn guard<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce() -> R,
{
    if is_disabled() {
        return fallback;
    }

    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(res) => res,
        Err(_) => {
            set_disabled();
            // Unit tests panic on purpose: they must not write into the real user's error log.
            #[cfg(not(test))]
            t3a_paths::log_error("panic caught across COM boundary; entering safe passthrough");
            fallback
        }
    }
}

/// Convenience guard for methods returning COM HRESULT.
pub fn guard_hr<F>(f: F) -> HRESULT
where
    F: FnOnce() -> windows::core::Result<()>,
{
    if is_disabled() {
        return E_FAIL;
    }

    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(res) => match res {
            Ok(()) => S_OK,
            Err(e) => e.code(),
        },
        Err(_) => {
            set_disabled();
            #[cfg(not(test))]
            t3a_paths::log_error("panic caught in COM method; entering safe passthrough");
            E_FAIL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard_catches_panic_and_disables() {
        // Run in separate logic so as not to affect other tests
        let res = guard(42, || {
            if true {
                panic!("intentional test panic");
            }
            100
        });
        assert_eq!(res, 42);
        assert!(is_disabled());
    }
}
