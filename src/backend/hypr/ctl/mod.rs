//! Commands to Hyprland through `hyprctl`.
//!
//! Hyprland 0.56 configures itself in Lua, and runtime changes go through
//! `hyprctl eval <lua>`; `hyprctl keyword` is refused there. Only three
//! calls are ever evaluated: `hl.bind(...)` and `hl.unbind(...)`, the same
//! ones a config file uses, and `hl.dispatch(hl.dsp.send_shortcut(...))`,
//! which presses a chord in the focused window with the real keymap (a
//! virtual keyboard's chords never reached Chromium and Electron apps). The
//! `HL.Keybind` object `hl.bind` returns is never kept: calling a method on
//! one whose bind is already gone crashes Hyprland 0.56.2.

use serde::Deserialize;

use super::super::{BackendError, BindInfo, BoxFuture, Hotkey, Hypr, Result, Window, run};
use crate::chord::{Chord, KeyPress};

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

    fn unbind<'a>(&'a self, chord: &'a Chord) -> BoxFuture<'a, Result<()>> {
        Box::pin(eval(unbind_code(chord)))
    }

    fn active_window(&self) -> BoxFuture<'_, Result<Option<Window>>> {
        Box::pin(async move {
            let output = run::run(TOOL, &["-j", "activewindow"]).await?;
            parse_window(&output.stdout).map_err(failed)
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

fn bind_code(hotkey: &Hotkey) -> String {
    format!(
        "hl.bind({}, hl.dsp.exec_cmd({}), {{ description = {} }})",
        lua_quote(&hotkey.chord.hyprland()),
        lua_quote(&hotkey.command),
        lua_quote(&hotkey.description)
    )
}

fn unbind_code(chord: &Chord) -> String {
    format!("hl.unbind({})", lua_quote(&chord.hyprland()))
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
fn lua_quote(text: &str) -> String {
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

#[derive(Deserialize)]
struct RawBind {
    #[serde(default)]
    modmask: u32,
    #[serde(default)]
    key: String,
    #[serde(default)]
    keycode: i64,
    #[serde(default)]
    submap: String,
    #[serde(default)]
    description: String,
}

fn parse_binds(json: &[u8]) -> std::result::Result<Vec<BindInfo>, String> {
    let raw: Vec<RawBind> = serde_json::from_slice(json)
        .map_err(|err| format!("unexpected output from `hyprctl -j binds`: {err}"))?;
    Ok(raw
        .into_iter()
        .map(|bind| {
            let key = if bind.key.is_empty() && bind.keycode > 0 {
                format!("code:{}", bind.keycode)
            } else {
                bind.key
            };
            BindInfo {
                chord: Chord::from_modmask(bind.modmask, &key),
                description: bind.description,
                submap: bind.submap,
            }
        })
        .collect())
}

#[derive(Deserialize)]
struct RawWindow {
    address: Option<String>,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    workspace: RawWorkspace,
    #[serde(default)]
    floating: bool,
}

#[derive(Deserialize, Default)]
struct RawWorkspace {
    #[serde(default)]
    name: String,
}

/// With nothing focused Hyprland answers `{}`.
fn parse_window(json: &[u8]) -> std::result::Result<Option<Window>, String> {
    let raw: RawWindow = serde_json::from_slice(json)
        .map_err(|err| format!("unexpected output from `hyprctl -j activewindow`: {err}"))?;
    Ok(raw.address.map(|address| Window {
        class: raw.class,
        title: raw.title,
        address,
        workspace: raw.workspace.name,
        floating: raw.floating,
    }))
}

#[cfg(test)]
mod tests;
