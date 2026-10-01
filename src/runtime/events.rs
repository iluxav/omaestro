//! What the runtime does with Hyprland's window, workspace and monitor
//! events: keeps track of what is focused and known, and fires the
//! matching triggers.

use mlua::MultiValue;

use super::Runtime;
use super::host::WindowEvent;
use super::registry::TriggerKind;
use crate::backend::hypr::events::{HyprEvent, WinRef};

impl Runtime {
    /// Focus moved. Blur handlers run for the window that lost it, then focus
    /// handlers for the one that got it. Handlers get `{class, title, address}`.
    pub(super) fn focus_changed(&mut self, window: Option<WinRef>) {
        if window == self.focused {
            return;
        }
        if let Some(window) = &window {
            self.remember(window);
        }
        let previous = std::mem::replace(&mut self.focused, window);
        if self.pending_reload.is_some() {
            return;
        }
        if let Some(previous) = previous {
            self.fire_window_triggers(WindowEvent::Blur, &previous);
        }
        if let Some(current) = self.focused.clone() {
            self.fire_window_triggers(WindowEvent::Focus, &current);
        }
    }

    /// Keeps the facts about a window that events carry, filling in what a
    /// later event leaves out.
    fn remember(&mut self, window: &WinRef) {
        let entry = self.known.entry(window.address.clone()).or_default();
        entry.address = window.address.clone();
        if !window.class.is_empty() {
            entry.class = window.class.clone();
        }
        if !window.title.is_empty() {
            entry.title = window.title.clone();
        }
        if !window.workspace.is_empty() {
            entry.workspace = window.workspace.clone();
        }
    }

    /// Open, close, title, workspace and monitor events.
    pub(super) fn window_event(&mut self, event: HyprEvent) {
        match event {
            HyprEvent::Opened(window) => {
                self.remember(&window);
                let window = self.known[&window.address].clone();
                if self.pending_reload.is_none() {
                    self.fire_window_triggers(WindowEvent::Open, &window);
                }
            }
            HyprEvent::Closed { address } => {
                // A focused window that closes loses focus first.
                if self.focused.as_ref().is_some_and(|f| f.address == address) {
                    self.focus_changed(None);
                }
                let window = self.known.remove(&address).unwrap_or(WinRef {
                    address,
                    ..WinRef::default()
                });
                if self.pending_reload.is_none() {
                    self.fire_window_triggers(WindowEvent::Close, &window);
                }
            }
            HyprEvent::Title { address, title } => {
                let entry = self.known.entry(address.clone()).or_default();
                entry.address = address.clone();
                entry.title = title;
                let window = entry.clone();
                if let Some(focused) = &mut self.focused
                    && focused.address == address
                {
                    focused.title = window.title.clone();
                }
                if self.pending_reload.is_none() {
                    self.fire_window_triggers(WindowEvent::Title, &window);
                }
            }
            HyprEvent::Workspace { id, name } => {
                if self.pending_reload.is_some() {
                    return;
                }
                for trigger in self.host.plain_triggers(&TriggerKind::Workspace) {
                    match self.host.table_of(&[("id", id)], &[("name", &name)]) {
                        Ok(table) => self.spawn_handler(
                            trigger,
                            MultiValue::from_iter([mlua::Value::Table(table)]),
                        ),
                        Err(err) => {
                            tracing::error!("cannot describe the workspace to a rule: {err}")
                        }
                    }
                }
            }
            HyprEvent::Monitor { name, change } => {
                if self.pending_reload.is_some() {
                    return;
                }
                for trigger in self.host.plain_triggers(&TriggerKind::Monitor) {
                    match self
                        .host
                        .table_of(&[], &[("name", &name), ("change", change.as_str())])
                    {
                        Ok(table) => self.spawn_handler(
                            trigger,
                            MultiValue::from_iter([mlua::Value::Table(table)]),
                        ),
                        Err(err) => tracing::error!("cannot describe the monitor to a rule: {err}"),
                    }
                }
            }
            HyprEvent::Submap(name) => {
                if let Some(Some(hint)) = self.host.mode_hint(&name) {
                    let backends = self.backends.clone();
                    self.jobs.spawn(async move {
                        if let Err(err) = backends.notifier.notify(&hint, "").await {
                            tracing::warn!("could not show the mode hint: {err}");
                        }
                    });
                }
            }
            HyprEvent::ConfigReloaded | HyprEvent::Focus(_) => {}
        }
    }

    fn fire_window_triggers(&mut self, which: WindowEvent, window: &WinRef) {
        for trigger in self.host.window_triggers(which, window) {
            match self.host.window_table(window) {
                Ok(table) => {
                    self.spawn_handler(trigger, MultiValue::from_iter([mlua::Value::Table(table)]))
                }
                Err(err) => tracing::error!("cannot describe the window to a rule: {err}"),
            }
        }
    }
}
