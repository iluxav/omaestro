//! In-memory backends for tests: they record what the rules asked for.

use std::sync::{Arc, Mutex, PoisonError};

use super::Backends;

mod hypr;
mod io;
mod notifier;

pub use hypr::FakeHypr;
pub use io::{
    FakeClipboard, FakeHttp, FakeInjector, FakeKeyboards, FakeLlm, FakeShell, FakeSystem,
};
pub use notifier::FakeNotifier;

/// What the rules made the backends do, in order. Shared by the fakes of
/// one test so a sequence across backends can be asserted.
#[derive(Clone, Default)]
pub struct Journal(Arc<Mutex<Vec<String>>>);

impl Journal {
    pub(super) fn push(&self, entry: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
    }

    pub fn entries(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn clear(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// Handles to the fakes behind a `Backends`, for assertions.
pub struct Fakes {
    pub notifier: Arc<FakeNotifier>,
    pub hypr: Arc<FakeHypr>,
    pub clipboard: Arc<FakeClipboard>,
    pub llm: Arc<FakeLlm>,
    pub shell: Arc<FakeShell>,
    pub http: Arc<FakeHttp>,
    /// Hyprland, clipboard, injection and shell calls, in the order they happened.
    pub journal: Journal,
}

pub fn backends() -> (Backends, Fakes) {
    let journal = Journal::default();
    let notifier = Arc::new(FakeNotifier::default());
    let hypr = Arc::new(FakeHypr::new(journal.clone()));
    let clipboard = Arc::new(FakeClipboard::new(journal.clone()));
    let injector = Arc::new(FakeInjector::new(journal.clone()));
    let llm = Arc::new(FakeLlm::default());
    let shell = Arc::new(FakeShell::new(journal.clone()));
    let http = Arc::new(FakeHttp::default());
    let backends = Backends {
        notifier: notifier.clone(),
        hypr: hypr.clone(),
        clipboard: clipboard.clone(),
        injector,
        llm: llm.clone(),
        shell: shell.clone(),
        http: http.clone(),
        keyboards: Arc::new(FakeKeyboards::new(journal.clone())),
        system: Arc::new(FakeSystem::new(journal.clone())),
    };
    (
        backends,
        Fakes {
            notifier,
            hypr,
            clipboard,
            llm,
            shell,
            http,
            journal,
        },
    )
}
