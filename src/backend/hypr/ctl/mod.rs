//! Commands to Hyprland through `hyprctl`.
//!
//! Hyprland 0.56 configures itself in Lua, and runtime changes go through
//! `hyprctl eval <lua>`; `hyprctl keyword` is refused there. Only three
//! calls are ever evaluated: `hl.bind(...)` and `hl.unbind(...)`, the same
//! ones a config file uses, and `hl.dispatch(hl.dsp.send_shortcut(...))`,
//! which presses a chord in the focused window with the real keymap (a
//! virtual keyboard's chords never reached Chromium and Electron apps). The
//! `HL.Keybind` object `hl.bind` returns is never kept: calling a method on
//! one whose bind is already gone crashes Hyprland 0.56.2. `hyprctl reload`
//! is the one way to bring back a bind of the config's that we displaced:
//! with the Lua config every bind is a closure inside Hyprland, nothing
//! `hyprctl -j binds` shows could recreate it.

use super::super::{
    BackendError, BindAction, BindInfo, BoxFuture, Hotkey, Hypr, Monitor, Result, Window,
    Workspace, run,
};
use crate::chord::{Chord, KeyPress};

mod parse;

use parse::{parse_binds, parse_clients, parse_monitors, parse_window, parse_workspaces};

const TOOL: &str = "hyprctl";

pub struct HyprCtl;

impl Hypr for HyprCtl {
    fn binds(&self) -> BoxFuture<'_, Result<Vec<BindInfo>>> {
        Box::pin(async move {
            let output = run::run(TOOL, &["-j", "binds"]).await?;
            parse_binds(&output.stdout).map_err(failed)
        })
    }

    fn bind<'a>(&'a self, hotkey: &'a Hotkey) -> BoxFuture<'a, Result<()>> {
        Box::pin(eval(bind_code(hotkey)))
    }

    fn unbind<'a>(&'a self, chord: &'a Chord, submap: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(eval(unbind_code(chord, submap)))
    }

    fn reload(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            run::run(TOOL, &["reload"]).await?;
            Ok(())
        })
    }

    fn active_window(&self) -> BoxFuture<'_, Result<Option<Window>>> {
        Box::pin(async move {
            let output = run::run(TOOL, &["-j", "activewindow"]).await?;
            parse_window(&output.stdout).map_err(failed)
        })
    }

    fn clients(&self) -> BoxFuture<'_, Result<Vec<Window>>> {
        Box::pin(async move {
            let output = run::run(TOOL, &["-j", "clients"]).await?;
            parse_clients(&output.stdout).map_err(failed)
        })
    }

    fn monitors(&self) -> BoxFuture<'_, Result<Vec<Monitor>>> {
        Box::pin(async move {
            let output = run::run(TOOL, &["-j", "monitors"]).await?;
            parse_monitors(&output.stdout).map_err(failed)
        })
    }

    fn workspaces(&self) -> BoxFuture<'_, Result<Vec<Workspace>>> {
        Box::pin(async move {
            let output = run::run(TOOL, &["-j", "workspaces"]).await?;
            parse_workspaces(&output.stdout).map_err(failed)
        })
    }

    fn cursor(&self) -> BoxFuture<'_, Result<(i64, i64)>> {
        Box::pin(async move {
            let output = run::run(TOOL, &["-j", "cursorpos"]).await?;
            let json: serde_json::Value =
                serde_json::from_slice(&output.stdout).map_err(|err| {
                    failed(format!(
                        "unexpected output from `hyprctl -j cursorpos`: {err}"
                    ))
                })?;
            Ok((
                json["x"].as_i64().unwrap_or(0),
                json["y"].as_i64().unwrap_or(0),
            ))
        })
    }

    fn dispatch<'a>(&'a self, code: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // `hyprctl dispatch X` is `hl.dispatch(X)` in Hyprland's Lua: X
            // has to be a dispatcher, so this cannot run arbitrary code there.
            let output = run::capture(TOOL, &["dispatch", code]).await?;
            eval_result(output.success, &output.stdout_text(), &output.stderr).map_err(failed)
        })
    }
}

