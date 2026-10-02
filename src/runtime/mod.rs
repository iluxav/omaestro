//! The runtime: one event loop that owns the Lua state, receives events,
//! dispatches them to Lua handlers and answers the control socket.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mlua::MultiValue;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::backend::Backends;
use crate::backend::hypr::events::{HyprEvent, WinRef};
use crate::ipc::{Request, Response, Status};

pub mod error;
mod events;
pub mod files;
pub mod handler;
pub mod host;
pub mod hotkeys;
pub mod registry;
pub mod render;
mod requests;
pub mod selection;
mod settings;
pub mod source;
#[cfg(test)]
mod tests;
pub mod timers;
pub mod typed;
pub mod watch;
mod watchers;

use error::{describe, has_position};
use handler::{BUSY, HOTKEY_PRESSED, notify_error, run_handler};
use host::LuaHost;
use hotkeys::{Hotkeys, Wanted};
use registry::{Trigger, TriggerKind};
use settings::Settings;
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
    /// A key the user typed (only sent while a rule watches typed text).
    #[cfg_attr(not(feature = "typed"), allow(dead_code))]
    Typed(typed::Key),
    /// A keyboard came or went; the typed-text monitor should start over.
    #[cfg_attr(not(feature = "typed"), allow(dead_code))]
    TypedRescan,
    /// The clipboard changed (only sent while a rule watches it).
    ClipboardChanged,
    /// The clipboard's text, read after a change.
    ClipboardText(String),
    /// The primary selection changed (always watched, see `selection`).
    SelectionChanged,
    /// Something under a watched path changed.
    File(files::Change),
    /// Sleep, wake, USB, battery, network.
    System(crate::backend::SystemEvent),
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
    /// Where `om.store` keeps its file.
    pub state_dir: PathBuf,
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
    /// The recent keystrokes, while any rule watches typed text.
    typed: typed::Typed,
    /// The keyboards being read, while any rule watches typed text.
    typed_watch: Option<crate::backend::Watching>,
    /// The clipboard watch, while any rule watches it.
    clip_watch: Option<crate::backend::Watching>,
    clip_error_shown: bool,
    files: files::Watches,
    /// The system sources being watched, while rules listen to them.
    system_watches: HashMap<crate::backend::SystemSource, crate::backend::Watching>,
    /// So that a keyboard that cannot be read is reported once per load.
    typed_error_shown: bool,
    /// For starting the typed-text monitor.
    events: mpsc::Sender<Event>,
    /// Poked by the API when a rule registers or removes a hotkey.
    triggers_changed: Arc<Notify>,
    /// What the user set from the panel or the command line, on disk.
    settings: Settings,
    /// The window with keyboard focus, as of the last event.
    focused: Option<WinRef>,
    /// Where the primary selection was made, shared with `om.selection`.
    selection: selection::Selection,
    /// The primary selection watch; `None` if it could not start.
    selection_watch: Option<crate::backend::Watching>,
    /// What is known about open windows, so a close event can name its
    /// window. Filled from open, focus and title events.
    known: HashMap<String, WinRef>,
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
        let (settings, settings_problem) = Settings::load(&info.state_dir);
        let selection = selection::Selection::default();
        let selection_watch = match backends.clipboard.watch_selection(events.clone()) {
            Ok(watching) => Some(watching),
            Err(err) => {
                tracing::warn!("{err}; om.selection cannot tell a stale selection");
                None
            }
        };
        selection.set_watching(selection_watch.is_some());
        let host = LuaHost::load(
            &Rules::default(),
            &backends,
            &selection,
            &triggers_changed,
            &info.state_dir,
            &info.config_dir,
            settings.disabled(),
        )
        .await?;
        let mut hotkeys = Hotkeys::new(info.trigger_command.clone());
        hotkeys.set_override(settings.override_binds());
        let mut runtime = Self {
            loader,
            backends,
            hotkeys,
            synced_hotkeys: Vec::new(),
            timers: Timers::new(events.clone()),
            typed: typed::Typed::default(),
            typed_watch: None,
            clip_watch: None,
            clip_error_shown: false,
            files: files::Watches::default(),
            system_watches: HashMap::new(),
            typed_error_shown: false,
            events,
            triggers_changed,
            settings,
            info,
            host,
            jobs: JoinSet::new(),
            pending_reload: None,
            focused: None,
            selection,
            selection_watch,
            known: HashMap::new(),
            started: Instant::now(),
            load_error: None,
        };
        if let Some(message) = settings_problem {
            notify_error(&runtime.backends, &message).await;
        }
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
                    Some(Event::Hypr(HyprEvent::Focus(window))) => {
                        self.focus_changed(window);
                        self.sync_app_hotkeys().await;
                    }
                    Some(Event::Hypr(event)) => {
                        self.window_event(event);
                        self.sync_app_hotkeys().await;
                    }
                    Some(Event::Timer(id)) => self.timer_due(&id),
                    Some(Event::Typed(key)) => self.typed_key(key),
                    Some(Event::TypedRescan) => self.sync_typed(true).await,
                    Some(Event::ClipboardChanged) => self.clipboard_changed(),
                    Some(Event::ClipboardText(text)) => self.clipboard_text(&text),
                    Some(Event::SelectionChanged) => self.selection.changed(),
                    Some(Event::File(change)) => self.file_changed(change),
                    Some(Event::System(event)) => self.system_event(event),
                    Some(Event::Request(request, reply)) => self.handle(request, reply).await,
                },
                _ = self.triggers_changed.notified() => self.sync_all().await,
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
        self.files.clear();
        self.system_watches.clear();
        self.clip_watch = None;
        self.selection_watch = None;
        self.typed_watch = None;
        self.jobs.shutdown().await;
        if exit == Exit::Shutdown && self.hotkeys.clear(self.backends.hypr.as_ref()).await {
            self.restore_binds().await;
        }
        exit
    }

    /// App hotkeys follow the focus: bound while a matching window has it.
    /// Cheap when nothing changed, as `sync_hotkeys` compares first.
    async fn sync_app_hotkeys(&mut self) {
        if self.host.has_app_hotkeys() {
            self.sync_hotkeys(false).await;
        }
    }

    /// Brings the binds, timers and watches in step with the triggers that
    /// are registered and switched on.
    async fn sync_all(&mut self) {
        self.sync_hotkeys(false).await;
        self.timers.sync(&self.host.timers());
        self.sync_typed(false).await;
        self.sync_watches().await;
        self.sync_system().await;
    }

    /// Brings Hyprland's binds in step with the hotkeys of the running
    /// rules. A hotkey that cannot be bound is dropped and reported.
    ///
    /// Without `force`, a set of hotkeys that was already synced is not
    /// looked at again. `force` is for when Hyprland may have lost our binds.
    async fn sync_hotkeys(&mut self, force: bool) {
        let wanted = self.host.hotkeys(self.focused.as_ref());
        if !force && wanted == self.synced_hotkeys {
            return;
        }
        let report = self
            .hotkeys
            .sync(self.backends.hypr.as_ref(), &wanted)
            .await;
        for (_, message) in report.rejected {
            notify_error(&self.backends, &message).await;
        }
        for (_, message) in report.taken {
            tell(&self.backends, &message).await;
        }
        if report.restore {
            self.restore_binds().await;
        }
        match report.error {
            Some(message) => notify_error(&self.backends, &message).await,
            // Remember the set only once Hyprland really has it.
            None => self.synced_hotkeys = wanted,
        }
    }

    /// Gives displaced binds back. Only Hyprland's config knows what they
    /// were, so it is re-read; the `configreloaded` event that follows
    /// re-syncs ours.
    async fn restore_binds(&mut self) {
        tracing::info!("reloading Hyprland's config to give its binds back");
        if let Err(err) = self.backends.hypr.reload().await {
            notify_error(
                &self.backends,
                &format!("could not reload Hyprland's config to give its binds back: {err}"),
            )
            .await;
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
        let trigger = match self.host.trigger(id) {
            Some(trigger) => trigger,
            None if self.host.has_trigger(id) => {
                return Err(format!(
                    "'{id}' is switched off; `om enable '{id}'` turns it back on"
                ));
            }
            None => return Err(format!("no trigger named '{id}'")),
        };
        if let TriggerKind::ModeKey { once: true, .. } = &trigger.kind {
            let hypr = self.backends.hypr.clone();
            self.jobs.spawn(async move {
                if let Err(err) = hypr.dispatch("hl.dsp.submap(\"reset\")").await {
                    tracing::warn!("could not leave the mode: {err}");
                }
            });
        }
        self.spawn_handler(trigger, MultiValue::new());
        Ok(())
    }

    fn spawn_handler(&mut self, trigger: Trigger, args: MultiValue) {
        self.spawn_handler_after(trigger, args, async {});
    }

    /// Like `spawn_handler`, with `before` awaited first in the same job.
    fn spawn_handler_after(
        &mut self,
        trigger: Trigger,
        args: MultiValue,
        before: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        let backends = self.backends.clone();
        tracing::info!("{}: fired ({})", trigger.id, trigger.origin);
        let started = Instant::now();
        let pressed = matches!(
            trigger.kind,
            TriggerKind::Hotkey(_) | TriggerKind::AppHotkey { .. }
        )
        .then_some(started);
        let busy = std::sync::Arc::new(std::sync::Mutex::new(None));
        let busy_slot = busy.clone();
        self.jobs.spawn(HOTKEY_PRESSED.scope(
            pressed,
            BUSY.scope(busy_slot, async move {
                before.await;
                let outcome = run_handler(&trigger, args).await;
                // An om.busy notification goes away with the run that showed it,
                // before any error notification takes its place.
                let shown = busy
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(id) = shown
                    && let Err(err) = backends.notifier.close(id).await
                {
                    tracing::warn!("could not take the busy notification down: {err}");
                }
                match outcome {
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
            }),
        ));
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
        let loading = LuaHost::load(
            &rules,
            &self.backends,
            &self.selection,
            &self.triggers_changed,
            &self.info.state_dir,
            &self.info.config_dir,
            self.settings.disabled(),
        );
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
        self.typed_error_shown = false;
        self.clip_error_shown = false;
        self.sync_all().await;
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

/// Something the user should know that is not an error: a chord taken
/// from another bind.
async fn tell(backends: &Backends, message: &str) {
    tracing::info!("{message}");
    if let Err(err) = backends.notifier.notify("omaestro override", message).await {
        tracing::warn!("could not notify: {err}");
    }
}
