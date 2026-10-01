//! Read-only keyboard monitoring through evdev, for `om.on_typed`. Nothing
//! is grabbed: Hyprland keeps seeing every key. Only runs while a rule
//! watches for typed text, and only the last few characters are kept, in
//! `runtime::typed`.
//!
//! Needs read access to `/dev/input/event*`, which the `input` group has.

use std::path::{Path, PathBuf};

use evdev::{Device, EventSummary, KeyCode};
use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{Keyboards, Watching};
use crate::runtime::Event;
use crate::runtime::typed::{Key, Modifier, key_to_char, modifier};

const INPUT_DIR: &str = "/dev/input";

pub struct EvdevKeyboards;

impl Keyboards for EvdevKeyboards {
    fn watch(&self, events: mpsc::Sender<Event>) -> Result<Watching, String> {
        Monitor::start(events).map(Watching::new)
    }
}

pub struct Monitor {
    tasks: Vec<JoinHandle<()>>,
    /// Tells the runtime when a keyboard is plugged in, so it restarts us.
    _hotplug: Option<notify::RecommendedWatcher>,
}

/// The keyboards evdev can open right now.
fn keyboards() -> Result<Vec<(PathBuf, Device)>, String> {
    let devices: Vec<(PathBuf, Device)> = evdev::enumerate()
        .filter(|(_, device)| {
            device.supported_keys().is_some_and(|keys| {
                keys.contains(KeyCode::KEY_A) && keys.contains(KeyCode::KEY_ENTER)
            })
        })
        .collect();
    if devices.is_empty() {
        let any_device = std::fs::read_dir(INPUT_DIR)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with("event"))
            })
            .unwrap_or(false);
        if any_device {
            return Err(format!(
                "cannot read {INPUT_DIR}/event*: add yourself to the input group \
                 (sudo usermod -aG input $USER) and log in again"
            ));
        }
        return Err(format!("no keyboard found under {INPUT_DIR}"));
    }
    Ok(devices)
}

/// Letter keys take Caps Lock into account; everything else only Shift.
fn is_letter(code: u16) -> bool {
    matches!(code, 16..=25 | 30..=38 | 44..=50)
}

impl Monitor {
    /// Opens every keyboard and streams its keys, as characters, into the
    /// event loop.
    pub fn start(events: mpsc::Sender<Event>) -> Result<Self, String> {
        let devices = keyboards()?;
        let mut tasks = Vec::new();
        for (path, device) in devices {
            let name = device.name().unwrap_or("keyboard").to_string();
            let stream = match device.into_event_stream() {
                Ok(stream) => stream,
                Err(err) => {
                    tracing::warn!("typed: cannot read {} ({name}): {err}", path.display());
                    continue;
                }
            };
            tracing::debug!("typed: reading {} ({name})", path.display());
            let events = events.clone();
            tasks.push(tokio::spawn(read_keys(stream, events)));
        }
        if tasks.is_empty() {
            return Err("no keyboard could be read".to_string());
        }

        let hotplug = {
            let events = events.clone();
            let mut watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    if let Ok(event) = result
                        && event.kind.is_create()
                        && event.paths.iter().any(|p| {
                            p.file_name()
                                .is_some_and(|n| n.to_string_lossy().starts_with("event"))
                        })
                    {
                        let _ = events.blocking_send(Event::TypedRescan);
                    }
                })
                .ok();
            if let Some(watcher) = watcher.as_mut()
                && let Err(err) = watcher.watch(Path::new(INPUT_DIR), RecursiveMode::NonRecursive)
            {
                tracing::debug!("typed: no hotplug watch on {INPUT_DIR}: {err}");
            }
            watcher
        };
        Ok(Self {
            tasks,
            _hotplug: hotplug,
        })
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// One keyboard: tracks the modifiers and turns presses into `Key`s.
async fn read_keys(mut stream: evdev::EventStream, events: mpsc::Sender<Event>) {
    let (mut shift, mut caps, mut other) = (0u32, false, 0u32);
    loop {
        let event = match stream.next_event().await {
            Ok(event) => event,
            Err(err) => {
                // Unplugged, most likely. A rescan picks up whatever is left.
                tracing::debug!("typed: keyboard stream ended: {err}");
                return;
            }
        };
        let EventSummary::Key(_, key, value) = event.destructure() else {
            continue;
        };
        let code = key.code();
        let pressed = value != 0;
        if let Some(modifier) = modifier(code) {
            match modifier {
                Modifier::Shift => {
                    shift = if pressed {
                        shift + 1
                    } else {
                        shift.saturating_sub(1)
                    }
                }
                Modifier::CapsLock if value == 1 => caps = !caps,
                Modifier::CapsLock => {}
                Modifier::Other => {
                    other = if pressed {
                        other + 1
                    } else {
                        other.saturating_sub(1)
                    }
                }
            }
            if pressed
                && modifier == Modifier::Other
                && events.send(Event::Typed(Key::Reset)).await.is_err()
            {
                return;
            }
            continue;
        }
        // Presses and repeats type; releases do not. A key with Ctrl, Alt or
        // Super held is a shortcut, not text.
        if !pressed || other > 0 {
            continue;
        }
        let shifted = if is_letter(code) {
            (shift > 0) != caps
        } else {
            shift > 0
        };
        if let Some(key) = key_to_char(code, shifted)
            && events.send(Event::Typed(key)).await.is_err()
        {
            return;
        }
    }
}
