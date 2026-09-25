//! Type3arabi Settings (docs/05 §7, ADR-0007). The UI (`ui/`) is plain HTML/JS; every read and write
//! of `%LOCALAPPDATA%\Type3arabi\config.toml` goes through these commands, which reuse the engine's
//! parser/serializer and validators, so the TIP and Settings can never disagree about the format.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use t3a_engine::config::{is_chord, reserved_shortcut};
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
        } else if let Some(r) = reserved_shortcut(v) {
            errors.push(format!("{v}: this shortcut is used {}", r.en));
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
        } else if let Some(r) = reserved_shortcut(&s.global_hotkey) {
            errors.push(format!(
                "{}: this shortcut is used {}",
                s.global_hotkey, r.en
            ));
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

/// Why `chord` cannot be used, in Arabic and English, or `None` if it can. `kind`: "chord" (typing
/// shortcuts) or "hotkey" (the global hotkey). Windows' and common app shortcuts are refused with the
/// reason (Owner, 2026-09-25).
#[tauri::command]
fn check_shortcut(chord: String, kind: String) -> Option<[String; 2]> {
    if kind == "hotkey" {
        if t3a_hotkey::parse(&chord).is_err() {
            return Some([
                "اختصار التفعيل العام يحتاج Ctrl أو Alt".into(),
                "the global hotkey needs Ctrl or Alt".into(),
            ]);
        }
    } else if !is_chord(&chord) {
        return Some([
            "سيعطّل الكتابة العادية — الأحرف تحتاج Ctrl أو Alt".into(),
            "it would get in the way of normal typing — letters need Ctrl or Alt".into(),
        ]);
    }
    reserved_shortcut(&chord).map(|r| {
        [
            format!("هذا الاختصار يستخدمه {}", r.ar),
            format!("this shortcut is used {}", r.en),
        ]
    })
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

// ---- Move learning to another PC (docs/03 §9.5, docs/05 §7) ----

#[derive(Serialize)]
struct Exported {
    path: String,
    records: usize,
}

#[derive(Serialize)]
struct LearningPreview {
    records: usize,
    has_settings: bool,
}

#[derive(Serialize)]
struct Imported {
    records: usize,
    settings_restored: bool,
}

/// `Downloads` when it exists (where people look for a file they just saved), else the user folder.
fn export_dir() -> std::path::PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|p| std::path::PathBuf::from(p).join("Downloads"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(t3a_paths::user_dir)
}

/// YYYY-MM-DD for today (UTC), without a date library.
fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

// ---- transfer history (Owner, 2026-09-25): which exports/imports happened on this PC, and when. Only
// counts, file names and times are kept — never typed text (R8). One JSON object per line.

const HISTORY_FILE: &str = "transfers.jsonl";
const HISTORY_MAX: usize = 200;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Transfer {
    /// Seconds since 1970 (UTC); the UI shows local time.
    t: u64,
    /// "export" | "import"
    kind: String,
    records: usize,
    /// File name (export: full path, so it can be found again).
    file: String,
    /// Import only: "merge" | "replace".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    mode: String,
    /// Settings were included (export) or restored (import).
    settings: bool,
}

fn history_path() -> std::path::PathBuf {
    t3a_paths::user_dir().join(HISTORY_FILE)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Append one entry, keeping the newest `HISTORY_MAX`. Failures are ignored: history is a convenience.
fn record_transfer(entry: Transfer) {
    let path = history_path();
    let mut all = read_history(&path);
    all.push(entry);
    let start = all.len().saturating_sub(HISTORY_MAX);
    let text: String = all[start..]
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .map(|l| l + "\n")
        .collect();
    let _ = t3a_paths::ensure_user_dir();
    let _ = std::fs::write(path, text);
}

fn read_history(path: &std::path::Path) -> Vec<Transfer> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Newest first.
#[tauri::command]
fn transfer_history() -> Vec<Transfer> {
    let mut v = read_history(&history_path());
    v.reverse();
    v
}

/// The only pages Settings may open, in the default browser (never inside the app; no other URL).
const LINKS: [&str; 4] = [
    "https://buymeacoffee.com/hassanobaida",
    "https://linktr.ee/hassanobaida",
    "https://type3arabi.com/",
    "https://github.com/ArabSeven/type3arabi",
];

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !LINKS.contains(&url.as_str()) {
        return Err("not an allowed link".into());
    }
    #[cfg(windows)]
    {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        // SAFETY: ShellExecuteW with NUL-terminated strings; opens the URL in the default browser.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(url.as_str()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }
    Ok(())
}

#[tauri::command]
fn export_learning(include_settings: bool) -> Result<Exported, String> {
    let store = UserStore::open(&t3a_paths::user_store_dir(), true).map_err(|e| e.to_string())?;
    let settings = include_settings
        .then(|| std::fs::read_to_string(t3a_paths::config_path()).ok())
        .flatten();
    let bytes = store
        .export(settings.as_deref())
        .map_err(|e| e.to_string())?;
    let records = t3a_engine::learning_file::decode(&bytes)
        .map(|f| f.records.len())
        .unwrap_or(0);
    let dir = export_dir();
    let stem = format!("Type3arabi-learning-{}", today());
    let ext = t3a_engine::learning_file::EXTENSION;
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    std::fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    record_transfer(Transfer {
        t: now_secs(),
        kind: "export".into(),
        records,
        file: path.display().to_string(),
        mode: String::new(),
        settings: settings.is_some(),
    });
    Ok(Exported {
        path: path.display().to_string(),
        records,
    })
}

#[tauri::command]
fn inspect_learning(bytes: Vec<u8>) -> Result<LearningPreview, String> {
    let f = t3a_engine::learning_file::decode(&bytes)?;
    Ok(LearningPreview {
        records: f.records.len(),
        has_settings: f.settings.is_some(),
    })
}

/// Replay the file's choices through the journal (every running app picks them up). Settings are
/// parsed and re-serialized, so only known keys with valid values are ever written.
#[tauri::command]
fn import_learning(
    bytes: Vec<u8>,
    replace: bool,
    restore_settings: bool,
    file_name: String,
) -> Result<Imported, String> {
    let file = t3a_engine::learning_file::decode(&bytes)?;
    let _ = t3a_paths::ensure_user_dir();
    let store = UserStore::open(&t3a_paths::user_store_dir(), false).map_err(|e| e.to_string())?;
    let records = store.import(&file, replace).map_err(|e| e.to_string())?;
    drop(store); // the writer thread flushes the journal before exiting
    std::thread::sleep(std::time::Duration::from_millis(150));
    let mut settings_restored = false;
    if restore_settings {
        if let Some(text) = &file.settings {
            let c = Config::parse(text).0;
            let path = t3a_paths::config_path();
            let tmp = path.with_extension("toml.tmp");
            std::fs::write(&tmp, c.to_toml()).map_err(|e| format!("cannot write settings: {e}"))?;
            std::fs::rename(&tmp, &path).map_err(|e| format!("cannot save settings: {e}"))?;
            notify_hotkey(c.global_hotkey_enabled);
            settings_restored = true;
        }
    }
    record_transfer(Transfer {
        t: now_secs(),
        kind: "import".into(),
        records,
        file: file_name,
        mode: if replace { "replace" } else { "merge" }.into(),
        settings: settings_restored,
    });
    Ok(Imported {
        records,
        settings_restored,
    })
}

/// This user's keyboard and hotkey state (docs/05 §7 General: "shows conflicts"; backlog M7 "enable for
/// this user" — a per-machine install turns the keyboard on only for the account that installed it).
#[derive(Serialize)]
struct KeyboardStatus {
    /// Type3arabi is in this user's keyboard list.
    enabled: bool,
    /// Companion's last registration: "ok" | "taken" | "invalid" | "off" | "" (not reported yet).
    hotkey_state: String,
    hotkey: String,
}

#[tauri::command]
fn keyboard_status() -> KeyboardStatus {
    #[cfg(windows)]
    let enabled = t3a_hotkey::profile::is_enabled();
    #[cfg(not(windows))]
    let enabled = false;
    let (hotkey_state, hotkey) = t3a_paths::read_hotkey_status().unwrap_or_default();
    KeyboardStatus {
        enabled,
        hotkey_state,
        hotkey,
    }
}

/// Add the Type3arabi keyboard to this user's keyboards (same as the installer's step for the
/// installing user: `InstallLayoutOrTip`, Arabic 101 not added).
#[tauri::command]
fn add_keyboard() -> Result<(), String> {
    #[cfg(windows)]
    {
        if t3a_hotkey::profile::enable() == 0 {
            return Ok(());
        }
        Err("Windows did not add the keyboard. Add it in Settings › Time & language › Language & region              (Arabic › Language options › Add a keyboard)."
            .into())
    }
    #[cfg(not(windows))]
    Err("Windows only".into())
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
            check_shortcut,
            save_settings,
            wipe_learning,
            export_learning,
            inspect_learning,
            import_learning,
            transfer_history,
            open_url,
            keyboard_status,
            add_keyboard,
            about
        ])
        .run(tauri::generate_context!())
        .expect("Type3arabi Settings failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (Owner, 2026-09-25): Ctrl+C was accepted as the global hotkey.
    #[test]
    fn windows_shortcuts_are_refused_with_the_reason() {
        let why = |c: &str, k: &str| check_shortcut(c.into(), k.into()).map(|[_, en]| en);
        assert!(why("Ctrl+C", "hotkey").unwrap().contains("copy"));
        assert!(why("Win+Shift+S", "hotkey").unwrap().contains("screenshot"));
        assert!(why("Ctrl+V", "chord").unwrap().contains("paste"));
        assert!(why("A", "chord").unwrap().contains("normal typing"));
        assert_eq!(why("Ctrl+Alt+A", "hotkey"), None);
        assert_eq!(why("Shift+Space", "chord"), None);
    }

    #[test]
    fn history_lines_round_trip_and_skip_garbage() {
        let dir = std::env::temp_dir().join(format!("t3a-hist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(HISTORY_FILE);
        let e = Transfer {
            t: 1,
            kind: "import".into(),
            records: 3,
            file: "a.t3learn".into(),
            mode: "merge".into(),
            settings: false,
        };
        let line = serde_json::to_string(&e).unwrap();
        std::fs::write(
            &p,
            format!(
                "{line}
not json
{line}
"
            ),
        )
        .unwrap();
        assert_eq!(read_history(&p), vec![e.clone(), e]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_known_links_open() {
        assert!(open_url("https://evil.example/".into()).is_err());
    }

    #[test]
    fn today_is_an_iso_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20") && &t[4..5] == "-" && &t[7..8] == "-");
    }

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
