//! The real Type3arabi engine for the website's playground (wasm32-unknown-unknown).
//!
//! A deliberately small C ABI — no bindings generator, no extra dependencies. JavaScript copies the
//! model bytes in with `t3a_alloc` + `t3a_init`, calls `t3a_push` / `t3a_commit` / … and reads each
//! result as UTF-8 JSON from `t3a_out()` (length = the call's return value). One session per page;
//! learning is off (nothing is stored in the browser).

use std::cell::RefCell;
use t3a_engine::dialect::argmax;
use t3a_engine::{CommitHow, Engine, EngineSettings, InputChar, NoUser, Session};

struct State {
    session: Session<'static>,
    out: Vec<u8>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// JSON string literal (quotes, backslashes and control characters escaped).
fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The current candidate list: `{"raw":…, "d":"LEV", "items":[{"t":…, "k":"Word"}, …]}`.
fn list_json(s: &Session) -> String {
    let list = s.candidates();
    let mut out = String::from("{\"raw\":");
    json_str(&mut out, &list.raw);
    out.push_str(",\"d\":");
    json_str(&mut out, argmax(&s.dialect()).code());
    out.push_str(",\"items\":[");
    for (i, c) in list.items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"t\":");
        json_str(&mut out, &c.text);
        out.push_str(",\"k\":");
        json_str(&mut out, &format!("{:?}", c.kind));
        out.push('}');
    }
    out.push_str("]}");
    out
}

fn with_state(f: impl FnOnce(&mut State) -> String) -> usize {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let Some(state) = st.as_mut() else {
            return 0;
        };
        let json = f(state);
        state.out = json.into_bytes();
        state.out.len()
    })
}

/// Allocate `len` bytes for JavaScript to fill (the model, or a Latin word for `t3a_restore`).
#[no_mangle]
pub extern "C" fn t3a_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Load the model from a buffer returned by `t3a_alloc(len)` and filled by the caller (ownership
/// moves here). Returns 1 on success, 0 if the bytes are not a valid model.
///
/// # Safety
/// `ptr` must come from `t3a_alloc(len)` and hold `len` initialized bytes; call at most once per buffer.
#[no_mangle]
pub unsafe extern "C" fn t3a_init(ptr: *mut u8, len: usize) -> i32 {
    // SAFETY: the caller guarantees `ptr` is a `t3a_alloc(len)` allocation with `len` bytes written.
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let Ok(engine) = Engine::from_bytes(bytes) else {
        return 0;
    };
    let engine: &'static Engine = Box::leak(Box::new(engine));
    let session = Session::new(engine, EngineSettings::default());
    STATE.with(|st| {
        *st.borrow_mut() = Some(State {
            session,
            out: Vec::new(),
        })
    });
    1
}

/// Result buffer of the last call (UTF-8 JSON).
#[no_mangle]
pub extern "C" fn t3a_out() -> *const u8 {
    STATE.with(|st| {
        st.borrow()
            .as_ref()
            .map_or(std::ptr::null(), |s| s.out.as_ptr())
    })
}

/// Type one character (a Unicode scalar); returns the length of the new list JSON.
#[no_mangle]
pub extern "C" fn t3a_push(ch: u32) -> usize {
    let Some(c) = char::from_u32(ch) else {
        return 0;
    };
    with_state(|s| {
        s.session.push(InputChar::new(c), &NoUser);
        list_json(&s.session)
    })
}

/// Remove the last typed character (Backspace while composing).
#[no_mangle]
pub extern "C" fn t3a_pop() -> usize {
    with_state(|s| {
        let raw = s.session.candidates().raw.clone();
        let mut chars: Vec<char> = raw.chars().collect();
        chars.pop();
        s.session.reset();
        for c in chars {
            s.session.push(InputChar::new(c), &NoUser);
        }
        list_json(&s.session)
    })
}

/// Commit candidate `index`. `how`: 0 Space, 1 Enter, 2 punctuation, 3 Ctrl+Enter (harakat from the
/// typed vowels). Returns `{"t": text, "space": bool}`; the dialect estimate adapts (auto mode).
#[no_mangle]
pub extern "C" fn t3a_commit(index: u32, how: u32) -> usize {
    let how = match how {
        0 => CommitHow::Space,
        1 => CommitHow::Enter,
        2 => CommitHow::Punctuation,
        _ => CommitHow::WithHarakat,
    };
    with_state(|s| {
        let c = s.session.commit(index as usize, how);
        let mut out = String::from("{\"t\":");
        json_str(&mut out, &c.text);
        out.push_str(&format!(
            ",\"space\":{}}}",
            c.trailing == t3a_engine::session::Trailing::Space
        ));
        out
    })
}

/// The tashkeel editor's quick picks for candidate `index`: `{"fromTyping":bool,"picks":[…]}`.
#[no_mangle]
pub extern "C" fn t3a_picks(index: u32) -> usize {
    with_state(|s| {
        let vh = s.session.vowel_harakat(index as usize);
        let picks = s.session.vocalizations(index as usize);
        let from_typing = vh.is_some() && picks.first() == vh.as_ref();
        let mut out = format!("{{\"fromTyping\":{from_typing},\"picks\":[");
        for (i, p) in picks.iter().take(8).enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_str(&mut out, p);
        }
        out.push_str("]}");
        out
    })
}

/// Reopen a committed word (Backspace right after a commit): `ptr/len` is its Latin text, written
/// into a `t3a_alloc(len)` buffer that this call frees. Returns the list JSON.
///
/// # Safety
/// `ptr` must come from `t3a_alloc(len)` with `len` initialized bytes.
#[no_mangle]
pub unsafe extern "C" fn t3a_restore(ptr: *mut u8, len: usize) -> usize {
    // SAFETY: the caller guarantees `ptr` is a `t3a_alloc(len)` allocation with `len` bytes written.
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let latin = String::from_utf8_lossy(&bytes).into_owned();
    with_state(|s| {
        s.session.restore(&latin, &NoUser);
        list_json(&s.session)
    })
}

/// Forget the word being typed (not the dialect estimate).
#[no_mangle]
pub extern "C" fn t3a_reset() {
    STATE.with(|st| {
        if let Some(s) = st.borrow_mut().as_mut() {
            s.session.reset();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out() -> String {
        STATE.with(|st| String::from_utf8(st.borrow().as_ref().unwrap().out.clone()).unwrap())
    }

    #[test]
    fn json_escapes_quotes_backslashes_and_controls() {
        let mut s = String::new();
        json_str(&mut s, "a\"b\\c\nd");
        assert_eq!(s, "\"a\\\"b\\\\c\\u000ad\"");
    }

    #[test]
    fn builtin_engine_session_round_trip() {
        // The builtin (seed-only) engine stands in for a model file here.
        let engine: &'static Engine = Box::leak(Box::new(Engine::builtin()));
        STATE.with(|st| {
            *st.borrow_mut() = Some(State {
                session: Session::new(engine, EngineSettings::default()),
                out: Vec::new(),
            })
        });
        for c in "salam".chars() {
            assert!(t3a_push(c as u32) > 0);
        }
        let list = out();
        assert!(
            list.contains("\"raw\":\"salam\"") && list.contains("\"k\":\"RawLatin\""),
            "{list}"
        );
        t3a_pop();
        assert!(out().contains("\"raw\":\"sala\""));
        assert!(t3a_commit(0, 0) > 0);
        assert!(out().starts_with("{\"t\":"));
    }
}
