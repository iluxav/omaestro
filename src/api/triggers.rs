//! Triggers: functions that register a handler and return a handle with
//! `:remove()`.

use std::sync::Arc;

use mlua::{Error, Function, Lua, MultiValue, Result, Table, UserData, UserDataMethods};
use tokio::sync::Notify;

use super::{Context, caller};
use crate::chord::Chord;
use crate::runtime::registry::{Matcher, SharedRegistry, TriggerKind};
use crate::runtime::timers::parse_interval;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "trigger",
        lua.create_function(move |lua, (name, handler): (String, Function)| {
            if name.is_empty() {
                return Err(Error::runtime("om.trigger: the name is empty"));
            }
            let origin = caller(lua).unwrap_or_else(|| "?".to_string());
            let serial = registry
                .lock()
                .insert(name.clone(), TriggerKind::Named, origin, handler)
                .map_err(Error::runtime)?;
            Ok(Handle {
                id: name,
                serial,
                registry: registry.clone(),
                triggers_changed: triggers_changed.clone(),
            })
        })?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "hotkey",
        lua.create_function(move |lua, (chord, handler): (String, Function)| {
            // Only the registration happens here. The runtime binds the chord
            // in Hyprland afterwards, once the rules have loaded.
            let chord =
                Chord::parse(&chord).map_err(|err| Error::runtime(format!("om.hotkey: {err}")))?;
            let id = format!("hotkey:{}", chord.id());
            let origin = caller(lua).unwrap_or_else(|| "?".to_string());
            let serial = registry
                .lock()
                .insert(id.clone(), TriggerKind::Hotkey(chord), origin, handler)
                .map_err(Error::runtime)?;
            triggers_changed.notify_one();
            Ok(Handle {
                id,
                serial,
                registry: registry.clone(),
                triggers_changed: triggers_changed.clone(),
            })
        })?,
    )?;

    for (name, prefix, kind) in [
        (
            "on_focus",
            "focus",
            TriggerKind::Focus as fn(Matcher) -> TriggerKind,
        ),
        (
            "on_blur",
            "blur",
            TriggerKind::Blur as fn(Matcher) -> TriggerKind,
        ),
    ] {
        let registry = cx.registry.clone();
        let triggers_changed = cx.triggers_changed.clone();
        om.set(
            name,
            lua.create_function(move |lua, (spec, handler): (Table, Function)| {
                let matcher = matcher_from(lua, name, &spec)?;
                let origin = caller(lua).unwrap_or_else(|| "?".to_string());
                let (id, serial) = {
                    let mut registry = registry.lock();
                    let id = registry.fresh_id(prefix);
                    let serial = registry
                        .insert(id.clone(), kind(matcher), origin, handler)
                        .map_err(Error::runtime)?;
                    (id, serial)
                };
                triggers_changed.notify_one();
                Ok(Handle {
                    id,
                    serial,
                    registry: registry.clone(),
                    triggers_changed: triggers_changed.clone(),
                })
            })?,
        )?;
    }

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "every",
        lua.create_function(move |lua, (interval, handler): (String, Function)| {
            let interval = parse_interval(&interval)
                .map_err(|err| Error::runtime(format!("om.every: {err}")))?;
            let origin = caller(lua).unwrap_or_else(|| "?".to_string());
            let (id, serial) = {
                let mut registry = registry.lock();
                let id = registry.fresh_id("every");
                let serial = registry
                    .insert(id.clone(), TriggerKind::Every(interval), origin, handler)
                    .map_err(Error::runtime)?;
                (id, serial)
            };
            triggers_changed.notify_one();
            Ok(Handle {
                id,
                serial,
                registry: registry.clone(),
                triggers_changed: triggers_changed.clone(),
            })
        })?,
    )?;

    om.set(
        "on_typed",
        lua.create_function(|_, _: MultiValue| -> Result<()> {
            Err(Error::runtime(
                "om.on_typed is not available in v1: typed triggers arrive in v2",
            ))
        })?,
    )
}

/// `{class = "...", title = "..."}` with the patterns checked: a bad pattern
/// fails here, at the rule's line, not on the first focus change.
fn matcher_from(lua: &Lua, function: &str, spec: &Table) -> Result<Matcher> {
    let find: Function = lua.globals().get::<Table>("string")?.get("find")?;
    let mut matcher = Matcher::default();
    for (field, slot) in [("class", &mut matcher.class), ("title", &mut matcher.title)] {
        let Some(pattern) = spec.get::<Option<String>>(field)? else {
            continue;
        };
        if let Err(err) = find.call::<()>(("", pattern.as_str())) {
            let reason = match err {
                Error::CallbackError { cause, .. } => cause.to_string(),
                other => other.to_string(),
            };
            let reason = reason.trim_start_matches("runtime error: ");
            return Err(Error::runtime(format!(
                "om.{function}: bad {field} pattern '{pattern}': {reason}"
            )));
        }
        *slot = Some(pattern);
    }
    Ok(matcher)
}

/// What a trigger function returns to the rule.
struct Handle {
    id: String,
    serial: u64,
    registry: SharedRegistry,
    triggers_changed: Arc<Notify>,
}

impl UserData for Handle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Returns whether there was something left to remove.
        methods.add_method("remove", |_, this, ()| {
            let removed = this.registry.lock().remove(&this.id, this.serial);
            if removed {
                this.triggers_changed.notify_one();
            }
            Ok(removed)
        });
    }
}
