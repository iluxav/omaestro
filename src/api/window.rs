//! `om.window()`: the focused window. `om.dispatch(code)`: a Hyprland
//! dispatcher.

use mlua::{Error, Lua, Result, Table};

use super::Context;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let hypr = cx.backends.hypr.clone();
    om.set(
        "dispatch",
        lua.create_async_function(move |_, code: String| {
            let hypr = hypr.clone();
            async move {
                if code.trim().is_empty() {
                    return Err(Error::runtime("om.dispatch: nothing to dispatch"));
                }
                hypr.dispatch(&code).await.map_err(Error::external)
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "window",
        lua.create_async_function(move |lua, ()| {
            let hypr = hypr.clone();
            async move {
                let Some(window) = hypr.active_window().await.map_err(Error::external)? else {
                    return Ok(None);
                };
                let table = lua.create_table()?;
                table.set("class", window.class)?;
                table.set("title", window.title)?;
                table.set("address", window.address)?;
                table.set("workspace", window.workspace)?;
                table.set("floating", window.floating)?;
                Ok(Some(table))
            }
        })?,
    )
}
