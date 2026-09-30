//! `om.shell(cmd)`: run a command, get its output. `om.prompt(label)`: ask
//! the user for a line of text.

use mlua::{Error, Lua, Result, Table};

use super::Context;
use crate::backend::shell::quote;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let shell = cx.backends.shell.clone();
    om.set(
        "shell",
        lua.create_async_function(move |_, command: String| {
            let shell = shell.clone();
            async move {
                let output = shell.run(&command).await.map_err(Error::external)?;
                if output.status != Some(0) {
                    let how = match output.status {
                        Some(code) => format!("exited with {code}"),
                        None => "was killed".to_string(),
                    };
                    let stderr = output.stderr.trim();
                    let detail = if stderr.is_empty() {
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
                let output = shell.run(&command).await.map_err(Error::external)?;
                // Cancelling is a non-zero exit or nothing typed; neither is an error.
                let answer = output.stdout.trim_end_matches('\n');
                Ok((output.status == Some(0) && !answer.is_empty()).then(|| answer.to_string()))
            }
        })?,
    )
}
