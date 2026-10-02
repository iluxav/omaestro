//! One Lua state with the API installed and the rule files loaded into it.
//! A reload builds a new host and drops the old one.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;

use mlua::{Function, Lua, MultiValue, Table, Value};
use tokio::sync::Notify;

use super::error::{EVAL_CHUNK, describe, has_position};
use super::hotkeys::Wanted;
use super::registry::{Matcher, Registry, SharedRegistry, Trigger, TriggerKind};

/// Which window event a matcher is checked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowEvent {
    Focus,
    Blur,
    Open,
    Close,
    Title,
}
use super::files;
use super::render::render;
use super::source::Rules;
use super::timers;
use super::typed;
use crate::api;
use crate::backend::Backends;
use crate::backend::hypr::events::WinRef;
use crate::ipc::TriggerRow;

pub struct LuaHost {
    lua: Lua,
    registry: SharedRegistry,
    files: Vec<String>,
}

impl LuaHost {
    /// Builds a fresh state and runs the rule files in order. The first
    /// error stops the load; its message names the file and line.
    /// `disabled` are the trigger ids the user switched off.
    pub async fn load(
        rules: &Rules,
        backends: &Backends,
        selection: &super::selection::Selection,
        triggers_changed: &Arc<Notify>,
        state_dir: &std::path::Path,
        config_dir: &std::path::Path,
        disabled: &BTreeSet<String>,
    ) -> Result<Self, String> {
        let lua = Lua::new();
        let registry = SharedRegistry::new(Registry::with_disabled(disabled.clone()));
        let context = api::Context {
            backends: backends.clone(),
            registry: registry.clone(),
            config: Arc::new(rules.config.clone()),
            triggers_changed: triggers_changed.clone(),
            state_dir: state_dir.to_path_buf(),
            config_dir: config_dir.to_path_buf(),
            selection: selection.clone(),
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

    /// The trigger with this id, if it is registered and switched on.
    pub fn trigger(&self, id: &str) -> Option<Trigger> {
        let registry = self.registry.lock();
        registry.get(id).filter(|t| registry.is_enabled(t))
    }

    /// Whether a trigger with this id is registered, on or off.
    pub fn has_trigger(&self, id: &str) -> bool {
        self.registry.lock().get(id).is_some()
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) {
        self.registry.lock().set_enabled(id, enabled);
    }

    /// Every registered trigger, on or off, in id order.
    pub fn triggers(&self) -> Vec<TriggerRow> {
        let registry = self.registry.lock();
        registry
            .iter()
            .map(|t| TriggerRow {
                id: t.id.clone(),
                kind: t.kind.as_str().to_string(),
                detail: t.kind.detail(),
                label: t.label.clone(),
                origin: t.origin.clone(),
                enabled: registry.is_enabled(t),
                // Filled in by the runtime, which knows the bind state.
                problem: None,
                overrides: None,
                bound: None,
            })
            .collect()
    }

    /// The binds the rules want right now: hotkeys, mode entries, the keys
    /// inside each mode's submap, and the app hotkeys whose window has
    /// `focused`. Two app hotkeys matching the same window on one chord
    /// would replace each other; the first registered wins.
    pub fn hotkeys(&self, focused: Option<&WinRef>) -> Vec<Wanted> {
        use super::hotkeys::Action;
        let registry = self.registry.lock();
        let mut wanted: Vec<Wanted> = registry
            .enabled()
            .filter_map(|t| {
                let (chord, submap, action) = match &t.kind {
                    TriggerKind::Hotkey(chord) => (chord, String::new(), Action::Trigger),
                    TriggerKind::Mode { chord, submap, .. } => {
                        (chord, String::new(), Action::Submap(submap.clone()))
                    }
                    TriggerKind::ModeKey { submap, chord, .. } => {
                        (chord, submap.clone(), Action::Trigger)
                    }
                    TriggerKind::ModeExit { submap, chord } => {
                        (chord, submap.clone(), Action::Submap("reset".into()))
                    }
                    _ => return None,
                };
                Some(Wanted {
                    id: t.id.clone(),
                    chord: chord.clone(),
                    origin: t.origin.clone(),
                    submap,
                    action,
                })
            })
            .collect();
        let Some(window) = focused else {
            return wanted;
        };
        let mut app: Vec<&Trigger> = registry
            .enabled()
            .filter(|t| match &t.kind {
                TriggerKind::AppHotkey { matcher, .. } => self.matches(matcher, window),
                _ => false,
            })
            .collect();
        app.sort_by_key(|t| t.serial);
        for t in app {
            let TriggerKind::AppHotkey { chord, .. } = &t.kind else {
                continue;
            };
            if wanted
                .iter()
                .any(|w| w.submap.is_empty() && w.chord.same_keys(chord))
            {
                tracing::warn!(
                    "{}: {chord} is also wanted by an earlier rule for this window; skipped",
                    t.origin
                );
                continue;
            }
            wanted.push(Wanted {
                id: t.id.clone(),
                chord: chord.clone(),
                origin: t.origin.clone(),
                submap: String::new(),
                action: Action::Trigger,
            });
        }
        wanted
    }

    /// Whether any rule has an app hotkey, so focus changes must re-sync.
    pub fn has_app_hotkeys(&self) -> bool {
        self.registry
            .lock()
            .enabled()
            .any(|t| matches!(t.kind, TriggerKind::AppHotkey { .. }))
    }

    /// The system sources the rules listen to.
    pub fn system_sources(&self) -> std::collections::HashSet<crate::backend::SystemSource> {
        self.registry
            .lock()
            .enabled()
            .filter_map(|t| match &t.kind {
                TriggerKind::System(kind) => Some(kind.source()),
                _ => None,
            })
            .collect()
    }

    /// The paths `om.on_file` rules watch.
    pub fn watched_paths(&self) -> Vec<files::Wanted> {
        self.registry
            .lock()
            .enabled()
            .filter_map(|t| match &t.kind {
                TriggerKind::File(path) => Some(files::Wanted {
                    id: t.id.clone(),
                    path: path.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The texts `om.on_typed` rules wait for.
    pub fn typed(&self) -> Vec<typed::Watched> {
        self.registry
            .lock()
            .enabled()
            .filter_map(|t| match &t.kind {
                TriggerKind::Typed(text) => Some(typed::Watched {
                    id: t.id.clone(),
                    text: text.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The hint of the mode living in `submap`, if a rule made one.
    pub fn mode_hint(&self, submap: &str) -> Option<Option<String>> {
        self.registry.lock().enabled().find_map(|t| match &t.kind {
            TriggerKind::Mode {
                submap: s, hint, ..
            } if s == submap => Some(hint.clone()),
            _ => None,
        })
    }

    /// The timers the rules want, with the delay until each one's next tick.
    pub fn timers(&self) -> Vec<timers::Wanted> {
        self.registry
            .lock()
            .enabled()
            .filter_map(|t| match &t.kind {
                TriggerKind::Every(interval) => Some(timers::Wanted {
                    id: t.id.clone(),
                    delay: *interval,
                    repeat: true,
                }),
                TriggerKind::After(delay) => Some(timers::Wanted {
                    id: t.id.clone(),
                    delay: *delay,
                    repeat: false,
                }),
                TriggerKind::At { hour, minute } => Some(timers::Wanted {
                    id: t.id.clone(),
                    delay: self.until_clock(*hour, *minute),
                    repeat: false,
                }),
                _ => None,
            })
            .collect()
    }

    /// Seconds until the next `hour:minute` on the local clock, by Lua's
    /// own `os.time`, which knows the time zone and daylight saving.
    fn until_clock(&self, hour: u32, minute: u32) -> std::time::Duration {
        let secs = self
            .lua
            .load(
                "local h, m = ...\n\
                 local now = os.time()\n\
                 local t = os.date('*t', now)\n\
                 t.hour, t.min, t.sec = h, m, 0\n\
                 local target = os.time(t)\n\
                 if target <= now then target = target + 86400 end\n\
                 return target - now",
            )
            .call::<u64>((hour, minute))
            .unwrap_or(86400);
        std::time::Duration::from_secs(secs.max(1))
    }

    /// The triggers of one window-event kind whose matcher fits `window`,
    /// in the order the rules registered them.
    pub fn window_triggers(&self, which: WindowEvent, window: &WinRef) -> Vec<Trigger> {
        let mut triggers: Vec<Trigger> = self
            .registry
            .lock()
            .enabled()
            .filter(|t| match (&t.kind, which) {
                (TriggerKind::Focus(m), WindowEvent::Focus)
                | (TriggerKind::Blur(m), WindowEvent::Blur)
                | (TriggerKind::Open(m), WindowEvent::Open)
                | (TriggerKind::Close(m), WindowEvent::Close)
                | (TriggerKind::Title(m), WindowEvent::Title) => self.matches(m, window),
                _ => false,
            })
            .cloned()
            .collect();
        triggers.sort_by_key(|t| t.serial);
        triggers
    }

    /// The triggers with no matcher of one kind (workspace, monitor, ...),
    /// in the order the rules registered them.
    pub fn plain_triggers(&self, kind: &TriggerKind) -> Vec<Trigger> {
        let mut triggers: Vec<Trigger> = self
            .registry
            .lock()
            .enabled()
            .filter(|t| &t.kind == kind)
            .cloned()
            .collect();
        triggers.sort_by_key(|t| t.serial);
        triggers
    }

    /// Lua-pattern matching, done by Lua's own `string.find`.
    fn matches(&self, matcher: &Matcher, window: &WinRef) -> bool {
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

    /// A window object for a handler's argument: the facts the event
    /// carried, with the same methods `om.window()` returns.
    pub fn window_table(&self, window: &WinRef) -> mlua::Result<Table> {
        api::window::from_event(&self.lua, window)
    }

    /// A Lua string for a handler's argument.
    pub fn lua_string(&self, text: &str) -> mlua::Result<Value> {
        Ok(Value::String(self.lua.create_string(text)?))
    }

    /// A plain table for a handler's argument.
    pub fn table_of(&self, ints: &[(&str, i64)], strings: &[(&str, &str)]) -> mlua::Result<Table> {
        let table = self.lua.create_table()?;
        for (key, value) in ints {
            table.set(*key, *value)?;
        }
        for (key, value) in strings {
            table.set(*key, *value)?;
        }
        Ok(table)
    }

    /// Forgets a trigger, for a hotkey Hyprland would not take.
    pub fn discard(&self, id: &str) {
        self.registry.lock().discard(id);
    }

    /// How many triggers are registered, on or off.
    pub fn trigger_count(&self) -> usize {
        self.registry.lock().len()
    }

    /// How many of them are off.
    pub fn disabled_count(&self) -> usize {
        let registry = self.registry.lock();
        registry.iter().filter(|t| !registry.is_enabled(t)).count()
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
