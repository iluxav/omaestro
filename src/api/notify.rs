//! `om.notify(title, body)`: a desktop notification.

use mlua::{Error, Lua, Result, Table};

use super::Context;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let notifier = cx.backends.notifier.clone();
    om.set(
        "notify",
        lua.create_async_function(move |_, (title, body): (String, Option<String>)| {
            let notifier = notifier.clone();
            async move {
                notifier
                    .notify(&title, body.as_deref().unwrap_or_default())
                    .await
                    .map_err(Error::external)
            }
        })?,
    )
}
