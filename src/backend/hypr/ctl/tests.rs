//! The Lua we send to Hyprland and how its answers are read.

use super::*;

fn hotkey(chord: &str, command: &str, description: &str) -> Hotkey {
    Hotkey {
        chord: Chord::parse(chord).unwrap(),
        action: BindAction::Exec(command.to_string()),
        description: description.to_string(),
        submap: String::new(),
    }
}

#[test]
fn bind_and_unbind_are_plain_config_calls() {
    assert_eq!(
        bind_code(&hotkey(
            "SUPER, J",
            "'/usr/bin/om' trigger 'hotkey:SUPER+J'",
            "omaestro: init.lua:3"
        )),
        r#"hl.bind("SUPER + J", hl.dsp.exec_cmd("'/usr/bin/om' trigger 'hotkey:SUPER+J'"), { description = "omaestro: init.lua:3" })"#
    );
    assert_eq!(
        unbind_code(&Chord::parse("super+shift+j").unwrap(), ""),
        r#"hl.unbind("SUPER + SHIFT + J")"#
    );
}

#[test]
fn submap_binds_are_wrapped_in_define_submap() {
    let entry = Hotkey {
        chord: Chord::parse("SUPER + ALT + W").unwrap(),
        action: BindAction::Submap("om-super+alt+w".into()),
        description: "omaestro: init.lua:1".into(),
        submap: String::new(),
    };
    assert_eq!(
        bind_code(&entry),
        r#"hl.bind("SUPER + ALT + W", hl.dsp.submap("om-super+alt+w"), { description = "omaestro: init.lua:1" })"#
    );
    let key = Hotkey {
        chord: Chord::parse("h").unwrap(),
        action: BindAction::Exec("om trigger 'mode:SUPER+ALT+W/H'".into()),
        description: "omaestro: init.lua:1".into(),
        submap: "om-super+alt+w".into(),
    };
    assert_eq!(
        bind_code(&key),
        r#"hl.define_submap("om-super+alt+w", function() hl.bind("H", hl.dsp.exec_cmd("om trigger 'mode:SUPER+ALT+W/H'"), { description = "omaestro: init.lua:1" }) end)"#
    );
    assert_eq!(
        unbind_code(&Chord::parse("Escape").unwrap(), "om-super+alt+w"),
        r#"hl.define_submap("om-super+alt+w", function() hl.unbind("Escape") end)"#
    );
}

#[test]
fn keys_are_pressed_in_one_batch() {
    let keys = [
        ("SHIFT", "h".to_string()),
        ("", "i".to_string()),
        ("", "Return".to_string()),
    ];
    assert_eq!(
        batch_code(&keys),
        "dispatch hl.dsp.send_shortcut({ mods = \"SHIFT\", key = \"h\" }); \
         dispatch hl.dsp.send_shortcut({ mods = \"\", key = \"i\" }); \
         dispatch hl.dsp.send_shortcut({ mods = \"\", key = \"Return\" })"
    );
    assert_eq!(batch_result(true, "ok\n\n\nok\n\n\nok\n\n", ""), Ok(()));
    assert_eq!(
        batch_result(true, "ok\n\n\nerror: nope\n\n", ""),
        Err("nope".to_string())
    );
    assert_eq!(
        batch_result(false, "", "Couldn't connect"),
        Err("Couldn't connect".to_string())
    );
}

