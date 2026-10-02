//! The handlers the rules registered, keyed by trigger id, and which of
//! them the user switched off.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mlua::Function;

use super::timers::format_interval;
use crate::chord::Chord;

#[derive(Debug, Clone, PartialEq)]
pub enum TriggerKind {
    /// `om.trigger("name", fn)`, fired by `om trigger name`.
    Named,
    /// `om.hotkey(chord, fn)`: Hyprland runs `om trigger <id>` on the chord.
    Hotkey(Chord),
    /// `om.app_hotkey(matcher, chord, fn)`: the same, bound only while a
    /// window matching `matcher` has focus.
    AppHotkey { chord: Chord, matcher: Matcher },
    /// `om.on_focus(matcher, fn)`: a matching window got focus.
    Focus(Matcher),
    /// `om.on_blur(matcher, fn)`: a matching window lost focus.
    Blur(Matcher),
    /// `om.on_open(matcher, fn)`: a matching window appeared.
    Open(Matcher),
    /// `om.on_close(matcher, fn)`: a matching window went away.
    Close(Matcher),
    /// `om.on_title(matcher, fn)`: a matching window's title changed.
    Title(Matcher),
    /// `om.on_workspace(fn)`: the active workspace changed.
    Workspace,
    /// `om.on_monitor(fn)`: a monitor was added, removed or focused.
    Monitor,
    /// `om.every(interval, fn)`: on a timer.
    Every(Duration),
    /// `om.after(delay, fn)`: once, then gone.
    After(Duration),
    /// `om.at("HH:MM", fn)`: every day at that time.
    At { hour: u32, minute: u32 },
    /// `om.mode(chord, keys, opts)`: the entry chord, bound to enter `submap`.
    Mode {
        chord: Chord,
        submap: String,
        hint: Option<String>,
    },
    /// One key inside a mode's submap.
    ModeKey {
        submap: String,
        chord: Chord,
        once: bool,
    },
    /// A key that leaves the mode.
    ModeExit { submap: String, chord: Chord },
    /// `om.on_typed(text, fn)`: the user typed `text`.
    Typed(String),
    /// `om.on_clipboard(fn)`: the clipboard changed.
    Clipboard,
    /// `om.on_file(path, fn)`: something under `path` changed.
    File(std::path::PathBuf),
    /// `om.on_sleep`, `om.on_wake`, `om.on_usb`, `om.on_battery`, `om.on_network`.
    System(SystemKind),
}

impl TriggerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerKind::Named => "trigger",
            TriggerKind::Hotkey(_) => "hotkey",
            TriggerKind::AppHotkey { .. } => "app_hotkey",
            TriggerKind::Focus(_) => "on_focus",
            TriggerKind::Blur(_) => "on_blur",
            TriggerKind::Open(_) => "on_open",
            TriggerKind::Close(_) => "on_close",
            TriggerKind::Title(_) => "on_title",
            TriggerKind::Workspace => "on_workspace",
            TriggerKind::Monitor => "on_monitor",
            TriggerKind::Every(_) => "every",
            TriggerKind::After(_) => "after",
            TriggerKind::At { .. } => "at",
            TriggerKind::Mode { .. } => "mode",
            TriggerKind::ModeKey { .. } => "mode_key",
            TriggerKind::ModeExit { .. } => "mode_exit",
            TriggerKind::Typed(_) => "on_typed",
            TriggerKind::Clipboard => "on_clipboard",
            TriggerKind::File(_) => "on_file",
            TriggerKind::System(kind) => kind.as_str(),
        }
    }

    /// What the trigger is about, as the rule wrote it: the chord, the
    /// matcher, the interval, the path. Empty for kinds without a parameter.
    pub fn detail(&self) -> String {
        match self {
            TriggerKind::Named
            | TriggerKind::Workspace
            | TriggerKind::Monitor
            | TriggerKind::Clipboard
            | TriggerKind::System(_) => String::new(),
            TriggerKind::Hotkey(chord)
            | TriggerKind::Mode { chord, .. }
            | TriggerKind::ModeKey { chord, .. }
            | TriggerKind::ModeExit { chord, .. } => chord.to_string(),
            TriggerKind::AppHotkey { chord, matcher } => format!("{chord} in {matcher}"),
            TriggerKind::Focus(m)
            | TriggerKind::Blur(m)
            | TriggerKind::Open(m)
            | TriggerKind::Close(m)
            | TriggerKind::Title(m) => m.to_string(),
            TriggerKind::Every(interval) | TriggerKind::After(interval) => {
                format_interval(*interval)
            }
            TriggerKind::At { hour, minute } => format!("{hour:02}:{minute:02}"),
            TriggerKind::Typed(text) => text.clone(),
            TriggerKind::File(path) => path.display().to_string(),
        }
    }

    /// The id a trigger of this kind gets, before any `#n` that keeps two
    /// alike ones apart: `on_focus:class=firefox`, `every:5m`, `on_clipboard`.
    pub fn base_id(&self) -> String {
        let detail = self.detail();
        if detail.is_empty() {
            self.as_str().to_string()
        } else {
            format!("{}:{detail}", self.as_str())
        }
    }
}

/// The system events a rule can listen to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKind {
    Sleep,
    Wake,
    Usb,
    Battery,
    Network,
}

