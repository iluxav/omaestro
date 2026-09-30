//! `om.paste(text)`, `om.key(chord)`, `om.type(text)`: getting text and key
//! presses into the focused window.

use std::time::{Duration, Instant};

use mlua::{Error, Lua, Result, Table};
use tokio::time::sleep;

use super::Context;
use crate::backend::ClipContent;
use crate::chord::Chord;
use crate::runtime::handler::HOTKEY_PRESSED;

/// `wl-copy` hands the text over from a background process; give it a moment
/// to own the clipboard before the paste chord asks for it.
const SETTLE: Duration = Duration::from_millis(50);

/// How long after a hotkey press the modifiers are assumed released. Keys
/// injected while SUPER or ALT are still held arrive as shortcuts, not text,
/// and Hyprland cannot tell us when the fingers have left.
const MODIFIER_RELEASE: Duration = Duration::from_millis(400);

/// How much longer to wait before injecting, given when the hotkey that
/// started this handler was pressed.
fn settle_left(pressed: Option<Instant>, now: Instant) -> Duration {
    match pressed {
        Some(pressed) => MODIFIER_RELEASE.saturating_sub(now.saturating_duration_since(pressed)),
        None => Duration::ZERO,
    }
}

/// In a handler fired by a hotkey, waits until the chord has had time to be
/// released. Elsewhere, returns at once.
async fn after_hotkey_release() {
    let pressed = HOTKEY_PRESSED.try_with(|pressed| *pressed).unwrap_or(None);
    let left = settle_left(pressed, Instant::now());
    if !left.is_zero() {
        sleep(left).await;
    }
}

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let injector = cx.backends.injector.clone();
    om.set(
        "key",
        lua.create_async_function(move |_, chord: String| {
            let injector = injector.clone();
            async move {
                let chord =
                    Chord::parse(&chord).map_err(|err| Error::runtime(format!("om.key: {err}")))?;
                after_hotkey_release().await;
                injector.key(&chord).await.map_err(Error::external)
            }
        })?,
    )?;

    let injector = cx.backends.injector.clone();
    om.set(
        "type",
        lua.create_async_function(move |_, text: String| {
            let injector = injector.clone();
            async move {
                after_hotkey_release().await;
                injector.type_text(&text).await.map_err(Error::external)
            }
        })?,
    )?;

    let backends = cx.backends.clone();
    let config = cx.config.clone();
    om.set(
        "paste",
        lua.create_async_function(move |_, text: String| {
            let backends = backends.clone();
            let config = config.clone();
            async move {
                // Which chord pastes depends on the app that has focus. If
                // Hyprland cannot say, fall back to the default chord.
                let window = backends.hypr.active_window().await.unwrap_or(None);
                let class = window.as_ref().map(|w| w.class.as_str());
                let chord = config.paste.chord_for(class).map_err(Error::runtime)?;
                tracing::debug!(
                    "paste: {} bytes into {} with {chord}",
                    text.len(),
                    class.map_or("no focused window".to_string(), |c| format!("class '{c}'"))
                );

                let clipboard = &backends.clipboard;
                let saved = clipboard.get().await.map_err(Error::external)?;
                clipboard
                    .set(&ClipContent::text(&text))
                    .await
                    .map_err(Error::external)?;
                sleep(SETTLE).await;
                after_hotkey_release().await;
                let pasted = backends.injector.key(&chord).await;

                // Whatever happened to the paste, the user gets their
                // clipboard back, after the app has had time to read ours.
                sleep(Duration::from_millis(config.paste.restore_ms)).await;
                let restored = match &saved {
                    Some(content) => clipboard.set(content).await,
                    None => clipboard.clear().await,
                };
                pasted.map_err(Error::external)?;
                restored.map_err(Error::external)
            }
        })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_out_the_rest_of_the_release_window_only_after_a_hotkey() {
        let pressed = Instant::now();
        let ms = |n| Duration::from_millis(n);
        assert_eq!(settle_left(None, pressed + ms(10)), Duration::ZERO);
        assert_eq!(settle_left(Some(pressed), pressed + ms(0)), ms(400));
        assert_eq!(settle_left(Some(pressed), pressed + ms(150)), ms(250));
        assert_eq!(
            settle_left(Some(pressed), pressed + ms(400)),
            Duration::ZERO
        );
        assert_eq!(
            settle_left(Some(pressed), pressed + ms(2000)),
            Duration::ZERO
        );
    }
}
