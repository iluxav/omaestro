//! Apps: `om.launch(cmd)`, `om.focus(match, cmd)` (focus a window if one
//! exists, else launch and wait for it), `om.apps()`.

use std::time::Duration;

use mlua::{Error, Function, Lua, Result, Table, Value};
use tokio::time::{Instant, sleep};

use super::Context;
use super::window::{from_window, matcher_from_value};
use crate::backend::hypr::ctl::lua_quote;
use crate::backend::{Hypr, Window};

/// How long `om.focus` waits for a launched app's window.
const LAUNCH_WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);

/// Starts `command` through Hyprland, so it runs in the session with
/// Hyprland's environment and is not a child of the daemon.
async fn launch(hypr: &dyn Hypr, command: &str) -> Result<()> {
    if command.trim().is_empty() {
        return Err(Error::runtime("launch: the command is empty"));
    }
    hypr.dispatch(&format!("hl.dsp.exec_cmd({})", lua_quote(command)))
        .await
        .map_err(Error::external)
}

/// The first window whose class or title fits the matcher.
async fn find(
    hypr: &dyn Hypr,
    find: &Function,
    class: &Option<String>,
    title: &Option<String>,
) -> Result<Option<Window>> {
    for window in hypr.clients().await.map_err(Error::external)? {
        let mut fits = true;
        for (pattern, text) in [(class, &window.class), (title, &window.title)] {
            if let Some(pattern) = pattern
                && find.call::<Value>((text.as_str(), pattern.as_str()))? == Value::Nil
            {
                fits = false;
            }
        }
        if fits {
            return Ok(Some(window));
        }
    }
    Ok(None)
}

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let hypr = cx.backends.hypr.clone();
    om.set(
        "launch",
        lua.create_async_function(move |_, command: String| {
            let hypr = hypr.clone();
            async move { launch(&*hypr, &command).await }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "focus",
        lua.create_async_function(move |lua, (what, command): (Value, Option<String>)| {
            let hypr = hypr.clone();
            async move {
                let (class, title) = matcher_from_value(&what, "om.focus")?;
                let string_find: Function = lua.globals().get::<Table>("string")?.get("find")?;
                if let Some(window) = find(&*hypr, &string_find, &class, &title).await? {
                    hypr.dispatch(&format!(
                        "hl.dsp.focus({{ window = \"address:{}\" }})",
                        window.address
                    ))
                    .await
                    .map_err(Error::external)?;
                    return Ok(Some(from_window(&lua, &window)?));
                }
                let Some(command) = command else {
                    return Ok(None);
                };
                launch(&*hypr, &command).await?;
                // Wait for its window; apps take their time to map one.
                let deadline = Instant::now() + LAUNCH_WAIT;
                while Instant::now() < deadline {
                    sleep(POLL).await;
                    if let Some(window) = find(&*hypr, &string_find, &class, &title).await? {
                        hypr.dispatch(&format!(
                            "hl.dsp.focus({{ window = \"address:{}\" }})",
                            window.address
                        ))
                        .await
                        .map_err(Error::external)?;
                        return Ok(Some(from_window(&lua, &window)?));
                    }
                }
                Ok(None)
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "apps",
        lua.create_async_function(move |lua, ()| {
            let hypr = hypr.clone();
            async move {
                // One entry per class, in order of first appearance.
                let mut apps: Vec<(String, Vec<Window>)> = Vec::new();
                for window in hypr.clients().await.map_err(Error::external)? {
                    match apps.iter_mut().find(|(class, _)| *class == window.class) {
                        Some((_, windows)) => windows.push(window),
                        None => apps.push((window.class.clone(), vec![window])),
                    }
                }
                let out = lua.create_table()?;
                for (class, windows) in apps {
                    let app = lua.create_table()?;
                    app.set("class", class)?;
                    app.set("count", windows.len())?;
                    let list = lua.create_table()?;
                    for window in &windows {
                        list.push(from_window(&lua, window)?)?;
                    }
                    app.set("windows", list)?;
                    out.push(app)?;
                }
                Ok(out)
            }
        })?,
    )
}
