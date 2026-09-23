//! `config.toml` (docs/13-config-schema.md): a minimal TOML-subset reader (no serde, R12) and the typed config.
//! Supported syntax: comments, `[section]`, `key = "string" | integer | true/false | ["a", "b"]` (one line).

use crate::display::{AllahForm, TanweenStyle};
use crate::tashkeel::HarakatMode;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
    List(Vec<String>),
}

/// Parse into `(section.key, value)` pairs. Malformed lines become warnings.
pub fn parse_toml(text: &str) -> (Vec<(String, Value)>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut section = String::new();
    for (i, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            warnings.push(format!("line {}: expected key = value", i + 1));
            continue;
        };
        let key = if section.is_empty() {
            k.trim().to_string()
        } else {
            format!("{}.{}", section, k.trim())
        };
        match parse_value(v.trim()) {
            Some(val) => out.push((key, val)),
            None => warnings.push(format!("line {}: cannot parse value for {key}", i + 1)),
        }
    }
    (out, warnings)
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_str(v: &str) -> Option<String> {
    let v = v.trim();
    (v.len() >= 2 && v.starts_with('"') && v.ends_with('"')).then(|| v[1..v.len() - 1].to_string())
}

fn parse_value(v: &str) -> Option<Value> {
    if let Some(s) = parse_str(v) {
        return Some(Value::Str(s));
    }
    match v {
        "true" => return Some(Value::Bool(true)),
        "false" => return Some(Value::Bool(false)),
        _ => {}
    }
    if let Ok(n) = v.parse::<i64>() {
        return Some(Value::Int(n));
    }
    if v.starts_with('[') && v.ends_with(']') {
        let inner = v[1..v.len() - 1].trim();
        if inner.is_empty() {
            return Some(Value::List(Vec::new()));
        }
        return inner
            .split(',')
            .map(parse_str)
            .collect::<Option<Vec<_>>>()
            .map(Value::List);
    }
    None
}

/// Typed configuration with the defaults of `config/config.default.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub schema: i64,
    pub mode_toggle: String,
    pub mode_scope: String,
    pub global_hotkey_enabled: bool,
    pub global_hotkey: String,
    pub latin_layout: String,
    pub inline_preview: String,
    pub candidates_per_page: i64,
    pub predictive_completions: bool,
    pub article_joining: bool,
    pub reedit_backspace: bool,
    pub commit_on_focus_loss: String,
    pub latin_in_url_email: bool,
    pub dialect_profile: String,
    pub seed_from_region: bool,
    pub allah_form: String,
    pub adverbial_tanween: bool,
    pub tanween_style: String,
    pub hamza: String,
    pub vowel_harakat: String,
    pub arabic_punctuation: bool,
    pub numerals: String,
    pub learning_enabled: bool,
    pub sticky_last_choice: bool,
    pub use_surrounding_text: bool,
    pub theme: String,
    pub font_family: String,
    pub font_size: i64,
    pub footer_hints: String,
    pub show_dialect_badge: bool,
    pub excluded_apps: Vec<String>,
    pub rtl_assist: bool,
    pub rtl_assist_classes: Vec<String>,
    /// `[keys]` — in-composition shortcuts (docs/13). Chord strings like `"Shift+Space"`, or `"none"`.
    pub key_commit_latin: String,
    pub key_open_tashkeel: String,
    pub key_commit_harakat: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: 1,
            mode_toggle: "Ctrl+Space".into(),
            mode_scope: "global".into(),
            global_hotkey_enabled: true,
            global_hotkey: "Ctrl+Alt+A".into(),
            latin_layout: "auto".into(),
            inline_preview: "arabic".into(),
            candidates_per_page: 7,
            predictive_completions: true,
            article_joining: true,
            reedit_backspace: true,
            commit_on_focus_loss: "preview".into(),
            latin_in_url_email: true,
            dialect_profile: "auto".into(),
            seed_from_region: true,
            allah_form: "shadda".into(),
            adverbial_tanween: true,
            tanween_style: "on_alif".into(),
            hamza: "standard".into(),
            vowel_harakat: "light".into(),
            arabic_punctuation: true,
            numerals: "western".into(),
            learning_enabled: true,
            sticky_last_choice: true,
            use_surrounding_text: true,
            theme: "system".into(),
            font_family: "Segoe UI".into(),
            font_size: 18,
            footer_hints: "auto".into(),
            show_dialect_badge: true,
            excluded_apps: Vec::new(),
            rtl_assist: false,
            rtl_assist_classes: vec!["Edit".into(), "RichEdit20W".into(), "RICHEDIT50W".into()],
            key_commit_latin: "Shift+Space".into(),
            key_open_tashkeel: "Tab".into(),
            key_commit_harakat: "Ctrl+Enter".into(),
        }
    }
}

