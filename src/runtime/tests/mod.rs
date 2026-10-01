//! The runtime and the Lua API against the fake backends: load a snippet,
//! talk to the event loop the way the control socket does, assert what the
//! fakes recorded.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::source::{Rules, Source};
use super::{Event, Exit, Info, Runtime};
use crate::backend::fake::{self, Fakes};
use crate::backend::hypr::events::HyprEvent;
use crate::config::{CONFIG_FILE, Config};
use crate::ipc::{Request, Response, Status, TriggerRow};
use crate::testutil::TempDir;

mod actions;
mod api;
mod app;
mod app_hotkeys;
mod builtin;
mod disabled;
mod events;
mod focus;
mod handlers;
mod hotkeys;
mod loading;
mod misc;
mod modes;
mod overrides;
mod shell;
mod store;
mod system;
mod timers;
mod typed;
mod watchers;
mod windows;

type Files = Vec<(String, String)>;

static NEXT_STATE: AtomicUsize = AtomicUsize::new(0);

struct Harness {
    events: mpsc::Sender<Event>,
    fakes: Fakes,
    files: Arc<Mutex<Files>>,
    runtime: JoinHandle<Exit>,
    /// Removed with the harness; `om.store` writes here.
    _state: TempDir,
}

fn owned(files: &[(&str, &str)]) -> Files {
    files
        .iter()
        .map(|(name, code)| (name.to_string(), code.to_string()))
        .collect()
}

/// What `source::load` does with a directory, done with files in memory:
/// `omaestro.toml` is the config, everything else is a rule file.
fn rules(files: &Files) -> Result<Rules, String> {
    let mut rules = Rules::default();
    for (name, text) in files {
        if name == CONFIG_FILE {
            rules.config = Config::parse(text).map_err(|err| format!("{CONFIG_FILE}: {err}"))?;
        } else {
            rules.sources.push(Source::new(name, text));
        }
    }
    Ok(rules)
}

impl Harness {
    async fn start(files: &[(&str, &str)]) -> Self {
        Self::start_with(files, |_| {}).await
    }

    /// `setup` sees the fakes before the first load.
    async fn start_with(files: &[(&str, &str)], setup: impl FnOnce(&Fakes)) -> Self {
        let state = TempDir::new(&format!(
            "state-{}",
            NEXT_STATE.fetch_add(1, Ordering::SeqCst)
        ));
        Self::start_on(owned(files), state, setup).await
    }

    /// A daemon on an existing state directory, as after a restart.
    async fn start_on(files: Files, state: TempDir, setup: impl FnOnce(&Fakes)) -> Self {
        let (backends, fakes) = fake::backends();
        setup(&fakes);
        let files = Arc::new(Mutex::new(files));
        let loader = {
            let files = files.clone();
            Box::new(move || rules(&files.lock().unwrap()))
        };
        let info = Info {
            config_dir: state.path().join("config"),
            hyprland_instance: "test".into(),
            trigger_command: "om trigger".into(),
            state_dir: state.path().to_path_buf(),
        };
        let (events, inbox) = mpsc::channel(16);
        let runtime = Runtime::start(loader, backends, info, events.clone())
            .await
            .unwrap();
        let runtime = tokio::spawn(runtime.run(inbox));
        Self {
            events,
            fakes,
            files,
            runtime,
            _state: state,
        }
    }

    /// Replaces the files, as if the user saved them. No reload yet.
    fn write(&self, files: &[(&str, &str)]) {
        *self.files.lock().unwrap() = owned(files);
    }

    /// Saves the files and reloads, like the file watcher would.
    async fn save(&self, files: &[(&str, &str)]) -> Response {
        self.write(files);
        self.ask(Request::Reload).await
    }

    async fn hyprland(&self, event: HyprEvent) {
        assert!(self.events.send(Event::Hypr(event)).await.is_ok());
    }

    /// Stops the daemon and returns why the loop ended.
    async fn stop(self, event: Event) -> (Fakes, Exit) {
        assert!(self.events.send(event).await.is_ok());
        (self.fakes, self.runtime.await.unwrap())
    }

    /// Stops the daemon and starts a fresh one (new fakes) on the same
    /// state directory and files.
    async fn restart(self) -> Self {
        self.restart_with(|_| {}).await
    }

    /// Like `restart`; `setup` sees the new fakes before the load.
    async fn restart_with(self, setup: impl FnOnce(&Fakes)) -> Self {
        let files = self.files.lock().unwrap().clone();
        assert!(self.events.send(Event::Shutdown).await.is_ok());
        assert_eq!(self.runtime.await.unwrap(), Exit::Shutdown);
        Self::start_on(files, self._state, setup).await
    }

    /// The state directory, where `om.store` and the settings live.
    fn state_dir(&self) -> &std::path::Path {
        self._state.path()
    }

    /// The config directory the runtime was told about; `lib/` under it is
    /// on the require path.
    fn config_dir(&self) -> std::path::PathBuf {
        self._state.path().join("config")
    }

    /// Puts a built-in plugin's files under `lib/`, as `om plugin add
    /// <name>` would. A rule then loads it with `om.use(name)`.
    fn install_builtin(&self, name: &str) {
        let plugin = crate::plugins::builtin::find(name).unwrap();
        crate::plugins::builtin::install(&self.config_dir().join("lib"), plugin).unwrap();
    }

    /// Sends a request without waiting for the answer.
    async fn send(&self, request: Request) -> oneshot::Receiver<Response> {
        let (reply, answer) = oneshot::channel();
        assert!(
            self.events
                .send(Event::Request(request, reply))
                .await
                .is_ok()
        );
        answer
    }

    async fn ask(&self, request: Request) -> Response {
        self.send(request).await.await.unwrap()
    }

    async fn eval(&self, chunk: &str) -> Result<Vec<String>, String> {
        let response = self
            .ask(Request::Eval {
                chunk: chunk.into(),
            })
            .await;
        match response.error {
            Some(error) => Err(error),
            None => Ok(serde_json::from_value(response.data.unwrap()).unwrap()),
        }
    }

    async fn trigger(&self, id: &str) -> Response {
        self.ask(Request::Trigger { id: id.into() }).await
    }

    async fn status(&self) -> Status {
        serde_json::from_value(self.ask(Request::Status).await.data.unwrap()).unwrap()
    }

    /// Titles of the notifications so far.
    fn titles(&self) -> Vec<String> {
        self.fakes
            .notifier
            .sent()
            .into_iter()
            .map(|(title, _)| title)
            .collect()
    }

    /// Bodies of the error notifications the runtime itself sent.
    fn errors(&self) -> Vec<String> {
        let sent = self.fakes.notifier.sent().into_iter();
        sent.filter(|(title, _)| title == "omaestro")
            .map(|(_, body)| body)
            .collect()
    }

    /// Lets the loop run until `done` holds.
    async fn until(&self, what: &str, done: impl Fn(&Self) -> bool) {
        for _ in 0..10_000 {
            if done(self) {
                return;
            }
            pause().await;
        }
        panic!("gave up waiting until {what}");
    }

    /// Lets the loop run until no handler is in flight.
    async fn settle(&self) {
        for _ in 0..10_000 {
            if self.status().await.running == 0 {
                return;
            }
            pause().await;
        }
        panic!("handlers never finished");
    }
}

/// Gives the runtime a turn. A short sleep rather than a bare yield: under a
/// paused clock it lets tokio advance time, so handlers that sleep finish.
async fn pause() {
    tokio::time::sleep(Duration::from_millis(1)).await;
}
