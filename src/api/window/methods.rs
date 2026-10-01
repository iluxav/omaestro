//! What a window table can do: each method runs a dispatcher against the
//! window's address, so it works on any window, focused or not.

use std::sync::Arc;

use mlua::{Error, Lua, Result, Table, Value};

use crate::backend::Hypr;
use crate::geometry::{Rect, fraction, place};

/// The methods every window table gets through its metatable. Each one
/// runs a dispatcher against the window's address, so it works on any
/// window, focused or not.
pub fn table(lua: &Lua, hypr: Arc<dyn Hypr>) -> Result<Table> {
    let methods = lua.create_table()?;

    fn address(this: &Table) -> Result<String> {
        this.get::<Option<String>>("address")?
            .ok_or_else(|| Error::runtime("not a window: no address (call methods with a colon)"))
    }

    fn quoted(address: &str) -> String {
        // Addresses are hex from Hyprland; nothing to escape.
        format!("\"address:{address}\"")
    }

    /// A dispatcher on `window`, and nothing else.
    async fn run(hypr: &dyn Hypr, code: String) -> Result<()> {
        hypr.dispatch(&code).await.map_err(Error::external)
    }

    let h = hypr.clone();
    methods.set(
        "move",
        lua.create_async_function(move |_, (this, x, y): (Table, i64, i64)| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                run(
                    &*hypr,
                    format!("hl.dsp.window.move({{ x = {x}, y = {y}, window = {a} }})"),
                )
                .await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "resize",
        lua.create_async_function(move |_, (this, w, hgt): (Table, i64, i64)| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                run(
                    &*hypr,
                    format!("hl.dsp.window.resize({{ x = {w}, y = {hgt}, window = {a} }})"),
                )
                .await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "float",
        lua.create_async_function(move |_, (this, on): (Table, Option<bool>)| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                let action = match on {
                    Some(true) => "enable",
                    Some(false) => "disable",
                    None => "toggle",
                };
                run(
                    &*hypr,
                    format!("hl.dsp.window.float({{ action = \"{action}\", window = {a} }})"),
                )
                .await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "pin",
        lua.create_async_function(move |_, (this, on): (Table, Option<bool>)| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                let action = match on {
                    Some(true) => "enable",
                    Some(false) => "disable",
                    None => "toggle",
                };
                run(
                    &*hypr,
                    format!("hl.dsp.window.pin({{ action = \"{action}\", window = {a} }})"),
                )
                .await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "fullscreen",
        lua.create_async_function(move |_, (this, mode): (Table, Option<String>)| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                let mode = mode.unwrap_or_else(|| "fullscreen".to_string());
                if !matches!(mode.as_str(), "fullscreen" | "maximized") {
                    return Err(Error::runtime(format!(
                        "fullscreen: mode is \"fullscreen\" or \"maximized\", not \"{mode}\""
                    )));
                }
                run(
                    &*hypr,
                    format!("hl.dsp.window.fullscreen({{ mode = \"{mode}\", window = {a} }})"),
                )
                .await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "focus",
        lua.create_async_function(move |_, this: Table| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                run(&*hypr, format!("hl.dsp.focus({{ window = {a} }})")).await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "close",
        lua.create_async_function(move |_, this: Table| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                run(&*hypr, format!("hl.dsp.window.close({{ window = {a} }})")).await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "to_workspace",
        lua.create_async_function(move |_, (this, workspace, follow): (Table, Value, Option<bool>)| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                let target = match workspace {
                    Value::Integer(id) => id.to_string(),
                    Value::Number(id) => (id as i64).to_string(),
                    Value::String(name) => name.to_string_lossy().replace('"', ""),
                    other => {
                        return Err(Error::runtime(format!(
                            "to_workspace: a number or a name, not {}",
                            other.type_name()
                        )));
                    }
                };
                let follow = follow.unwrap_or(false);
                run(
                    &*hypr,
                    format!("hl.dsp.window.move({{ workspace = \"{target}\", follow = {follow}, window = {a} }})"),
                )
                .await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "center",
        lua.create_async_function(move |_, this: Table| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                run(&*hypr, format!("hl.dsp.window.center({{ window = {a} }})")).await
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "refresh",
        lua.create_async_function(move |lua, this: Table| {
            let hypr = h.clone();
            async move {
                let addr = address(&this)?;
                let clients = hypr.clients().await.map_err(Error::external)?;
                match clients.iter().find(|c| c.address == addr) {
                    Some(live) => Ok(Some(super::window_table(
                        &lua,
                        live,
                        &this
                            .metatable()
                            .ok_or_else(|| Error::runtime("refresh: not a window"))?,
                    )?)),
                    None => Ok(None),
                }
            }
        })?,
    )?;

    let h = hypr.clone();
    methods.set(
        "place",
        lua.create_async_function(move |_, (this, spec): (Table, Value)| {
            let hypr = h.clone();
            async move {
                let a = quoted(&address(&this)?);
                // Fresh facts: the table may come from an event, or be stale.
                let addr = address(&this)?;
                let clients = hypr.clients().await.map_err(Error::external)?;
                let live = clients
                    .iter()
                    .find(|c| c.address == addr)
                    .ok_or_else(|| Error::runtime(format!("place: window {addr} is gone")))?;
                let monitor_id = live.monitor;
                let current = Rect {
                    x: live.x,
                    y: live.y,
                    w: live.width,
                    h: live.height,
                };
                let monitors = hypr.monitors().await.map_err(Error::external)?;
                let monitor = monitors
                    .iter()
                    .find(|m| m.id == monitor_id)
                    .ok_or_else(|| Error::runtime(format!("place: the window's monitor {monitor_id} is gone")))?;
                let area = monitor.usable();
                let target = match spec {
                    Value::String(name) => place(&name.to_string_lossy(), area, current).map_err(Error::runtime)?,
                    Value::Table(f) => fraction(
                        area,
                        f.get::<Option<f64>>("x")?.unwrap_or(0.0),
                        f.get::<Option<f64>>("y")?.unwrap_or(0.0),
                        f.get::<Option<f64>>("w")?.unwrap_or(1.0),
                        f.get::<Option<f64>>("h")?.unwrap_or(1.0),
                    ),
                    other => {
                        return Err(Error::runtime(format!(
                            "place: a name like \"left\" or a table {{x=, y=, w=, h=}} of fractions, not {}",
                            other.type_name()
                        )));
                    }
                };
                // Only floating windows take exact geometry.
                run(&*hypr, format!("hl.dsp.window.float({{ action = \"enable\", window = {a} }})")).await?;
                run(
                    &*hypr,
                    format!("hl.dsp.window.resize({{ x = {}, y = {}, window = {a} }})", target.w, target.h),
                )
                .await?;
                run(
                    &*hypr,
                    format!("hl.dsp.window.move({{ x = {}, y = {}, window = {a} }})", target.x, target.y),
                )
                .await
            }
        })?,
    )?;

    Ok(methods)
}
