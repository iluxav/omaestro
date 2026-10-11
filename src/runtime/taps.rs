//! Taps (`om.tap`): a modifier pressed and let go on its own, quickly, once
//! or `count` times in a row. Hyprland holds two binds on the key, both
//! passing it on to the window: the press runs `om trigger <id>/down`, noted
//! here, and the release runs `om trigger <id>`. A press that is let go
//! within `within` is a tap; it continues a run of taps when it began within
//! `within` of the last one's release, and the run that reaches `count`
//! fires the handler. A chord made while the key was down (SUPER+J) runs its
//! own bind, and Hyprland then skips the release bind: that press is no tap
//! and breaks the run.

use std::collections::HashMap;
use std::time::Duration;

use tokio::time::Instant;

/// What the press bind adds to the tap's id.
pub const DOWN: &str = "/down";

/// What a release of the key made of it.
#[derive(Debug, PartialEq)]
pub enum Release {
    /// The tap that completes the run: run the handler.
    Fire,
    /// A tap, this many in the run so far, short of `count`.
    Counted(u32),
    /// Down longer than `within`: no tap, and the run starts over.
    Held(Duration),
    /// A release whose press was not seen.
    Unseen,
}

#[derive(Default)]
struct Key {
    /// When the key went down, until it comes up.
    down: Option<Instant>,
    /// When the last tap was let go.
    last_tap: Option<Instant>,
    /// Taps in a row so far.
    run: u32,
}

#[derive(Default)]
pub struct Taps {
    keys: HashMap<String, Key>,
}

impl Taps {
    pub fn press(&mut self, id: &str) {
        let key = self.keys.entry(id.to_string()).or_default();
        // The last press was never let go: its release went to a chord.
        if key.down.is_some() {
            key.run = 0;
        }
        key.down = Some(Instant::now());
    }

    pub fn release(&mut self, id: &str, within: Duration, count: u32) -> Release {
        let Some(key) = self.keys.get_mut(id) else {
            return Release::Unseen;
        };
        let Some(down) = key.down.take() else {
            return Release::Unseen;
        };
        let now = Instant::now();
        let held = now - down;
        if held > within {
            key.run = 0;
            return Release::Held(held);
        }
        let follows = key
            .last_tap
            .is_some_and(|up| down.saturating_duration_since(up) <= within);
        key.run = if follows { key.run + 1 } else { 1 };
        key.last_tap = Some(now);
        if key.run >= count {
            key.run = 0;
            Release::Fire
        } else {
            Release::Counted(key.run)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITHIN: Duration = Duration::from_millis(300);
    const ID: &str = "tap:Super_L";

    async fn tap(taps: &mut Taps, held: u64, count: u32) -> Release {
        taps.press(ID);
        tokio::time::sleep(Duration::from_millis(held)).await;
        taps.release(ID, WITHIN, count)
    }

    async fn pause(ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn one_tap_fires_when_one_is_asked_for() {
        let mut taps = Taps::default();
        assert_eq!(tap(&mut taps, 100, 1).await, Release::Fire);
        assert_eq!(
            tap(&mut taps, 400, 1).await,
            Release::Held(Duration::from_millis(400))
        );
        assert_eq!(taps.release(ID, WITHIN, 1), Release::Unseen);
    }

    #[tokio::test(start_paused = true)]
    async fn a_double_tap_fires_on_its_second_tap_and_starts_over() {
        let mut taps = Taps::default();
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Counted(1));
        pause(150).await;
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Fire);
        // A third tap right after begins a new run.
        pause(100).await;
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Counted(1));
    }

    #[tokio::test(start_paused = true)]
    async fn taps_too_far_apart_are_two_singles() {
        let mut taps = Taps::default();
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Counted(1));
        pause(500).await;
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Counted(1));
    }

    #[tokio::test(start_paused = true)]
    async fn a_long_press_or_a_chord_breaks_the_run() {
        let mut taps = Taps::default();
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Counted(1));
        pause(50).await;
        assert_eq!(
            tap(&mut taps, 350, 2).await,
            Release::Held(Duration::from_millis(350))
        );
        pause(50).await;
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Counted(1));
        // SUPER+J: the press is seen, its release goes to the chord.
        pause(50).await;
        taps.press(ID);
        pause(100).await;
        assert_eq!(tap(&mut taps, 80, 2).await, Release::Counted(1));
    }
}
