//! Running the command-line tools the backends are built on.

use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{BackendError, Result};

pub struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

fn spawn_error(tool: &'static str, err: std::io::Error) -> BackendError {
    match err.kind() {
        ErrorKind::NotFound => BackendError::MissingTool { tool },
        _ => BackendError::Failed {
            tool,
            message: err.to_string(),
        },
    }
}

/// Runs `tool` and captures its output. A non-zero exit is not an error
/// here; callers decide what it means.
pub async fn capture(tool: &'static str, args: &[&str]) -> Result<Output> {
    let output = Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| spawn_error(tool, err))?;
    Ok(Output {
        success: output.status.success(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// Runs `tool` and fails unless it exits with zero.
pub async fn run(tool: &'static str, args: &[&str]) -> Result<Output> {
    let output = capture(tool, args).await?;
    if output.success {
        Ok(output)
    } else {
        let message = if output.stderr.is_empty() {
            output.stdout_text().trim().to_string()
        } else {
            output.stderr
        };
        Err(BackendError::Failed { tool, message })
    }
}

/// Runs `tool` with `input` on stdin and nothing captured. For tools that
/// stay alive in the background holding their output open (`wl-copy`):
/// waiting on a pipe to them would never end.
pub async fn feed(tool: &'static str, args: &[&str], input: &[u8]) -> Result<()> {
    let mut child = Command::new(tool)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| spawn_error(tool, err))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input)
            .await
            .map_err(|err| BackendError::Failed {
                tool,
                message: err.to_string(),
            })?;
    }
    let status = child.wait().await.map_err(|err| spawn_error(tool, err))?;
    if status.success() {
        Ok(())
    } else {
        Err(BackendError::Failed {
            tool,
            message: format!("exited with {status}"),
        })
    }
}

/// The executable `tool` on `PATH`, if any.
pub fn find_on_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    find_in(tool, std::env::split_paths(&path))
}

/// The executable `tool` in the first of `dirs` that has it.
pub fn find_in(tool: &str, dirs: impl Iterator<Item = PathBuf>) -> Option<PathBuf> {
    dirs.map(|dir| dir.join(tool)).find(|candidate| {
        candidate
            .metadata()
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    })
}
