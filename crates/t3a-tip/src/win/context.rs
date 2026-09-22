//! Context inspection and input scope gating (docs/02 §7).

use crate::keyrouter::ContextMode;
use windows::Win32::UI::TextServices::{ITfContext, TS_SD_READONLY};

/// Determines the context mode (Off, Latin, Arabic) for the given TSF context.
pub fn evaluate_context_mode(
    context: Option<&ITfContext>,
    arabic_mode: bool,
    secure_mode: bool,
) -> ContextMode {
    let Some(ctx) = context else {
        return ContextMode::Off;
    };

    unsafe {
        if let Ok(status) = ctx.GetStatus() {
            // Read-only context -> engine off
            if (status.dwDynamicFlags & TS_SD_READONLY) != 0 {
                return ContextMode::Off;
            }
        }
    }

    if secure_mode {
        return ContextMode::Latin;
    }

    if !arabic_mode {
        ContextMode::Latin
    } else {
        ContextMode::Arabic
    }
}
