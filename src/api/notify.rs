//! `om.notify(title, body, opts)`: a desktop notification; with
//! `opts.actions` it has buttons and returns the pressed one.

use std::time::Duration;

use mlua::{Error, Lua, Result, Table};

use super::Context;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let notifier = cx.backends.notifier.clone();
    om.set(
        "notify",
        lua.create_async_function(
            move |_, (title, body, options): (String, Option<String>, Option<Table>)| {
                let notifier = notifier.clone();
                async move {
                    let body = body.unwrap_or_default();
                    let Some(options) = options else {
                        notifier
                            .notify(&title, &body)
                            .await
                            .map_err(Error::external)?;
                        return Ok(None);
                    };
                    let mut actions = Vec::new();
                    if let Some(table) = options.get::<Option<Table>>("actions")? {
                        for pair in table.pairs::<String, String>() {
                            actions.push(pair?);
                        }
                        // A stable order for the buttons.
                        actions.sort();
                    }
                    let timeout = options
                        .get::<Option<f64>>("timeout")?
                        .map(Duration::from_secs_f64);
                    if actions.is_empty() {
                        notifier
                            .notify(&title, &body)
                            .await
                            .map_err(Error::external)?;
                        return Ok(None);
                    }
                    notifier
                        .ask(&title, &body, &actions, timeout)
                        .await
                        .map_err(Error::external)
                }
            },
        )?,
    )
}
