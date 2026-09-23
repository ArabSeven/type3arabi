//! Type3arabi Settings (docs/05 §7, ADR-0007). The UI (`ui/`) is plain HTML/JS; every read and write
//! of `%LOCALAPPDATA%\Type3arabi\config.toml` goes through these commands, which reuse the engine's
//! parser/serializer and validators, so the TIP and Settings can never disagree about the format.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use t3a_engine::config::is_chord;
use t3a_engine::{Config, UserStore};

/// The settings the UI edits (a subset of `Config`; untouched keys are preserved on save).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Settings {
    // Keyboard
    mode_toggle: String,
    global_hotkey_enabled: bool,
    global_hotkey: String,
    key_commit_latin: String,
    key_open_tashkeel: String,
    key_commit_harakat: String,
    // Typing
    inline_preview: String,
    candidates_per_page: i64,
    predictive_completions: bool,
    article_joining: bool,
    reedit_backspace: bool,
    arabic_punctuation: bool,
    numerals: String,
    dialect_profile: String,
    show_dialect_badge: bool,
    // Diacritics & style
    allah_form: String,
    adverbial_tanween: bool,
    hamza: String,
    vowel_harakat: String,
    // Learning & privacy
    learning_enabled: bool,
    sticky_last_choice: bool,
    use_surrounding_text: bool,
}

impl From<&Config> for Settings {
    fn from(c: &Config) -> Self {
        Settings {
            mode_toggle: c.mode_toggle.clone(),
            global_hotkey_enabled: c.global_hotkey_enabled,
            global_hotkey: c.global_hotkey.clone(),
            key_commit_latin: c.key_commit_latin.clone(),
            key_open_tashkeel: c.key_open_tashkeel.clone(),
            key_commit_harakat: c.key_commit_harakat.clone(),
            inline_preview: c.inline_preview.clone(),
            candidates_per_page: c.candidates_per_page,
            predictive_completions: c.predictive_completions,
            article_joining: c.article_joining,
            reedit_backspace: c.reedit_backspace,
            arabic_punctuation: c.arabic_punctuation,
            numerals: c.numerals.clone(),
            dialect_profile: c.dialect_profile.clone(),
            show_dialect_badge: c.show_dialect_badge,
            allah_form: c.allah_form.clone(),
            adverbial_tanween: c.adverbial_tanween,
            hamza: c.hamza.clone(),
            vowel_harakat: c.vowel_harakat.clone(),
            learning_enabled: c.learning_enabled,
            sticky_last_choice: c.sticky_last_choice,
            use_surrounding_text: c.use_surrounding_text,
        }
    }
}

fn load() -> Config {
    std::fs::read_to_string(t3a_paths::config_path())
        .map(|t| Config::parse(&t).0)
        .unwrap_or_default()
}

/// Problems that block saving (invalid values) — the UI shows them next to the fields.
fn validate(s: &Settings) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, v) in [
        ("key_commit_latin", &s.key_commit_latin),
        ("key_open_tashkeel", &s.key_open_tashkeel),
        ("key_commit_harakat", &s.key_commit_harakat),
    ] {
        if !is_chord(v) {
            errors.push(format!("{name}: not a valid shortcut"));
        }
    }
    let toggles = [
        "Ctrl+Space",
        "Shift+Space",
        "Ctrl+Shift+Space",
        "ShiftTap",
        "none",
    ];
    if !toggles.contains(&s.mode_toggle.as_str()) {
        errors.push("mode_toggle: choose one of the listed toggles".into());
    }
    if s.global_hotkey_enabled {
        if let Err(e) = t3a_hotkey::parse(&s.global_hotkey) {
            errors.push(format!("global_hotkey: {e}"));
        }
    }
    let keys = [
        &s.key_commit_latin,
        &s.key_open_tashkeel,
        &s.key_commit_harakat,
    ];
    for (i, a) in keys.iter().enumerate() {
        for b in &keys[i + 1..] {
            if !a.eq_ignore_ascii_case("none") && a.eq_ignore_ascii_case(b) {
                errors.push(format!("{a}: used for two actions"));
            }
        }
    }
    if !(5..=9).contains(&s.candidates_per_page) {
        errors.push("candidates_per_page: 5 to 9".into());
    }
    errors
}

#[tauri::command]
fn get_settings() -> Settings {
    Settings::from(&load())
}

#[tauri::command]
fn defaults() -> Settings {
    Settings::from(&Config::default())
}

