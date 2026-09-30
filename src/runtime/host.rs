//! One Lua state with the API installed and the rule files loaded into it.
//! A reload builds a new host and drops the old one.

use std::future::Future;
use std::sync::Arc;

use mlua::{Function, Lua, MultiValue, Table, Value};
use tokio::sync::Notify;

use super::error::{EVAL_CHUNK, describe, has_position};
use super::hotkeys::Wanted;
use super::registry::{Matcher, SharedRegistry, Trigger, TriggerKind};
use super::render::render;
use super::source::Rules;
use super::timers;
use crate::api;
use crate::backend::Backends;
use crate::backend::hypr::events::Focused;
use crate::ipc::TriggerRow;

pub struct LuaHost {
    lua: Lua,
    registry: SharedRegistry,
    files: Vec<String>,
}

impl LuaHost {
    /// Builds a fresh state and runs the rule files in order. The first
    /// error stops the load; its message names the file and line.
    pub async fn load(
        rules: &Rules,
        backends: &Backends,
        triggers_changed: &Arc<Notify>,
    ) -> Result<Self, String> {
        let lua = Lua::new();
        let registry = SharedRegistry::default();
        let context = api::Context {
            backends: backends.clone(),
            registry: registry.clone(),
            config: Arc::new(rules.config.clone()),
            triggers_changed: triggers_changed.clone(),
        };
        api::install(&lua, &context)
            .map_err(|err| format!("setting up the Lua state: {}", describe(&err)))?;

        let sources = &rules.sources;
        for source in sources {
            lua.load(&source.code)
                .set_name(format!("@{}", source.name))
                .exec_async()
                .await
                .map_err(|err| {
                    let message = describe(&err);
                    if has_position(&message) {
                        message
                    } else {
                        format!("{}: {message}", source.name)
                    }
                })?;
        }

        Ok(Self {
            lua,
            registry,
            files: sources.iter().map(|s| s.name.clone()).collect(),
        })
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    pub fn trigger(&self, id: &str) -> Option<Trigger> {
        self.registry.lock().get(id)
    }

    /// Every registered trigger, in id order.
    pub fn triggers(&self) -> Vec<TriggerRow> {
        self.registry
            .lock()
            .iter()
            .map(|t| TriggerRow {
                id: t.id.clone(),
                kind: t.kind.as_str().to_string(),
                origin: t.origin.clone(),
            })
            .collect()
    }

    /// The hotkeys the rules want bound.
    pub fn hotkeys(&self) -> Vec<Wanted> {
        self.registry
            .lock()
            .iter()
            .filter_map(|t| match &t.kind {
                TriggerKind::Hotkey(chord) => Some(Wanted {
                    id: t.id.clone(),
                    chord: chord.clone(),
                    origin: t.origin.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The timers the rules want.
    pub fn timers(&self) -> Vec<timers::Wanted> {
        self.registry
            .lock()
            .iter()
            .filter_map(|t| match &t.kind {
                TriggerKind::Every(interval) => Some(timers::Wanted {
                    id: t.id.clone(),
                    interval: *interval,
                }),
                _ => None,
            })
            .collect()
    }

    /// The focus (or blur) triggers whose matcher fits `window`.
    pub fn focus_triggers(&self, window: &Focused, blur: bool) -> Vec<Trigger> {
        self.registry
            .lock()
            .iter()
            .filter(|t| match (&t.kind, blur) {
                (TriggerKind::Focus(m), false) | (TriggerKind::Blur(m), true) => {
                    self.matches(m, window)
                }
                _ => false,
            })
            .cloned()
            .collect()
    }

    /// Lua-pattern matching, done by Lua's own `string.find`.
    fn matches(&self, matcher: &Matcher, window: &Focused) -> bool {
        let find = self
            .lua
            .globals()
            .get::<Table>("string")
            .and_then(|s| s.get::<Function>("find"));
        let Ok(find) = find else {
            return false;
        };
        [
            (&matcher.class, &window.class),
            (&matcher.title, &window.title),
        ]
        .into_iter()
        .all(|(pattern, text)| match pattern {
            None => true,
            Some(pattern) => matches!(
                find.call::<Value>((text.as_str(), pattern.as_str())),
                Ok(value) if value != Value::Nil
            ),
        })
    }

    /// `{class=, title=, address=}` for a handler's argument.
    pub fn window_table(&self, window: &Focused) -> mlua::Result<Table> {
        let table = self.lua.create_table()?;
        table.set("class", window.class.as_str())?;
        table.set("title", window.title.as_str())?;
        table.set("address", window.address.as_str())?;
        Ok(table)
    }

    /// Forgets a trigger, for a hotkey Hyprland would not take.
    pub fn discard(&self, id: &str) {
        self.registry.lock().discard(id);
    }

    pub fn trigger_count(&self) -> usize {
        self.registry.lock().len()
    }

    /// Runs a chunk in this state (an expression or statements, like the Lua
    /// REPL) and renders what it returned. This backs `om eval`, the
    /// debugging command: arbitrary code by design, reachable only through
    /// the control socket, which is owner-only (see `ipc::bind`).
    pub fn eval(
        &self,
        chunk: String,
    ) -> impl Future<Output = Result<Vec<String>, String>> + Send + 'static {
        let lua = self.lua.clone();
        async move {
            let values = lua
                .load(chunk)
                .set_name(format!("={EVAL_CHUNK}"))
                .eval_async::<MultiValue>()
                .await
                .map_err(|err| describe(&err))?;
            Ok(values.iter().map(render).collect())
        }
    }
}
