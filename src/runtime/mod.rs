//! The runtime: one event loop that owns the Lua state, receives events,
//! dispatches them to Lua handlers and answers the control socket.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mlua::MultiValue;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::backend::Backends;
use crate::backend::hypr::events::{Focused, HyprEvent};
use crate::ipc::{Request, Response, Status};

pub mod error;
pub mod handler;
pub mod host;
pub mod hotkeys;
pub mod registry;
pub mod render;
pub mod source;
#[cfg(test)]
mod tests;
pub mod timers;
pub mod watch;

use error::{describe, has_position};
use handler::{HOTKEY_PRESSED, notify_error, run_handler};
use host::LuaHost;
use hotkeys::{Hotkeys, Wanted};
use registry::{Trigger, TriggerKind};
use source::Rules;
use timers::Timers;

/// Top-level code of the rule files gets this long before the load is abandoned.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything the event loop reacts to.
pub enum Event {
    /// A control socket request and where to send the answer.
    Request(Request, oneshot::Sender<Response>),
    /// The rule files changed on disk (already debounced).
    FilesChanged,
    Hypr(HyprEvent),
    /// A timer's interval passed; the trigger with this id is due.
    Timer(String),
    /// Hyprland closed its event socket: the session is over.
    HyprGone,
    Shutdown,
}

/// Why the event loop ended.
#[derive(Debug, PartialEq)]
pub enum Exit {
    /// Asked to stop. Our binds were removed.
    Shutdown,
    /// Hyprland went away under us; there was nothing left to clean up.
    HyprGone,
}

/// Reads the current rule files and settings. A closure so tests can serve
/// them from memory.
pub type Loader = Box<dyn Fn() -> Result<Rules, String> + Send>;

/// Facts about this daemon: what `status` reports, and how Hyprland reaches
/// it back.
pub struct Info {
    pub config_dir: PathBuf,
    pub hyprland_instance: String,
    /// The shell command that fires a trigger (see `hotkeys::trigger_command`).
    pub trigger_command: String,
}

pub struct Runtime {
    loader: Loader,
    backends: Backends,
    info: Info,
    host: LuaHost,
    /// Handlers and evals in flight.
    jobs: JoinSet<()>,
    /// `Some` while a reload waits for `jobs` to drain; holds who asked.
    pending_reload: Option<Vec<oneshot::Sender<Response>>>,
    hotkeys: Hotkeys,
    /// The hotkeys as of the last successful sync with Hyprland.
    synced_hotkeys: Vec<Wanted>,
    timers: Timers,
    /// Poked by the API when a rule registers or removes a hotkey.
    triggers_changed: Arc<Notify>,
    /// The window with keyboard focus, as of the last event.
    focused: Option<Focused>,
    started: Instant,
    load_error: Option<String>,
}

impl Runtime {
    /// Builds the runtime and loads the rules. Rules that fail to load leave
    /// an empty runtime behind and a notification: the daemon still starts,
    /// and the next save of the file gets another chance.
    pub async fn start(
        loader: Loader,
        backends: Backends,
        info: Info,
        events: mpsc::Sender<Event>,
    ) -> Result<Self, String> {
        let triggers_changed = Arc::new(Notify::new());
        let host = LuaHost::load(&Rules::default(), &backends, &triggers_changed).await?;
        let mut runtime = Self {
            loader,
            backends,
            hotkeys: Hotkeys::new(info.trigger_command.clone()),
            synced_hotkeys: Vec::new(),
            timers: Timers::new(events),
            triggers_changed,
            info,
            host,
            jobs: JoinSet::new(),
            pending_reload: None,
            focused: None,
            started: Instant::now(),
            load_error: None,
        };
        if let Err(message) = runtime.reload().await {
            runtime
                .report(&format!("{message} (no rules loaded)"))
                .await;
        }
        Ok(runtime)
    }