#[tauri::command]
fn check_chord(chord: String) -> bool {
    is_chord(&chord)
}

/// Validate, merge into the current config (keys the UI does not show are kept), write atomically,
/// and tell the hotkey companion to re-register. The TIP picks the file up at the next word.
#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), Vec<String>> {
    let errors = validate(&settings);
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut c = load();
    let s = settings;
    c.mode_toggle = s.mode_toggle;
    c.global_hotkey_enabled = s.global_hotkey_enabled;
    c.global_hotkey = s.global_hotkey;
    c.key_commit_latin = s.key_commit_latin;
    c.key_open_tashkeel = s.key_open_tashkeel;
    c.key_commit_harakat = s.key_commit_harakat;
    c.inline_preview = s.inline_preview;
    c.candidates_per_page = s.candidates_per_page;
    c.predictive_completions = s.predictive_completions;
    c.article_joining = s.article_joining;
    c.reedit_backspace = s.reedit_backspace;
    c.arabic_punctuation = s.arabic_punctuation;
    c.numerals = s.numerals;
    c.dialect_profile = s.dialect_profile;
    c.show_dialect_badge = s.show_dialect_badge;
    c.allah_form = s.allah_form;
    c.adverbial_tanween = s.adverbial_tanween;
    c.hamza = s.hamza;
    c.vowel_harakat = s.vowel_harakat;
    c.learning_enabled = s.learning_enabled;
    c.sticky_last_choice = s.sticky_last_choice;
    c.use_surrounding_text = s.use_surrounding_text;

    let path = t3a_paths::config_path();
    let _ = t3a_paths::ensure_user_dir();
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, c.to_toml()).map_err(|e| vec![format!("cannot write settings: {e}")])?;
    std::fs::rename(&tmp, &path).map_err(|e| vec![format!("cannot save settings: {e}")])?;
    notify_hotkey(c.global_hotkey_enabled);
    Ok(())
}

/// Forget everything learned (docs/05 §7 "My words"): a WIPE journal record, which every running app
/// applies when it next syncs (at the start of a word).
#[tauri::command]
fn wipe_learning() -> Result<(), String> {
    let store = UserStore::open(&t3a_paths::user_store_dir(), false).map_err(|e| e.to_string())?;
    store.wipe();
    drop(store); // the writer thread flushes the record before exiting
    std::thread::sleep(std::time::Duration::from_millis(150));
    Ok(())
}

#[derive(Serialize)]
struct About {
    version: String,
    config_path: String,
    data_file: String,
}

#[tauri::command]
fn about() -> About {
    About {
        version: env!("CARGO_PKG_VERSION").into(),
        config_path: t3a_paths::config_path().display().to_string(),
        data_file: t3a_paths::data_file_path().display().to_string(),
    }
}

/// Re-register the global hotkey in the running companion, or start it if it is not running.
fn notify_hotkey(enabled: bool) {
    #[cfg(windows)]
    {
        use windows::core::{w, PCWSTR};
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowExW, PostMessageW, HWND_MESSAGE, WM_APP,
        };
        // SAFETY: find the companion's message-only window and post it a private message.
        let found = unsafe {
            FindWindowExW(
                Some(HWND_MESSAGE),
                None,
                w!("Type3arabi_Hotkey"),
                PCWSTR::null(),
            )
        };
        match found {
            Ok(h) => unsafe {
                let _ = PostMessageW(Some(h), WM_APP + 1, WPARAM(0), LPARAM(0));
            },
            Err(_) if enabled => {
                if let Some(exe) = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("t3a-hotkey.exe")))
                    .filter(|p| p.exists())
                {
                    let _ = std::process::Command::new(exe).spawn();
                }
            }
            Err(_) => {}
        }
    }
    #[cfg(not(windows))]
    let _ = enabled;
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_settings,
            defaults,
            check_chord,
            save_settings,
            wipe_learning,
            about
        ])
        .run(tauri::generate_context!())
        .expect("Type3arabi Settings failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(validate(&Settings::from(&Config::default())).is_empty());
    }

    #[test]
    fn duplicate_and_invalid_shortcuts_are_rejected() {
        let mut s = Settings::from(&Config::default());
        s.key_open_tashkeel = "Shift+Space".into();
        s.key_commit_harakat = "Hyper+Q".into();
        let e = validate(&s);
        assert!(e.iter().any(|m| m.contains("two actions")));
        assert!(e.iter().any(|m| m.starts_with("key_commit_harakat")));
    }
}
