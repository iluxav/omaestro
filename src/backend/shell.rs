//! Commands through `sh -c`, for `om.shell`, `om.prompt` and `om.choose`.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout as within;

use super::{BackendError, BoxFuture, Result, Shell, ShellOutput};

pub struct Sh;

fn failed(err: impl std::fmt::Display) -> BackendError {
    BackendError::Failed {
        tool: "sh",
        message: err.to_string(),
    }
}

impl Shell for Sh {
    fn run<'a>(
        &'a self,
        command: &'a str,
        stdin: Option<&'a str>,
        timeout: Option<Duration>,
    ) -> BoxFuture<'a, Result<ShellOutput>> {
        Box::pin(async move {
            let mut child = Command::new("sh")
                .args(["-c", command])
                .stdin(if stdin.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(failed)?;
            if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
                // A command that never reads its input must not hang us.
                let _ = pipe.write_all(text.as_bytes()).await;
                drop(pipe);
            }
            let waited = match timeout {
                Some(limit) => within(limit, child.wait_with_output()).await,
                None => Ok(child.wait_with_output().await),
            };
            match waited {
                Ok(output) => {
                    let output = output.map_err(failed)?;
                    Ok(ShellOutput {
                        status: output.status.code(),
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    })
                }
                // Dropping the future killed the child (kill_on_drop).
                Err(_) => Ok(ShellOutput {
                    status: None,
                    stdout: String::new(),
                    stderr: format!("timed out after {}s", timeout.map_or(0, |t| t.as_secs())),
                }),
            }
        })
    }

    fn spawn<'a>(&'a self, command: &'a str) -> BoxFuture<'a, Result<u32>> {
        Box::pin(Sh::spawn_detached(command))
    }
}

impl Sh {
    async fn spawn_detached(command: &str) -> Result<u32> {
        // A new session through setsid: the daemon's exit or signals do not
        // take the command down. The shell prints the pid of what it started.
        let child = Command::new("sh")
            .args([
                "-c",
                &format!(
                    "setsid sh -c {} </dev/null >/dev/null 2>&1 & echo $!",
                    quote(command)
                ),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(failed)?;
        let output = child.wait_with_output().await.map_err(failed)?;
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .map_err(|_| failed("could not start the command"))
    }
}

/// `text` as one shell word.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(quote("plain"), "'plain'");
        assert_eq!(quote("it's $HOME `x`"), r"'it'\''s $HOME `x`'");
        assert_eq!(quote(""), "''");
    }
}
