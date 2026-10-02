//! The control socket: newline-delimited JSON over a Unix socket. The CLI is
//! a thin client over it.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use crate::runtime::Event;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    List,
    Reload,
    Trigger {
        id: String,
    },
    Eval {
        chunk: String,
    },
    /// Switch a trigger on or off; the choice outlives reloads and restarts.
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    /// Whether rules may take chords Hyprland already has. Kept too.
    Override {
        on: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The `data` of a `status` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub pid: u32,
    pub uptime_secs: u64,
    pub config_dir: String,
    pub hyprland_instance: String,
    /// Rule files of the running state, in load order.
    pub files: Vec<String>,
    pub triggers: usize,
    /// Of `triggers`, how many are switched off.
    pub disabled: usize,
    /// `om override on`: rules take chords Hyprland already has.
    #[serde(rename = "override")]
    pub override_binds: bool,
    /// Handlers in flight.
    pub running: usize,
    pub reload_pending: bool,
    /// Why the last load failed, if it did.
    pub load_error: Option<String>,
}

/// One row of a `list` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerRow {
    pub id: String,
    pub kind: String,
    /// The chord, matcher, interval, time, text or path the rule gave.
    pub detail: String,
    /// What the rule calls it (`{label = ...}`), when it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub origin: String,
    pub enabled: bool,
    /// Why a hotkey has no bind right now: a chord somebody else holds, or
    /// a key Hyprland refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// The bind this hotkey displaced (`om override on`), by its description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<String>,
    /// For hotkeys and modes: whether the bind is in Hyprland right now (an
    /// app hotkey's comes and goes with the focus).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound: Option<bool>,
}

impl Response {
    /// A success carrying any serializable payload.
    pub fn data(data: &impl Serialize) -> Self {
        match serde_json::to_value(data) {
            Ok(data) => Self::ok(data),
            Err(err) => Self::err(format!("could not encode the response: {err}")),
        }
    }

    pub fn ok(data: impl Into<Value>) -> Self {
        Self {
            ok: true,
            data: Some(data.into()),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

/// Binds the control socket. A socket file nobody answers on is a leftover
/// from a crash and is replaced; one that answers means a daemon is running.
pub fn bind(path: &Path) -> Result<UnixListener> {
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            bail!(
                "another omaestro daemon is already running on {}",
                path.display()
            );
        }
        fs::remove_file(path)
            .with_context(|| format!("removing the stale socket {}", path.display()))?;
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    // `eval` runs code as the user: nobody else gets to connect.
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting {}", path.display()))?;
    Ok(listener)
}

/// Accepts connections until the runtime goes away.
pub async fn serve(listener: UnixListener, events: mpsc::Sender<Event>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let events = events.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, events).await {
                        tracing::debug!("control connection ended: {err}");
                    }
                });
            }
            Err(err) => {
                tracing::error!("control socket accept failed: {err}");
                return;
            }
        }
    }
}

/// One request per line, one response line for each.
async fn handle_connection<S>(stream: S, events: mpsc::Sender<Event>) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                let (reply, answer) = oneshot::channel();
                if events.send(Event::Request(request, reply)).await.is_err() {
                    Response::err("the daemon is shutting down")
                } else {
                    answer
                        .await
                        .unwrap_or_else(|_| Response::err("the daemon dropped the request"))
                }
            }
            Err(err) => Response::err(format!("bad request: {err}")),
        };
        let mut out = serde_json::to_vec(&response).map_err(io::Error::other)?;
        out.push(b'\n');
        writer.write_all(&out).await?;
    }
    Ok(())
}

/// Client side: sends one request and waits for its response.
pub async fn request(path: &Path, request: &Request) -> Result<Response> {
    let stream = match UnixStream::connect(path).await {
        Ok(stream) => stream,
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            bail!(
                "the omaestro daemon is not running (no answer on {}); start it with \
             `systemctl --user start omaestro` or `om daemon --foreground`",
                path.display()
            )
        }
        Err(err) => return Err(err).with_context(|| format!("connecting to {}", path.display())),
    };
    let (reader, mut writer) = stream.into_split();
    let mut out = serde_json::to_vec(request)?;
    out.push(b'\n');
    writer.write_all(&out).await?;
    let line = BufReader::new(reader)
        .lines()
        .next_line()
        .await?
        .context("the daemon closed the connection without answering")?;
    serde_json::from_str(&line)
        .context("the daemon sent a response this client does not understand")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::AsyncReadExt;

    use super::*;

    #[test]
    fn wire_format() {
        let cases = [
            (Request::Status, json!({"cmd": "status"})),
            (Request::List, json!({"cmd": "list"})),
            (Request::Reload, json!({"cmd": "reload"})),
            (
                Request::Trigger { id: "x".into() },
                json!({"cmd": "trigger", "id": "x"}),
            ),
            (
                Request::Eval {
                    chunk: "return 1".into(),
                },
                json!({"cmd": "eval", "chunk": "return 1"}),
            ),
        ];
        for (request, wire) in cases {
            assert_eq!(serde_json::to_value(&request).unwrap(), wire);
            assert_eq!(serde_json::from_value::<Request>(wire).unwrap(), request);
        }
        assert_eq!(
            serde_json::to_value(Response::ok("hi")).unwrap(),
            json!({"ok": true, "data": "hi"})
        );
        assert_eq!(
            serde_json::to_value(Response::err("no")).unwrap(),
            json!({"ok": false, "error": "no"})
        );
    }

    #[tokio::test]
    async fn connection_answers_each_line_and_survives_garbage() {
        let (mut client, server) = tokio::io::duplex(4096);
        let (events, mut inbox) = mpsc::channel(8);
        let connection = tokio::spawn(handle_connection(server, events));
        // Stands in for the runtime: echoes the trigger id back.
        tokio::spawn(async move {
            while let Some(Event::Request(request, reply)) = inbox.recv().await {
                let Request::Trigger { id } = request else {
                    panic!("unexpected request");
                };
                let _ = reply.send(Response::ok(id));
            }
        });

        client
            .write_all(b"{\"cmd\":\"trigger\",\"id\":\"a\"}\nnot json\n\n{\"cmd\":\"trigger\",\"id\":\"b\"}\n")
            .await
            .unwrap();
        client.shutdown().await.unwrap();
        let mut answers = String::new();
        client.read_to_string(&mut answers).await.unwrap();
        connection.await.unwrap().unwrap();

        let answers: Vec<Response> = answers
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(answers.len(), 3);
        assert_eq!(answers[0], Response::ok("a"));
        assert!(!answers[1].ok);
        assert!(
            answers[1]
                .error
                .as_deref()
                .unwrap()
                .starts_with("bad request")
        );
        assert_eq!(answers[2], Response::ok("b"));
    }
}
