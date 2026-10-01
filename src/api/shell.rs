//! `om.shell(cmd, opts)`: run a command, get its output. `om.prompt(label)`:
//! ask the user for a line of text. `om.choose(label, options)`: let the
//! user pick one.

use std::time::Duration;

use mlua::{Error, Lua, Result, Table};

use super::Context;
use crate::backend::shell::quote;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let shell = cx.backends.shell.clone();
    om.set(
        "shell",
        lua.create_async_function(move |_, (command, options): (String, Option<Table>)| {
            let shell = shell.clone();
            async move {
                let (stdin, timeout) = match &options {
                    Some(options) => (
                        options.get::<Option<String>>("stdin")?,
                        options
                            .get::<Option<f64>>("timeout")?
                            .map(Duration::from_secs_f64),
                    ),
                    None => (None, None),
                };
                let output = shell
                    .run(&command, stdin.as_deref(), timeout)
                    .await
                    .map_err(Error::external)?;
                if output.status != Some(0) {
                    let how = match output.status {
                        Some(code) => format!("exited with {code}"),
                        None if output.stderr.starts_with("timed out") => {
                            output.stderr.trim().to_string()
                        }
                        None => "was killed".to_string(),
                    };
                    let stderr = output.stderr.trim();
                    let detail = if stderr.is_empty() || how == stderr {
                        String::new()
                    } else {
                        format!(": {stderr}")
                    };
                    return Err(Error::runtime(format!("`{command}` {how}{detail}")));
                }
                // Like `$(...)`: the trailing newline is not part of the answer.
                Ok(output.stdout.trim_end_matches('\n').to_string())
            }
        })?,
    )?;

    let shell = cx.backends.shell.clone();
    om.set(
        "spawn",
        lua.create_async_function(move |_, command: String| {
            let shell = shell.clone();
            async move {
                if command.trim().is_empty() {
                    return Err(Error::runtime("spawn: the command is empty"));
                }
                shell.spawn(&command).await.map_err(Error::external)
            }
        })?,
    )?;

    let shell = cx.backends.shell.clone();
    let config = cx.config.clone();
    om.set(
        "prompt",
        lua.create_async_function(move |_, label: Option<String>| {
            let shell = shell.clone();
            let config = config.clone();
            async move {
                let template = config.prompt_command().map_err(Error::runtime)?;
                let label = label.unwrap_or_default();
                let command = template.replace("{label}", &quote(&label));
                let output = shell
                    .run(&command, None, None)
                    .await
                    .map_err(Error::external)?;
                // Cancelling is a non-zero exit or nothing typed; neither is an error.
                let answer = output.stdout.trim_end_matches('\n');
                Ok((output.status == Some(0) && !answer.is_empty()).then(|| answer.to_string()))
            }
        })?,
    )?;

    let shell = cx.backends.shell.clone();
    let config = cx.config.clone();
    om.set(
        "choose",
        lua.create_async_function(move |_, (label, options): (Option<String>, Vec<String>)| {
            let shell = shell.clone();
            let config = config.clone();
            async move {
                if options.is_empty() {
                    return Ok(None);
                }
                let template = config.choose_command().map_err(Error::runtime)?;
                let label = label.unwrap_or_default();
                let mut command = template.replace("{label}", &quote(&label));
                // Tools that take the options as arguments say so with
                // {options}; the others read them from stdin, one per line.
                let stdin = if command.contains("{options}") {
                    let words: Vec<String> = options.iter().map(|o| quote(o)).collect();
                    command = command.replace("{options}", &words.join(" "));
                    None
                } else {
                    Some(format!("{}\n", options.join("\n")))
                };
                let output = shell
                    .run(&command, stdin.as_deref(), None)
                    .await
                    .map_err(Error::external)?;
                let answer = output.stdout.trim_end_matches('\n');
                Ok((output.status == Some(0) && !answer.is_empty()).then(|| answer.to_string()))
            }
        })?,
    )
}
