//! A card drawer that records, and can be told there is nothing to draw
//! with. Like `Cards.qml` without its timer: one card up at a time, until
//! it is closed or another is shown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::backend::{BackendError, BoxFuture, Card, Result, Ui};

#[derive(Default)]
pub struct FakeUi {
    shown: Mutex<Vec<Card>>,
    updated: Mutex<Vec<Card>>,
    closed: Mutex<Vec<u64>>,
    /// The id of the card up now.
    up: Mutex<Option<u64>>,
    absent: AtomicBool,
}

fn copy<T: Clone>(list: &Mutex<Vec<T>>) -> Vec<T> {
    list.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

impl FakeUi {
    /// Every card shown so far, in order.
    pub fn shown(&self) -> Vec<Card> {
        copy(&self.shown)
    }

    /// The updates that reached a card still up, in order.
    pub fn updated(&self) -> Vec<Card> {
        copy(&self.updated)
    }

    /// The ids of the cards closed.
    pub fn closed(&self) -> Vec<u64> {
        copy(&self.closed)
    }

    /// The card up now goes, as when its time is up.
    pub fn expire(&self) {
        *self.up.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// From now on nothing can draw, like a machine with no quickshell.
    pub fn remove(&self) {
        self.absent.store(true, Ordering::SeqCst);
    }

    fn missing(&self) -> Result<()> {
        if self.absent.load(Ordering::SeqCst) {
            Err(BackendError::MissingTool { tool: "quickshell" })
        } else {
            Ok(())
        }
    }
}

impl Ui for FakeUi {
    fn show<'a>(&'a self, card: &'a Card) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.missing()?;
            self.shown
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(card.clone());
            *self.up.lock().unwrap_or_else(PoisonError::into_inner) = Some(card.id);
            Ok(())
        })
    }

    fn update<'a>(&'a self, card: &'a Card) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.missing()?;
            if *self.up.lock().unwrap_or_else(PoisonError::into_inner) != Some(card.id) {
                return Ok(false);
            }
            self.updated
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(card.clone());
            Ok(true)
        })
    }

    fn close(&self, id: u64) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.missing()?;
            self.closed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(id);
            let mut up = self.up.lock().unwrap_or_else(PoisonError::into_inner);
            if *up == Some(id) {
                *up = None;
            }
            Ok(())
        })
    }
}
