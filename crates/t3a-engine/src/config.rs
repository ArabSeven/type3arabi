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

/// Check for `[keys]` chords: `none`, or `Mod+…+Key` with modifiers Ctrl/Alt/Shift and a key
/// name (Space, Enter, Tab, Backspace, a–z, 0–9). The TIP's key router interprets them.
/// Rejected because they would break normal typing: a letter or digit without Ctrl or Alt (it
/// types that character), Space/Enter/Backspace without a modifier (commit, newline, delete), and
/// Esc in any form (cancel; Ctrl+Esc is the Start menu).
pub fn is_chord(s: &str) -> bool {
    if s.eq_ignore_ascii_case("none") {
        return true;
    }
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let Some((key, mods)) = parts.split_last() else {
        return false;
    };
    let mods: Vec<String> = mods.iter().map(|m| m.to_ascii_lowercase()).collect();
    if !mods
        .iter()
        .all(|m| matches!(m.as_str(), "ctrl" | "alt" | "shift"))
    {
        return false;
    }
    let has = |m: &str| mods.iter().any(|x| x == m);
    match key.to_ascii_lowercase().as_str() {
        "tab" => true,
        "space" | "enter" | "backspace" => !mods.is_empty(),
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => {
            has("ctrl") || has("alt")
        }
        _ => false,
    }
}

/// Why a shortcut cannot be used: Windows, or nearly every app, already uses it (Owner, 2026-09-25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reserved {
    /// English reason, completing "This shortcut is used …".
    pub en: &'static str,
    /// Arabic reason, completing «هذا الاختصار يستخدمه …».
    pub ar: &'static str,
}

