//! `om.launch`, `om.focus`, `om.apps`.

use std::time::Duration;

use super::*;
use crate::backend::Window;

fn client(address: &str, class: &str, title: &str) -> Window {
    Window {
        address: address.into(),
        class: class.into(),
        title: title.into(),
        initial_class: class.into(),
        workspace: "1".into(),
        workspace_id: 1,
        width: 800,
        height: 600,
        ..Window::default()
    }
}

#[tokio::test]
async fn launch_goes_through_hyprland_exec() {
    let h = Harness::start(&[]).await;
    h.eval("om.launch('uwsm-app -- firefox --new-window \"https://example.com\"')")
        .await
        .unwrap();
    assert_eq!(
        h.fakes.journal.entries(),
        [r#"dispatch hl.dsp.exec_cmd("uwsm-app -- firefox --new-window \"https://example.com\"")"#]
    );
    let err = h
        .eval("local r = om.launch('  ') return r")
        .await
        .unwrap_err();
    assert_eq!(err, "eval:1: launch: the command is empty");
}

#[tokio::test]
async fn focus_finds_an_existing_window_by_class_or_by_table() {
    let h = Harness::start(&[]).await;
    h.fakes
        .hypr
        .add_client(client("0xa", "firefox", "Docs - Firefox"));
    h.fakes
        .hypr
        .add_client(client("0xb", "code", "main.rs - VS Code"));

    assert_eq!(
        h.eval("return om.focus('^code$').address").await.unwrap(),
        ["0xb"]
    );
    assert_eq!(
        h.eval("return om.focus({title = 'Docs'}).class")
            .await
            .unwrap(),
        ["firefox"]
    );
    assert_eq!(
        h.eval("return om.focus('nothing-here')").await.unwrap(),
        ["nil"]
    );
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.focus({ window = \"address:0xb\" })",
            "dispatch hl.dsp.focus({ window = \"address:0xa\" })",
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn focus_launches_and_waits_for_the_window_when_there_is_none() {
    let h = Harness::start(&[]).await;
    // The app takes a moment to show a window.
    let hypr = h.fakes.hypr.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(350)).await;
        hypr.add_client(client("0xc", "Spotify", "Spotify"));
    });
    assert_eq!(
        h.eval("return om.focus('^Spotify$', 'uwsm-app -- spotify').address")
            .await
            .unwrap(),
        ["0xc"]
    );
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.exec_cmd(\"uwsm-app -- spotify\")",
            "dispatch hl.dsp.focus({ window = \"address:0xc\" })",
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn focus_gives_up_after_the_launch_wait() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("return om.focus('^never$', 'true')").await.unwrap(),
        ["nil"]
    );
    assert_eq!(
        h.fakes.journal.entries(),
        ["dispatch hl.dsp.exec_cmd(\"true\")"]
    );
}

#[tokio::test]
async fn apps_groups_windows_by_class() {
    let h = Harness::start(&[]).await;
    h.fakes.hypr.add_client(client("0xa", "firefox", "Docs"));
    h.fakes.hypr.add_client(client("0xb", "code", "main.rs"));
    h.fakes.hypr.add_client(client("0xc", "firefox", "Mail"));
    assert_eq!(
        h.eval(
            "local out = {} for _, app in ipairs(om.apps()) do out[#out + 1] = app.class .. '=' .. app.count .. ':' .. app.windows[1].address end return table.concat(out, ' ')"
        )
        .await
        .unwrap(),
        ["firefox=2:0xa code=1:0xb"]
    );
}
