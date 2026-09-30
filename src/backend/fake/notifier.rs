//! A notifier that records, and can be told to fail or to park.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use tokio::sync::Semaphore;

use crate::backend::{BackendError, BoxFuture, Notifier, Result};

pub struct FakeNotifier {
    sent: Mutex<Vec<(String, String)>>,
    failing: AtomicUsize,
    holding: AtomicUsize,
    permits: Semaphore,
}

impl Default for FakeNotifier {
    fn default() -> Self {
        Self {
            sent: Mutex::default(),
            failing: AtomicUsize::new(0),
            holding: AtomicUsize::new(0),
            permits: Semaphore::new(0),
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

/// Takes one from a countdown, if any are left.
fn take(counter: &AtomicUsize) -> bool {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
            left.checked_sub(1)
        })
        .is_ok()
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
}
