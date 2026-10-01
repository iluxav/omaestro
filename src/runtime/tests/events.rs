//! `om.on_open`, `om.on_close`, `om.on_title`, `om.on_workspace`, `om.on_monitor`.

use super::*;
use crate::backend::hypr::events::{MonitorChange, WinRef};

fn win(class: &str, title: &str, address: &str, workspace: &str) -> WinRef {
    WinRef {
        class: class.into(),
        title: title.into(),
        address: address.into(),
        workspace: workspace.into(),
    }
}

const RULES: (&str, &str) = (
    "init.lua",
    "om.on_open({class = '^spotify$'}, function(w) om.notify('open', w.class .. '@' .. w.workspace) end)\n\
     om.on_close({class = '^spotify$'}, function(w) om.notify('close', w.class .. '|' .. w.title) end)\n\
     om.on_title({class = '^spotify$', title = 'Radio'}, function(w) om.notify('title', w.title) end)\n\
     om.on_workspace(function(ws) om.notify('workspace', ws.name .. '#' .. ws.id) end)\n\
     om.on_monitor(function(m) om.notify('monitor', m.name .. ':' .. m.change) end)",
);

#[tokio::test]
async fn open_title_and_close_fire_for_matching_windows_with_what_is_known() {
    let h = Harness::start(&[RULES]).await;
    h.hyprland(HyprEvent::Opened(win("spotify", "Spotify", "0x5", "3")))
        .await;
    h.hyprland(HyprEvent::Opened(win("firefox", "Docs", "0x6", "3")))
        .await;
    h.hyprland(HyprEvent::Title {
        address: "0x5".into(),
        title: "Radio - Spotify".into(),
    })
    .await;
    h.hyprland(HyprEvent::Title {
        address: "0x5".into(),
        title: "Playlist".into(),
    })
    .await;
    // Close only carries the address; the class and last title come from memory.
    h.hyprland(HyprEvent::Closed {
        address: "0x5".into(),
    })
    .await;
    h.hyprland(HyprEvent::Closed {
        address: "0x6".into(),
    })
    .await;
    h.settle().await;
    let seen: Vec<String> = h
        .fakes
        .notifier
        .sent()
        .into_iter()
        .map(|(t, b)| format!("{t}:{b}"))
        .collect();
    assert_eq!(
        seen,
        [
            "open:spotify@3",
            "title:Radio - Spotify",
            "close:spotify|Playlist"
        ]
    );
}

#[tokio::test]
async fn workspace_and_monitor_events_reach_their_handlers() {
    let h = Harness::start(&[RULES]).await;
    h.hyprland(HyprEvent::Workspace {
        id: 4,
        name: "4".into(),
    })
    .await;
    h.hyprland(HyprEvent::Monitor {
        name: "DP-8".into(),
        change: MonitorChange::Added,
    })
    .await;
    h.settle().await;
    let seen: Vec<String> = h
        .fakes
        .notifier
        .sent()
        .into_iter()
        .map(|(t, b)| format!("{t}:{b}"))
        .collect();
    assert_eq!(seen, ["workspace:4#4", "monitor:DP-8:added"]);
}

#[tokio::test]
async fn event_windows_are_objects_with_methods() {
    let h = Harness::start(&[(
        "init.lua",
        "om.on_open({}, function(w) w:to_workspace(9) w:focus() end)",
    )])
    .await;
    h.hyprland(HyprEvent::Opened(win("spotify", "Spotify", "0x5", "3")))
        .await;
    h.settle().await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.window.move({ workspace = \"9\", follow = false, window = \"address:0x5\" })",
            "dispatch hl.dsp.focus({ window = \"address:0x5\" })",
        ]
    );
}

#[tokio::test]
async fn a_focused_window_that_closes_blurs_and_is_forgotten() {
    let h = Harness::start(&[(
        "init.lua",
        "om.on_blur({}, function(w) om.notify('blur', w.address) end)\n\
         om.on_focus({}, function(w) om.notify('focus', w.address) end)",
    )])
    .await;
    h.hyprland(HyprEvent::Focus(Some(win("foot", "shell", "0x1", ""))))
        .await;
    h.hyprland(HyprEvent::Closed {
        address: "0x1".into(),
    })
    .await;
    // Focus lands on the same address again (a new window reused it).
    h.hyprland(HyprEvent::Focus(Some(win("foot", "shell", "0x1", ""))))
        .await;
    h.settle().await;
    let seen: Vec<String> = h
        .fakes
        .notifier
        .sent()
        .into_iter()
        .map(|(t, b)| format!("{t}:{b}"))
        .collect();
    assert_eq!(seen, ["focus:0x1", "blur:0x1", "focus:0x1"]);
}

#[tokio::test]
async fn the_window_rules_plugin_moves_a_new_window_to_its_workspace() {
    let h = Harness::start(&[]).await;
    h.install_builtin("window-rules");
    assert!(
        h.save(&[(
            "rules.d/wr.lua",
            "om.use('window-rules').setup({ rules = { { class = '^[Ss]potify$', workspace = 9 } } })"
        )])
        .await
        .ok
    );
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    h.hyprland(HyprEvent::Opened(win("Spotify", "Spotify", "0x7", "1")))
        .await;
    h.settle().await;
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.window.move({ workspace = \"9\", follow = false, window = \"address:0x7\" })"
        ]
    );
}
