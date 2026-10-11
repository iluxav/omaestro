//! Cards on the screen (`om.show`). The daemon draws nothing: quickshell
//! does, from `ui/Cards.qml`, which answers the IPC target `omaestro.ui`.
//! Two places can host it, tried in this order:
//!
//! 1. The Omarchy shell, when omaestro's plugin is loaded in it
//!    (`Service.qml` loads `ui/OmarchyCards.qml`), reached through
//!    `omarchy-shell`.
//! 2. An instance of our own: `ui/shell.qml` written to a directory under
//!    `$XDG_RUNTIME_DIR`, started with `quickshell -p` for the first card
//!    and killed with the daemon (it also quits by itself if the daemon
//!    dies).
//!
//! Neither: `MissingTool`, and the API sends a notification instead.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep, timeout};

use super::{BackendError, BoxFuture, Result, run};

/// Where a card sits on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Center,
    Top,
    Bottom,
}

impl Position {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

/// What `om.show` puts on the screen: a large line over smaller ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// A card with the id on screen is updated; any other is replaced.
    pub id: u64,
    pub big: String,
    pub lines: Vec<String>,
    /// How long it stays up; `None` until closed.
    pub duration: Option<Duration>,
    pub position: Position,
}

pub trait Ui: Send + Sync {
    /// Puts `card` on the screen in place of the one there. `MissingTool`
    /// when nothing on this machine can draw it.
    fn show<'a>(&'a self, card: &'a Card) -> BoxFuture<'a, Result<()>>;
    /// New text for the card with `card.id`, in place and for the time it
    /// has left. False when it is gone: its time is up, it was closed, or
    /// another card took its place.
    fn update<'a>(&'a self, card: &'a Card) -> BoxFuture<'a, Result<bool>>;
    /// Takes the card with this id down, if it is still the one on screen.
    fn close(&self, id: u64) -> BoxFuture<'_, Result<()>>;
}

/// Our own instance's files, written to its directory before it starts.
const FILES: [(&str, &str); 2] = [
    ("shell.qml", include_str!("../../ui/shell.qml")),
    ("Cards.qml", include_str!("../../ui/Cards.qml")),
];

const TARGET: &str = "omaestro.ui";
/// One IPC call; a hung shell must not hold a rule up.
const CALL_TIMEOUT: Duration = Duration::from_secs(3);
/// How long our own instance gets to start answering.
const START_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Host {
    Omarchy,
    Own,
}

/// Why a host did not take a call.
enum Miss {
    /// Nothing there: not installed, not running, or without our cards.
    Absent,
    /// It is there and failed.
    Failed(BackendError),
}

fn failed(message: String) -> Miss {
    Miss::Failed(BackendError::Failed {
        tool: "quickshell",
        message,
    })
}

pub struct Quickshell {
    dir: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The host that drew the last card: updates and closes go there.
    last: Option<Host>,
    /// Our own instance; dropping it (with the daemon) kills it.
    child: Option<Child>,
    /// Whether this daemon has written the files yet.
    written: bool,
}

