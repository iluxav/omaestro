//! The handlers the rules registered, keyed by trigger id.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mlua::Function;

use crate::chord::Chord;

#[derive(Debug, Clone, PartialEq)]
pub enum TriggerKind {
    /// `om.trigger("name", fn)`, fired by `om trigger name`.
    Named,
    /// `om.hotkey(chord, fn)`: Hyprland runs `om trigger <id>` on the chord.
    Hotkey(Chord),
    /// `om.on_focus(matcher, fn)`: a matching window got focus.
    Focus(Matcher),
    /// `om.on_blur(matcher, fn)`: a matching window lost focus.
    Blur(Matcher),
    /// `om.every(interval, fn)`: on a timer.
    Every(Duration),
}

impl TriggerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerKind::Named => "trigger",
            TriggerKind::Hotkey(_) => "hotkey",
            TriggerKind::Focus(_) => "on_focus",
            TriggerKind::Blur(_) => "on_blur",
            TriggerKind::Every(_) => "every",
        }
    }
}

/// Which windows a focus trigger is about: Lua patterns, all of which must
/// match. No patterns means every window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Matcher {
    pub class: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone)]
pub struct Trigger {
    pub id: String,
    /// Distinguishes this registration from a later one under the same id.
    pub serial: u64,
    pub kind: TriggerKind,
    /// Where the rule registered it, `rules.d/foo.lua:12`.
    pub origin: String,
    pub handler: Function,
    /// Held while the handler runs: one run at a time per trigger.
    pub gate: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Default)]
pub struct Registry {
    triggers: BTreeMap<String, Trigger>,
    next_serial: u64,
}

impl Registry {
    /// Adds a trigger and returns its serial. An id that is already taken is
    /// refused, and the error names where the first one came from.
    pub fn insert(
        &mut self,
        id: String,
        kind: TriggerKind,
        origin: String,
        handler: Function,
    ) -> Result<u64, String> {
        if let Some(existing) = self.triggers.get(&id) {
            let what = match &existing.kind {
                TriggerKind::Named => format!("trigger '{}'", existing.id),
                TriggerKind::Hotkey(chord) => format!("hotkey {chord}"),
                other => format!("{} '{}'", other.as_str(), existing.id),
            };
            return Err(format!(
                "{what} is already registered at {}",
                existing.origin
            ));
        }
        let serial = self.next_serial;
        self.next_serial += 1;
        let trigger = Trigger {
            id: id.clone(),
            serial,
            kind,
            origin,
            handler,
            gate: Arc::default(),
        };
        self.triggers.insert(id, trigger);
        Ok(serial)
    }

    /// Removes the registration a handle points at. A stale handle (the id
    /// was removed and registered again since) removes nothing.
    pub fn remove(&mut self, id: &str, serial: u64) -> bool {
        match self.triggers.get(id) {
            Some(trigger) if trigger.serial == serial => self.triggers.remove(id).is_some(),
            _ => false,
        }
    }

    /// Drops a trigger whatever its serial. For the runtime, when a hotkey
    /// could not be bound.
    pub fn discard(&mut self, id: &str) {
        self.triggers.remove(id);
    }

    /// An id no rule can collide with, for triggers without a natural name.
    pub fn fresh_id(&self, prefix: &str) -> String {
        format!("{prefix}:{}", self.next_serial + 1)
    }

    pub fn get(&self, id: &str) -> Option<Trigger> {
        self.triggers.get(id).cloned()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Trigger> {
        self.triggers.values()
    }

    pub fn len(&self) -> usize {
        self.triggers.len()
    }
}

/// Shared between the runtime and the API closures living inside the Lua state.
#[derive(Clone, Default)]
pub struct SharedRegistry(Arc<Mutex<Registry>>);

impl SharedRegistry {
    pub fn lock(&self) -> MutexGuard<'_, Registry> {
        // A panic while holding the lock cannot leave the map half-updated.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
