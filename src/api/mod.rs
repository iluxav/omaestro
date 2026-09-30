//! The `om` table the rules see. One file per group of functions.

use std::sync::Arc;

use mlua::{Lua, Result};
use tokio::sync::Notify;

use crate::backend::Backends;
use crate::config::Config;
use crate::runtime::error::PRELUDE_CHUNK;
use crate::runtime::registry::SharedRegistry;

mod clip;
mod input;
mod llm;
mod log;
mod notify;
mod shell;
mod triggers;
mod window;

const PRELUDE: &str = include_str!("../../lua/prelude.lua");

/// What the API functions close over.
pub struct Context {
    pub backends: Backends,
    pub registry: SharedRegistry,
    pub config: Arc<Config>,
    /// Poked whenever a trigger is registered or removed, so the runtime
    /// brings Hyprland's binds and the timers in step.
    pub triggers_changed: Arc<Notify>,
}

/// Creates the global `om` table and runs the prelude.
pub fn install(lua: &Lua, cx: &Context) -> Result<()> {
    let om = lua.create_table()?;
    clip::install(lua, &om, cx)?;
    input::install(lua, &om, cx)?;
    llm::install(lua, &om, cx)?;
    log::install(lua, &om)?;
    notify::install(lua, &om, cx)?;
    shell::install(lua, &om, cx)?;
    triggers::install(lua, &om, cx)?;
    window::install(lua, &om, cx)?;
    lua.globals().set("om", om)?;
    lua.load(PRELUDE)
        .set_name(format!("={PRELUDE_CHUNK}"))
        .exec()
}

/// `rules.d/foo.lua:12` of the Lua code calling the running API function.
fn caller(lua: &Lua) -> Option<String> {
    const MAX_DEPTH: usize = 16;
    (0..MAX_DEPTH).find_map(|level| {
        lua.inspect_stack(level, |debug| {
            let source = debug.source();
            let file = source.short_src?;
            if source.what == "C" || file.starts_with("__mlua") {
                return None;
            }
            Some(match debug.current_line() {
                Some(line) => format!("{file}:{line}"),
                None => file.into_owned(),
            })
        })
        .flatten()
    })
}
