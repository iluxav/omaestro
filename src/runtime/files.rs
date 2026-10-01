//! `om.on_file`: one file-system watcher per trigger, sending changes into
//! the event loop.

use std::collections::HashMap;
use std::path::PathBuf;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use super::Event;

/// A file (or directory) a rule watches.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub id: String,
    pub path: PathBuf,
}

/// What happened to a path, for the handler.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub id: String,
    pub path: PathBuf,
    pub kind: &'static str,
}

pub fn kind_name(kind: &EventKind) -> Option<&'static str> {
    match kind {
        EventKind::Create(_) => Some("create"),
        EventKind::Modify(_) => Some("modify"),
        EventKind::Remove(_) => Some("remove"),
        _ => None,
    }
}

#[derive(Default)]
pub struct Watches {
    running: HashMap<String, (PathBuf, RecommendedWatcher)>,
}

impl Watches {
    /// Watches exactly `wanted`. A path that cannot be watched is reported
    /// back as `(id, message)`.
    pub fn sync(
        &mut self,
        wanted: &[Wanted],
        events: &mpsc::Sender<Event>,
    ) -> Vec<(String, String)> {
        self.running
            .retain(|id, (path, _)| wanted.iter().any(|w| w.id == *id && w.path == *path));
        let mut failed = Vec::new();
        for want in wanted {
            if self.running.contains_key(&want.id) {
                continue;
            }
            match start(want, events.clone()) {
                Ok(watcher) => {
                    self.running
                        .insert(want.id.clone(), (want.path.clone(), watcher));
                }
                Err(message) => failed.push((want.id.clone(), message)),
            }
        }
        failed
    }

    pub fn clear(&mut self) {
        self.running.clear();
    }
}

fn start(want: &Wanted, events: mpsc::Sender<Event>) -> Result<RecommendedWatcher, String> {
    let id = want.id.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else {
            return;
        };
        let Some(kind) = kind_name(&event.kind) else {
            return;
        };
        for path in event.paths {
            // Editors write swap files and backups next to the file.
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.starts_with('.') || name.ends_with('~') || name == "4913" {
                continue;
            }
            let _ = events.blocking_send(Event::File(Change {
                id: id.clone(),
                path: path.clone(),
                kind,
            }));
        }
    })
    .map_err(|err| format!("cannot create a watcher: {err}"))?;
    watcher
        .watch(&want.path, RecursiveMode::Recursive)
        .map_err(|err| format!("cannot watch {}: {err}", want.path.display()))?;
    Ok(watcher)
}
