//! Hot reload: watch the config directory and tell the runtime, once, after
//! the writes settle. Editors write a file several times per save.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::Event;
use super::source::RULES_DIR;

const DEBOUNCE: Duration = Duration::from_millis(200);
const CONFIG_FILE: &str = "omaestro.toml";

/// Starts watching `dir`. The returned watcher must be kept alive.
pub fn spawn(dir: &Path, events: mpsc::Sender<Event>) -> Result<RecommendedWatcher> {
    let (changed, changes) = mpsc::unbounded_channel();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) if is_relevant(&event) => {
                let _ = changed.send(());
            }
            Ok(_) => {}
            Err(err) => tracing::warn!("file watcher: {err}"),
        })
        .context("creating the file watcher")?;
    watcher
        .watch(dir, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", dir.display()))?;
    tokio::spawn(debounce(changes, events));
    Ok(watcher)
}

/// A change to a rule file, the config file, or `rules.d` itself. Editor
/// droppings (swap files, backups, `4913`) and plain reads do not count.
fn is_relevant(event: &notify::Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                !name.starts_with('.')
                    && (name.ends_with(".lua") || name == CONFIG_FILE || name == RULES_DIR)
            })
    })
}

/// Collapses a burst of changes into one `FilesChanged`, sent once the
/// directory has been quiet for `DEBOUNCE`.
async fn debounce(mut changes: mpsc::UnboundedReceiver<()>, events: mpsc::Sender<Event>) {
    while changes.recv().await.is_some() {
        loop {
            match timeout(DEBOUNCE, changes.recv()).await {
                Ok(Some(())) => continue,
                Ok(None) => return,
                Err(_) => break,
            }
        }
        if events.send(Event::FilesChanged).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use notify::event::{AccessKind, CreateKind, ModifyKind};

    use super::*;

    fn event(kind: EventKind, path: &str) -> notify::Event {
        notify::Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn only_rule_and_config_files_trigger_a_reload() {
        let modify = EventKind::Modify(ModifyKind::Any);
        for path in [
            "/c/init.lua",
            "/c/rules.d/10-a.lua",
            "/c/omaestro.toml",
            "/c/rules.d",
        ] {
            assert!(is_relevant(&event(modify, path)), "{path}");
        }
        for path in [
            "/c/rules.d/.10-a.lua.swp",
            "/c/rules.d/10-a.lua~",
            "/c/rules.d/4913",
            "/c/notes.md",
        ] {
            assert!(!is_relevant(&event(modify, path)), "{path}");
        }
        assert!(is_relevant(&event(
            EventKind::Create(CreateKind::File),
            "/c/init.lua"
        )));
        assert!(!is_relevant(&event(
            EventKind::Access(AccessKind::Any),
            "/c/init.lua"
        )));
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_of_writes_is_one_reload() {
        let (changed, changes) = mpsc::unbounded_channel();
        let (events, mut inbox) = mpsc::channel(8);
        tokio::spawn(debounce(changes, events));

        // Five writes 50 ms apart: each one restarts the quiet period.
        for _ in 0..5 {
            changed.send(()).unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            inbox.try_recv().is_err(),
            "reloaded before the writes settled"
        );
        tokio::time::sleep(DEBOUNCE).await;
        assert!(matches!(inbox.try_recv(), Ok(Event::FilesChanged)));
        assert!(inbox.try_recv().is_err(), "one burst, one reload");

        // A later save is a new burst.
        changed.send(()).unwrap();
        tokio::time::sleep(DEBOUNCE * 2).await;
        assert!(matches!(inbox.try_recv(), Ok(Event::FilesChanged)));
    }
}
