//! The Lua we send to Hyprland and how its answers are read.

use super::*;

fn hotkey(chord: &str, command: &str, description: &str) -> Hotkey {
    Hotkey {
        chord: Chord::parse(chord).unwrap(),
        command: command.to_string(),
        description: description.to_string(),
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
        unbind_code(&Chord::parse("super+shift+j").unwrap()),
        r#"hl.unbind("SUPER + SHIFT + J")"#
    );
}

#[test]
fn a_chord_is_pressed_through_send_shortcut() {
    assert_eq!(
        shortcut_code(&Chord::parse("ctrl+shift+v").unwrap()),
        r#"hl.dispatch(hl.dsp.send_shortcut({ mods = "CTRL SHIFT", key = "V" }))"#
    );
    assert_eq!(
        shortcut_code(&Chord::parse("Return").unwrap()),
        r#"hl.dispatch(hl.dsp.send_shortcut({ mods = "", key = "Return" }))"#
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
    let json = br#"{"address": "0x59ef37d8e800", "mapped": true, "at": [4, 30],
        "workspace": {"id": 6, "name": "6"}, "floating": false, "class": "google-chrome",
        "title": "Some page - Google Chrome"}"#;
    assert_eq!(
        parse_window(json).unwrap(),
        Some(Window {
            class: "google-chrome".into(),
            title: "Some page - Google Chrome".into(),
            address: "0x59ef37d8e800".into(),
            workspace: "6".into(),
            floating: false,
        })
    );
    assert_eq!(parse_window(b"{}").unwrap(), None);
    assert!(parse_window(b"nope").is_err());
}
