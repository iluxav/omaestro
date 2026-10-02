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
        lua.create_function(
            move |lua, (name, handler, options): (String, Function, Option<Table>)| {
                if name.is_empty() {
                    return Err(Error::runtime("om.trigger: the name is empty"));
                }
                let label = label_from("trigger", options)?;
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Fixed(name),
                    TriggerKind::Named,
                    handler,
                    label,
                )
            },
        )?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "hotkey",
        lua.create_function(
            move |lua, (chord, handler, options): (String, Function, Option<Table>)| {
                // Only the registration happens here. The runtime binds the chord
                // in Hyprland afterwards, once the rules have loaded.
                let chord = Chord::parse(&chord)
                    .map_err(|err| Error::runtime(format!("om.hotkey: {err}")))?;
                if let Some(taken) = registry.lock().app_chord_taken(&chord) {
                    return Err(Error::runtime(format!(
                        "om.hotkey: {chord} is an app hotkey at {taken}; a global hotkey cannot share its chord"
                    )));
                }
                let mut label = label_from("hotkey", options)?;
                // init.lua's `om.hotkey(chord, om.panel)` says what it is.
                if label.is_none() && is_panel(lua, &handler) {
                    label = Some(PANEL_LABEL.to_string());
                }
                let id = Id::Fixed(format!("hotkey:{}", chord.id()));
                let kind = TriggerKind::Hotkey(chord);
                register(lua, &registry, &triggers_changed, id, kind, handler, label)
            },
        )?,
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
            lua.create_function(
                move |lua, (spec, handler, options): (Table, Function, Option<Table>)| {
                    let matcher = matcher_from(lua, name, &spec)?;
                    let kind = kind(matcher);
                    let label = label_from(name, options)?;
                    register(
                        lua,
                        &registry,
                        &triggers_changed,
                        Id::Derived,
                        kind,
                        handler,
                        label,
                    )
                },
            )?,
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
            lua.create_function(move |lua, (handler, options): (Function, Option<Table>)| {
                let kind = kind.clone();
                let label = label_from(name, options)?;
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Derived,
                    kind,
                    handler,
                    label,
                )
            })?,
        )?;
    }

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "every",
        lua.create_function(
            move |lua, (interval, handler, options): (String, Function, Option<Table>)| {
                let interval = parse_interval(&interval)
                    .map_err(|err| Error::runtime(format!("om.every: {err}")))?;
                let kind = TriggerKind::Every(interval);
                let label = label_from("every", options)?;
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Derived,
                    kind,
                    handler,
                    label,
                )
            },
        )?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "after",
        lua.create_function(
            move |lua, (delay, handler, options): (String, Function, Option<Table>)| {
                let delay = parse_interval(&delay)
                    .map_err(|err| Error::runtime(format!("om.after: {err}")))?;
                let kind = TriggerKind::After(delay);
                let label = label_from("after", options)?;
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Derived,
                    kind,
                    handler,
                    label,
                )
            },
        )?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "at",
        lua.create_function(
            move |lua, (clock, handler, options): (String, Function, Option<Table>)| {
                let (hour, minute) =
                    parse_clock(&clock).map_err(|err| Error::runtime(format!("om.at: {err}")))?;
                let kind = TriggerKind::At { hour, minute };
                let label = label_from("at", options)?;
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Derived,
                    kind,
                    handler,
                    label,
                )
            },
        )?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "on_file",
        lua.create_function(
            move |lua, (path, handler, options): (String, Function, Option<Table>)| {
                if path.trim().is_empty() {
                    return Err(Error::runtime("om.on_file: the path is empty"));
                }
                let kind = TriggerKind::File(path.into());
                let label = label_from("on_file", options)?;
                register(
                    lua,
                    &registry,
                    &triggers_changed,
                    Id::Derived,
                    kind,
                    handler,
                    label,
                )
            },
        )?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "on_typed",
        lua.create_function(
            move |lua, (text, handler, options): (String, Function, Option<Table>)| {
                check_text(&text).map_err(|err| Error::runtime(format!("om.on_typed: {err}")))?;
                let label = label_from("on_typed", options)?;
                let id = Id::Fixed(format!("on_typed:{text}"));
                let kind = TriggerKind::Typed(text);
                register(lua, &registry, &triggers_changed, id, kind, handler, label)
            },
        )?,
    )?;

    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "app_hotkey",
        lua.create_function(
            move |lua,
                  (what, chord, handler, options): (Value, String, Function, Option<Table>)| {
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
                let label = label_from("app_hotkey", options)?;
                let id = Id::Fixed(format!("app_hotkey:{}:{matcher}", chord.id()));
                let kind = TriggerKind::AppHotkey { chord, matcher };
                register(lua, &registry, &triggers_changed, id, kind, handler, label)
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
    label: Option<String>,
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
        registry.set_label(&id, serial, label);
        (id, serial)
    };
    triggers_changed.notify_one();
    Ok(Handle::new(
        vec![(id, serial)],
        registry.clone(),
        triggers_changed.clone(),
    ))
}

/// What `om.hotkey(chord, om.panel)` is called when the rule names nothing.
const PANEL_LABEL: &str = "omaestro menu";

/// `{label = "..."}`, the last argument every trigger takes: what the panel
/// and `om list` call the rule.
pub fn label_from(function: &str, options: Option<Table>) -> Result<Option<String>> {
    let Some(options) = options else {
        return Ok(None);
    };
    match options.get::<Value>("label")? {
        Value::Nil => Ok(None),
        Value::String(text) => {
            let text = text.to_string_lossy().trim().to_string();
            Ok((!text.is_empty()).then_some(text))
        }
        other => Err(Error::runtime(format!(
            "om.{function}: label is text, not {}",
            other.type_name()
        ))),
    }
}

/// Whether `handler` is the prelude's `om.panel` itself.
fn is_panel(lua: &Lua, handler: &Function) -> bool {
    lua.globals()
        .get::<Table>("om")
        .and_then(|om| om.get::<Option<Function>>("panel"))
        .ok()
        .flatten()
        .is_some_and(|panel| &panel == handler)
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
