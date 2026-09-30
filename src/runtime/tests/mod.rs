//! The runtime and the Lua API against the fake backends: load a snippet,
//! talk to the event loop the way the control socket does, assert what the
//! fakes recorded.

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

mod actions;
mod api;
mod examples;
mod focus;
mod handlers;
mod hotkeys;
mod loading;
mod shell;

type Files = Vec<(String, String)>;

struct Harness {
    events: mpsc::Sender<Event>,
    fakes: Fakes,
    files: Arc<Mutex<Files>>,
    runtime: JoinHandle<Exit>,
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
        let (backends, fakes) = fake::backends();
        setup(&fakes);
        let files = Arc::new(Mutex::new(owned(files)));
        let loader = {
            let files = files.clone();
            Box::new(move || rules(&files.lock().unwrap()))
        };
        let info = Info {
            config_dir: "/test".into(),
            hyprland_instance: "test".into(),
            trigger_command: "om trigger".into(),
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