impl Quickshell {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            state: Mutex::default(),
        }
    }

    /// What our own instance prints, next to its directory.
    fn log_path(&self) -> PathBuf {
        self.dir.with_extension("log")
    }

    /// A call on `host`, answered with one of `expected`; `start` lets it
    /// start our own instance.
    async fn call(
        &self,
        state: &mut State,
        host: Host,
        method: &str,
        args: &[&str],
        start: bool,
        expected: &[&str],
    ) -> std::result::Result<String, Miss> {
        let answer = match host {
            Host::Omarchy => omarchy(method, args).await?,
            Host::Own => self.own(state, method, args, start).await?,
        };
        match host {
            _ if expected.contains(&answer.as_str()) => Ok(answer),
            Host::Omarchy => Err(failed(answer)),
            Host::Own => Err(failed(format!(
                "{answer} (log: {})",
                self.log_path().display()
            ))),
        }
    }

    async fn own(
        &self,
        state: &mut State,
        method: &str,
        args: &[&str],
        start: bool,
    ) -> std::result::Result<String, Miss> {
        if start && !state.written {
            write_files(&self.dir).map_err(|err| {
                failed(format!(
                    "could not write the cards to {}: {err}",
                    self.dir.display()
                ))
            })?;
            state.written = true;
        }
        if let Some(answer) = self.ask(method, args).await? {
            return Ok(answer);
        }
        if !start {
            return Err(Miss::Absent);
        }
        self.start(state).await?;
        self.ask(method, args).await?.ok_or_else(|| {
            failed(format!(
                "the cards started but do not answer (log: {})",
                self.log_path().display()
            ))
        })
    }

    /// One IPC call to our own instance: `None` when it is not running, or
    /// not ready yet.
    async fn ask(&self, method: &str, args: &[&str]) -> std::result::Result<Option<String>, Miss> {
        let dir = self.dir.to_string_lossy();
        let mut call = vec!["ipc", "-p", &dir, "call", "--", TARGET, method];
        call.extend_from_slice(args);
        let output = match timeout(CALL_TIMEOUT, run::capture("quickshell", &call)).await {
            Ok(Ok(output)) => output,
            Ok(Err(BackendError::MissingTool { .. })) => return Err(Miss::Absent),
            Ok(Err(err)) => return Err(Miss::Failed(err)),
            Err(_) => return Err(failed("the cards do not answer".to_string())),
        };
        // No instance on the directory is a non-zero exit. One still
        // loading says so on stdout with a zero exit, like every answer.
        let text = output.stdout_text().trim().to_string();
        if !output.success || text.starts_with("Not ready") {
            return Ok(None);
        }
        Ok(Some(text))
    }

    /// Starts our own instance, unless it is running, and waits until it
    /// answers.
    async fn start(&self, state: &mut State) -> std::result::Result<(), Miss> {
        let running = state
            .child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if !running {
            state.child = Some(self.spawn()?);
        }
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if self.ask("ping", &[]).await?.is_some() {
                return Ok(());
            }
            // A zero exit is `-n` finding another instance on the
            // directory, which will answer; anything else is a failure.
            if let Some(child) = state.child.as_mut()
                && let Ok(Some(status)) = child.try_wait()
                && !status.success()
            {
                return Err(failed(format!(
                    "exited with {status} (log: {})",
                    self.log_path().display()
                )));
            }
            if Instant::now() >= deadline {
                return Err(failed(format!(
                    "the cards did not start in {} s (log: {})",
                    START_TIMEOUT.as_secs(),
                    self.log_path().display()
                )));
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    fn spawn(&self) -> std::result::Result<Child, Miss> {
        let log = self.log_path();
        let open = |what: &str| {
            std::fs::File::create(&log)
                .map_err(|err| failed(format!("could not open {} ({what}): {err}", log.display())))
        };
        let stdout = open("stdout")?;
        let stderr = stdout
            .try_clone()
            .map_err(|err| failed(format!("could not open {}: {err}", log.display())))?;
        Command::new("quickshell")
            .arg("-n")
            .arg("-p")
            .arg(&self.dir)
            // shell.qml quits when this process is gone.
            .env("OMAESTRO_PID", std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| match err.kind() {
                ErrorKind::NotFound => Miss::Absent,
                _ => failed(err.to_string()),
            })
    }
}

/// A call to the Omarchy shell. `omarchy-shell` fails when the shell is not
/// running or has no `omaestro.ui` (omaestro's plugin is not loaded in it).
async fn omarchy(method: &str, args: &[&str]) -> std::result::Result<String, Miss> {
    let mut call = vec![TARGET, method];
    call.extend_from_slice(args);
    match timeout(CALL_TIMEOUT, run::capture("omarchy-shell", &call)).await {
        Ok(Ok(output)) if output.success => Ok(output.stdout_text().trim().to_string()),
        Ok(Ok(_)) | Ok(Err(BackendError::MissingTool { .. })) | Err(_) => Err(Miss::Absent),
        Ok(Err(err)) => Err(Miss::Failed(err)),
    }
}

/// Writes our instance's files, leaving alone those already right:
/// quickshell reloads on every change it sees.
fn write_files(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, text) in FILES {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(&path, text)?;
        }
    }
    Ok(())
}

