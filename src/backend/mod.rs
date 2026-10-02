//! Backends: everything that touches the system. Each one is a trait with a
//! real implementation and a fake one (`fake.rs`) that tests assert against.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::chord::Chord;

pub mod clip;
#[cfg(feature = "typed")]
pub mod evdev;
#[cfg(test)]
pub mod fake;
pub mod http;
pub mod hypr;
pub mod inject;
pub mod llm;
pub mod notify;
pub mod run;
pub mod shell;
pub mod system;

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
    /// A notification with buttons: waits until one is pressed (its key)
    /// or the notification goes away (`None`).
    fn ask<'a>(
        &'a self,
        title: &'a str,
        body: &'a str,
        actions: &'a [(String, String)],
        timeout: Option<Duration>,
    ) -> BoxFuture<'a, Result<Option<String>>>;
    /// A notification that stays up until closed (`om.busy`), replacing the
    /// one with id `replaces` if given. Returns its id.
    fn progress<'a>(
        &'a self,
        title: &'a str,
        body: &'a str,
        replaces: Option<u32>,
    ) -> BoxFuture<'a, Result<u32>>;
    /// Takes a notification down.
    fn close(&self, id: u32) -> BoxFuture<'_, Result<()>>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HttpResponse {
    pub status: u16,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub trait Http: Send + Sync {
    fn request<'a>(&'a self, request: &'a HttpRequest) -> BoxFuture<'a, Result<HttpResponse>>;
}

/// A window as Hyprland describes it. Position and size are logical pixels.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Window {
    pub address: String,
    pub class: String,
    pub title: String,
    pub initial_class: String,
    pub workspace: String,
    pub workspace_id: i64,
    /// The id of the monitor it is on.
    pub monitor: i64,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub floating: bool,
    /// 0 none, 1 maximized, 2 fullscreen.
    pub fullscreen: i64,
    pub pinned: bool,
    pub pid: i64,
    pub xwayland: bool,
    pub focused: bool,
}

/// A monitor as Hyprland describes it: physical size, scale and rotation.
/// `geometry` turns that into logical rectangles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Monitor {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub scale: f64,
    pub transform: i64,
    pub focused: bool,
    pub workspace: String,
    pub workspace_id: i64,
    /// Edges taken by bars: left, top, right, bottom.
    pub reserved: [i64; 4],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub monitor: String,
    pub windows: i64,
    pub has_fullscreen: bool,
}

/// One bind registered in Hyprland, ours or not.
#[derive(Debug, Clone, PartialEq)]
pub struct BindInfo {
    pub chord: Chord,
    pub description: String,
    /// Empty for the global keymap.
    pub submap: String,
}

/// What a bind does when pressed.
#[derive(Debug, Clone, PartialEq)]
pub enum BindAction {
    /// Run a shell command.
    Exec(String),
    /// Enter a submap; `"reset"` leaves the current one.
    Submap(String),
}

/// A bind to register.
#[derive(Debug, Clone, PartialEq)]
pub struct Hotkey {
    pub chord: Chord,
    pub action: BindAction,
    pub description: String,
    /// The submap it lives in; empty for the global keymap.
    pub submap: String,
}

pub trait Hypr: Send + Sync {
    /// Every bind Hyprland currently has.
    fn binds(&self) -> BoxFuture<'_, Result<Vec<BindInfo>>>;
    /// Adds a bind. Does not check for conflicts; `runtime::hotkeys` does.
    fn bind<'a>(&'a self, hotkey: &'a Hotkey) -> BoxFuture<'a, Result<()>>;
    /// Removes every bind on the chord in `submap` (empty: global). Hyprland
    /// has no narrower removal that is safe to use, so callers check whose
    /// binds are on it first.
    fn unbind<'a>(&'a self, chord: &'a Chord, submap: &'a str) -> BoxFuture<'a, Result<()>>;
    /// Makes Hyprland re-read its config (`hyprctl reload`): the binds the
    /// config defines come back, binds made at runtime are gone. The only
    /// way to restore a bind we displaced.
    fn reload(&self) -> BoxFuture<'_, Result<()>>;
    fn active_window(&self) -> BoxFuture<'_, Result<Option<Window>>>;
    fn clients(&self) -> BoxFuture<'_, Result<Vec<Window>>>;
    fn monitors(&self) -> BoxFuture<'_, Result<Vec<Monitor>>>;
    fn workspaces(&self) -> BoxFuture<'_, Result<Vec<Workspace>>>;
    /// Where the pointer is, in logical pixels.
    fn cursor(&self) -> BoxFuture<'_, Result<(i64, i64)>>;
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
    /// Sends `Event::ClipboardChanged` on every change until the result is
    /// dropped.
    fn watch(
        &self,
        events: tokio::sync::mpsc::Sender<crate::runtime::Event>,
    ) -> std::result::Result<Watching, String>;
    /// Sends `Event::SelectionChanged` whenever the primary selection
    /// changes, until the result is dropped.
    fn watch_selection(
        &self,
        events: tokio::sync::mpsc::Sender<crate::runtime::Event>,
    ) -> std::result::Result<Watching, String>;
}

