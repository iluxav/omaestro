//! `om.show(card, opts)`: a card over everything on the screen, a large line
//! over smaller ones, gone after `opts.seconds`. quickshell draws it
//! (`backend/ui.rs`); where nothing can, it is a notification. Returns a
//! handle: `:update(card)` changes its text while it is up, `:close()`
//! takes it down.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use mlua::{Error, FromLua, Lua, Result, Table, UserData, UserDataMethods, Value};

use super::Context;
use crate::backend::{BackendError, Card, Position, Ui};

/// How long a card stays up unless the rule says.
const DEFAULT_SECONDS: f64 = 2.0;

/// Card ids for the daemon's life, so a card from before a reload is never
/// taken for a new one.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let ui = cx.backends.ui.clone();
    let notifier = cx.backends.notifier.clone();
    om.set(
        "show",
        lua.create_async_function(move |lua, (content, options): (Value, Option<Table>)| {
            let ui = ui.clone();
            let notifier = notifier.clone();
            async move {
                let (big, lines) = content_from(&lua, "om.show", content)?;
                let (duration, position) = options_from(options)?;
                let card = Card {
                    id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                    big,
                    lines,
                    duration,
                    position,
                };
                match ui.show(&card).await {
                    Ok(()) => {}
                    // Nothing on this machine draws cards: say it in a
                    // notification instead.
                    Err(BackendError::MissingTool { .. }) => {
                        let (title, body) = as_notification(&card);
                        notifier
                            .notify(&title, &body)
                            .await
                            .map_err(Error::external)?;
                    }
                    Err(err) => return Err(Error::external(err)),
                }
                Ok(Shown {
                    id: card.id,
                    duration,
                    position,
                    ui,
                })
            }
        })?,
    )
}

/// `"text"` (the large line alone) or `{big = "...", lines = {...}}`.
fn content_from(lua: &Lua, function: &str, value: Value) -> Result<(String, Vec<String>)> {
    let (big, lines) = match value {
        Value::Table(table) => {
            let big = table.get::<Option<String>>("big")?.unwrap_or_default();
            let lines = match table.get::<Value>("lines")? {
                Value::Nil => Vec::new(),
                Value::Table(list) => list
                    .sequence_values::<Value>()
                    .map(|line| text(lua, function, line?))
                    .collect::<Result<_>>()?,
                line => vec![text(lua, function, line)?],
            };
            (big, lines)
        }
        other => (text(lua, function, other)?, Vec::new()),
    };
    if big.trim().is_empty() && lines.iter().all(|line| line.trim().is_empty()) {
        return Err(Error::runtime(format!(
            "{function}: nothing to show (give the card a big line or lines)"
        )));
    }
    Ok((big, lines))
}

/// A line of the card: text, or a number written out.
fn text(lua: &Lua, function: &str, value: Value) -> Result<String> {
    match value {
        Value::String(_) | Value::Integer(_) | Value::Number(_) => String::from_lua(value, lua),
        other => Err(Error::runtime(format!(
            "{function}: a card holds text, not {}",
            other.type_name()
        ))),
    }
}

/// `{seconds = 2, position = "center"}`; seconds 0 keeps the card up until
/// closed.
fn options_from(options: Option<Table>) -> Result<(Option<Duration>, Position)> {
    let Some(options) = options else {
        return Ok((
            Some(Duration::from_secs_f64(DEFAULT_SECONDS)),
            Position::Center,
        ));
    };
    let seconds = options
        .get::<Option<f64>>("seconds")?
        .unwrap_or(DEFAULT_SECONDS);
    let duration = match Duration::try_from_secs_f64(seconds) {
        Ok(duration) if duration.is_zero() => None,
        Ok(duration) => Some(duration),
        Err(_) => {
            return Err(Error::runtime(format!(
                "om.show: seconds is 0 or more, not {seconds}"
            )));
        }
    };
    let position = match options.get::<Option<String>>("position")?.as_deref() {
        None | Some("center") => Position::Center,
        Some("top") => Position::Top,
        Some("bottom") => Position::Bottom,
        Some(other) => {
            return Err(Error::runtime(format!(
                "om.show: position is \"center\", \"top\" or \"bottom\", not \"{other}\""
            )));
        }
    };
    Ok((duration, position))
}

/// The card as a notification: the large line is the title, or the first
/// line with text when there is none; the other lines with text are the body.
fn as_notification(card: &Card) -> (String, String) {
    let mut lines = card.lines.iter().filter(|line| !line.trim().is_empty());
    let title = if card.big.trim().is_empty() {
        lines.next().cloned().unwrap_or_default()
    } else {
        card.big.clone()
    };
    let body: Vec<&str> = lines.map(String::as_str).collect();
    (title, body.join("\n"))
}

/// What `om.show` returns.
struct Shown {
    id: u64,
    duration: Option<Duration>,
    position: Position,
    ui: Arc<dyn Ui>,
}

impl UserData for Shown {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // New text on the card while it is up, for the time it has left;
        // true if it was up. Where a notification stood in for the card
        // there is nothing to update.
        methods.add_async_method("update", |lua, this, content: Value| {
            let ui = this.ui.clone();
            let (id, duration, position) = (this.id, this.duration, this.position);
            async move {
                let (big, lines) = content_from(&lua, "card:update", content)?;
                let card = Card {
                    id,
                    big,
                    lines,
                    duration,
                    position,
                };
                match ui.update(&card).await {
                    Ok(up) => Ok(up),
                    Err(BackendError::MissingTool { .. }) => Ok(false),
                    Err(err) => Err(Error::external(err)),
                }
            }
        });
        methods.add_async_method("close", |_, this, ()| {
            let ui = this.ui.clone();
            let id = this.id;
            async move {
                match ui.close(id).await {
                    Ok(()) | Err(BackendError::MissingTool { .. }) => Ok(()),
                    Err(err) => Err(Error::external(err)),
                }
            }
        });
    }
}
