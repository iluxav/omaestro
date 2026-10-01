//! Keeping Hyprland's binds in step with the hotkeys and modes the rules
//! registered.
//!
//! Hyprland runs every bind on a chord, and can only remove binds by chord:
//! `hl.unbind` takes every bind on it, the user's own included. So the rules
//! here are strict. A chord that already has somebody else's bind is refused
//! (the hotkey stays known, with the reason, and is bound as soon as the
//! chord is free), and a chord is only ever unbound when every bind on it is
//! ours. Binds inside a submap (a mode) follow the same rules within that
//! submap.
//!
//! With `override_binds` on (`om override on`) the first rule is dropped: a
//! wanted chord is taken, whatever is on it. The displaced bind cannot be
//! recreated by us (with the Lua config every bind is a closure inside
//! Hyprland), so when it should come back the caller reloads Hyprland's
//! config, which also drops our binds; the sync after that reload makes ours
//! again.

use std::collections::BTreeMap;
use std::path::Path;

use crate::backend::shell::quote as shell_quote;
use crate::backend::{BindAction, BindInfo, Hotkey, Hypr};
use crate::chord::Chord;

/// Our binds carry this at the start of their description. It is how we
/// recognize them in `hyprctl binds`, and what the user sees in a keybinding
/// list.
const OWN_PREFIX: &str = "omaestro: ";

/// What a bind does when pressed.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Run `om trigger <id>`.
    Trigger,
    /// Enter a submap (`"reset"` leaves it).
    Submap(String),
}

/// A bind the rules want.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub id: String,
    pub chord: Chord,
    /// Where the rule registered it, `rules.d/foo.lua:12`.
    pub origin: String,
    /// The submap the bind lives in; empty for the global keymap.
    pub submap: String,
    pub action: Action,
}

/// What a sync did that the user should hear about.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// Hotkeys newly left without a bind, as `(id, message for the user)`.
    /// A hotkey refused for the same reason as last time is not repeated.
    pub rejected: Vec<(String, String)>,
    /// Chords newly taken from somebody else's bind, as `(id, message)`.
    pub taken: Vec<(String, String)>,
    /// A displaced bind should come back: Hyprland's config needs a reload.
    pub restore: bool,
    /// A failure that is not about one bind.
    pub error: Option<String>,
}

pub struct Hotkeys {
    /// The shell command that fires a trigger, without the id.
    trigger_command: String,
    /// Binds this process registered and has not removed, by trigger id:
    /// the chord and the submap it is in.
    applied: BTreeMap<String, (Chord, String)>,
    /// Hotkeys without a bind right now, by id, with the reason: a chord
    /// somebody else holds, or a key Hyprland would not take.
    refused: BTreeMap<String, String>,
    /// The binds our hotkeys displaced, by id, as the description of what
    /// was there. Only ever filled while `override_binds` is on.
    displaced: BTreeMap<String, String>,
    override_binds: bool,
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

/// The binds on `chord` in `submap` (empty: the global keymap).
fn on_chord<'a>(listing: &'a [BindInfo], chord: &Chord, submap: &str) -> Vec<&'a BindInfo> {
    listing
        .iter()
        .filter(|bind| bind.submap == submap && bind.chord.same_keys(chord))
        .collect()
}

fn where_it_is(chord: &Chord, submap: &str) -> String {
    if submap.is_empty() {
        chord.to_string()
    } else {
        format!("{chord} in mode {submap}")
    }
}

/// How a bind that is not ours is named to the user.
fn what_is(bind: &BindInfo) -> String {
    if bind.description.is_empty() {
        "a bind without a description".to_string()
    } else {
        bind.description.clone()
    }
}

impl Hotkeys {
    pub fn new(trigger_command: String) -> Self {
        Self {
            trigger_command,
            applied: BTreeMap::new(),
            refused: BTreeMap::new(),
            displaced: BTreeMap::new(),
            override_binds: false,
        }
    }

    /// Hotkeys without a bind, by id, with the reason.
    pub fn refused(&self) -> &BTreeMap<String, String> {
        &self.refused
    }

    /// What our hotkeys displaced, by id.
    pub fn displaced(&self) -> &BTreeMap<String, String> {
        &self.displaced
    }

    /// Whether this process holds a bind for the trigger right now.
    pub fn is_applied(&self, id: &str) -> bool {
        self.applied.contains_key(id)
    }

    /// Switches override on or off. Returns whether displaced binds should
    /// come back now (a config reload); after it, the next sync refuses
    /// those chords again.
    pub fn set_override(&mut self, on: bool) -> bool {
        self.override_binds = on;
        if on {
            return false;
        }
        let had_displaced = !self.displaced.is_empty();
        self.displaced.clear();
        had_displaced
    }