#[test]
fn nothing_escapes_a_lua_string() {
    assert_eq!(lua_quote("plain"), r#""plain""#);
    assert_eq!(lua_quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    assert_eq!(lua_quote("line\nbreak\t1"), r#""line\010break\0091""#);
    assert_eq!(lua_quote("]] os.exit() --"), r#""]] os.exit() --""#);
    assert_eq!(lua_quote("ключ \"x\""), "\"ключ \\\"x\\\"\"");

    // Round trip through a real Lua parser.
    let lua = mlua::Lua::new();
    for text in [
        "",
        "a \"b\" \\c",
        "line\nbreak\r\t\0 1",
        "ключ ✓",
        "'single' $HOME `x`",
    ] {
        let back: String = lua
            .load(format!("return {}", lua_quote(text)))
            .eval()
            .unwrap();
        assert_eq!(back, text);
    }
}

#[test]
fn eval_answers() {
    assert_eq!(eval_result(true, "ok\n", ""), Ok(()));
    assert_eq!(
        eval_result(
            false,
            "error: hl.bind: failed to parse key string: Unknown keysym: \"NOPE\"\n",
            ""
        ),
        Err("hl.bind: failed to parse key string: Unknown keysym: \"NOPE\"".to_string())
    );
    // A success exit code with anything but `ok` is still a failure.
    assert_eq!(
        eval_result(
            true,
            "keyword can't work with non-legacy parsers. Use eval.",
            ""
        ),
        Err("keyword can't work with non-legacy parsers. Use eval.".to_string())
    );
    assert_eq!(
        eval_result(
            false,
            "",
            "Couldn't connect to /run/user/1000/hypr/x/.socket.sock. (4)"
        ),
        Err("Couldn't connect to /run/user/1000/hypr/x/.socket.sock. (4)".to_string())
    );
}

#[test]
fn binds_listing() {
    let json = br#"[
        {"locked": false, "mouse": false, "modmask": 64, "submap": "", "key": "J", "keycode": 0,
         "catch_all": false, "description": "Toggle window split", "dispatcher": "__lua", "arg": "25"},
        {"modmask": 0, "submap": "resize", "key": "", "keycode": 36, "description": "", "dispatcher": "exec", "arg": "x"}
    ]"#;
    let binds = parse_binds(json).unwrap();
    assert_eq!(binds.len(), 2);
    assert!(
        binds[0]
            .chord
            .same_keys(&Chord::parse("SUPER + J").unwrap())
    );
    assert_eq!(binds[0].description, "Toggle window split");
    assert_eq!(binds[0].submap, "");
    assert_eq!(binds[1].chord.hyprland(), "code:36");
    assert_eq!(binds[1].submap, "resize");
    assert!(parse_binds(b"Couldn't connect").is_err());
}

#[test]
fn active_window() {
    let json = br#"{"address": "0x59ef37d8e800", "mapped": true, "at": [4, 30], "size": [2027, 1694],
        "workspace": {"id": 6, "name": "6"}, "monitor": 1, "floating": false, "fullscreen": 0, "pinned": false,
        "class": "google-chrome", "title": "Some page - Google Chrome", "initialClass": "google-chrome",
        "pid": 4242, "xwayland": false, "focusHistoryID": 0}"#;
    assert_eq!(
        parse_window(json).unwrap(),
        Some(Window {
            address: "0x59ef37d8e800".into(),
            class: "google-chrome".into(),
            title: "Some page - Google Chrome".into(),
            initial_class: "google-chrome".into(),
            workspace: "6".into(),
            workspace_id: 6,
            monitor: 1,
            x: 4,
            y: 30,
            width: 2027,
            height: 1694,
            floating: false,
            fullscreen: 0,
            pinned: false,
            pid: 4242,
            xwayland: false,
            focused: true,
        })
    );
    assert_eq!(parse_window(b"{}").unwrap(), None);
    assert!(parse_window(b"nope").is_err());
}

#[test]
fn clients_monitors_and_workspaces() {
    let clients = parse_clients(
        br#"[{"address": "0xa", "class": "code", "focusHistoryID": 1, "at": [0, 0], "size": [1, 1]},
             {"address": "0xb", "class": "foot", "focusHistoryID": 0, "at": [0, 0], "size": [1, 1]}]"#,
    )
    .unwrap();
    assert_eq!(clients.len(), 2);
    assert!(!clients[0].focused);
    assert!(clients[1].focused);

    let monitors = parse_monitors(
        br#"[{"id": 1, "name": "DP-8", "description": "BNQ BenQ RD320U", "width": 3840, "height": 2160,
             "x": -1728, "y": 0, "scale": 1.25, "transform": 1, "focused": false,
             "activeWorkspace": {"id": 2, "name": "2"}, "reserved": [0, 26, 28, 0]}]"#,
    )
    .unwrap();
    assert_eq!(monitors[0].name, "DP-8");
    assert_eq!(monitors[0].workspace_id, 2);
    assert_eq!(monitors[0].reserved, [0, 26, 28, 0]);
    assert_eq!(monitors[0].transform, 1);

    let workspaces = parse_workspaces(
        br#"[{"id": 1, "name": "1", "monitor": "DP-2", "windows": 4, "hasfullscreen": false}]"#,
    )
    .unwrap();
    assert_eq!(workspaces[0].monitor, "DP-2");
    assert_eq!(workspaces[0].windows, 4);
}
