//! Commands through `sh -c`, for `om.shell` and `om.prompt`.

use std::process::Stdio;

use tokio::process::Command;

use super::{BackendError, BoxFuture, Result, Shell, ShellOutput};

pub struct Sh;

impl Shell for Sh {
    fn run<'a>(&'a self, command: &'a str) -> BoxFuture<'a, Result<ShellOutput>> {
        Box::pin(async move {
            let output = Command::new("sh")
                .args(["-c", command])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .map_err(|err| BackendError::Failed {
                    tool: "sh",
                    message: err.to_string(),
                })?;
            Ok(ShellOutput {
                status: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        })
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
