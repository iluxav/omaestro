//! `om.mode(chord, keys, opts)`: a leader chord opens a mode in which single
//! keys run handlers, until Escape. Built on Hyprland submaps: the entry
//! chord is a bind that enters the submap, each key is a bind inside it
//! that fires a trigger, and Escape (plus any `opts.exit` keys) leaves it.

use mlua::{Error, Function, Lua, Result, Table, Value};

use super::triggers::Handle;
use super::{Context, caller};
use crate::chord::Chord;
use crate::runtime::registry::TriggerKind;

/// The submap a mode lives in, from its chord: `om-super+alt+w`.
pub fn submap_name(chord: &Chord) -> String {
    format!("om-{}", chord.id().to_lowercase())
}

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "mode",
        lua.create_function(
            move |lua, (chord, keys, options): (String, Table, Option<Table>)| {
                let chord = Chord::parse(&chord)
                    .map_err(|err| Error::runtime(format!("om.mode: {err}")))?;
                let origin = caller(lua).unwrap_or_else(|| "?".to_string());
                let (hint, once, exits) = match &options {
                    Some(options) => (
                        options.get::<Option<String>>("hint")?,
                        options.get::<Option<bool>>("once")?.unwrap_or(false),
                        options
                            .get::<Option<Vec<String>>>("exit")?
                            .unwrap_or_default(),
                    ),
                    None => (None, false, Vec::new()),
                };

                // Every key first, so a bad one fails before anything is registered.
                let mut bound = Vec::new();
                for pair in keys.pairs::<Value, Function>() {
                    let (key, handler) = pair?;
                    let Value::String(key) = key else {
                        return Err(Error::runtime(format!(
                            "om.mode: keys are chords like \"h\" or \"SHIFT + h\", not {}",
                            key.type_name()
                        )));
                    };
                    let key = key.to_string_lossy();
                    let key_chord = Chord::parse(&key)
                        .map_err(|err| Error::runtime(format!("om.mode: key {err}")))?;
                    bound.push((key_chord, handler));
                }
                if bound.is_empty() {
                    return Err(Error::runtime("om.mode: no keys"));
                }
                let mut exit_chords = vec![Chord::parse("Escape").map_err(Error::runtime)?];
                for exit in exits {
                    exit_chords.push(
                        Chord::parse(&exit)
                            .map_err(|err| Error::runtime(format!("om.mode: exit {err}")))?,
                    );
                }

                let submap = submap_name(&chord);
                let mode_id = format!("mode:{}", chord.id());
                let noop: Function = lua.create_function(|_, ()| Ok(()))?;
                let mut entries = Vec::new();
                {
                    let mut registry = registry.lock();
                    if let Some(taken) = registry
                        .chord_taken(&chord)
                        .or_else(|| registry.app_chord_taken(&chord))
                    {
                        return Err(Error::runtime(format!(
                            "om.mode: {chord} is already registered at {taken}"
                        )));
                    }
                    let serial = registry
                        .insert(
                            mode_id.clone(),
                            TriggerKind::Mode {
                                chord: chord.clone(),
                                submap: submap.clone(),
                                hint,
                            },
                            origin.clone(),
                            noop.clone(),
                        )
                        .map_err(Error::runtime)?;
                    entries.push((mode_id.clone(), serial));
                    for (key_chord, handler) in bound {
                        let id = format!("{mode_id}/{}", key_chord.id());
                        let serial = registry
                            .insert(
                                id.clone(),
                                TriggerKind::ModeKey {
                                    submap: submap.clone(),
                                    chord: key_chord,
                                    once,
                                },
                                origin.clone(),
                                handler,
                            )
                            .map_err(Error::runtime)?;
                        entries.push((id, serial));
                    }
                    for exit in exit_chords {
                        let id = format!("{mode_id}/exit:{}", exit.id());
                        let serial = registry
                            .insert(
                                id.clone(),
                                TriggerKind::ModeExit {
                                    submap: submap.clone(),
                                    chord: exit,
                                },
                                origin.clone(),
                                noop.clone(),
                            )
                            .map_err(Error::runtime)?;
                        entries.push((id, serial));
                    }
                }
                triggers_changed.notify_one();
                Ok(Handle::new(
                    entries,
                    registry.clone(),
                    triggers_changed.clone(),
                ))
            },
        )?,
    )
}