    pub async fn run(mut self, mut events: mpsc::Receiver<Event>) -> Exit {
        let exit = loop {
            tokio::select! {
                event = events.recv() => match event {
                    None | Some(Event::Shutdown) => break Exit::Shutdown,
                    Some(Event::HyprGone) => break Exit::HyprGone,
                    Some(Event::FilesChanged) => self.request_reload(None).await,
                    // A config reload drops the binds we registered at runtime.
                    Some(Event::Hypr(HyprEvent::ConfigReloaded)) => self.sync_hotkeys(true).await,
                    Some(Event::Hypr(HyprEvent::Focus(window))) => self.focus_changed(window),
                    Some(Event::Timer(id)) => self.timer_due(&id),
                    Some(Event::Request(request, reply)) => self.handle(request, reply).await,
                },
                _ = self.triggers_changed.notified() => {
                    self.sync_hotkeys(false).await;
                    self.timers.sync(&self.host.timers());
                }
                Some(finished) = self.jobs.join_next(), if !self.jobs.is_empty() => {
                    if let Err(err) = finished {
                        tracing::error!("a handler task died: {err}");
                    }
                    if self.jobs.is_empty() && self.pending_reload.is_some() {
                        self.finish_reload().await;
                    }
                }
            }
        };
        self.timers.clear();
        self.jobs.shutdown().await;
        if exit == Exit::Shutdown {
            self.hotkeys.clear(self.backends.hypr.as_ref()).await;
        }
        exit
    }

    /// Brings Hyprland's binds in step with the hotkeys of the running
    /// rules. A hotkey that cannot be bound is dropped and reported.
    ///
    /// Without `force`, a set of hotkeys that was already synced is not
    /// looked at again. `force` is for when Hyprland may have lost our binds.
    async fn sync_hotkeys(&mut self, force: bool) {
        let wanted = self.host.hotkeys();
        if !force && wanted == self.synced_hotkeys {
            return;
        }
        let report = self
            .hotkeys
            .sync(self.backends.hypr.as_ref(), &wanted)
            .await;
        for (id, message) in report.rejected {
            self.host.discard(&id);
            notify_error(&self.backends, &message).await;
        }
        match report.error {
            Some(message) => notify_error(&self.backends, &message).await,
            // Remember the set only once Hyprland really has it.
            None => self.synced_hotkeys = self.host.hotkeys(),
        }
    }

    async fn handle(&mut self, request: Request, reply: oneshot::Sender<Response>) {
        let response = match request {
            Request::Status => Response::data(&self.status()),
            Request::List => Response::data(&self.host.triggers()),
            Request::Trigger { id } => match self.fire(&id) {
                Ok(()) => Response::ok(format!("fired {id}")),
                Err(message) => Response::err(message),
            },
            Request::Eval { chunk } => {
                if self.pending_reload.is_some() {
                    Response::err("a reload is waiting for running handlers, try again in a moment")
                } else {
                    let eval = self.host.eval(chunk);
                    self.jobs.spawn(async move {
                        let _ = reply.send(match eval.await {
                            Ok(values) => Response::ok(values),
                            Err(message) => Response::err(message),
                        });
                    });
                    return;
                }
            }
            Request::Reload => return self.request_reload(Some(reply)).await,
        };
        let _ = reply.send(response);
    }

    fn status(&self) -> Status {
        Status {
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            uptime_secs: self.started.elapsed().as_secs(),
            config_dir: self.info.config_dir.display().to_string(),
            hyprland_instance: self.info.hyprland_instance.clone(),
            files: self.host.files().to_vec(),
            triggers: self.host.trigger_count(),
            running: self.jobs.len(),
            reload_pending: self.pending_reload.is_some(),
            load_error: self.load_error.clone(),
        }
    }

    /// A timer went off. A run that is still going from the last tick is not
    /// queued behind; the tick is skipped.
    fn timer_due(&mut self, id: &str) {
        if self.pending_reload.is_some() {
            return;
        }
        let Some(trigger) = self.host.trigger(id) else {
            return;
        };
        if !matches!(trigger.kind, TriggerKind::Every(_)) {
            return;
        }
        if trigger.gate.try_lock().is_err() {
            tracing::warn!("{id}: still running from its last tick, skipping this one");
            return;
        }
        self.spawn_handler(trigger, MultiValue::new());
    }

