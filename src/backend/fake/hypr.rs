//! A stand-in for Hyprland's binds and focused window.

use std::sync::{Mutex, MutexGuard, PoisonError};

use super::Journal;
use crate::backend::{BackendError, BindInfo, BoxFuture, Hotkey, Hypr, Result, Window};
use crate::chord::Chord;

struct FakeBind {
    info: BindInfo,
    /// Registered through `Hypr::bind`, so a config reload drops it.
    runtime: bool,
}

#[derive(Default)]
struct HyprState {
    binds: Vec<FakeBind>,
    window: Option<Window>,
    fail_bind: Option<String>,
    fail_listing: bool,
}

/// Behaves like Hyprland where it matters: binds pile up per chord, and
/// unbinding a chord removes all of them.
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
        self.state().binds.push(FakeBind {
            info: BindInfo {
                chord: Chord::parse(chord).unwrap(),
                description: description.to_string(),
                submap: String::new(),
            },
            runtime: false,
        });
    }

    /// Chords of all binds, in registration order.
    pub fn chords(&self) -> Vec<String> {
        self.state()
            .binds
            .iter()
            .map(|b| b.info.chord.hyprland())
            .collect()
    }

    /// Hyprland re-reads its config: binds added at runtime are gone.
    pub fn reload_config(&self) {
        self.state().binds.retain(|bind| !bind.runtime);
    }

    pub fn set_window(&self, class: &str, title: &str) {
        self.state().window = Some(Window {
            class: class.to_string(),
            title: title.to_string(),
            address: "0x1".to_string(),
            workspace: "1".to_string(),
            floating: false,
        });
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
            Ok(state.binds.iter().map(|b| b.info.clone()).collect())
        })
    }

    fn bind<'a>(&'a self, hotkey: &'a Hotkey) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut state = self.state();
            if let Some(message) = state.fail_bind.take() {
                return Err(hyprctl_failed(message));
            }
            self.journal.push(format!(
                "bind {} -> {} [{}]",
                hotkey.chord, hotkey.command, hotkey.description
            ));
            state.binds.push(FakeBind {
                info: BindInfo {
                    chord: hotkey.chord.clone(),
                    description: hotkey.description.clone(),
                    submap: String::new(),
                },
                runtime: true,
            });
            Ok(())
        })
    }

    fn unbind<'a>(&'a self, chord: &'a Chord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.journal.push(format!("unbind {chord}"));
            self.state()
                .binds
                .retain(|bind| !bind.info.chord.same_keys(chord));
            Ok(())
        })
    }

    fn active_window(&self) -> BoxFuture<'_, Result<Option<Window>>> {
        Box::pin(async move { Ok(self.state().window.clone()) })
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