pub trait Injector: Send + Sync {
    /// Presses a chord in the focused window, as if on the real keyboard.
    fn key<'a>(&'a self, chord: &'a Chord) -> BoxFuture<'a, Result<()>>;
    /// Types text into the focused window.
    fn type_text<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<()>>;
    /// Presses Backspace `count` times.
    fn erase(&self, count: usize) -> BoxFuture<'_, Result<()>>;
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
    /// Runs `command` through `sh -c` with `stdin` fed to it and waits, at
    /// most `timeout`; past it the command is killed and `status` is `None`.
    fn run<'a>(
        &'a self,
        command: &'a str,
        stdin: Option<&'a str>,
        timeout: Option<Duration>,
    ) -> BoxFuture<'a, Result<ShellOutput>>;
    /// Starts `command` through `sh -c` in its own session, detached from
    /// the daemon, and returns its pid without waiting.
    fn spawn<'a>(&'a self, command: &'a str) -> BoxFuture<'a, Result<u32>>;
}

/// A running keyboard watch; dropping it stops the reading.
pub struct Watching(#[allow(dead_code)] Box<dyn std::any::Any + Send>);

impl Watching {
    #[cfg_attr(not(feature = "typed"), allow(dead_code))]
    pub fn new(inner: impl std::any::Any + Send) -> Self {
        Self(Box::new(inner))
    }
}

pub trait Keyboards: Send + Sync {
    /// Starts reading the keyboards, sending `Event::Typed` and
    /// `Event::TypedRescan` into `events` until the result is dropped.
    fn watch(
        &self,
        events: tokio::sync::mpsc::Sender<crate::runtime::Event>,
    ) -> std::result::Result<Watching, String>;
}

/// The build without the `typed` feature: nothing can be watched.
#[cfg(not(feature = "typed"))]
pub struct NoKeyboards;

#[cfg(not(feature = "typed"))]
impl Keyboards for NoKeyboards {
    fn watch(
        &self,
        _: tokio::sync::mpsc::Sender<crate::runtime::Event>,
    ) -> std::result::Result<Watching, String> {
        Err("this build has no typed triggers (built without the `typed` feature)".to_string())
    }
}

/// What system events come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemSource {
    /// logind's sleep and wake signals.
    Login1,
    Usb,
    Battery,
    Network,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemEvent {
    Sleep,
    Wake,
    Usb { action: String, device: String },
    Battery { percent: i64, status: String },
    Network { line: String },
}

pub trait System: Send + Sync {
    /// Sends `Event::System` for `source` until the result is dropped.
    fn watch(
        &self,
        source: SystemSource,
        events: tokio::sync::mpsc::Sender<crate::runtime::Event>,
    ) -> std::result::Result<Watching, String>;
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
    pub http: Arc<dyn Http>,
    pub keyboards: Arc<dyn Keyboards>,
    pub system: Arc<dyn System>,
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
            http: Arc::new(http::ReqwestHttp::new()?),
            #[cfg(feature = "typed")]
            keyboards: Arc::new(evdev::EvdevKeyboards),
            #[cfg(not(feature = "typed"))]
            keyboards: Arc::new(NoKeyboards),
            system: Arc::new(system::Tools),
        })
    }
}
