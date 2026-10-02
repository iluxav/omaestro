//! The `om` table the rules see. One file per group of functions.

use std::sync::Arc;

use mlua::{Lua, Result, Table, Value};
use tokio::sync::Notify;

use crate::backend::Backends;
use crate::config::Config;
use crate::runtime::error::PRELUDE_CHUNK;
use crate::runtime::registry::SharedRegistry;

mod app;
mod clip;
mod http;
mod input;
mod llm;
mod log;
mod modes;
mod notify;
mod shell;
mod store;
pub mod triggers;
pub mod window;

const PRELUDE: &str = include_str!("../../lua/prelude.lua");

/// What the API functions close over.
pub struct Context {
    pub backends: Backends,
    pub registry: SharedRegistry,
    pub config: Arc<Config>,
    /// Poked whenever a trigger is registered or removed, so the runtime
    /// brings Hyprland's binds and the timers in step.
    pub triggers_changed: Arc<Notify>,
    /// Where `om.store` keeps its file.
    pub state_dir: std::path::PathBuf,
    /// The rules directory; `lib/` under it is on `package.path`.
    pub config_dir: std::path::PathBuf,
}

/// Creates the global `om` table and runs the prelude.
pub fn install(lua: &Lua, cx: &Context) -> Result<()> {
    let om = lua.create_table()?;
    // Windows first: the app group uses the window metatable it registers.
    window::install(lua, &om, cx)?;
    app::install(lua, &om, cx)?;
    clip::install(lua, &om, cx)?;
    http::install(lua, &om, cx)?;
    input::install(lua, &om, cx)?;
    llm::install(lua, &om, cx)?;
    log::install(lua, &om)?;
    notify::install(lua, &om, cx)?;
    shell::install(lua, &om, cx)?;
    store::install(lua, &om, cx)?;
    triggers::install(lua, &om, cx)?;
    modes::install(lua, &om, cx)?;
    om.set("config_dir", cx.config_dir.to_string_lossy().to_string())?;
    lua.globals().set("om", om)?;
    // Modules under lib/ are loaded under a chunk name relative to the
    // config directory (`@lib/x/init.lua`), so a position in an error or a
    // trigger's origin reads `lib/x/init.lua:12`, not Lua's truncated
    // absolute path. Tried before Lua's own path searcher.
    let config_dir = cx.config_dir.clone();
    let searcher = lua.create_function(move |lua, name: String| {
        let relative = name.replace('.', "/");
        let candidates = [
            format!("lib/{relative}.lua"),
            format!("lib/{relative}/init.lua"),
        ];
        for candidate in &candidates {
            let Ok(code) = std::fs::read_to_string(config_dir.join(candidate)) else {
                continue;
            };
            let loader = lua
                .load(code)
                .set_name(format!("@{candidate}"))
                .into_function()?;
            return Ok(mlua::MultiValue::from_iter([
                Value::Function(loader),
                Value::String(lua.create_string(candidate)?),
            ]));
        }
        let tried: Vec<String> = candidates
            .iter()
            .map(|c| format!("\n\tno file '{}'", config_dir.join(c).display()))
            .collect();
        Ok(mlua::MultiValue::from_iter([Value::String(
            lua.create_string(tried.concat())?,
        )]))
    })?;
    let searchers: Table = lua.globals().get::<Table>("package")?.get("searchers")?;
    searchers.raw_insert(2, searcher)?;
    // `require("name")` finds ~/.config/omaestro/lib/name.lua or lib/name/init.lua.
    let lib = cx.config_dir.join("lib");
    let package: Table = lua.globals().get("package")?;
    let path: String = package.get("path")?;
    package.set(
        "path",
        format!("{0}/?.lua;{0}/?/init.lua;{path}", lib.to_string_lossy()),
    )?;
    lua.load(PRELUDE)
        .set_name(format!("={PRELUDE_CHUNK}"))
        .exec()
}

/// `rules.d/foo.lua:12` of the Lua code calling the running API function.
/// A module under the config directory (`lib/<plugin>/init.lua`) is named
/// relative to it too, like the rule files.
fn caller(lua: &Lua) -> Option<String> {
    const MAX_DEPTH: usize = 16;
    let config_dir = lua
        .globals()
        .get::<Table>("om")
        .ok()
        .and_then(|om| om.get::<String>("config_dir").ok())
        .map(|dir| format!("{}/", dir.trim_end_matches('/')));
    (0..MAX_DEPTH).find_map(|level| {
        lua.inspect_stack(level, |debug| {
            let source = debug.source();
            if source.what == "C" {
                return None;
            }
            // The chunk name: `@rules.d/foo.lua` for a file, `=eval` for a
            // named chunk, `@/home/.../lib/x/init.lua` for a required module.
            let name = source.source?;
            let file = name
                .strip_prefix('@')
                .or_else(|| name.strip_prefix('='))
                .unwrap_or(&name);
            // Helpers in the prelude (om.menu) register triggers for the
            // rule that called them: the origin is that rule's line.
            if file.starts_with("__mlua") || file == PRELUDE_CHUNK {
                return None;
            }
            let file = match &config_dir {
                Some(dir) => file.strip_prefix(dir.as_str()).unwrap_or(file),
                None => file,
            };
            Some(match debug.current_line() {
                Some(line) => format!("{file}:{line}"),
                None => file.to_string(),
            })
        })
        .flatten()
    })
}
