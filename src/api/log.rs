//! `om.log(...)`: a line in the daemon's log (the journal under systemd).

use mlua::{Lua, Result, Table, Value, Variadic};

pub fn install(lua: &Lua, om: &Table) -> Result<()> {
    om.set(
        "log",
        lua.create_function(|_, args: Variadic<Value>| {
            // Same shape as Lua's print: tostring on each value, tab separated.
            let parts = args
                .iter()
                .map(Value::to_string)
                .collect::<Result<Vec<_>>>()?;
            tracing::info!("lua: {}", parts.join("\t"));
            Ok(())
        })?,
    )
}
