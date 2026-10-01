//! The watches that follow the rules: keyboards for `om.on_typed`, the
//! clipboard for `om.on_clipboard`, paths for `om.on_file`. Each starts
//! when a rule wants it and stops when none does.

use mlua::MultiValue;

use super::handler::notify_error;
use super::registry::{SystemKind, TriggerKind};
use super::{Event, Runtime, files, typed};
use crate::backend::SystemEvent;

impl Runtime {
    /// Starts keyboard monitoring while a rule watches typed text, stops it
    /// when none does. `restart` reopens the keyboards (after a hotplug).
    pub(super) async fn sync_typed(&mut self, restart: bool) {
        let watched = self.host.typed();
        if watched.is_empty() {
            self.typed_watch = None;
            self.typed.clear();
            return;
        }
        if restart {
            self.typed_watch = None;
        }
        if self.typed_watch.is_some() {
            return;
        }
        match self.backends.keyboards.watch(self.events.clone()) {
            Ok(watching) => self.typed_watch = Some(watching),
            Err(err) => {
                if !self.typed_error_shown {
                    self.typed_error_shown = true;
                    let origin = self
                        .host
                        .trigger(&watched[0].id)
                        .map(|t| t.origin)
                        .unwrap_or_default();
                    notify_error(&self.backends, &format!("{origin}: om.on_typed: {err}")).await;
                }
            }
        }
    }

    /// Clipboard and file watches follow the rules: started while one wants
    /// them, stopped when none does. A path that cannot be watched drops its
    /// trigger and is reported.
    pub(super) async fn sync_watches(&mut self) {
        let wants_clipboard = !self.host.plain_triggers(&TriggerKind::Clipboard).is_empty();
        if !wants_clipboard {
            self.clip_watch = None;
        } else if self.clip_watch.is_none() {
            match self.backends.clipboard.watch(self.events.clone()) {
                Ok(watching) => self.clip_watch = Some(watching),
                Err(err) => {
                    if !self.clip_error_shown {
                        self.clip_error_shown = true;
                        notify_error(&self.backends, &format!("om.on_clipboard: {err}")).await;
                    }
                }
            }
        }
        for (id, message) in self.files.sync(&self.host.watched_paths(), &self.events) {
            let origin = self.host.trigger(&id).map(|t| t.origin).unwrap_or_default();
            self.host.discard(&id);
            notify_error(&self.backends, &format!("{origin}: {message}")).await;
        }
    }

    /// System sources follow the rules too: one process per source, only
    /// while something listens.
    pub(super) async fn sync_system(&mut self) {
        let wanted = self.host.system_sources();
        self.system_watches
            .retain(|source, _| wanted.contains(source));
        for source in wanted {
            if self.system_watches.contains_key(&source) {
                continue;
            }
            match self.backends.system.watch(source, self.events.clone()) {
                Ok(watching) => {
                    self.system_watches.insert(source, watching);
                }
                Err(err) => {
                    notify_error(&self.backends, &format!("{source:?} events: {err}")).await
                }
            }
        }
    }

    pub(super) fn system_event(&mut self, event: SystemEvent) {
        if self.pending_reload.is_some() {
            return;
        }
        let kind = match &event {
            SystemEvent::Sleep => SystemKind::Sleep,
            SystemEvent::Wake => SystemKind::Wake,
            SystemEvent::Usb { .. } => SystemKind::Usb,
            SystemEvent::Battery { .. } => SystemKind::Battery,
            SystemEvent::Network { .. } => SystemKind::Network,
        };
        for trigger in self.host.plain_triggers(&TriggerKind::System(kind)) {
            let table = match &event {
                SystemEvent::Sleep | SystemEvent::Wake => None,
                SystemEvent::Usb { action, device } => Some(
                    self.host
                        .table_of(&[], &[("action", action), ("device", device)]),
                ),
                SystemEvent::Battery { percent, status } => Some(
                    self.host
                        .table_of(&[("percent", *percent)], &[("status", status)]),
                ),
                SystemEvent::Network { line } => Some(self.host.table_of(&[], &[("line", line)])),
            };
            match table {
                None => self.spawn_handler(trigger, MultiValue::new()),
                Some(Ok(table)) => {
                    self.spawn_handler(trigger, MultiValue::from_iter([mlua::Value::Table(table)]))
                }
                Some(Err(err)) => tracing::error!("cannot describe the event to a rule: {err}"),
            }
        }
    }

    /// The clipboard changed: read it (in a job, it is a process call) and
    /// come back with the text.
    pub(super) fn clipboard_changed(&mut self) {
        if self.host.plain_triggers(&TriggerKind::Clipboard).is_empty() {
            return;
        }
        let clipboard = self.backends.clipboard.clone();
        let events = self.events.clone();
        self.jobs.spawn(async move {
            let text = match clipboard.get().await {
                Ok(Some(content))
                    if content.mime.starts_with("text/") || content.mime.contains("STRING") =>
                {
                    String::from_utf8_lossy(&content.data).into_owned()
                }
                Ok(_) => String::new(),
                Err(err) => {
                    tracing::warn!("could not read the clipboard after a change: {err}");
                    return;
                }
            };
            let _ = events.send(Event::ClipboardText(text)).await;
        });
    }

    pub(super) fn clipboard_text(&mut self, text: &str) {
        if self.pending_reload.is_some() {
            return;
        }
        for trigger in self.host.plain_triggers(&TriggerKind::Clipboard) {
            match self.host.lua_string(text) {
                Ok(value) => self.spawn_handler(trigger, MultiValue::from_iter([value])),
                Err(err) => tracing::error!("cannot hand the clipboard to a rule: {err}"),
            }
        }
    }

    pub(super) fn file_changed(&mut self, change: files::Change) {
        if self.pending_reload.is_some() {
            return;
        }
        let Some(trigger) = self.host.trigger(&change.id) else {
            return;
        };
        let path = change.path.display().to_string();
        match self
            .host
            .table_of(&[], &[("path", &path), ("kind", change.kind)])
        {
            Ok(table) => {
                self.spawn_handler(trigger, MultiValue::from_iter([mlua::Value::Table(table)]))
            }
            Err(err) => tracing::error!("cannot describe the file change to a rule: {err}"),
        }
    }

    /// A key was typed. When the buffer ends with a watched text, that text
    /// is erased from the window and its handler runs.
    pub(super) fn typed_key(&mut self, key: typed::Key) {
        let watched = self.host.typed();
        let Some(hit) = self.typed.feed(key, &watched) else {
            return;
        };
        if self.pending_reload.is_some() {
            return;
        }
        let Some(trigger) = self.host.trigger(&hit.id) else {
            return;
        };
        let injector = self.backends.injector.clone();
        let count = hit.text.chars().count();
        self.spawn_handler_after(trigger, MultiValue::new(), async move {
            if let Err(err) = injector.erase(count).await {
                tracing::warn!("could not erase the typed text: {err}");
            }
        });
    }
}
