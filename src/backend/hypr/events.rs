//! Hyprland's event socket: one `name>>data` line per event. Read-only.

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::Instance;
use crate::runtime::Event;

#[derive(Debug, Clone, PartialEq)]
pub enum HyprEvent {
    /// Hyprland re-read its config. Binds registered at runtime are gone.
    ConfigReloaded,
    /// Keyboard focus moved to this window, or to nothing.
    Focus(Option<WinRef>),
    /// A window appeared.
    Opened(WinRef),
    /// A window went away. Only its address is known here.
    Closed { address: String },
    /// A window's title changed.
    Title { address: String, title: String },
    /// The active workspace changed.
    Workspace { id: i64, name: String },
    /// A monitor came, went, or got focus.
    Monitor { name: String, change: MonitorChange },
    /// The active submap changed; empty when back in the global keymap.
    Submap(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MonitorChange {
    Added,
    Removed,
    Focused,
}

impl MonitorChange {
    pub fn as_str(self) -> &'static str {
        match self {
            MonitorChange::Added => "added",
            MonitorChange::Removed => "removed",
            MonitorChange::Focused => "focused",
        }
    }
}

/// A window as the event socket describes it. `om.window()` and the window
/// methods ask Hyprland for the rest (geometry, workspace, floating).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WinRef {
    pub class: String,
    pub title: String,
    /// `0x...`, as `hyprctl` prints it.
    pub address: String,
    /// The workspace name, when the event carried it.
    pub workspace: String,
}

fn hex(address: &str) -> String {
    format!("0x{}", address.trim_start_matches("0x"))
}

/// Turns event lines into events. Focus comes as two lines, `activewindow`
/// (class and title) then `activewindowv2` (address), so the first is kept
/// until the second arrives.
#[derive(Default)]
pub struct Parser {
    pending: Option<(String, String)>,
}

impl Parser {
    pub fn feed(&mut self, line: &str) -> Option<HyprEvent> {
        let (name, data) = line.split_once(">>")?;
        match name {
            "configreloaded" => Some(HyprEvent::ConfigReloaded),
            "activewindow" => {
                // The class has no commas; the title may.
                self.pending = match data.split_once(',') {
                    Some((class, title)) if !class.is_empty() => {
                        Some((class.to_string(), title.to_string()))
                    }
                    _ => None,
                };
                None
            }
            "activewindowv2" => {
                let address = data.trim_start_matches(',');
                if address.is_empty() {
                    self.pending = None;
                    return Some(HyprEvent::Focus(None));
                }
                let (class, title) = self.pending.take().unwrap_or_default();
                Some(HyprEvent::Focus(Some(WinRef {
                    class,
                    title,
                    address: hex(address),
                    workspace: String::new(),
                })))
            }
            // openwindow>>ADDRESS,WORKSPACE,CLASS,TITLE
            "openwindow" => {
                let mut parts = data.splitn(4, ',');
                let address = parts.next()?;
                let workspace = parts.next().unwrap_or_default();
                let class = parts.next().unwrap_or_default();
                let title = parts.next().unwrap_or_default();
                (!address.is_empty()).then(|| {
                    HyprEvent::Opened(WinRef {
                        class: class.to_string(),
                        title: title.to_string(),
                        address: hex(address),
                        workspace: workspace.to_string(),
                    })
                })
            }
            "closewindow" => (!data.is_empty()).then(|| HyprEvent::Closed { address: hex(data) }),
            // windowtitlev2>>ADDRESS,TITLE
            "windowtitlev2" => {
                let (address, title) = data.split_once(',')?;
                Some(HyprEvent::Title {
                    address: hex(address),
                    title: title.to_string(),
                })
            }
            // workspacev2>>ID,NAME
            "workspacev2" => {
                let (id, name) = data.split_once(',')?;
                Some(HyprEvent::Workspace {
                    id: id.parse().ok()?,
                    name: name.to_string(),
                })
            }
            // focusedmon>>NAME,WORKSPACE
            "focusedmon" => Some(HyprEvent::Monitor {
                name: data.split(',').next().unwrap_or_default().to_string(),
                change: MonitorChange::Focused,
            }),
            "monitoradded" => Some(HyprEvent::Monitor {
                name: data.to_string(),
                change: MonitorChange::Added,
            }),
            "monitorremoved" => Some(HyprEvent::Monitor {
                name: data.to_string(),
                change: MonitorChange::Removed,
            }),
            "submap" => Some(HyprEvent::Submap(data.to_string())),
            _ => None,
        }
    }
}

