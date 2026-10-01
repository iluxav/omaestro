//! Answering the control socket (status, list, trigger, eval, reload,
//! enable, disable) and the timer ticks.

use mlua::MultiValue;
use tokio::sync::oneshot;

use super::handler::notify_error;
use super::registry::TriggerKind;
use super::{Request, Response, Runtime, Status};
use crate::ipc::TriggerRow;

impl Runtime {
    pub(super) async fn handle(&mut self, request: Request, reply: oneshot::Sender<Response>) {
        let response = match request {
            Request::Status => Response::data(&self.status()),
            Request::List => Response::data(&self.rows()),
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
            Request::Enable { id } => self.set_enabled(&id, true).await,
            Request::Disable { id } => self.set_enabled(&id, false).await,
            Request::Override { on } => self.set_override(on).await,
        };
        let _ = reply.send(response);
    }

    /// The triggers, with what the bind sync knows about the hotkeys.
    fn rows(&self) -> Vec<TriggerRow> {
        let mut rows = self.host.triggers();
        for row in &mut rows {
            row.problem = self.hotkeys.refused().get(&row.id).cloned();
            row.overrides = self.hotkeys.displaced().get(&row.id).cloned();
            if matches!(
                row.kind.as_str(),
                "hotkey" | "app_hotkey" | "mode" | "mode_key" | "mode_exit"
            ) {
                row.bound = Some(self.hotkeys.is_applied(&row.id));
            }
        }
        rows
    }

    /// Lets rules take chords Hyprland already has, or stops that. Turning
    /// it on binds the refused chords now; turning it off reloads Hyprland's
    /// config so the displaced binds come back (the sync after that refuses
    /// the conflicting rules again).
    async fn set_override(&mut self, on: bool) -> Response {
        if let Err(message) = self.settings.set_override(on) {
            notify_error(&self.backends, &message).await;
        }
        let restore = self.hotkeys.set_override(on);
        if on {
            self.sync_hotkeys(true).await;
            let taken = self.hotkeys.displaced().len();
            tracing::info!("override on, {taken} chord(s) taken");
            Response::ok(format!(
                "override on; {taken} chord(s) taken from other binds"
            ))
        } else {
            if restore {
                self.restore_binds().await;
            }
            tracing::info!("override off");
            Response::ok("override off; rules give way to Hyprland's own binds")
        }
    }

    /// Switches a trigger on or off, remembers the choice on disk, and
    /// brings binds, timers and watches in step. An id that is neither
    /// registered nor remembered as off is unknown.
    async fn set_enabled(&mut self, id: &str, enabled: bool) -> Response {
        if !self.host.has_trigger(id) && !self.settings.disabled().contains(id) {
            return Response::err(format!("no trigger named '{id}'"));
        }
        if let Err(message) = self.settings.set_enabled(id, enabled) {
            // The switch still holds, until the daemon restarts.
            notify_error(&self.backends, &message).await;
        }
        self.host.set_enabled(id, enabled);
        self.sync_all().await;
        let what = if enabled { "enabled" } else { "disabled" };
        tracing::info!("{what} {id}");
        Response::ok(format!("{what} {id}"))
    }

    pub(super) fn status(&self) -> Status {
        Status {
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            uptime_secs: self.started.elapsed().as_secs(),
            config_dir: self.info.config_dir.display().to_string(),
            hyprland_instance: self.info.hyprland_instance.clone(),
            files: self.host.files().to_vec(),
            triggers: self.host.trigger_count(),
            disabled: self.host.disabled_count(),
            override_binds: self.settings.override_binds(),
            running: self.jobs.len(),
            reload_pending: self.pending_reload.is_some(),
            load_error: self.load_error.clone(),
        }
    }

    /// A timer went off. A run that is still going from the last tick is not
    /// queued behind; the tick is skipped.
    pub(super) fn timer_due(&mut self, id: &str) {
        if self.pending_reload.is_some() {
            return;
        }
        let Some(trigger) = self.host.trigger(id) else {
            return;
        };
        match &trigger.kind {
            TriggerKind::Every(_) => {
                if trigger.gate.try_lock().is_err() {
                    tracing::warn!("{id}: still running from its last tick, skipping this one");
                    return;
                }
                self.spawn_handler(trigger, MultiValue::new());
            }
            TriggerKind::After(_) => {
                self.host.discard(id);
                self.spawn_handler(trigger, MultiValue::new());
                self.timers.sync(&self.host.timers());
            }
            TriggerKind::At { .. } => {
                self.spawn_handler(trigger, MultiValue::new());
                if let Some(next) = self.host.timers().into_iter().find(|w| w.id == id) {
                    self.timers.rearm(&next);
                }
            }
            _ => {}
        }
    }
}
