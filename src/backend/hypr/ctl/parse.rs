//! The JSON `hyprctl -j` prints, as our types.

use serde::Deserialize;

use crate::backend::{BindInfo, Monitor, Window, Workspace};
use crate::chord::Chord;

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

pub(super) fn parse_binds(json: &[u8]) -> std::result::Result<Vec<BindInfo>, String> {
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
    #[serde(default, rename = "initialClass")]
    initial_class: String,
    #[serde(default)]
    workspace: RawWorkspaceRef,
    #[serde(default)]
    monitor: i64,
    #[serde(default)]
    at: [i64; 2],
    #[serde(default)]
    size: [i64; 2],
    #[serde(default)]
    floating: bool,
    #[serde(default)]
    fullscreen: i64,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    pid: i64,
    #[serde(default)]
    xwayland: bool,
    #[serde(default = "no_focus", rename = "focusHistoryID")]
    focus_history_id: i64,
}

fn no_focus() -> i64 {
    -1
}

#[derive(Deserialize, Default)]
struct RawWorkspaceRef {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
}

impl RawWindow {
    fn into_window(self) -> Option<Window> {
        let address = self.address?;
        Some(Window {
            address,
            class: self.class,
            title: self.title,
            initial_class: self.initial_class,
            workspace: self.workspace.name,
            workspace_id: self.workspace.id,
            monitor: self.monitor,
            x: self.at[0],
            y: self.at[1],
            width: self.size[0],
            height: self.size[1],
            floating: self.floating,
            fullscreen: self.fullscreen,
            pinned: self.pinned,
            pid: self.pid,
            xwayland: self.xwayland,
            focused: self.focus_history_id == 0,
        })
    }
}

/// With nothing focused Hyprland answers `{}`.
pub(super) fn parse_window(json: &[u8]) -> std::result::Result<Option<Window>, String> {
    let raw: RawWindow = serde_json::from_slice(json)
        .map_err(|err| format!("unexpected output from `hyprctl -j activewindow`: {err}"))?;
    Ok(raw.into_window())
}

pub(super) fn parse_clients(json: &[u8]) -> std::result::Result<Vec<Window>, String> {
    let raw: Vec<RawWindow> = serde_json::from_slice(json)
        .map_err(|err| format!("unexpected output from `hyprctl -j clients`: {err}"))?;
    Ok(raw.into_iter().filter_map(RawWindow::into_window).collect())
}

#[derive(Deserialize)]
struct RawMonitor {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64,
    #[serde(default)]
    width: i64,
    #[serde(default)]
    height: i64,
    #[serde(default)]
    scale: f64,
    #[serde(default)]
    transform: i64,
    #[serde(default)]
    focused: bool,
    #[serde(default, rename = "activeWorkspace")]
    active_workspace: RawWorkspaceRef,
    #[serde(default)]
    reserved: [i64; 4],
}

pub(super) fn parse_monitors(json: &[u8]) -> std::result::Result<Vec<Monitor>, String> {
    let raw: Vec<RawMonitor> = serde_json::from_slice(json)
        .map_err(|err| format!("unexpected output from `hyprctl -j monitors`: {err}"))?;
    Ok(raw
        .into_iter()
        .map(|m| Monitor {
            id: m.id,
            name: m.name,
            description: m.description,
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
            scale: m.scale,
            transform: m.transform,
            focused: m.focused,
            workspace: m.active_workspace.name,
            workspace_id: m.active_workspace.id,
            reserved: m.reserved,
        })
        .collect())
}

#[derive(Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    monitor: String,
    #[serde(default)]
    windows: i64,
    #[serde(default)]
    hasfullscreen: bool,
}

pub(super) fn parse_workspaces(json: &[u8]) -> std::result::Result<Vec<Workspace>, String> {
    let raw: Vec<RawWorkspace> = serde_json::from_slice(json)
        .map_err(|err| format!("unexpected output from `hyprctl -j workspaces`: {err}"))?;
    Ok(raw
        .into_iter()
        .map(|w| Workspace {
            id: w.id,
            name: w.name,
            monitor: w.monitor,
            windows: w.windows,
            has_fullscreen: w.hasfullscreen,
        })
        .collect())
}