/// Connects to the event socket and forwards the events the runtime cares
/// about. When Hyprland closes the socket it is gone, and the runtime is told.
pub async fn listen(instance: &Instance, events: mpsc::Sender<Event>) -> Result<()> {
    let path = instance.event_socket();
    let stream = UnixStream::connect(&path).await.with_context(|| {
        format!(
            "connecting to Hyprland's event socket {} (is that instance still running?)",
            path.display()
        )
    })?;
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut parser = Parser::default();
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line).await {
                Ok(read) if read > 0 => {
                    // Window titles are arbitrary bytes; never fail on them.
                    let text = String::from_utf8_lossy(&line);
                    if let Some(event) = parser.feed(text.trim_end())
                        && events.send(Event::Hypr(event)).await.is_err()
                    {
                        return;
                    }
                }
                _ => {
                    let _ = events.send(Event::HyprGone).await;
                    return;
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(class: &str, title: &str, address: &str, workspace: &str) -> WinRef {
        WinRef {
            class: class.into(),
            title: title.into(),
            address: address.into(),
            workspace: workspace.into(),
        }
    }

    #[test]
    fn focus_is_assembled_from_its_two_lines() {
        let mut parser = Parser::default();
        assert_eq!(
            parser.feed("activewindow>>firefox,Docs, with commas - Mozilla Firefox"),
            None
        );
        assert_eq!(
            parser.feed("activewindowv2>>5cf5226be380"),
            Some(HyprEvent::Focus(Some(win(
                "firefox",
                "Docs, with commas - Mozilla Firefox",
                "0x5cf5226be380",
                ""
            ))))
        );
        // Nothing focused.
        assert_eq!(parser.feed("activewindow>>,"), None);
        assert_eq!(
            parser.feed("activewindowv2>>"),
            Some(HyprEvent::Focus(None))
        );
        // An address on its own still counts, with what is known.
        assert_eq!(
            parser.feed("activewindowv2>>0xabc"),
            Some(HyprEvent::Focus(Some(win("", "", "0xabc", ""))))
        );
    }

    #[test]
    fn window_lifecycle_lines() {
        let mut parser = Parser::default();
        assert_eq!(
            parser.feed("openwindow>>62b66d157580,1,evt,foot, and more"),
            Some(HyprEvent::Opened(win(
                "evt",
                "foot, and more",
                "0x62b66d157580",
                "1"
            )))
        );
        assert_eq!(
            parser.feed("closewindow>>62b66d157580"),
            Some(HyprEvent::Closed {
                address: "0x62b66d157580".into()
            })
        );
        assert_eq!(parser.feed("windowtitle>>62b66d157580"), None);
        assert_eq!(
            parser.feed("windowtitlev2>>62b66d157580,New, Title"),
            Some(HyprEvent::Title {
                address: "0x62b66d157580".into(),
                title: "New, Title".into()
            })
        );
    }

    #[test]
    fn workspace_and_monitor_lines() {
        let mut parser = Parser::default();
        assert_eq!(parser.feed("workspace>>4"), None);
        assert_eq!(
            parser.feed("workspacev2>>4,4"),
            Some(HyprEvent::Workspace {
                id: 4,
                name: "4".into()
            })
        );
        assert_eq!(
            parser.feed("workspacev2>>-98,special:scratch"),
            Some(HyprEvent::Workspace {
                id: -98,
                name: "special:scratch".into()
            })
        );
        for (line, change) in [
            ("focusedmon>>DP-8,2", MonitorChange::Focused),
            ("monitoradded>>DP-8", MonitorChange::Added),
            ("monitorremoved>>DP-8", MonitorChange::Removed),
        ] {
            assert_eq!(
                parser.feed(line),
                Some(HyprEvent::Monitor {
                    name: "DP-8".into(),
                    change
                })
            );
        }
        assert_eq!(
            parser.feed("submap>>om-super+alt+w"),
            Some(HyprEvent::Submap("om-super+alt+w".into()))
        );
        assert_eq!(
            parser.feed("submap>>"),
            Some(HyprEvent::Submap(String::new()))
        );
        assert_eq!(
            parser.feed("configreloaded>>"),
            Some(HyprEvent::ConfigReloaded)
        );
        assert_eq!(parser.feed("garbage"), None);
    }
}