impl Config {
    /// Parse; unknown keys ignored, invalid values keep defaults and produce warnings.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let (pairs, mut warnings) = parse_toml(text);
        let mut c = Config::default();
        for (k, v) in pairs {
            let ok = match (k.as_str(), v) {
                ("schema", Value::Int(n)) => set(&mut c.schema, n),
                ("general.mode_toggle", Value::Str(s)) => set(&mut c.mode_toggle, s),
                ("general.mode_scope", Value::Str(s)) => set(&mut c.mode_scope, s),
                ("general.global_hotkey_enabled", Value::Bool(b)) => {
                    set(&mut c.global_hotkey_enabled, b)
                }
                ("general.global_hotkey", Value::Str(s)) => set(&mut c.global_hotkey, s),
                ("typing.latin_layout", Value::Str(s)) => set(&mut c.latin_layout, s),
                ("typing.inline_preview", Value::Str(s)) => set(&mut c.inline_preview, s),
                ("typing.candidates_per_page", Value::Int(n)) if (5..=9).contains(&n) => {
                    set(&mut c.candidates_per_page, n)
                }
                ("typing.predictive_completions", Value::Bool(b)) => {
                    set(&mut c.predictive_completions, b)
                }
                ("typing.article_joining", Value::Bool(b)) => set(&mut c.article_joining, b),
                ("typing.reedit_backspace", Value::Bool(b)) => set(&mut c.reedit_backspace, b),
                ("typing.commit_on_focus_loss", Value::Str(s)) => {
                    set(&mut c.commit_on_focus_loss, s)
                }
                ("typing.latin_in_url_email", Value::Bool(b)) => set(&mut c.latin_in_url_email, b),
                ("dialect.profile", Value::Str(s)) => set(&mut c.dialect_profile, s),
                ("dialect.seed_from_region", Value::Bool(b)) => set(&mut c.seed_from_region, b),
                ("style.allah_form", Value::Str(s)) if AllahForm::parse(&s).is_some() => {
                    set(&mut c.allah_form, s)
                }
                ("style.adverbial_tanween", Value::Bool(b)) => set(&mut c.adverbial_tanween, b),
                ("style.tanween_style", Value::Str(s)) if TanweenStyle::parse(&s).is_some() => {
                    set(&mut c.tanween_style, s)
                }
                ("style.hamza", Value::Str(s)) => set(&mut c.hamza, s),
                ("style.vowel_harakat", Value::Str(s)) => set(&mut c.vowel_harakat, s),
                ("style.arabic_punctuation", Value::Bool(b)) => set(&mut c.arabic_punctuation, b),
                ("style.numerals", Value::Str(s)) => set(&mut c.numerals, s),
                ("learning.enabled", Value::Bool(b)) => set(&mut c.learning_enabled, b),
                ("learning.sticky_last_choice", Value::Bool(b)) => {
                    set(&mut c.sticky_last_choice, b)
                }
                ("privacy.use_surrounding_text", Value::Bool(b)) => {
                    set(&mut c.use_surrounding_text, b)
                }
                ("appearance.theme", Value::Str(s)) => set(&mut c.theme, s),
                ("appearance.font_family", Value::Str(s)) => set(&mut c.font_family, s),
                ("appearance.font_size", Value::Int(n)) if (14..=28).contains(&n) => {
                    set(&mut c.font_size, n)
                }
                ("appearance.footer_hints", Value::Str(s)) => set(&mut c.footer_hints, s),
                ("appearance.show_dialect_badge", Value::Bool(b)) => {
                    set(&mut c.show_dialect_badge, b)
                }
                ("apps.excluded", Value::List(l)) => set(&mut c.excluded_apps, l),
                ("apps.rtl_assist", Value::Bool(b)) => set(&mut c.rtl_assist, b),
                ("apps.rtl_assist_classes", Value::List(l)) => set(&mut c.rtl_assist_classes, l),
                ("keys.commit_latin", Value::Str(s)) if is_chord(&s) => {
                    set(&mut c.key_commit_latin, s)
                }
                ("keys.open_tashkeel", Value::Str(s)) if is_chord(&s) => {
                    set(&mut c.key_open_tashkeel, s)
                }
                ("keys.commit_harakat", Value::Str(s)) if is_chord(&s) => {
                    set(&mut c.key_commit_harakat, s)
                }
                (other, _) if KNOWN.contains(&other) => false,
                _ => true, // unknown keys are ignored silently (forward compatibility)
            };
            if !ok {
                warnings.push(format!("invalid value for {k}; using default"));
            }
        }
        (c, warnings)
    }

    pub fn allah_form(&self) -> AllahForm {
        AllahForm::parse(&self.allah_form).unwrap_or_default()
    }
    pub fn tanween(&self) -> TanweenStyle {
        if self.adverbial_tanween {
            TanweenStyle::parse(&self.tanween_style).unwrap_or_default()
        } else {
            TanweenStyle::Off
        }
    }
    pub fn harakat_mode(&self) -> HarakatMode {
        if self.vowel_harakat == "full" {
            HarakatMode::Full
        } else {
            HarakatMode::Light
        }
    }

    /// Serialize to the `config.toml` format (written by the Settings app; docs/13). Every key is
    /// written, so the file is self-describing. Quotes inside strings are dropped (the reader has no
    /// escapes). `Config::parse(&c.to_toml()).0 == c` for every valid config.
    pub fn to_toml(&self) -> String {
        fn q(s: &str) -> String {
            format!("\"{}\"", s.replace('"', ""))
        }
        fn list(v: &[String]) -> String {
            format!(
                "[{}]",
                v.iter().map(|s| q(s)).collect::<Vec<_>>().join(", ")
            )
        }
        let c = self;
        format!(
            "# Type3arabi settings (docs/13-config-schema.md). Written by Type3arabi Settings.
             schema = {}

             [general]
mode_toggle = {}
mode_scope = {}
global_hotkey_enabled = {}
global_hotkey = {}

             [typing]
latin_layout = {}
inline_preview = {}
candidates_per_page = {}
             predictive_completions = {}
article_joining = {}
reedit_backspace = {}
             commit_on_focus_loss = {}
latin_in_url_email = {}

             [dialect]
profile = {}
seed_from_region = {}

             [style]
allah_form = {}
adverbial_tanween = {}
tanween_style = {}
hamza = {}
             vowel_harakat = {}
arabic_punctuation = {}
numerals = {}

             [learning]
enabled = {}
sticky_last_choice = {}

             [privacy]
use_surrounding_text = {}

             [appearance]
theme = {}
font_family = {}
font_size = {}
footer_hints = {}
             show_dialect_badge = {}

             [apps]
excluded = {}
rtl_assist = {}
rtl_assist_classes = {}

             [keys]
commit_latin = {}
open_tashkeel = {}
commit_harakat = {}
",
            c.schema,
            q(&c.mode_toggle),
            q(&c.mode_scope),
            c.global_hotkey_enabled,
            q(&c.global_hotkey),
            q(&c.latin_layout),
            q(&c.inline_preview),
            c.candidates_per_page,
            c.predictive_completions,
            c.article_joining,
            c.reedit_backspace,
            q(&c.commit_on_focus_loss),
            c.latin_in_url_email,
            q(&c.dialect_profile),
            c.seed_from_region,
            q(&c.allah_form),
            c.adverbial_tanween,
            q(&c.tanween_style),
            q(&c.hamza),
            q(&c.vowel_harakat),
            c.arabic_punctuation,
            q(&c.numerals),
            c.learning_enabled,
            c.sticky_last_choice,
            c.use_surrounding_text,
            q(&c.theme),
            q(&c.font_family),
            c.font_size,
            q(&c.footer_hints),
            c.show_dialect_badge,
            list(&c.excluded_apps),
            c.rtl_assist,
            list(&c.rtl_assist_classes),
            q(&c.key_commit_latin),
            q(&c.key_open_tashkeel),
            q(&c.key_commit_harakat),
        )
    }

    pub fn to_engine_settings(&self) -> crate::session::EngineSettings {
        crate::session::EngineSettings::from(self)
    }
}

