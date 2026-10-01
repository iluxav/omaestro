//! Windows, monitors and workspaces: `om.window()`, `om.windows(filter)`,
//! `om.monitors()`, `om.monitor(name)`, `om.workspace()`, `om.workspaces()`,
//! and `om.dispatch(code)`. A window is a table of facts with methods that
//! act on it through Hyprland dispatchers.

use mlua::{Error, Function, Lua, Result, Table, Value};

use super::Context;
use crate::backend::hypr::events::WinRef;
use crate::backend::{Monitor, Window, Workspace};

mod methods;

/// Where the window metatable lives, so the runtime can build window
/// objects for handler arguments.
const META_KEY: &str = "omaestro.window_meta";

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

    let methods = methods::table(lua, cx.backends.hypr.clone())?;
    let meta = lua.create_table()?;
    meta.set("__index", methods)?;
    lua.set_named_registry_value(META_KEY, meta.clone())?;

    let hypr = cx.backends.hypr.clone();
    let window_meta = meta.clone();
    om.set(
        "window",
        lua.create_async_function(move |lua, ()| {
            let hypr = hypr.clone();
            let meta = window_meta.clone();
            async move {
                match hypr.active_window().await.map_err(Error::external)? {
                    Some(window) => Ok(Some(window_table(&lua, &window, &meta)?)),
                    None => Ok(None),
                }
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    let window_meta = meta.clone();
    om.set(
        "windows",
        lua.create_async_function(move |lua, filter: Option<Table>| {
            let hypr = hypr.clone();
            let meta = window_meta.clone();
            async move {
                let filter = match filter {
                    Some(filter) => Filter::from_table(&filter)?,
                    None => Filter::default(),
                };
                let monitors = if filter.monitor_name.is_some() {
                    hypr.monitors().await.map_err(Error::external)?
                } else {
                    Vec::new()
                };
                let find: Function = lua.globals().get::<Table>("string")?.get("find")?;
                let mut out = Vec::new();
                for window in hypr.clients().await.map_err(Error::external)? {
                    if filter.matches(&find, &window, &monitors)? {
                        out.push(window_table(&lua, &window, &meta)?);
                    }
                }
                Ok(out)
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "monitors",
        lua.create_async_function(move |lua, ()| {
            let hypr = hypr.clone();
            async move {
                let monitors = hypr.monitors().await.map_err(Error::external)?;
                monitors
                    .iter()
                    .map(|m| monitor_table(&lua, m))
                    .collect::<Result<Vec<_>>>()
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "monitor",
        lua.create_async_function(move |lua, name: Option<String>| {
            let hypr = hypr.clone();
            async move {
                let monitors = hypr.monitors().await.map_err(Error::external)?;
                let wanted = monitors.iter().find(|m| match &name {
                    Some(name) => &m.name == name,
                    None => m.focused,
                });
                match wanted {
                    Some(monitor) => Ok(Some(monitor_table(&lua, monitor)?)),
                    None => Ok(None),
                }
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "mouse",
        lua.create_async_function(move |lua, ()| {
            let hypr = hypr.clone();
            async move {
                let (x, y) = hypr.cursor().await.map_err(Error::external)?;
                let table = lua.create_table()?;
                table.set("x", x)?;
                table.set("y", y)?;
                Ok(table)
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "mouse_to",
        lua.create_async_function(move |_, (x, y): (i64, i64)| {
            let hypr = hypr.clone();
            async move {
                hypr.dispatch(&format!("hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
                    .await
                    .map_err(Error::external)
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "workspaces",
        lua.create_async_function(move |lua, ()| {
            let hypr = hypr.clone();
            async move {
                let workspaces = hypr.workspaces().await.map_err(Error::external)?;
                workspaces
                    .iter()
                    .map(|w| workspace_table(&lua, w))
                    .collect::<Result<Vec<_>>>()
            }
        })?,
    )?;

    let hypr = cx.backends.hypr.clone();
    om.set(
        "workspace",
        lua.create_async_function(move |lua, ()| {
            let hypr = hypr.clone();
            async move {
                // The active workspace is the focused monitor's.
                let monitors = hypr.monitors().await.map_err(Error::external)?;
                let Some(focused) = monitors.iter().find(|m| m.focused) else {
                    return Ok(None);
                };
                let workspaces = hypr.workspaces().await.map_err(Error::external)?;
                match workspaces.iter().find(|w| w.id == focused.workspace_id) {
                    Some(workspace) => Ok(Some(workspace_table(&lua, workspace)?)),
                    None => Ok(None),
                }
            }
        })?,
    )
}

/// What `om.windows` filters on. Strings are Lua patterns.
#[derive(Default)]
struct Filter {
    class: Option<String>,
    title: Option<String>,
    workspace: Option<Value>,
    monitor_id: Option<i64>,
    monitor_name: Option<String>,
}

impl Filter {
    fn from_table(table: &Table) -> Result<Self> {
        let monitor: Option<Value> = table.get("monitor")?;
        let (monitor_id, monitor_name) = match monitor {
            Some(Value::Integer(id)) => (Some(id), None),
            Some(Value::Number(id)) => (Some(id as i64), None),
            Some(Value::String(name)) => (None, Some(name.to_string_lossy())),
            Some(Value::Nil) | None => (None, None),
            Some(other) => {
                return Err(Error::runtime(format!(
                    "om.windows: monitor must be a name or an id, not {}",
                    other.type_name()
                )));
            }
        };
        Ok(Self {
            class: table.get("class")?,
            title: table.get("title")?,
            workspace: table
                .get::<Option<Value>>("workspace")?
                .filter(|v| *v != Value::Nil),
            monitor_id,
            monitor_name,
        })
    }

    fn matches(&self, find: &Function, window: &Window, monitors: &[Monitor]) -> Result<bool> {
        for (pattern, text) in [(&self.class, &window.class), (&self.title, &window.title)] {
            if let Some(pattern) = pattern
                && find.call::<Value>((text.as_str(), pattern.as_str()))? == Value::Nil
            {
                return Ok(false);
            }
        }
        match &self.workspace {
            Some(Value::Integer(id)) if *id != window.workspace_id => return Ok(false),
            Some(Value::Number(id)) if *id as i64 != window.workspace_id => return Ok(false),
            Some(Value::String(name)) if name.to_string_lossy() != window.workspace => {
                return Ok(false);
            }
            _ => {}
        }
        if let Some(id) = self.monitor_id
            && id != window.monitor
        {
            return Ok(false);
        }
        if let Some(name) = &self.monitor_name {
            let Some(monitor) = monitors.iter().find(|m| &m.name == name) else {
                return Ok(false);
            };
            if monitor.id != window.monitor {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// A window object from a full `Window`, for API functions outside this
/// module.
pub fn from_window(lua: &Lua, window: &Window) -> Result<Table> {
    let meta: Table = lua.named_registry_value(META_KEY)?;
    window_table(lua, window, &meta)
}

/// `"pattern"` (a class pattern) or `{class = "...", title = "..."}` as
/// the two optional patterns.
pub fn matcher_from_value(
    what: &Value,
    function: &str,
) -> Result<(Option<String>, Option<String>)> {
    match what {
        Value::String(class) => Ok((Some(class.to_string_lossy()), None)),
        Value::Table(table) => Ok((table.get("class")?, table.get("title")?)),
        other => Err(Error::runtime(format!(
            "{function}: a class pattern or a table {{class=, title=}}, not {}",
            other.type_name()
        ))),
    }
}

/// A window object from what an event said about it: address, class,
/// title and maybe the workspace. Geometry is fetched when a method needs it.
pub fn from_event(lua: &Lua, window: &WinRef) -> Result<Table> {
    let meta: Table = lua.named_registry_value(META_KEY)?;
    let table = lua.create_table()?;
    table.set("address", window.address.as_str())?;
    table.set("class", window.class.as_str())?;
    table.set("title", window.title.as_str())?;
    if !window.workspace.is_empty() {
        table.set("workspace", window.workspace.as_str())?;
    }
    table.set_metatable(Some(meta))?;
    Ok(table)
}

pub(super) fn window_table(lua: &Lua, window: &Window, meta: &Table) -> Result<Table> {
    let table = lua.create_table()?;
    table.set("address", window.address.as_str())?;
    table.set("class", window.class.as_str())?;
    table.set("title", window.title.as_str())?;
    table.set("initial_class", window.initial_class.as_str())?;
    table.set("workspace", window.workspace.as_str())?;
    table.set("workspace_id", window.workspace_id)?;
    table.set("monitor", window.monitor)?;
    table.set("x", window.x)?;
    table.set("y", window.y)?;
    table.set("width", window.width)?;
    table.set("height", window.height)?;
    table.set("floating", window.floating)?;
    // `fullscreen` is the method; the fact gets its own name.
    table.set(
        "fullscreen_mode",
        match window.fullscreen {
            0 => "none",
            1 => "maximized",
            _ => "fullscreen",
        },
    )?;
    table.set("pinned", window.pinned)?;
    table.set("pid", window.pid)?;
    table.set("xwayland", window.xwayland)?;
    table.set("focused", window.focused)?;
    table.set_metatable(Some(meta.clone()))?;
    Ok(table)
}

fn monitor_table(lua: &Lua, monitor: &Monitor) -> Result<Table> {
    let table = lua.create_table()?;
    let area = monitor.logical();
    table.set("id", monitor.id)?;
    table.set("name", monitor.name.as_str())?;
    table.set("description", monitor.description.as_str())?;
    table.set("x", area.x)?;
    table.set("y", area.y)?;
    table.set("width", area.w)?;
    table.set("height", area.h)?;
    table.set("scale", monitor.scale)?;
    table.set("transform", monitor.transform)?;
    table.set("focused", monitor.focused)?;
    table.set("workspace", monitor.workspace.as_str())?;
    table.set("workspace_id", monitor.workspace_id)?;
    Ok(table)
}

fn workspace_table(lua: &Lua, workspace: &Workspace) -> Result<Table> {
    let table = lua.create_table()?;
    table.set("id", workspace.id)?;
    table.set("name", workspace.name.as_str())?;
    table.set("monitor", workspace.monitor.as_str())?;
    table.set("windows", workspace.windows)?;
    table.set("has_fullscreen", workspace.has_fullscreen)?;
    Ok(table)
}
