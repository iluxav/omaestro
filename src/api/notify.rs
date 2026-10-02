//! `om.notify(title, body, opts)`: a desktop notification; with
//! `opts.actions` it has buttons and returns the pressed one.
//! `om.busy(title, body)`: a notification that stays up while the handler
//! works and goes away when it ends; `om.busy()` takes it down earlier.

use std::sync::PoisonError;
use std::time::Duration;

use mlua::{Error, Lua, Result, Table};

use super::Context;
use crate::runtime::handler::BUSY;

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
    )?;

    let notifier = cx.backends.notifier.clone();
    om.set(
        "busy",
        lua.create_async_function(move |_, (title, body): (Option<String>, Option<String>)| {
            let notifier = notifier.clone();
            async move {
                // The notification of this handler run, if it showed
                // one; outside a handler (`om eval`) there is no run to
                // take it down, so it stays until om.busy().
                let slot = BUSY.try_with(|slot| slot.clone()).ok();
                let current = slot
                    .as_ref()
                    .and_then(|slot| *slot.lock().unwrap_or_else(PoisonError::into_inner));
                let Some(title) = title else {
                    if let Some(id) = current {
                        notifier.close(id).await.map_err(Error::external)?;
                    }
                    if let Some(slot) = &slot {
                        *slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
                    }
                    return Ok(());
                };
                let id = notifier
                    .progress(&title, &body.unwrap_or_default(), current)
                    .await
                    .map_err(Error::external)?;
                if let Some(slot) = &slot {
                    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(id);
                }
                Ok(())
            }
        })?,
    )
}
