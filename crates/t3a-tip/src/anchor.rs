//! Where the popup is anchored (docs/02 §9). Portable: the Windows side asks the app
//! (`ITfContextView::GetTextExt`) and checks that the answer lies on a monitor; this decides what to use.

/// Screen rectangle (left, top, right, bottom).
pub type Rect = (i32, i32, i32, i32);

/// What the popup is placed next to after a layout query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// The composition's rectangle as the app reported it.
    App(Rect),
    /// Stay where the popup is: the app has no layout for the changed text yet (Chrome answers
    /// `TS_E_NOLAYOUT` until the page reports its character bounds) or reported nothing usable.
    /// The layout-change notification that follows brings the real position.
    Keep(Rect),
    /// No position from the app yet for this field: the caret from `GetGUIThreadInfo`, else the pointer.
    Fallback,
}

/// Not the empty rectangle at the screen origin that some hosts return for "no answer".
pub fn plausible(r: Rect) -> bool {
    let (l, t, rt, b) = r;
    b > t || rt > l || l != 0 || t != 0
}

/// `app`: the app's answer if the query succeeded, was plausible and lies on a monitor.
/// `known`: the last position the app reported for the focused field.
///
/// Never the caret or the pointer once the app has given a position: in Chrome (Google Docs) every
/// edit is followed by a "no layout yet" answer, and the pointer fallback moved the diacritics editor
/// away from under the mouse just before the click landed (the click then hit nothing).
pub fn choose(app: Option<Rect>, known: Option<Rect>) -> Anchor {
    match (app, known) {
        (Some(r), _) => Anchor::App(r),
        (None, Some(k)) => Anchor::Keep(k),
        (None, None) => Anchor::Fallback,
    }
}

/// A preview from the diacritics editor only changes marks on letters already on screen, so the word
/// does not move: keep the popup still instead of asking the app again (a restarted composition, or
/// no known position, still asks).
pub fn requery_on_preview(editor_open: bool, known: bool, restarted: bool) -> bool {
    !(editor_open && known && !restarted)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORD: Rect = (400, 300, 480, 324);

    #[test]
    fn app_answer_wins() {
        assert_eq!(choose(Some(WORD), None), Anchor::App(WORD));
        let moved = (400, 360, 480, 384);
        assert_eq!(choose(Some(moved), Some(WORD)), Anchor::App(moved));
    }

    #[test]
    fn no_layout_keeps_the_known_position() {
        // Regression (Owner report 2026-10-15, Google Docs): clicking a mark in the diacritics editor
        // moved the editor to the pointer instead of applying the mark.
        assert_eq!(choose(None, Some(WORD)), Anchor::Keep(WORD));
    }

    #[test]
    fn fallback_only_without_any_position() {
        assert_eq!(choose(None, None), Anchor::Fallback);
    }

    #[test]
    fn implausible_rects() {
        assert!(plausible(WORD));
        assert!(plausible((10, 10, 10, 30))); // zero-width caret-like rect
        assert!(plausible((0, 0, 1, 20))); // a word at the screen's corner
        assert!(!plausible((0, 0, 0, 0)));
    }

    #[test]
    fn editor_previews_keep_the_popup_still() {
        assert!(!requery_on_preview(true, true, false));
        assert!(requery_on_preview(true, true, true)); // composition restarted: ask again
        assert!(requery_on_preview(true, false, false)); // nothing known yet
        assert!(requery_on_preview(false, true, false)); // the list: the word grows as you type
    }
}
