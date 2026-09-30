//! Backends: everything that touches the system. Each one is a trait with a
//! real implementation and a fake one (`fake.rs`) that tests assert against.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::chord::Chord;

pub mod clip;
#[cfg(test)]
pub mod fake;
pub mod hypr;
pub mod inject;
pub mod llm;
pub mod notify;
pub mod run;
pub mod shell;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type Result<T> = std::result::Result<T, BackendError>;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("{tool} is not installed or not on PATH")]
    MissingTool { tool: &'static str },
    #[error("{tool} failed: {message}")]
    Failed { tool: &'static str, message: String },
    #[error("{0}")]
    Other(String),
}

pub trait Notifier: Send + Sync {
    fn notify<'a>(&'a self, title: &'a str, body: &'a str) -> BoxFuture<'a, Result<()>>;
}

/// A window as Hyprland describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub class: String,
    pub title: String,
    pub address: String,
    pub workspace: String,
    pub floating: bool,
}

/// One bind registered in Hyprland, ours or not.
#[derive(Debug, Clone, PartialEq)]
pub struct BindInfo {
    pub chord: Chord,
    pub description: String,
    /// Empty for the global keymap.
    pub submap: String,
}

/// A bind to register: `chord` runs `command`.
#[derive(Debug, Clone, PartialEq)]
pub struct Hotkey {
    pub chord: Chord,
    pub command: String,
    pub description: String,
}

pub trait Hypr: Send + Sync {
    /// Every bind Hyprland currently has.
    fn binds(&self) -> BoxFuture<'_, Result<Vec<BindInfo>>>;
    /// Adds a bind. Does not check for conflicts; `runtime::hotkeys` does.
    fn bind<'a>(&'a self, hotkey: &'a Hotkey) -> BoxFuture<'a, Result<()>>;
    /// Removes every bind on the chord. Hyprland has no narrower removal
    /// that is safe to use, so callers check whose binds are on it first.
    fn unbind<'a>(&'a self, chord: &'a Chord) -> BoxFuture<'a, Result<()>>;
    fn active_window(&self) -> BoxFuture<'_, Result<Option<Window>>>;
    /// Runs a dispatcher expression, `hl.dsp.window.float()`.
    fn dispatch<'a>(&'a self, code: &'a str) -> BoxFuture<'a, Result<()>>;
}

/// What a clipboard holds: bytes of one MIME type.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipContent {
    pub mime: String,
    pub data: Vec<u8>,
}

impl ClipContent {
    pub fn text(text: &str) -> Self {
        Self {
            mime: "text/plain".to_string(),
            data: text.as_bytes().to_vec(),
        }
    }
}

pub trait Clipboard: Send + Sync {
    /// The primary selection as text, `""` if there is none.
    fn selection(&self) -> BoxFuture<'_, Result<String>>;
    /// The clipboard, `None` if it is empty.
    fn get(&self) -> BoxFuture<'_, Result<Option<ClipContent>>>;
    fn set<'a>(&'a self, content: &'a ClipContent) -> BoxFuture<'a, Result<()>>;
    fn clear(&self) -> BoxFuture<'_, Result<()>>;
}

pub trait Injector: Send + Sync {
    /// Presses a chord in the focused window, as if on the real keyboard.
    fn key<'a>(&'a self, chord: &'a Chord) -> BoxFuture<'a, Result<()>>;
    /// Types text into the focused window.
    fn type_text<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<()>>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub endpoint: String,
    pub model: String,
    pub system: Option<String>,
    pub prompt: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

/// What a command produced. `status` is `None` when a signal ended it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShellOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub trait Shell: Send + Sync {
    /// Runs `command` through `sh -c` and waits for it.
    fn run<'a>(&'a self, command: &'a str) -> BoxFuture<'a, Result<ShellOutput>>;
}

pub trait Llm: Send + Sync {
    /// One prompt in, the model's text out.
    fn chat<'a>(&'a self, request: &'a ChatRequest) -> BoxFuture<'a, Result<String>>;
}

/// The set of backends the runtime and the Lua API work against.
#[derive(Clone)]
pub struct Backends {
    pub notifier: Arc<dyn Notifier>,
    pub hypr: Arc<dyn Hypr>,
    pub clipboard: Arc<dyn Clipboard>,
    pub injector: Arc<dyn Injector>,
    pub llm: Arc<dyn Llm>,
    pub shell: Arc<dyn Shell>,
}

impl Backends {
    pub fn real() -> Result<Self> {
        Ok(Self {
            notifier: Arc::new(notify::NotifySend),
            hypr: Arc::new(hypr::HyprCtl),
            clipboard: Arc::new(clip::WlClipboard),
            injector: Arc::new(inject::Keys),
            llm: Arc::new(llm::HttpLlm::new()?),
            shell: Arc::new(shell::Sh),
        })
    }
}
