//! Running one handler: its turn on the trigger's gate, the slow-handler
//! log line, and where its errors go.

use std::time::{Duration, Instant};

use mlua::MultiValue;
use tokio::time::timeout;

use super::registry::Trigger;
use crate::backend::Backends;

/// A handler running longer than this is logged as slow. It is not killed.
const SLOW_HANDLER: Duration = Duration::from_secs(30);

tokio::task_local! {
    /// When the hotkey that started this handler was pressed, if one did.
    /// Injection waits for the user's fingers to leave the modifiers.
    pub static HOTKEY_PRESSED: Option<Instant>;
}

/// Runs one handler to completion. One run at a time per trigger: a second
/// firing waits for the first.
pub async fn run_handler(trigger: &Trigger, args: MultiValue) -> mlua::Result<()> {
    let _turn = trigger.gate.lock().await;
    let call = trigger.handler.call_async::<MultiValue>(args);
    tokio::pin!(call);
    match timeout(SLOW_HANDLER, &mut call).await {
        Ok(result) => result.map(drop),
        Err(_) => {
            tracing::warn!(
                "handler '{}' ({}) has been running for {}s",
                trigger.id,
                trigger.origin,
                SLOW_HANDLER.as_secs()
            );
            call.await.map(drop)
        }
    }
}

/// Rule errors go to the log and to the desktop, never to a crash.
pub async fn notify_error(backends: &Backends, message: &str) {
    tracing::error!("{message}");
    if let Err(err) = backends.notifier.notify("omaestro", message).await {
        tracing::warn!("could not show the error as a notification: {err}");
    }
}
