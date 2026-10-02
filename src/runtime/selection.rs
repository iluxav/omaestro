//! Where the primary selection came from, so `om.selection()` does not hand
//! a rule text the user is no longer looking at. `wl-paste --primary` keeps
//! returning the last selection after the user clicked away, after a rule
//! replaced it, or while another window has focus; a rule that rewrites it
//! then pastes old text somewhere else.
//!
//! The runtime watches the selection (`wl-paste --primary --watch`) and
//! notes the focused window at every change; `om.paste` and `om.type` mark
//! it replaced. Without the watch (no data-control protocol) nothing is
//! known and the selection is returned as before.

use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Default)]
pub struct Selection(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    watching: bool,
    focused: Option<String>,
    origin: Origin,
}

#[derive(Default, Clone, PartialEq, Debug)]
enum Origin {
    /// No change seen since the watch started: made before the daemon was.
    #[default]
    Unknown,
    /// Made while this window (its address) had focus.
    Made(Option<String>),
    /// A rule pasted or typed over it, and nothing was selected since.
    Replaced,
}

impl Selection {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_watching(&self, watching: bool) {
        let mut state = self.state();
        state.watching = watching;
        state.origin = Origin::Unknown;
    }

    pub fn focus(&self, address: Option<&str>) {
        self.state().focused = address.map(str::to_string);
    }

    /// The selection changed: the focused window made it.
    pub fn changed(&self) {
        let mut state = self.state();
        state.origin = Origin::Made(state.focused.clone());
    }

    /// A rule's text went in where the selection was.
    pub fn replaced(&self) {
        let mut state = self.state();
        if state.watching {
            state.origin = Origin::Replaced;
        }
    }

    /// Why the current selection should not be used, if it should not.
    pub fn stale(&self) -> Option<&'static str> {
        let state = self.state();
        if !state.watching {
            return None;
        }
        match &state.origin {
            Origin::Unknown => None,
            Origin::Made(window) if *window == state.focused => None,
            Origin::Made(_) => Some("it was made in another window"),
            Origin::Replaced => Some("a rule already replaced it"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_is_good_in_its_window_until_replaced() {
        let s = Selection::default();
        s.set_watching(true);
        assert_eq!(s.stale(), None);
        s.focus(Some("0x1"));
        s.changed();
        assert_eq!(s.stale(), None);
        s.focus(Some("0x2"));
        assert_eq!(s.stale(), Some("it was made in another window"));
        s.focus(Some("0x1"));
        assert_eq!(s.stale(), None);
        s.replaced();
        assert_eq!(s.stale(), Some("a rule already replaced it"));
        s.changed();
        assert_eq!(s.stale(), None);
    }

    #[test]
    fn without_the_watch_nothing_is_stale() {
        let s = Selection::default();
        s.focus(Some("0x1"));
        s.changed();
        s.focus(Some("0x2"));
        s.replaced();
        assert_eq!(s.stale(), None);
    }
}