/// Presses `chord` in the focused window through Hyprland, which uses the
/// real keymap. An unknown key is not an error for Hyprland; it presses
/// nothing.
pub async fn press(chord: &Chord) -> Result<()> {
    eval(shortcut_code(chord)).await
}

/// Presses `keys` in order, in one `hyprctl --batch` call per chunk.
pub async fn press_keys(keys: &[KeyPress]) -> Result<()> {
    // Well under the argument length limit, and a failure loses little.
    for chunk in keys.chunks(200) {
        let batch = batch_code(chunk);
        let output = run::capture(TOOL, &["--batch", &batch]).await?;
        batch_result(output.success, &output.stdout_text(), &output.stderr).map_err(failed)?;
    }
    Ok(())
}

/// `dispatch <send_shortcut>; dispatch <send_shortcut>; ...`. Key names
/// never contain `;`, which is what `--batch` splits on.
fn batch_code(keys: &[KeyPress]) -> String {
    keys.iter()
        .map(|(mods, key)| {
            format!(
                "dispatch hl.dsp.send_shortcut({{ mods = {}, key = {} }})",
                lua_quote(mods),
                lua_quote(key)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A batch answers `ok` per command, separated by blank lines.
fn batch_result(success: bool, stdout: &str, stderr: &str) -> std::result::Result<(), String> {
    let answers: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if success && !answers.is_empty() && answers.iter().all(|a| *a == "ok") {
        return Ok(());
    }
    let problem = answers
        .iter()
        .find(|a| **a != "ok")
        .copied()
        .unwrap_or_else(|| stderr.trim());
    Err(problem
        .strip_prefix("error: ")
        .unwrap_or(problem)
        .to_string())
}

fn failed(message: String) -> BackendError {
    BackendError::Failed {
        tool: TOOL,
        message,
    }
}

async fn eval(code: String) -> Result<()> {
    let output = run::capture(TOOL, &["eval", &code]).await?;
    eval_result(output.success, &output.stdout_text(), &output.stderr).map_err(failed)
}

/// `hyprctl eval` prints `ok`, or `error: <lua error>`. Anything that is not
/// a plain `ok` is a failure, whatever the exit code says.
fn eval_result(success: bool, stdout: &str, stderr: &str) -> std::result::Result<(), String> {
    let stdout = stdout.trim();
    if success && stdout == "ok" {
        return Ok(());
    }
    let message = if stdout.is_empty() {
        stderr.trim()
    } else {
        stdout
    };
    Err(message
        .strip_prefix("error: ")
        .unwrap_or(message)
        .to_string())
}

/// Binds inside a submap are made by redefining it: Hyprland adds to a
/// submap on each `define_submap` and never clears it.
fn in_submap(submap: &str, code: String) -> String {
    if submap.is_empty() {
        code
    } else {
        format!(
            "hl.define_submap({}, function() {code} end)",
            lua_quote(submap)
        )
    }
}

fn bind_code(hotkey: &Hotkey) -> String {
    let dispatcher = match &hotkey.action {
        BindAction::Exec(command) => format!("hl.dsp.exec_cmd({})", lua_quote(command)),
        BindAction::Submap(name) => format!("hl.dsp.submap({})", lua_quote(name)),
    };
    in_submap(
        &hotkey.submap,
        format!(
            "hl.bind({}, {dispatcher}, {{ description = {} }})",
            lua_quote(&hotkey.chord.hyprland()),
            lua_quote(&hotkey.description)
        ),
    )
}

fn unbind_code(chord: &Chord, submap: &str) -> String {
    in_submap(
        submap,
        format!("hl.unbind({})", lua_quote(&chord.hyprland())),
    )
}

fn shortcut_code(chord: &Chord) -> String {
    format!(
        "hl.dispatch(hl.dsp.send_shortcut({{ mods = {}, key = {} }}))",
        lua_quote(&chord.mods_hyprland()),
        lua_quote(&chord.key_hyprland())
    )
}

/// A Lua string literal holding exactly `text`. Everything sent to
/// `hyprctl eval` is one of the two fixed calls above with its arguments
/// quoted through here, so rule-provided text cannot become code.
pub fn lua_quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            // Decimal escapes are always three digits so a following digit
            // cannot be read as part of them.
            c if c.is_ascii_control() => quoted.push_str(&format!("\\{:03}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests;