    /// Focus moved. Blur handlers run for the window that lost it, then focus
    /// handlers for the one that got it. Handlers get `{class, title, address}`.
    fn focus_changed(&mut self, window: Option<Focused>) {
        if window == self.focused {
            return;
        }
        let previous = std::mem::replace(&mut self.focused, window);
        if self.pending_reload.is_some() {
            return;
        }
        let rounds = [(previous, true), (self.focused.clone(), false)];
        for (window, blur) in rounds {
            let Some(window) = window else {
                continue;
            };
            for trigger in self.host.focus_triggers(&window, blur) {
                match self.host.window_table(&window) {
                    Ok(table) => self
                        .spawn_handler(trigger, MultiValue::from_iter([mlua::Value::Table(table)])),
                    Err(err) => tracing::error!("cannot describe the window to a rule: {err}"),
                }
            }
        }
    }

    /// Starts the handler of trigger `id` as a job. Errors it raises are
    /// reported by the job; the caller only learns whether it was started.
    fn fire(&mut self, id: &str) -> Result<(), String> {
        if self.pending_reload.is_some() {
            return Err(format!(
                "a reload is waiting for running handlers, '{id}' was not fired"
            ));
        }
        let trigger = self
            .host
            .trigger(id)
            .ok_or_else(|| format!("no trigger named '{id}'"))?;
        self.spawn_handler(trigger, MultiValue::new());
        Ok(())
    }

    fn spawn_handler(&mut self, trigger: Trigger, args: MultiValue) {
        let backends = self.backends.clone();
        tracing::info!("{}: fired ({})", trigger.id, trigger.origin);
        let started = Instant::now();
        let pressed = matches!(trigger.kind, TriggerKind::Hotkey(_)).then_some(started);
        self.jobs.spawn(HOTKEY_PRESSED.scope(pressed, async move {
            match run_handler(&trigger, args).await {
                Ok(()) => tracing::info!(
                    "{}: done in {:.1}s",
                    trigger.id,
                    started.elapsed().as_secs_f64()
                ),
                Err(err) => {
                    let mut message = describe(&err);
                    // A tail call (`return om.llm(...)`) leaves no line behind;
                    // point at where the handler was registered instead.
                    if !has_position(&message) {
                        message = format!("{}: {message}", trigger.origin);
                    }
                    notify_error(&backends, &message).await;
                }
            }
        }));
    }

    /// Reloads now if nothing is running, otherwise as soon as the running
    /// handlers finish: a reload never pulls the Lua state from under one.
    async fn request_reload(&mut self, reply: Option<oneshot::Sender<Response>>) {
        self.pending_reload.get_or_insert_default().extend(reply);
        if self.jobs.is_empty() {
            self.finish_reload().await;
        } else {
            tracing::info!(
                "reload queued behind {} running handler(s)",
                self.jobs.len()
            );
        }
    }

    async fn finish_reload(&mut self) {
        let waiting = self.pending_reload.take().unwrap_or_default();
        let response = match self.reload().await {
            Ok(files) => Response::ok(format!("reloaded {files} file(s)")),
            Err(message) => {
                let message = format!("{message} (reload failed, previous rules kept)");
                self.report(&message).await;
                Response::err(message)
            }
        };
        for reply in waiting {
            let _ = reply.send(response.clone());
        }
    }

    /// Loads the rule files into a new Lua state and swaps it in. On any
    /// failure the current state stays untouched.
    async fn reload(&mut self) -> Result<usize, String> {
        let rules = (self.loader)()?;
        let loading = LuaHost::load(&rules, &self.backends, &self.triggers_changed);
        let host = match timeout(LOAD_TIMEOUT, loading).await {
            Ok(loaded) => loaded?,
            Err(_) => {
                return Err(format!(
                    "the rule files took longer than {}s to load",
                    LOAD_TIMEOUT.as_secs()
                ));
            }
        };
        self.host = host;
        self.load_error = None;
        self.sync_hotkeys(false).await;
        self.timers.sync(&self.host.timers());
        tracing::info!(
            "loaded {} file(s), {} trigger(s)",
            rules.sources.len(),
            self.host.trigger_count()
        );
        Ok(rules.sources.len())
    }

    /// A rule problem the runtime itself found: log, remember, notify.
    async fn report(&mut self, message: &str) {
        self.load_error = Some(message.to_string());
        notify_error(&self.backends, message).await;
    }
}
