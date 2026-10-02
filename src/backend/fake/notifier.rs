//! A notifier that records, and can be told to fail or to park.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Semaphore;

use crate::backend::{BackendError, BoxFuture, Notifier, Result};

/// One call to `ask`: title, body, actions, timeout.
pub type Asked = (String, String, Vec<(String, String)>, Option<Duration>);
/// A `progress` notification: (id, title, body, the id it replaced).
pub type Busy = (u32, String, String, Option<u32>);

pub struct FakeNotifier {
    sent: Mutex<Vec<(String, String)>>,
    /// Notifications with buttons: title, body, actions, timeout.
    asked: Mutex<Vec<Asked>>,
    /// The next answers to `ask`, oldest first; "" means dismissed.
    choices: Mutex<std::collections::VecDeque<String>>,
    failing: AtomicUsize,
    holding: AtomicUsize,
    permits: Semaphore,
    /// `progress` notifications: (id, title, body, replaced id).
    busy: Mutex<Vec<Busy>>,
    closed: Mutex<Vec<u32>>,
    next_id: AtomicUsize,
}

impl Default for FakeNotifier {
    fn default() -> Self {
        Self {
            sent: Mutex::default(),
            asked: Mutex::default(),
            choices: Mutex::default(),
            failing: AtomicUsize::new(0),
            holding: AtomicUsize::new(0),
            permits: Semaphore::new(0),
            busy: Mutex::default(),
            closed: Mutex::default(),
            next_id: AtomicUsize::new(1),
        }
    }
}

impl FakeNotifier {
    /// Every notification delivered so far, as `(title, body)`.
    pub fn sent(&self) -> Vec<(String, String)> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What `ask` was called with so far.
    pub fn asked(&self) -> Vec<Asked> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The `progress` notifications shown: (id, title, body, replaced id).
    pub fn busy(&self) -> Vec<Busy> {
        self.busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The ids of the notifications taken down.
    pub fn closed(&self) -> Vec<u32> {
        self.closed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The button the user will press next; "" for dismissed.
    pub fn choose(&self, key: &str) {
        self.choices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(key.to_string());
    }

    /// The next `calls` calls fail, like a machine without `notify-send`.
    pub fn fail_next(&self, calls: usize) {
        self.failing.store(calls, Ordering::SeqCst);
    }

    /// The next `calls` calls record their notification and then park until
    /// `release`. Lets a test keep a handler mid-flight.
    pub fn hold_next(&self, calls: usize) {
        self.holding.store(calls, Ordering::SeqCst);
    }

    /// Lets `calls` parked calls return.
    pub fn release(&self, calls: usize) {
        self.permits.add_permits(calls);
    }
}

/// Takes one from a countdown, if any are left. A plain compare-exchange
/// loop: `fetch_update` is deprecated on newer toolchains (renamed
/// `try_update`, which older ones lack).
fn take(counter: &AtomicUsize) -> bool {
    let mut left = counter.load(Ordering::SeqCst);
    while left > 0 {
        match counter.compare_exchange(left, left - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(now) => left = now,
        }
    }
    false
}

impl Notifier for FakeNotifier {
    fn notify<'a>(&'a self, title: &'a str, body: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if take(&self.failing) {
                return Err(BackendError::MissingTool {
                    tool: "notify-send",
                });
            }
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((title.to_string(), body.to_string()));
            if take(&self.holding)
                && let Ok(permit) = self.permits.acquire().await
            {
                permit.forget();
            }
            Ok(())
        })
    }

    fn ask<'a>(
        &'a self,
        title: &'a str,
        body: &'a str,
        actions: &'a [(String, String)],
        timeout: Option<Duration>,
    ) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async move {
            self.asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((
                    title.to_string(),
                    body.to_string(),
                    actions.to_vec(),
                    timeout,
                ));
            let chosen = self
                .choices
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
                .unwrap_or_default();
            Ok((!chosen.is_empty()).then_some(chosen))
        })
    }

    fn progress<'a>(
        &'a self,
        title: &'a str,
        body: &'a str,
        replaces: Option<u32>,
    ) -> BoxFuture<'a, Result<u32>> {
        Box::pin(async move {
            let id = match replaces {
                Some(id) => id,
                None => self.next_id.fetch_add(1, Ordering::SeqCst) as u32,
            };
            self.busy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((id, title.to_string(), body.to_string(), replaces));
            Ok(id)
        })
    }

    fn close(&self, id: u32) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.closed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(id);
            Ok(())
        })
    }
}
