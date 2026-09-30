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
    Focus(Option<Focused>),
}

/// The focused window as the event socket describes it. `om.window()` asks
/// Hyprland for the rest (workspace, floating).
#[derive(Debug, Clone, PartialEq)]
pub struct Focused {
    pub class: String,
    pub title: String,
    /// `0x...`, as `hyprctl` prints it.
    pub address: String,
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
                Some(HyprEvent::Focus(Some(Focused {
                    class,
                    title,
                    address: format!("0x{}", address.trim_start_matches("0x")),
                })))
            }
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

    fn focused(class: &str, title: &str, address: &str) -> Option<HyprEvent> {
        Some(HyprEvent::Focus(Some(Focused {
            class: class.into(),
            title: title.into(),
            address: address.into(),
        })))
    }

    #[test]
    fn focus_is_assembled_from_its_two_lines() {
        let mut parser = Parser::default();
        assert_eq!(parser.feed("openwindow>>5cf5226be380,1,foot,foot"), None);
        assert_eq!(
            parser.feed("activewindow>>firefox,Docs, with commas - Mozilla Firefox"),
            None
        );
        assert_eq!(
            parser.feed("activewindowv2>>5cf5226be380"),
            focused(
                "firefox",
                "Docs, with commas - Mozilla Firefox",
                "0x5cf5226be380"
            )
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
            focused("", "", "0xabc")
        );
    }

    #[test]
    fn other_lines() {
        let mut parser = Parser::default();
        assert_eq!(
            parser.feed("configreloaded>>"),
            Some(HyprEvent::ConfigReloaded)
        );
        assert_eq!(parser.feed("workspace>>3"), None);
        assert_eq!(parser.feed("garbage"), None);
    }
}
