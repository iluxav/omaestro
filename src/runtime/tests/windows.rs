//! Window objects, their methods, and the monitor and workspace queries.

use super::*;
use crate::backend::{Monitor, Window};

fn client(address: &str, class: &str, title: &str, workspace: i64, monitor: i64) -> Window {
    Window {
        address: address.into(),
        class: class.into(),
        title: title.into(),
        initial_class: class.into(),
        workspace: workspace.to_string(),
        workspace_id: workspace,
        monitor,
        x: 10,
        y: 40,
        width: 800,
        height: 600,
        ..Window::default()
    }
}

#[tokio::test]
async fn the_focused_window_is_a_table_of_facts() {
    let h = Harness::start(&[]).await;
    assert_eq!(h.eval("return om.window()").await.unwrap(), ["nil"]);
    h.fakes.hypr.set_window("firefox", "Some page");
    assert_eq!(
        h.eval("local w = om.window() return w.class, w.title, w.address, w.workspace, w.x, w.width, w.floating, w.focused, w.fullscreen_mode")
            .await
            .unwrap(),
        ["firefox", "Some page", "0x1", "1", "0", "1200", "false", "true", "none"]
    );
}

#[tokio::test]
async fn windows_lists_and_filters_clients() {
    let h = Harness::start(&[]).await;
    h.fakes
        .hypr
        .add_client(client("0xa", "firefox", "Docs - Firefox", 1, 0));
    h.fakes
        .hypr
        .add_client(client("0xb", "code", "main.rs - VS Code", 1, 0));
    h.fakes
        .hypr
        .add_client(client("0xc", "firefox", "Cats - YouTube - Firefox", 2, 1));

    async fn addresses(h: &Harness, filter: &str) -> Vec<String> {
        let code = format!(
            "local out = {{}} for _, w in ipairs(om.windows({filter})) do out[#out + 1] = w.address end return table.concat(out, ',')"
        );
        h.eval(&code).await.unwrap()
    }
    let classes = addresses;
    let h_ref = &h;
    assert_eq!(classes(h_ref, "").await, ["0xa,0xb,0xc"]);
    assert_eq!(classes(h_ref, "{class = '^firefox$'}").await, ["0xa,0xc"]);
    assert_eq!(classes(h_ref, "{title = 'YouTube'}").await, ["0xc"]);
    assert_eq!(classes(h_ref, "{workspace = 1}").await, ["0xa,0xb"]);
    assert_eq!(classes(h_ref, "{workspace = '2'}").await, ["0xc"]);
    assert_eq!(classes(h_ref, "{monitor = 1}").await, ["0xc"]);
    assert_eq!(classes(h_ref, "{monitor = 'WL-1'}").await, ["0xa,0xb"]);
    assert_eq!(
        classes(h_ref, "{class = 'firefox', workspace = 1}").await,
        ["0xa"]
    );
    assert_eq!(classes(h_ref, "{class = 'nothing'}").await, [""]);
}

