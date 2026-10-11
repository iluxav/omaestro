//! `om.tap(key, fn, opts)`: a modifier key pressed and let go on its own,
//! within `opts.within` seconds (0.3 unless the rule says), `opts.count`
//! times in a row (1: a single tap, 2: a double tap), each tap within
//! `within` of the last. The key keeps its job: chords with it work as
//! before, and a press that made one is no tap (`runtime/taps.rs`).

use std::time::Duration;

use mlua::{Error, Function, Lua, Result, Table};

use super::Context;
use super::triggers::{Id, label_from, register};
use crate::chord::Chord;
use crate::runtime::registry::TriggerKind;

/// How long the key may be down, unless the rule says.
const DEFAULT_WITHIN: f64 = 0.3;

/// The keys a rule can tap, by the names it may use, and the key Hyprland
/// knows. A modifier's name alone is its left key.
const KEYS: [(&[&str], &str); 8] = [
    (&["SUPER", "SUPER_L", "WIN", "LOGO"], "Super_L"),
    (&["SUPER_R"], "Super_R"),
    (&["ALT", "ALT_L"], "Alt_L"),
    (&["ALT_R"], "Alt_R"),
    (&["CTRL", "CTRL_L", "CONTROL", "CONTROL_L"], "Control_L"),
    (&["CTRL_R", "CONTROL_R"], "Control_R"),
    (&["SHIFT", "SHIFT_L"], "Shift_L"),
    (&["SHIFT_R"], "Shift_R"),
];

fn key(name: &str) -> Option<&'static str> {
    let name = name.trim();
    KEYS.iter()
        .find(|(names, _)| names.iter().any(|n| n.eq_ignore_ascii_case(name)))
        .map(|(_, key)| *key)
}

fn count_from(options: Option<&Table>) -> Result<u32> {
    let count = match options {
        Some(options) => options.get::<Option<i64>>("count")?.unwrap_or(1),
        None => 1,
    };
    u32::try_from(count)
        .ok()
        .filter(|count| (1..=5).contains(count))
        .ok_or_else(|| {
            Error::runtime(format!(
                "om.tap: count is how many taps in a row, 1 to 5, not {count}"
            ))
        })
}

fn within_from(options: Option<&Table>) -> Result<Duration> {
    let seconds = match options {
        Some(options) => options
            .get::<Option<f64>>("within")?
            .unwrap_or(DEFAULT_WITHIN),
        None => DEFAULT_WITHIN,
    };
    match Duration::try_from_secs_f64(seconds) {
        Ok(within) if !within.is_zero() => Ok(within),
        _ => Err(Error::runtime(format!(
            "om.tap: within is a number of seconds above 0, not {seconds}"
        ))),
    }
}

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let registry = cx.registry.clone();
    let triggers_changed = cx.triggers_changed.clone();
    om.set(
        "tap",
        lua.create_function(
            move |lua, (name, handler, options): (String, Function, Option<Table>)| {
                let Some(key) = key(&name) else {
                    return Err(Error::runtime(format!(
                        "om.tap: '{name}' is not a key to tap; use SUPER, ALT, CTRL or SHIFT \
                         (the left one), or SUPER_R, ALT_R, CTRL_R, SHIFT_R"
                    )));
                };
                let within = within_from(options.as_ref())?;
                let count = count_from(options.as_ref())?;
                let chord = Chord::parse(key).map_err(Error::runtime)?;
                {
                    let registry = registry.lock();
                    if let Some(taken) = registry
                        .chord_taken(&chord)
                        .or_else(|| registry.app_chord_taken(&chord))
                        .filter(|_| registry.tap_taken(&chord).is_none())
                    {
                        return Err(Error::runtime(format!(
                            "om.tap: {key} is already bound at {taken}; a tap cannot share its key"
                        )));
                    }
                }
                let label = label_from("tap", options)?;
                let id = Id::Fixed(format!("tap:{key}"));
                let kind = TriggerKind::Tap {
                    chord,
                    within,
                    count,
                };
                register(lua, &registry, &triggers_changed, id, kind, handler, label)
            },
        )?,
    )
}
