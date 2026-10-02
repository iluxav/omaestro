//! Key injection. Everything that can be pressed on the real keymap is
//! pressed by Hyprland itself (`send_shortcut`), so every app sees ordinary
//! key presses. `wtype`, a `zwp_virtual_keyboard_v1` client with its own
//! keymap, only types the characters a US layout has no key for: an input
//! method such as fcitx5 keeps only the first keymap it uploads and drops
//! the rest, and Chromium and Electron apps ignore its chords.

use super::hypr::ctl;
use super::{BackendError, BoxFuture, Injector, Result, run};
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
        Box::pin(async move {
            match ctl::press(chord).await {
                // Hyprland looks keys up in the keymap of the keyboard used
                // last. After any wtype input (ours, or another tool's) that
                // is wtype's own small keymap, and every key it lacks is "not
                // found" until the real keyboard is touched again. A single
                // chord (the paste) is safe to press again through wtype.
                Err(err) if key_not_found(&err) => match chord.wtype_args() {
                    Some(args) => {
                        let args: Vec<&str> = args.iter().map(String::as_str).collect();
                        run::run("wtype", &args).await.map(|_| ())
                    }
                    None => Err(err),
                },
                result => result,
            }
        })
    }

    fn erase(&self, count: usize) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let keys: Vec<KeyPress> = (0..count).map(|_| ("", "BackSpace".to_string())).collect();
            ctl::press_keys(&keys).await
        })
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

/// Hyprland's answer when the last keyboard's keymap has no such key.
fn key_not_found(err: &BackendError) -> bool {
    matches!(err, BackendError::Failed { message, .. } if message.contains("key not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_missing_key_falls_back_to_wtype() {
        let failed = |message: &str| BackendError::Failed {
            tool: "hyprctl",
            message: message.to_string(),
        };
        assert!(key_not_found(&failed(
            "=[C]:-1: send_shortcut: key not found"
        )));
        assert!(!key_not_found(&failed("no such dispatcher")));
        assert!(!key_not_found(&BackendError::MissingTool {
            tool: "hyprctl"
        }));
    }

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