/// Shortcuts Settings refuses for Type3arabi, with the reason (docs/05 §7): the global hotkey is
/// registered system-wide, so a copy of Ctrl+C would break copying everywhere; the typing shortcuts
/// act while a word is being typed and would hide the app's own shortcut. `chord` is written like
/// `Ctrl+Shift+T` (modifiers Ctrl, Alt, Shift, Win in any order and case).
pub fn reserved_shortcut(chord: &str) -> Option<Reserved> {
    let parts: Vec<String> = chord
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    let (key, mods) = parts.split_last()?;
    let has = |m: &str| {
        mods.iter()
            .any(|x| x == m || (m == "ctrl" && x == "control"))
    };
    let (ctrl, alt, shift, win) = (
        has("ctrl"),
        has("alt"),
        has("shift"),
        has("win") || has("windows"),
    );
    let r = |en, ar| Some(Reserved { en, ar });
    if win {
        return match key.as_str() {
            "s" if shift => r(
                "by Windows to take a screenshot (Win+Shift+S)",
                "ويندوز لالتقاط صورة للشاشة",
            ),
            "space" => r(
                "by Windows to switch keyboards (Win+Space)",
                "ويندوز للتبديل بين لوحات المفاتيح",
            ),
            "v" => r(
                "by Windows for the clipboard history (Win+V)",
                "ويندوز لسجلّ الحافظة",
            ),
            "l" => r("by Windows to lock the PC (Win+L)", "ويندوز لقفل الجهاز"),
            "d" => r(
                "by Windows to show the desktop (Win+D)",
                "ويندوز لإظهار سطح المكتب",
            ),
            "e" => r(
                "by Windows to open File Explorer (Win+E)",
                "ويندوز لفتح مستكشف الملفات",
            ),
            "tab" => r("by Windows for Task View (Win+Tab)", "ويندوز لعرض المهام"),
            _ => r(
                "by Windows: Win shortcuts belong to Windows",
                "ويندوز: اختصارات مفتاح Win محجوزة له",
            ),
        };
    }
    match (ctrl, alt, shift, key.as_str()) {
        (_, true, _, "tab") => r(
            "by Windows to switch between windows (Alt+Tab)",
            "ويندوز للتنقّل بين النوافذ",
        ),
        (false, true, false, "f4") => r(
            "by Windows to close the app (Alt+F4)",
            "ويندوز لإغلاق البرنامج",
        ),
        (false, true, false, "space") => r(
            "by Windows for the window menu (Alt+Space)",
            "ويندوز لقائمة النافذة",
        ),
        (false, true, false, "enter") => r(
            "by many apps for full screen or properties (Alt+Enter)",
            "برامج كثيرة لملء الشاشة أو الخصائص",
        ),
        (true, false, false, "space") => r(
            "for Type3arabi's own Arabic/Latin switch and by other input methods (Ctrl+Space)",
            "«اكتب عربي» نفسه وطرق إدخال أخرى للتبديل بين العربي واللاتيني",
        ),
        (true, false, true, "z") => r("by apps to redo (Ctrl+Shift+Z)", "البرامج للإعادة"),
        (true, false, true, "t") => r(
            "by browsers to reopen a closed tab (Ctrl+Shift+T)",
            "المتصفحات لإعادة فتح تبويب مغلق",
        ),
        (true, false, true, "n") => r(
            "by browsers and File Explorer for a new private window or folder (Ctrl+Shift+N)",
            "المتصفحات ومستكشف الملفات لنافذة خاصة أو مجلد جديد",
        ),
        (true, false, true, "tab") => r(
            "by apps to go to the previous tab (Ctrl+Shift+Tab)",
            "البرامج للانتقال إلى التبويب السابق",
        ),
        (true, false, true, "s") => r("by apps to save as (Ctrl+Shift+S)", "البرامج للحفظ باسم"),
        (true, false, false, k) => match k {
            "c" => r("by Windows to copy (Ctrl+C)", "ويندوز للنسخ"),
            "v" => r("by Windows to paste (Ctrl+V)", "ويندوز للّصق"),
            "x" => r("by Windows to cut (Ctrl+X)", "ويندوز للقصّ"),
            "z" => r("by Windows to undo (Ctrl+Z)", "ويندوز للتراجع"),
            "y" => r("by apps to redo (Ctrl+Y)", "البرامج للإعادة"),
            "a" => r("by Windows to select all (Ctrl+A)", "ويندوز لتحديد الكل"),
            "s" => r("by apps to save (Ctrl+S)", "البرامج للحفظ"),
            "p" => r("by apps to print (Ctrl+P)", "البرامج للطباعة"),
            "f" => r("by apps to find (Ctrl+F)", "البرامج للبحث"),
            "n" => r(
                "by apps for a new document or window (Ctrl+N)",
                "البرامج لمستند أو نافذة جديدة",
            ),
            "o" => r("by apps to open a file (Ctrl+O)", "البرامج لفتح ملف"),
            "w" => r(
                "by apps to close a tab or window (Ctrl+W)",
                "البرامج لإغلاق تبويب أو نافذة",
            ),
            "t" => r(
                "by browsers for a new tab (Ctrl+T)",
                "المتصفحات لتبويب جديد",
            ),
            "r" => r("by browsers to reload (Ctrl+R)", "المتصفحات لإعادة التحميل"),
            "b" => r("by editors for bold (Ctrl+B)", "المحرّرات للخط العريض"),
            "i" => r("by editors for italic (Ctrl+I)", "المحرّرات للخط المائل"),
            "u" => r(
                "by editors for underline (Ctrl+U)",
                "المحرّرات لوضع خط تحت النص",
            ),
            "k" => r("by apps to insert a link (Ctrl+K)", "البرامج لإدراج رابط"),
            "l" => r(
                "by browsers for the address bar (Ctrl+L)",
                "المتصفحات لشريط العنوان",
            ),
            "h" => r(
                "by apps for history or replace (Ctrl+H)",
                "البرامج للسجلّ أو الاستبدال",
            ),
            "d" => r(
                "by browsers to bookmark a page (Ctrl+D)",
                "المتصفحات لإضافة إشارة مرجعية",
            ),
            "e" => r(
                "by apps to search or center text (Ctrl+E)",
                "البرامج للبحث أو توسيط النص",
            ),
            "g" => r("by apps to find next (Ctrl+G)", "البرامج للبحث عن التالي"),
            "j" => r("by browsers for downloads (Ctrl+J)", "المتصفحات للتنزيلات"),
            "q" => r("by apps to quit (Ctrl+Q)", "البرامج للخروج"),
            "tab" => r(
                "by apps to go to the next tab (Ctrl+Tab)",
                "البرامج للانتقال إلى التبويب التالي",
            ),
            "backspace" => r(
                "by Windows to delete the previous word (Ctrl+Backspace)",
                "ويندوز لحذف الكلمة السابقة",
            ),
            _ => None,
        },
        _ => None,
    }
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
    #[test]
    fn chords_that_would_break_typing_are_rejected() {
        for ok in [
            "none",
            "Tab",
            "Shift+Space",
            "Ctrl+Enter",
            "Ctrl+Shift+K",
            "Alt+3",
            "Shift+Tab",
        ] {
            assert!(is_chord(ok), "{ok}");
        }
        for bad in [
            "A",
            "Shift+A",
            "7",
            "Space",
            "Enter",
            "Backspace",
            "Esc",
            "Ctrl+Esc",
            "Win+A",
            "Ctrl+",
            "",
            "Ctrl+F5",
        ] {
            assert!(!is_chord(bad), "{bad}");
        }
    }

    /// Regression (Owner, 2026-09-25): Settings accepted Ctrl+C as a shortcut.
    #[test]
    fn windows_shortcuts_are_reserved_with_a_reason() {
        for (chord, why) in [
            ("Ctrl+C", "copy"),
            ("ctrl+v", "paste"),
            ("Control+X", "cut"),
            ("Ctrl+Z", "undo"),
            ("Ctrl+A", "select all"),
            ("Win+Shift+S", "screenshot"),
            ("Shift+Win+S", "screenshot"),
            ("Win+Space", "switch keyboards"),
            ("Win+K", "Win shortcuts"),
            ("Alt+Tab", "switch between windows"),
            ("Alt+F4", "close"),
            ("Ctrl+Shift+T", "reopen"),
            ("Ctrl+Backspace", "delete the previous word"),
        ] {
            let r = reserved_shortcut(chord).unwrap_or_else(|| panic!("{chord} not reserved"));
            assert!(r.en.contains(why), "{chord}: {}", r.en);
            assert!(!r.ar.is_empty());
        }
        // Type3arabi's own defaults and ordinary choices stay available.
        for ok in [
            "Ctrl+Alt+A",
            "Shift+Space",
            "Tab",
            "Ctrl+Enter",
            "Ctrl+Shift+K",
            "Alt+3",
            "none",
        ] {
            assert_eq!(reserved_shortcut(ok), None, "{ok}");
        }
    }

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
