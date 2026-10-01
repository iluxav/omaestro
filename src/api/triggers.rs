//! Triggers: functions that register a handler and return a handle with
//! `:remove()`.

use std::sync::Arc;

use mlua::{Error, Function, Lua, Result, Table, UserData, UserDataMethods, Value};
use tokio::sync::Notify;

use super::{Context, caller};
use crate::chord::Chord;
use crate::runtime::registry::{Matcher, SharedRegistry, SystemKind, TriggerKind};
use crate::runtime::timers::{parse_clock, parse_interval};
use crate::runtime::typed::check_text;

/// How a trigger gets its id.
enum Id {
    /// The rule named it (`om.trigger`), or its parameter is its name (a
    /// hotkey's chord, a typed text): a second one with the same id is a
    /// conflict the rule hears about.
    Fixed(String),
    /// From the kind and its detail; a second alike one gets `#2`.
    Derived,
}

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "trigger",
        lua.create_function(move |lua, (name, handler): (String, Function)| {
            if name.is_empty() {
                return Err(Error::runtime("om.trigger: the name is empty"));
            }
            register(
                lua,
                &registry,
                &triggers_changed,
                Id::Fixed(name),
                TriggerKind::Named,
                handler,
            )
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
            if let Some(taken) = registry.lock().app_chord_taken(&chord) {
                return Err(Error::runtime(format!(
                    "om.hotkey: {chord} is an app hotkey at {taken}; a global hotkey cannot share its chord"
                )));
            }
            let id = Id::Fixed(format!("hotkey:{}", chord.id()));
            let kind = TriggerKind::Hotkey(chord);
            register(lua, &registry, &triggers_changed, id, kind, handler)
        })?,
    )?;

    for (name, kind) in [
        ("on_focus", TriggerKind::Focus as fn(Matcher) -> TriggerKind),
        ("on_blur", TriggerKind::Blur as fn(Matcher) -> TriggerKind),
        ("on_open", TriggerKind::Open as fn(Matcher) -> TriggerKind),
        ("on_close", TriggerKind::Close as fn(Matcher) -> TriggerKind),
        ("on_title", TriggerKind::Title as fn(Matcher) -> TriggerKind),
    ] {
        let registry = cx.registry.clone();
        let triggers_changed = cx.triggers_changed.clone();
        om.set(
            name,
            lua.create_function(move |lua, (spec, handler): (Table, Function)| {
                let matcher = matcher_from(lua, name, &spec)?;
                let kind = kind(matcher);
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Derived,
                    kind,
                    handler,
                )
            })?,
        )?;
    }

    for (name, kind) in [
        ("on_workspace", TriggerKind::Workspace),
        ("on_monitor", TriggerKind::Monitor),
        ("on_clipboard", TriggerKind::Clipboard),
        ("on_sleep", TriggerKind::System(SystemKind::Sleep)),
        ("on_wake", TriggerKind::System(SystemKind::Wake)),
        ("on_usb", TriggerKind::System(SystemKind::Usb)),
        ("on_battery", TriggerKind::System(SystemKind::Battery)),
        ("on_network", TriggerKind::System(SystemKind::Network)),
    ] {
        let registry = cx.registry.clone();
        let triggers_changed = cx.triggers_changed.clone();
        om.set(
            name,
            lua.create_function(move |lua, handler: Function| {
                let kind = kind.clone();
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Derived,
                    kind,
                    handler,
                )
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
            let kind = TriggerKind::Every(interval);
            register(
                lua,
                &registry,
                &triggers_changed,
                Id::Derived,
                kind,
                handler,
            )
        })?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "after",
        lua.create_function(move |lua, (delay, handler): (String, Function)| {
            let delay =
                parse_interval(&delay).map_err(|err| Error::runtime(format!("om.after: {err}")))?;
            let kind = TriggerKind::After(delay);
            register(
                lua,
                &registry,
                &triggers_changed,
                Id::Derived,
                kind,
                handler,
            )
        })?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "at",
        lua.create_function(move |lua, (clock, handler): (String, Function)| {
            let (hour, minute) =
                parse_clock(&clock).map_err(|err| Error::runtime(format!("om.at: {err}")))?;
            let kind = TriggerKind::At { hour, minute };
            register(
                lua,
                &registry,
                &triggers_changed,
                Id::Derived,
                kind,
                handler,
            )
        })?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "on_file",
        lua.create_function(move |lua, (path, handler): (String, Function)| {
            if path.trim().is_empty() {
                return Err(Error::runtime("om.on_file: the path is empty"));
            }
            let kind = TriggerKind::File(path.into());
            register(
                lua,
                &registry,
                &triggers_changed,
                Id::Derived,
                kind,
                handler,
            )
        })?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "on_typed",
        lua.create_function(move |lua, (text, handler): (String, Function)| {
            check_text(&text).map_err(|err| Error::runtime(format!("om.on_typed: {err}")))?;
            let id = Id::Fixed(format!("on_typed:{text}"));
            let kind = TriggerKind::Typed(text);
            register(lua, &registry, &triggers_changed, id, kind, handler)
        })?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "app_hotkey",
        lua.create_function(
            move |lua, (what, chord, handler): (Value, String, Function)| {
                let matcher = match &what {
                    Value::String(class) => {
                        let spec = lua.create_table()?;
                        spec.set("class", class.clone())?;
                        matcher_from(lua, "app_hotkey", &spec)?
                    }
                    Value::Table(spec) => matcher_from(lua, "app_hotkey", spec)?,
                    other => {
                        return Err(Error::runtime(format!(
                            "om.app_hotkey: a class pattern or a table {{class=, title=}}, not {}",
                            other.type_name()
                        )));
                    }
                };
                let chord = Chord::parse(&chord)
                    .map_err(|err| Error::runtime(format!("om.app_hotkey: {err}")))?;
                if let Some(taken) = registry.lock().chord_taken(&chord) {
                    return Err(Error::runtime(format!(
                        "om.app_hotkey: {chord} is a global hotkey at {taken}; an app hotkey cannot share its chord"
                    )));
                }
                let id = Id::Fixed(format!("app_hotkey:{}:{matcher}", chord.id()));
                let kind = TriggerKind::AppHotkey { chord, matcher };
                register(lua, &registry, &triggers_changed, id, kind, handler)
            },
        )?,
    )
}