    /// Makes Hyprland's binds match `wanted`: removes ours that are no
    /// longer wanted, adds the missing ones. Safe to call at any time; with
    /// nothing to do it does not talk to Hyprland's bind list at all.
    pub async fn sync(&mut self, hypr: &dyn Hypr, wanted: &[Wanted]) -> Report {
        let mut report = Report::default();
        if wanted.is_empty() && self.applied.is_empty() && self.refused.is_empty() {
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

        self.refused
            .retain(|id, _| wanted.iter().any(|w| &w.id == id));
        let unwanted: Vec<String> = self
            .applied
            .keys()
            .filter(|id| !wanted.iter().any(|w| &w.id == *id))
            .cloned()
            .collect();
        for id in unwanted {
            if let Some((chord, submap)) = self.applied.remove(&id) {
                remove(hypr, &listing, &chord, &submap).await;
                if self.displaced.remove(&id).is_some() {
                    report.restore = true;
                }
            }
        }

        for hotkey in wanted {
            let binds = on_chord(&listing, &hotkey.chord, &hotkey.submap);
            let foreign = binds.iter().find(|bind| !is_ours(bind)).copied();
            if let Some(foreign) = foreign
                && !self.override_binds
            {
                self.applied.remove(&hotkey.id);
                self.refuse(
                    &mut report,
                    hotkey,
                    format!(
                        "{} is already bound in Hyprland ({}); remove that bind, pick another chord, or `om override on`",
                        where_it_is(&hotkey.chord, &hotkey.submap),
                        what_is(foreign)
                    ),
                );
                continue;
            }
            let present = !binds.is_empty();
            if present && foreign.is_none() && self.applied.contains_key(&hotkey.id) {
                continue;
            }
            let bind = Hotkey {
                chord: hotkey.chord.clone(),
                action: match &hotkey.action {
                    Action::Trigger => BindAction::Exec(format!(
                        "{} {}",
                        self.trigger_command,
                        shell_quote(&hotkey.id)
                    )),
                    Action::Submap(name) => BindAction::Submap(name.clone()),
                },
                description: format!("{OWN_PREFIX}{}", hotkey.origin),
                submap: hotkey.submap.clone(),
            };
            // What is on the chord goes: a bind of ours that this process did
            // not make is a leftover of an earlier daemon (its command may
            // point somewhere else), and anything else is being overridden.
            let result = match present {
                true => hypr.unbind(&hotkey.chord, &hotkey.submap).await,
                false => Ok(()),
            };
            let result = match result {
                Ok(()) => hypr.bind(&bind).await,
                Err(err) => Err(err),
            };
            match result {
                Ok(()) => {
                    self.applied.insert(
                        hotkey.id.clone(),
                        (hotkey.chord.clone(), hotkey.submap.clone()),
                    );
                    self.refused.remove(&hotkey.id);
                    if let Some(foreign) = foreign {
                        let what = what_is(foreign);
                        // Told once: after a Hyprland reload the same bind is
                        // taken again quietly.
                        if self
                            .displaced
                            .insert(hotkey.id.clone(), what.clone())
                            .is_none()
                        {
                            report.taken.push((
                                hotkey.id.clone(),
                                format!(
                                    "{} now runs {} instead of {what}",
                                    where_it_is(&hotkey.chord, &hotkey.submap),
                                    hotkey.origin
                                ),
                            ));
                        }
                    }
                }
                Err(err) => {
                    self.applied.remove(&hotkey.id);
                    self.refuse(
                        &mut report,
                        hotkey,
                        format!(
                            "could not bind {}: {err}",
                            where_it_is(&hotkey.chord, &hotkey.submap)
                        ),
                    );
                }
            }
        }
        report
    }

    /// Notes a hotkey without a bind; reported when the reason is new.
    fn refuse(&mut self, report: &mut Report, hotkey: &Wanted, reason: String) {
        let before = self.refused.insert(hotkey.id.clone(), reason.clone());
        if before.as_deref() != Some(reason.as_str()) {
            report
                .rejected
                .push((hotkey.id.clone(), format!("{}: {reason}", hotkey.origin)));
        }
    }

    /// Removes every bind this process registered. For shutdown. Returns
    /// whether displaced binds should come back (a config reload).
    pub async fn clear(&mut self, hypr: &dyn Hypr) -> bool {
        if self.applied.is_empty() {
            return false;
        }
        match hypr.binds().await {
            Ok(listing) => {
                for (chord, submap) in std::mem::take(&mut self.applied).into_values() {
                    remove(hypr, &listing, &chord, &submap).await;
                }
                !std::mem::take(&mut self.displaced).is_empty()
            }
            Err(err) => {
                tracing::warn!("could not read Hyprland's binds, leaving ours in place: {err}");
                false
            }
        }
    }
}

/// Unbinds `chord` in `submap` if, and only if, every bind on it is ours.
async fn remove(hypr: &dyn Hypr, listing: &[BindInfo], chord: &Chord, submap: &str) {
    let binds = on_chord(listing, chord, submap);
    if binds.is_empty() {
        return;
    }
    if binds.iter().any(|bind| !is_ours(bind)) {
        tracing::warn!(
            "{} now also has a bind that is not ours; leaving the chord alone",
            where_it_is(chord, submap)
        );
        return;
    }
    if let Err(err) = hypr.unbind(chord, submap).await {
        tracing::warn!("could not unbind {}: {err}", where_it_is(chord, submap));
    }
}

#[cfg(test)]
mod tests;
