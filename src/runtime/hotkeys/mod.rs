//! Keeping Hyprland's binds in step with the hotkeys the rules registered.
//!
//! Hyprland can only remove binds by chord: `hl.unbind` takes every bind on
//! it, the user's own included. So the rules here are strict. A chord that
//! already has somebody else's bind is refused, and a chord is only ever
//! unbound when every bind on it is ours.

use std::collections::BTreeMap;
use std::path::Path;

use crate::backend::shell::quote as shell_quote;
use crate::backend::{BindInfo, Hotkey, Hypr};
use crate::chord::Chord;

/// Our binds carry this at the start of their description. It is how we
/// recognize them in `hyprctl binds`, and what the user sees in a keybinding
/// list.
const OWN_PREFIX: &str = "omaestro: ";

/// A hotkey the rules want bound.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub id: String,
    pub chord: Chord,
    /// Where the rule registered it, `rules.d/foo.lua:12`.
    pub origin: String,
}

/// What a sync could not do.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// Hotkeys that were not bound, as `(id, message for the user)`.
    pub rejected: Vec<(String, String)>,
    /// A failure that is not about one hotkey.
    pub error: Option<String>,
}

pub struct Hotkeys {
    /// The shell command that fires a trigger, without the id.
    trigger_command: String,
    /// Binds this process registered and has not removed, by trigger id.
    applied: BTreeMap<String, Chord>,
}

/// `'<exe>' [--socket '<path>'] trigger`: what Hyprland runs on a chord.
pub fn trigger_command(exe: &Path, socket: Option<&Path>) -> String {
    let mut command = shell_quote(&exe.to_string_lossy());
    if let Some(socket) = socket {
        command.push_str(" --socket ");
        command.push_str(&shell_quote(&socket.to_string_lossy()));
    }
    command.push_str(" trigger");
    command
}

fn is_ours(bind: &BindInfo) -> bool {
    bind.description.starts_with(OWN_PREFIX)
}

/// The binds on `chord` in the global keymap.
fn on_chord<'a>(listing: &'a [BindInfo], chord: &Chord) -> Vec<&'a BindInfo> {
    listing
        .iter()
        .filter(|bind| bind.submap.is_empty() && bind.chord.same_keys(chord))
        .collect()
}

impl Hotkeys {
    pub fn new(trigger_command: String) -> Self {
        Self {
            trigger_command,
            applied: BTreeMap::new(),
        }
    }

    /// Makes Hyprland's binds match `wanted`: removes ours that are no
    /// longer wanted, adds the missing ones. Safe to call at any time; with
    /// nothing to do it does not talk to Hyprland's bind list at all.
    pub async fn sync(&mut self, hypr: &dyn Hypr, wanted: &[Wanted]) -> Report {
        let mut report = Report::default();
        if wanted.is_empty() && self.applied.is_empty() {
            return report;
        }
        let listing = match hypr.binds().await {
            Ok(listing) => listing,
            Err(err) => {
                report.error = Some(format!(
                    "could not read Hyprland's binds, hotkeys are not registered: {err}"
                ));
                return report;
            }
        };

        let unwanted: Vec<String> = self
            .applied
            .keys()
            .filter(|id| !wanted.iter().any(|w| &w.id == *id))
            .cloned()
            .collect();
        for id in unwanted {
            if let Some(chord) = self.applied.remove(&id) {
                remove(hypr, &listing, &chord).await;
            }
        }

        for hotkey in wanted {
            let binds = on_chord(&listing, &hotkey.chord);
            if let Some(foreign) = binds.iter().find(|bind| !is_ours(bind)) {
                self.applied.remove(&hotkey.id);
                let what = if foreign.description.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", foreign.description)
                };
                report.rejected.push((
                    hotkey.id.clone(),
                    format!(
                        "{}: {} is already bound in Hyprland{what}; remove that bind or pick another chord",
                        hotkey.origin, hotkey.chord
                    ),
                ));
                continue;
            }
            let present = !binds.is_empty();
            if present && self.applied.contains_key(&hotkey.id) {
                continue;
            }
            let bind = Hotkey {
                chord: hotkey.chord.clone(),
                command: format!("{} {}", self.trigger_command, shell_quote(&hotkey.id)),
                description: format!("{OWN_PREFIX}{}", hotkey.origin),
            };
            // A bind of ours that this process did not make is a leftover of
            // an earlier daemon; its command may point somewhere else.
            let result = match present {
                true => hypr.unbind(&hotkey.chord).await,
                false => Ok(()),
            };
            let result = match result {
                Ok(()) => hypr.bind(&bind).await,
                Err(err) => Err(err),
            };
            match result {
                Ok(()) => {
                    self.applied.insert(hotkey.id.clone(), hotkey.chord.clone());
                }
                Err(err) => {
                    self.applied.remove(&hotkey.id);
                    report.rejected.push((
                        hotkey.id.clone(),
                        format!("{}: could not bind {}: {err}", hotkey.origin, hotkey.chord),
                    ));
                }
            }
        }
        report
    }

    /// Removes every bind this process registered. For shutdown.
    pub async fn clear(&mut self, hypr: &dyn Hypr) {
        if self.applied.is_empty() {
            return;
        }
        match hypr.binds().await {
            Ok(listing) => {
                for chord in std::mem::take(&mut self.applied).into_values() {
                    remove(hypr, &listing, &chord).await;
                }
            }
            Err(err) => {
                tracing::warn!("could not read Hyprland's binds, leaving ours in place: {err}")
            }
        }
    }
}

/// Unbinds `chord` if, and only if, every bind on it is ours.
async fn remove(hypr: &dyn Hypr, listing: &[BindInfo], chord: &Chord) {
    let binds = on_chord(listing, chord);
    if binds.is_empty() {
        return;
    }
    if binds.iter().any(|bind| !is_ours(bind)) {
        tracing::warn!("{chord} now also has a bind that is not ours; leaving the chord alone");
        return;
    }
    if let Err(err) = hypr.unbind(chord).await {
        tracing::warn!("could not unbind {chord}: {err}");
    }
}

#[cfg(test)]
mod tests;