/// The card as `Cards.qml` reads it; `ms` 0 keeps it up until closed.
fn payload(card: &Card) -> String {
    let ms = card
        .duration
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    serde_json::json!({
        "id": card.id,
        "big": card.big,
        "lines": card.lines,
        "ms": ms,
        "position": card.position.as_str(),
    })
    .to_string()
}

impl Ui for Quickshell {
    fn show<'a>(&'a self, card: &'a Card) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let payload = payload(card);
            let mut state = self.state.lock().await;
            let mut failure = None;
            for host in [Host::Omarchy, Host::Own] {
                match self
                    .call(&mut state, host, "show", &[&payload], true, &["ok"])
                    .await
                {
                    Ok(_) => {
                        // The shell is back (after a restart, say): our own
                        // instance is not needed any more.
                        if host == Host::Omarchy {
                            state.child = None;
                        }
                        state.last = Some(host);
                        return Ok(());
                    }
                    Err(Miss::Absent) => {}
                    Err(Miss::Failed(err)) => {
                        failure.get_or_insert(err);
                    }
                }
            }
            Err(failure.unwrap_or(BackendError::MissingTool { tool: "quickshell" }))
        })
    }

    fn update<'a>(&'a self, card: &'a Card) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let payload = payload(card);
            let mut state = self.state.lock().await;
            // Where the card was drawn; nowhere means it is not up.
            let Some(host) = state.last else {
                return Ok(false);
            };
            let answer = self
                .call(
                    &mut state,
                    host,
                    "update",
                    &[&payload],
                    false,
                    &["ok", "gone"],
                )
                .await;
            match answer {
                Ok(answer) => Ok(answer == "ok"),
                Err(Miss::Absent) => Ok(false),
                Err(Miss::Failed(err)) => Err(err),
            }
        })
    }

    fn close(&self, id: u64) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut state = self.state.lock().await;
            let Some(host) = state.last else {
                return Ok(());
            };
            let id = id.to_string();
            match self
                .call(&mut state, host, "close", &[&id], false, &["ok"])
                .await
            {
                Ok(_) | Err(Miss::Absent) => Ok(()),
                Err(Miss::Failed(err)) => Err(err),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn the_payload_is_what_cards_qml_reads() {
        let card = Card {
            id: 7,
            big: "14:32".into(),
            lines: vec!["Saturday".into(), "18 °C \"sunny\"".into()],
            duration: Some(Duration::from_millis(1500)),
            position: Position::Top,
        };
        let value: serde_json::Value = serde_json::from_str(&payload(&card)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "id": 7,
                "big": "14:32",
                "lines": ["Saturday", "18 °C \"sunny\""],
                "ms": 1500,
                "position": "top",
            })
        );
        let until_closed = Card {
            duration: None,
            ..card
        };
        let value: serde_json::Value = serde_json::from_str(&payload(&until_closed)).unwrap();
        assert_eq!(value["ms"], 0);
    }

    #[test]
    fn the_files_are_written_and_put_right() {
        let tmp = TempDir::new("ui-files");
        let dir = tmp.path().join("omaestro-ui");
        write_files(&dir).unwrap();
        for (name, text) in FILES {
            assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), text);
        }
        std::fs::write(dir.join("Cards.qml"), "stale").unwrap();
        write_files(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("Cards.qml")).unwrap(),
            FILES[1].1
        );
    }

    #[test]
    fn the_log_sits_next_to_the_directory() {
        let ui = Quickshell::new(PathBuf::from("/run/user/1000/omaestro-ui"));
        assert_eq!(
            ui.log_path(),
            PathBuf::from("/run/user/1000/omaestro-ui.log")
        );
    }
}
