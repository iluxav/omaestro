//! A stand-in for Hyprland's binds and focused window.

use std::sync::{Mutex, MutexGuard, PoisonError};

use super::Journal;
use crate::backend::{
    BackendError, BindAction, BindInfo, BoxFuture, Hotkey, Hypr, Monitor, Result, Window, Workspace,
};
use crate::chord::Chord;

struct HyprState {
    /// The binds right now: the config's plus those made through `bind`.
    binds: Vec<BindInfo>,
    /// What the config defines (`add`, `add_in`): all back after a reload.
    config: Vec<BindInfo>,
    clients: Vec<Window>,
    monitors: Vec<Monitor>,
    fail_bind: Option<String>,
    fail_listing: bool,
}

impl Default for HyprState {
    fn default() -> Self {
        Self {
            binds: Vec::new(),
            config: Vec::new(),
            clients: Vec::new(),
            // One 1920x1080 monitor with a 30 px bar on top, on workspace 1.
            monitors: vec![Monitor {
                id: 0,
                name: "WL-1".to_string(),
                description: "Fake monitor".to_string(),
                width: 1920,
                height: 1080,
                scale: 1.0,
                focused: true,
                workspace: "1".to_string(),
                workspace_id: 1,
                reserved: [0, 30, 0, 0],
                ..Monitor::default()
            }],
            fail_bind: None,
            fail_listing: false,
        }
    }
}

/// Behaves like Hyprland where it matters: binds pile up per chord,
/// unbinding a chord removes all of them, and a config reload drops the
/// binds made at runtime and brings the config's back.
#[derive(Default)]
pub struct FakeHypr {
    journal: Journal,
    state: Mutex<HyprState>,
}