#[tokio::test]
async fn methods_dispatch_against_the_window_address() {
    let h = Harness::start(&[]).await;
    h.fakes
        .hypr
        .add_client(client("0xb", "code", "VS Code", 1, 0));
    h.eval(
        "local w = om.windows({class = 'code'})[1]\n\
         w:move(100, 200) w:resize(640, 480) w:float(true) w:float(false) w:float()\n\
         w:pin(true) w:fullscreen() w:fullscreen('maximized') w:focus() w:center()\n\
         w:to_workspace(3) w:to_workspace('special:scratch', true) w:close()",
    )
    .await
    .unwrap();
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.window.move({ x = 100, y = 200, window = \"address:0xb\" })",
            "dispatch hl.dsp.window.resize({ x = 640, y = 480, window = \"address:0xb\" })",
            "dispatch hl.dsp.window.float({ action = \"enable\", window = \"address:0xb\" })",
            "dispatch hl.dsp.window.float({ action = \"disable\", window = \"address:0xb\" })",
            "dispatch hl.dsp.window.float({ action = \"toggle\", window = \"address:0xb\" })",
            "dispatch hl.dsp.window.pin({ action = \"enable\", window = \"address:0xb\" })",
            "dispatch hl.dsp.window.fullscreen({ mode = \"fullscreen\", window = \"address:0xb\" })",
            "dispatch hl.dsp.window.fullscreen({ mode = \"maximized\", window = \"address:0xb\" })",
            "dispatch hl.dsp.focus({ window = \"address:0xb\" })",
            "dispatch hl.dsp.window.center({ window = \"address:0xb\" })",
            "dispatch hl.dsp.window.move({ workspace = \"3\", follow = false, window = \"address:0xb\" })",
            "dispatch hl.dsp.window.move({ workspace = \"special:scratch\", follow = true, window = \"address:0xb\" })",
            "dispatch hl.dsp.window.close({ window = \"address:0xb\" })",
        ]
    );

    let err = h
        .eval("local w = om.windows()[1] local r = w:fullscreen('huge') return r")
        .await
        .unwrap_err();
    assert_eq!(
        err,
        "eval:1: fullscreen: mode is \"fullscreen\" or \"maximized\", not \"huge\""
    );
    // Calling a method with a dot instead of a colon is a bad argument, not a crash.
    let err = h
        .eval("local w = om.windows()[1] local r = w.move(1, 2) return r")
        .await
        .unwrap_err();
    assert!(err.starts_with("eval:1: bad argument"), "{err}");
}

#[tokio::test]
async fn place_floats_and_puts_the_window_in_the_monitor_area() {
    let h = Harness::start(&[]).await;
    // The fake monitor: 1920x1080 at 0,0 with a 30 px bar reserved on top.
    h.fakes.hypr.set_window("firefox", "Docs");
    h.eval("om.window():place('left')").await.unwrap();
    h.eval("om.window():place({x = 0.5, y = 0.5, w = 0.5, h = 0.5})")
        .await
        .unwrap();
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.window.float({ action = \"enable\", window = \"address:0x1\" })",
            "dispatch hl.dsp.window.resize({ x = 960, y = 1050, window = \"address:0x1\" })",
            "dispatch hl.dsp.window.move({ x = 0, y = 30, window = \"address:0x1\" })",
            "dispatch hl.dsp.window.float({ action = \"enable\", window = \"address:0x1\" })",
            "dispatch hl.dsp.window.resize({ x = 960, y = 525, window = \"address:0x1\" })",
            "dispatch hl.dsp.window.move({ x = 960, y = 555, window = \"address:0x1\" })",
        ]
    );
    let err = h
        .eval("local r = om.window():place('middle') return r")
        .await
        .unwrap_err();
    assert!(
        err.starts_with("eval:1: unknown placement 'middle'"),
        "{err}"
    );
}

#[tokio::test]
async fn monitors_and_workspaces() {
    let h = Harness::start(&[]).await;
    h.fakes.hypr.add_monitor(Monitor {
        id: 1,
        name: "DP-8".into(),
        description: "BNQ BenQ RD320U".into(),
        x: -1728,
        y: 0,
        width: 3840,
        height: 2160,
        scale: 1.25,
        transform: 1,
        focused: false,
        workspace: "2".into(),
        workspace_id: 2,
        reserved: [0, 26, 28, 0],
    });
    assert_eq!(
        h.eval("local m = om.monitors() return #m, m[2].name, m[2].width, m[2].height, m[2].x, m[2].focused")
            .await
            .unwrap(),
        ["2", "DP-8", "1728", "3072", "-1728", "false"]
    );
    assert_eq!(
        h.eval("local m = om.monitor() return m.name, m.width, m.height")
            .await
            .unwrap(),
        ["WL-1", "1920", "1080"]
    );
    assert_eq!(
        h.eval("return om.monitor('DP-8').workspace").await.unwrap(),
        ["2"]
    );
    assert_eq!(h.eval("return om.monitor('nope')").await.unwrap(), ["nil"]);
    assert_eq!(
        h.eval("local w = om.workspace() return w.id, w.name, w.monitor")
            .await
            .unwrap(),
        ["1", "1", "WL-1"]
    );
    assert_eq!(h.eval("return #om.workspaces()").await.unwrap(), ["2"]);
}