/// Puts the trigger in the registry under the calling rule's position and
/// hands the rule its handle.
fn register(
    lua: &Lua,
    registry: &SharedRegistry,
    triggers_changed: &Arc<Notify>,
    id: Id,
    kind: TriggerKind,
    handler: Function,
) -> Result<Handle> {
    let origin = caller(lua).unwrap_or_else(|| "?".to_string());
    let (id, serial) = {
        let mut registry = registry.lock();
        let id = match id {
            Id::Fixed(id) => id,
            Id::Derived => registry.unique_id(&kind.base_id()),
        };
        let serial = registry
            .insert(id.clone(), kind, origin, handler)
            .map_err(Error::runtime)?;
        (id, serial)
    };
    triggers_changed.notify_one();
    Ok(Handle::new(
        vec![(id, serial)],
        registry.clone(),
        triggers_changed.clone(),
    ))
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

/// What a trigger function returns to the rule. A mode's handle covers its
/// entry and every key.
pub struct Handle {
    entries: Vec<(String, u64)>,
    registry: SharedRegistry,
    triggers_changed: Arc<Notify>,
}

impl Handle {
    pub fn new(
        entries: Vec<(String, u64)>,
        registry: SharedRegistry,
        triggers_changed: Arc<Notify>,
    ) -> Self {
        Self {
            entries,
            registry,
            triggers_changed,
        }
    }
}

impl UserData for Handle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Returns whether there was something left to remove. `cancel` is the
        // same thing, for timers.
        for name in ["remove", "cancel"] {
            methods.add_method(name, |_, this, ()| {
                let mut removed = false;
                {
                    let mut registry = this.registry.lock();
                    for (id, serial) in &this.entries {
                        removed |= registry.remove(id, *serial);
                    }
                }
                if removed {
                    this.triggers_changed.notify_one();
                }
                Ok(removed)
            });
        }
    }
}