impl FakeHypr {
    pub fn new(journal: Journal) -> Self {
        Self {
            journal,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> MutexGuard<'_, HyprState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A bind that is there before the daemon does anything, as if it came
    /// from the user's Hyprland config.
    pub fn add(&self, chord: &str, description: &str) {
        self.add_in("", chord, description);
    }

    /// A bind inside a submap that is there before the daemon does anything.
    pub fn add_in(&self, submap: &str, chord: &str, description: &str) {
        let info = BindInfo {
            chord: Chord::parse(chord).unwrap(),
            description: description.to_string(),
            submap: submap.to_string(),
        };
        let mut state = self.state();
        state.config.push(info.clone());
        state.binds.push(info);
    }

    /// Chords of all binds, in registration order.
    pub fn chords(&self) -> Vec<String> {
        self.state()
            .binds
            .iter()
            .map(|b| b.chord.hyprland())
            .collect()
    }

    /// Hyprland re-reads its config: binds added at runtime are gone, the
    /// config's are all there again.
    pub fn reload_config(&self) {
        let mut state = self.state();
        state.binds = state.config.clone();
    }

    /// Focuses a window of `class`, creating it at `0x1` on workspace 1 if
    /// there is none.
    pub fn set_window(&self, class: &str, title: &str) {
        let mut state = self.state();
        for client in &mut state.clients {
            client.focused = false;
        }
        match state.clients.iter_mut().find(|c| c.address == "0x1") {
            Some(client) => {
                client.class = class.to_string();
                client.title = title.to_string();
                client.focused = true;
            }
            None => state.clients.push(Window {
                address: "0x1".to_string(),
                class: class.to_string(),
                title: title.to_string(),
                initial_class: class.to_string(),
                workspace: "1".to_string(),
                workspace_id: 1,
                width: 1200,
                height: 800,
                focused: true,
                ..Window::default()
            }),
        }
    }

    pub fn add_client(&self, window: Window) {
        self.state().clients.push(window);
    }

    /// The window is gone, as if its app closed it.
    pub fn remove_client(&self, address: &str) {
        self.state().clients.retain(|c| c.address != address);
    }

    /// The window is now on `workspace`. Dispatches are only recorded, so a
    /// test says where a move left a window.
    pub fn move_client(&self, address: &str, workspace: &str, workspace_id: i64) {
        for client in &mut self.state().clients {
            if client.address == address {
                client.workspace = workspace.to_string();
                client.workspace_id = workspace_id;
            }
        }
    }

    pub fn add_monitor(&self, monitor: Monitor) {
        self.state().monitors.push(monitor);
    }

    pub fn fail_next_bind(&self, message: &str) {
        self.state().fail_bind = Some(message.to_string());
    }

    pub fn fail_next_listing(&self) {
        self.state().fail_listing = true;
    }

    pub fn calls(&self) -> Vec<String> {
        self.journal.entries()
    }

    pub fn forget_calls(&self) {
        self.journal.clear();
    }
}

fn hyprctl_failed(message: String) -> BackendError {
    BackendError::Failed {
        tool: "hyprctl",
        message,
    }
}

impl Hypr for FakeHypr {
    fn binds(&self) -> BoxFuture<'_, Result<Vec<BindInfo>>> {
        Box::pin(async move {
            self.journal.push("binds".to_string());
            let mut state = self.state();
            if std::mem::take(&mut state.fail_listing) {
                return Err(hyprctl_failed("Couldn't connect".to_string()));
            }
            Ok(state.binds.clone())
        })
    }

    fn bind<'a>(&'a self, hotkey: &'a Hotkey) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut state = self.state();
            if let Some(message) = state.fail_bind.take() {
                return Err(hyprctl_failed(message));
            }
            let action = match &hotkey.action {
                BindAction::Exec(command) => command.clone(),
                BindAction::Submap(name) => format!("submap {name}"),
            };
            let at = if hotkey.submap.is_empty() {
                "bind".to_string()
            } else {
                format!("bind@{}", hotkey.submap)
            };
            self.journal.push(format!(
                "{at} {} -> {action} [{}]",
                hotkey.chord, hotkey.description
            ));
            state.binds.push(BindInfo {
                chord: hotkey.chord.clone(),
                description: hotkey.description.clone(),
                submap: hotkey.submap.clone(),
            });
            Ok(())
        })
    }

    fn unbind<'a>(&'a self, chord: &'a Chord, submap: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if submap.is_empty() {
                self.journal.push(format!("unbind {chord}"));
            } else {
                self.journal.push(format!("unbind@{submap} {chord}"));
            }
            self.state()
                .binds
                .retain(|bind| !(bind.submap == submap && bind.chord.same_keys(chord)));
            Ok(())
        })
    }

    fn reload(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.journal.push("reload".to_string());
            self.reload_config();
            Ok(())
        })
    }

    fn active_window(&self) -> BoxFuture<'_, Result<Option<Window>>> {
        Box::pin(async move { Ok(self.state().clients.iter().find(|c| c.focused).cloned()) })
    }

    fn clients(&self) -> BoxFuture<'_, Result<Vec<Window>>> {
        Box::pin(async move { Ok(self.state().clients.clone()) })
    }

    fn monitors(&self) -> BoxFuture<'_, Result<Vec<Monitor>>> {
        Box::pin(async move { Ok(self.state().monitors.clone()) })
    }

    fn workspaces(&self) -> BoxFuture<'_, Result<Vec<Workspace>>> {
        Box::pin(async move {
            // One workspace per monitor, holding that monitor's clients.
            let state = self.state();
            Ok(state
                .monitors
                .iter()
                .map(|m| Workspace {
                    id: m.workspace_id,
                    name: m.workspace.clone(),
                    monitor: m.name.clone(),
                    windows: state
                        .clients
                        .iter()
                        .filter(|c| c.workspace_id == m.workspace_id)
                        .count() as i64,
                    has_fullscreen: false,
                })
                .collect())
        })
    }

    fn cursor(&self) -> BoxFuture<'_, Result<(i64, i64)>> {
        Box::pin(async move { Ok((640, 360)) })
    }

    fn dispatch<'a>(&'a self, code: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.journal.push(format!("dispatch {code}"));
            if code.contains("nil") {
                return Err(hyprctl_failed(format!(
                    "attempt to call a nil value in {code}"
                )));
            }
            Ok(())
        })
    }
}
