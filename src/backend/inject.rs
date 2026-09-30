//! Key injection. Everything that can be pressed on the real keymap is
//! pressed by Hyprland itself (`send_shortcut`), so every app sees ordinary
//! key presses. `wtype`, a `zwp_virtual_keyboard_v1` client with its own
//! keymap, only types the characters a US layout has no key for: an input
//! method such as fcitx5 keeps only the first keymap it uploads and drops
//! the rest, and Chromium and Electron apps ignore its chords.

use super::hypr::ctl;
use super::{BoxFuture, Injector, Result, run};
use crate::chord::{Chord, KeyPress, key_for_char};

pub struct Keys;

/// A run of text to inject one way or the other.
#[derive(Debug, PartialEq)]
enum Run {
    Keys(Vec<KeyPress>),
    Other(String),
}

/// Splits text into runs of characters with a key and runs without one.
fn runs(text: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    for c in text.chars() {
        match (key_for_char(c), runs.last_mut()) {
            (Some(press), Some(Run::Keys(keys))) => keys.push(press),
            (Some(press), _) => runs.push(Run::Keys(vec![press])),
            (None, Some(Run::Other(rest))) => rest.push(c),
            (None, _) => runs.push(Run::Other(c.to_string())),
        }
    }
    runs
}

impl Injector for Keys {
    fn key<'a>(&'a self, chord: &'a Chord) -> BoxFuture<'a, Result<()>> {
        Box::pin(ctl::press(chord))
    }

    fn type_text<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            for run in runs(text) {
                match run {
                    Run::Keys(keys) => ctl::press_keys(&keys).await?,
                    // `--` so text starting with a dash is text.
                    Run::Other(rest) => {
                        run::run("wtype", &["--", &rest]).await?;
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_splits_into_key_presses_and_the_rest() {
        let keys = |s: &str| Run::Keys(s.chars().map(|c| key_for_char(c).unwrap()).collect());
        assert_eq!(runs(""), vec![]);
        assert_eq!(runs("2026-09-30"), vec![keys("2026-09-30")]);
        assert_eq!(
            runs("naïve ok ✓"),
            vec![
                keys("na"),
                Run::Other("ï".into()),
                keys("ve ok "),
                Run::Other("✓".into())
            ]
        );
    }
}
