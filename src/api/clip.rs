//! `om.selection()`: the text the user has selected. `om.clipboard()` and
//! `om.set_clipboard(text)`: the clipboard as text.

use mlua::{Error, Lua, Result, Table};

use super::Context;
use crate::backend::ClipContent;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let clipboard = cx.backends.clipboard.clone();
    om.set(
        "selection",
        lua.create_async_function(move |_, ()| {
            let clipboard = clipboard.clone();
            async move {
                let text = clipboard.selection().await.map_err(Error::external)?;
                tracing::debug!("selection: {} bytes", text.len());
                Ok(text)
            }
        })?,
    )?;

    let clipboard = cx.backends.clipboard.clone();
    om.set(
        "clipboard",
        lua.create_async_function(move |_, ()| {
            let clipboard = clipboard.clone();
            async move {
                let content = clipboard.get().await.map_err(Error::external)?;
                // Anything that is not text (an image, files) reads as "".
                Ok(match content {
                    Some(content) if is_text(&content.mime) => {
                        String::from_utf8_lossy(&content.data).into_owned()
                    }
                    _ => String::new(),
                })
            }
        })?,
    )?;

    let clipboard = cx.backends.clipboard.clone();
    om.set(
        "set_clipboard",
        lua.create_async_function(move |_, text: String| {
            let clipboard = clipboard.clone();
            async move {
                clipboard
                    .set(&ClipContent::text(&text))
                    .await
                    .map_err(Error::external)
            }
        })?,
    )
}

fn is_text(mime: &str) -> bool {
    mime.starts_with("text/") || matches!(mime, "STRING" | "UTF8_STRING" | "TEXT")
}