/// Syntax check for `[keys]` chords: `none`, or `Mod+…+Key` with modifiers Ctrl/Alt/Shift and a key
/// name (Space, Enter, Tab, Esc, Backspace, a–z, 0–9). The TIP's key router interprets them.
pub fn is_chord(s: &str) -> bool {
    if s.eq_ignore_ascii_case("none") {
        return true;
    }
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let Some((key, mods)) = parts.split_last() else {
        return false;
    };
    let key_ok = matches!(
        key.to_ascii_lowercase().as_str(),
        "space" | "enter" | "tab" | "esc" | "escape" | "backspace"
    ) || (key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()));
    key_ok
        && mods
            .iter()
            .all(|m| matches!(m.to_ascii_lowercase().as_str(), "ctrl" | "alt" | "shift"))
}

fn set<T>(slot: &mut T, v: T) -> bool {
    *slot = v;
    true
}

const KNOWN: [&str; 36] = [
    "schema",
    "general.mode_toggle",
    "general.mode_scope",
    "general.global_hotkey_enabled",
    "general.global_hotkey",
    "typing.latin_layout",
    "typing.inline_preview",
    "typing.candidates_per_page",
    "typing.predictive_completions",
    "typing.article_joining",
    "typing.reedit_backspace",
    "typing.commit_on_focus_loss",
    "typing.latin_in_url_email",
    "dialect.profile",
    "dialect.seed_from_region",
    "style.allah_form",
    "style.adverbial_tanween",
    "style.tanween_style",
    "style.hamza",
    "style.vowel_harakat",
    "style.arabic_punctuation",
    "style.numerals",
    "learning.enabled",
    "learning.sticky_last_choice",
    "privacy.use_surrounding_text",
    "appearance.theme",
    "appearance.font_family",
    "appearance.font_size",
    "appearance.footer_hints",
    "appearance.show_dialect_badge",
    "apps.excluded",
    "apps.rtl_assist",
    "apps.rtl_assist_classes",
    "keys.commit_latin",
    "keys.open_tashkeel",
    "keys.commit_harakat",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_equals_builtin_defaults() {
        let (c, w) = Config::parse(include_str!("../../../config/config.default.toml"));
        assert!(w.is_empty(), "warnings: {w:?}");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn to_toml_round_trips() {
        let d = Config::default();
        let (back, w) = Config::parse(&d.to_toml());
        assert!(w.is_empty(), "warnings: {w:?}");
        assert_eq!(back, d);
        let c = Config {
            key_commit_latin: "Ctrl+Shift+L".into(),
            global_hotkey: "Ctrl+Alt+Q".into(),
            learning_enabled: false,
            excluded_apps: vec!["game.exe".into(), "vim.exe".into()],
            candidates_per_page: 9,
            ..Default::default()
        };
        let (back, w) = Config::parse(&c.to_toml());
        assert!(w.is_empty(), "warnings: {w:?}");
        assert_eq!(back, c);
    }

    #[test]
    fn key_chords_are_validated() {
        let (c, w) = Config::parse(
            "[keys]\ncommit_latin = \"Ctrl+Shift+L\"\nopen_tashkeel = \"Hyper+Q\"\ncommit_harakat = \"none\"\n",
        );
        assert_eq!(c.key_commit_latin, "Ctrl+Shift+L");
        assert_eq!(c.key_open_tashkeel, "Tab"); // invalid -> default
        assert_eq!(c.key_commit_harakat, "none");
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn invalid_values_fall_back() {
        let (c, w) = Config::parse(
            "[style]\nallah_form = \"fancy\"\n[typing]\ncandidates_per_page = 42\nunknown = 1\n",
        );
        assert_eq!(c.allah_form, "shadda");
        assert_eq!(c.candidates_per_page, 7);
        assert_eq!(w.len(), 2);
    }
}
