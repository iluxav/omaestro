//! Clipboard and primary selection through `wl-paste` and `wl-copy`.

use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use super::{BackendError, BoxFuture, ClipContent, Clipboard, Result, Watching, run};
use crate::runtime::Event;

const PASTE: &str = "wl-paste";
const COPY: &str = "wl-copy";

pub struct WlClipboard;

/// `wl-paste` exits non-zero both when there is nothing to paste and when
/// something is wrong. Only the first is an empty clipboard.
fn is_empty(stderr: &str) -> bool {
    stderr.contains("Nothing is copied")
        || stderr.contains("No selection")
        || stderr.contains("No suitable type")
}

/// Runs `wl-paste`; `None` means there was nothing to paste.
async fn paste(args: &[&str]) -> Result<Option<Vec<u8>>> {
    let output = run::capture(PASTE, args).await?;
    if output.success {
        Ok(Some(output.stdout))
    } else if is_empty(&output.stderr) {
        Ok(None)
    } else {
        Err(BackendError::Failed {
            tool: PASTE,
            message: output.stderr,
        })
    }
}

/// The type to save a clipboard as: text if it offers any, else whatever
/// comes first (an image, a file list).
fn preferred_type(offered: &str) -> Option<&str> {
    const TEXT: [&str; 4] = [
        "text/plain;charset=utf-8",
        "text/plain",
        "UTF8_STRING",
        "STRING",
    ];
    let offered: Vec<&str> = offered
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    TEXT.iter()
        .find(|text| offered.contains(text))
        .copied()
        .or_else(|| offered.first().copied())
}

impl Clipboard for WlClipboard {
    fn selection(&self) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let data = paste(&["--primary", "--no-newline"]).await?;
            Ok(String::from_utf8_lossy(&data.unwrap_or_default()).into_owned())
        })
    }

    fn get(&self) -> BoxFuture<'_, Result<Option<ClipContent>>> {
        Box::pin(async move {
            let Some(types) = paste(&["--list-types"]).await? else {
                return Ok(None);
            };
            let types = String::from_utf8_lossy(&types);
            let Some(mime) = preferred_type(&types) else {
                return Ok(None);
            };
            let data = paste(&["--no-newline", "--type", mime]).await?;
            Ok(data.map(|data| ClipContent {
                mime: mime.to_string(),
                data,
            }))
        })
    }

    fn set<'a>(&'a self, content: &'a ClipContent) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { run::feed(COPY, &["--type", &content.mime], &content.data).await })
    }

    fn clear(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { run::feed(COPY, &["--clear"], &[]).await })
    }

    fn watch(&self, events: mpsc::Sender<Event>) -> std::result::Result<Watching, String> {
        watch(&[], events, || Event::ClipboardChanged, false)
    }

    fn watch_selection(
        &self,
        events: mpsc::Sender<Event>,
    ) -> std::result::Result<Watching, String> {
        // The first run is the selection there already was, not a change.
        watch(&["--primary"], events, || Event::SelectionChanged, true)
    }
}

/// `wl-paste --watch` runs a command on every change (and once at start,
/// for what is there); a line of output per change is all the daemon needs,
/// it reads the content itself.
fn watch(
    args: &[&str],
    events: mpsc::Sender<Event>,
    event: fn() -> Event,
    skip_first: bool,
) -> std::result::Result<Watching, String> {
    let mut child = tokio::process::Command::new(PASTE)
        .args(args)
        .args(["--watch", "echo", "changed"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| format!("cannot start `{PASTE} --watch`: {err}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no output from wl-paste".to_string())?;
    let task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut skip = skip_first;
        while let Ok(Some(_)) = lines.next_line().await {
            if std::mem::take(&mut skip) {
                continue;
            }
            if events.send(event()).await.is_err() {
                break;
            }
        }
        // Dropping the child here ends the watch.
        drop(child);
    });
    Ok(Watching::new(AbortOnDrop(task)))
}

/// Stops the reader task, and with it `wl-paste`, when the watch is dropped.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_preferred_when_saving_the_clipboard() {
        assert_eq!(
            preferred_type("text/html\ntext/plain\ntext/plain;charset=utf-8\n"),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(preferred_type("text/html\nSTRING\n"), Some("STRING"));
        assert_eq!(preferred_type("image/png\nimage/bmp\n"), Some("image/png"));
        assert_eq!(preferred_type("\n"), None);
    }

    #[test]
    fn empty_is_told_apart_from_broken() {
        assert!(is_empty("Nothing is copied"));
        assert!(is_empty("No selection"));
        assert!(!is_empty("Failed to connect to a Wayland server"));
    }
}
