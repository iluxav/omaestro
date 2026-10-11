//! Taps (`om.tap`): a modifier pressed and let go on its own, quickly.
//! Hyprland holds two binds on the key, both passing it on to the window:
//! the press runs `om trigger <id>/down`, noted here, and the release runs
//! `om trigger <id>`, which fires the handler if the key was down for at
//! most the tap's `within`. A chord made while the key was down (SUPER+J)
//! runs its own bind, and Hyprland then skips the release bind: that press
//! never fires.

use std::collections::HashMap;
use std::time::Duration;

use tokio::time::Instant;

/// What the press bind adds to the tap's id.
pub const DOWN: &str = "/down";

#[derive(Default)]
pub struct Taps {
    /// When each tap's key went down, by trigger id.
    pressed: HashMap<String, Instant>,
}

impl Taps {
    pub fn press(&mut self, id: &str) {
        self.pressed.insert(id.to_string(), Instant::now());
    }

    /// How long the key was down, if its press was seen.
    pub fn release(&mut self, id: &str) -> Option<Duration> {
        self.pressed.remove(id).map(|at| at.elapsed())
    }
}