impl SystemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SystemKind::Sleep => "on_sleep",
            SystemKind::Wake => "on_wake",
            SystemKind::Usb => "on_usb",
            SystemKind::Battery => "on_battery",
            SystemKind::Network => "on_network",
        }
    }

    pub fn source(self) -> crate::backend::SystemSource {
        use crate::backend::SystemSource;
        match self {
            SystemKind::Sleep | SystemKind::Wake => SystemSource::Login1,
            SystemKind::Usb => SystemSource::Usb,
            SystemKind::Battery => SystemSource::Battery,
            SystemKind::Network => SystemSource::Network,
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

impl std::fmt::Display for Matcher {
    /// `class=firefox`, `class=firefox,title=Inbox`, or `*` for any window.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<String> = [("class", &self.class), ("title", &self.title)]
            .into_iter()
            .filter_map(|(name, pattern)| pattern.as_ref().map(|p| format!("{name}={p}")))
            .collect();
        if parts.is_empty() {
            f.write_str("*")
        } else {
            f.write_str(&parts.join(","))
        }
    }
}

#[derive(Clone)]
pub struct Trigger {
    pub id: String,
    /// Distinguishes this registration from a later one under the same id.
    pub serial: u64,
    pub kind: TriggerKind,
    /// Where the rule registered it, `rules.d/foo.lua:12`.
    pub origin: String,
    /// What the rule calls it (`{label = ...}`), for the panel and `om list`.
    pub label: Option<String>,
    pub handler: Function,
    /// Held while the handler runs: one run at a time per trigger.
    pub gate: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Default)]
pub struct Registry {
    triggers: BTreeMap<String, Trigger>,
    next_serial: u64,
    /// Ids the user switched off (`om disable`). Kept whether or not a
    /// trigger with the id is registered right now: the rule may be back
    /// after the next reload.
    disabled: BTreeSet<String>,
}

impl Registry {
    /// Starts with the ids the user had switched off.
    pub fn with_disabled(disabled: BTreeSet<String>) -> Self {
        Self {
            disabled,
            ..Self::default()
        }
    }

    /// Switches a trigger on or off. Returns whether anything changed.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if enabled {
            self.disabled.remove(id)
        } else {
            self.disabled.insert(id.to_string())
        }
    }

    /// Whether a trigger acts right now: not switched off, and for a key
    /// inside a mode, the mode not switched off either.
    pub fn is_enabled(&self, trigger: &Trigger) -> bool {
        if self.disabled.contains(&trigger.id) {
            return false;
        }
        match &trigger.kind {
            TriggerKind::ModeKey { submap, .. } | TriggerKind::ModeExit { submap, .. } => {
                self.triggers.values().any(|t| {
                    matches!(&t.kind, TriggerKind::Mode { submap: s, .. } if s == submap)
                        && !self.disabled.contains(&t.id)
                })
            }
            _ => true,
        }
    }

    /// The triggers that act right now, in id order.
    pub fn enabled(&self) -> impl Iterator<Item = &Trigger> {
        self.triggers.values().filter(|t| self.is_enabled(t))
    }

    /// `base` if no trigger has that id yet, else `base#2`, `base#3`, ...:
    /// an id that stays the same across reloads as long as the rule does.
    pub fn unique_id(&self, base: &str) -> String {
        if !self.triggers.contains_key(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}#{n}"))
            .find(|id| !self.triggers.contains_key(id))
            .unwrap_or_default()
    }

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
                TriggerKind::AppHotkey { chord, matcher } => {
                    format!("app hotkey {chord} for {matcher}")
                }
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
            label: None,
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

    /// Where a global chord is already used by a hotkey or a mode, if it is.
    pub fn chord_taken(&self, chord: &Chord) -> Option<String> {
        self.triggers.values().find_map(|t| match &t.kind {
            TriggerKind::Hotkey(c) | TriggerKind::Mode { chord: c, .. } if c.same_keys(chord) => {
                Some(t.origin.clone())
            }
            _ => None,
        })
    }

    /// Where an app hotkey already uses the chord, if one does. An app
    /// hotkey and a global bind cannot share a chord: the global one would
    /// always win, or be taken for a leftover and replaced.
    pub fn app_chord_taken(&self, chord: &Chord) -> Option<String> {
        self.triggers.values().find_map(|t| match &t.kind {
            TriggerKind::AppHotkey { chord: c, .. } if c.same_keys(chord) => Some(t.origin.clone()),
            _ => None,
        })
    }

    /// The trigger with this id, switched on or off.
    /// Names the registration `serial` of `id`.
    pub fn set_label(&mut self, id: &str, serial: u64, label: Option<String>) {
        if let Some(trigger) = self.triggers.get_mut(id)
            && trigger.serial == serial
        {
            trigger.label = label;
        }
    }

    pub fn get(&self, id: &str) -> Option<Trigger> {
        self.triggers.get(id).cloned()
    }

    /// Every trigger, switched on or off, in id order.
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
    pub fn new(registry: Registry) -> Self {
        Self(Arc::new(Mutex::new(registry)))
    }

    pub fn lock(&self) -> MutexGuard<'_, Registry> {
        // A panic while holding the lock cannot leave the map half-updated.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
