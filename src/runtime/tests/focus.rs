//! `om.on_focus`, `om.on_blur`, `om.dispatch`, `om.key`, `om.type`.

use super::*;
use crate::backend::hypr::events::WinRef;

fn window(class: &str, title: &str, address: &str) -> Option<WinRef> {
    Some(WinRef {
        class: class.into(),
        title: title.into(),
        address: address.into(),
        workspace: String::new(),
    })
}

const RULES: (&str, &str) = (
    "init.lua",
    "om.on_focus({class = '^firefox$'}, function(win)\n\
       om.notify('focus', win.class .. ' | ' .. win.title .. ' | ' .. win.address)\n\
     end)\n\
     om.on_blur({class = '^firefox$'}, function(win)\n\
       om.notify('blur', win.title)\n\
     end)\n\
     om.on_focus({title = 'Inbox'}, function(win)\n\
       om.notify('inbox', win.class)\n\
     end)",
);

#[tokio::test]
async fn focus_and_blur_handlers_get_the_window() {
    let h = Harness::start(&[RULES]).await;
    h.hyprland(HyprEvent::Focus(window(
        "firefox",
        "Docs - Mozilla Firefox",
        "0x1",
    )))
    .await;
    h.settle().await;
    assert_eq!(
        h.fakes.notifier.sent(),
        [(
            "focus".to_string(),
            "firefox | Docs - Mozilla Firefox | 0x1".to_string()
        )]
    );

    // Moving to another window: blur for firefox, and the title matcher.
    h.hyprland(HyprEvent::Focus(window(
        "thunderbird",
        "Inbox - Mail",
        "0x2",
    )))
    .await;
    h.settle().await;
    let sent = h.fakes.notifier.sent();
    assert_eq!(
        sent[1],
        ("blur".to_string(), "Docs - Mozilla Firefox".to_string())
    );
    assert_eq!(sent[2], ("inbox".to_string(), "thunderbird".to_string()));
    assert_eq!(sent.len(), 3);

    // Focus going nowhere blurs; nothing to focus.
    h.hyprland(HyprEvent::Focus(None)).await;
    h.settle().await;
    assert_eq!(h.fakes.notifier.sent().len(), 3);
    h.hyprland(HyprEvent::Focus(window(
        "firefox",
        "Inbox - Firefox",
        "0x3",
    )))
    .await;
    h.settle().await;
    let sent = h.fakes.notifier.sent();
    assert_eq!(sent.len(), 5, "{sent:?}");
    assert!(sent[3..].iter().any(|(t, _)| t == "focus"));
    assert!(sent[3..].iter().any(|(t, _)| t == "inbox"));
}

#[tokio::test]
async fn the_same_window_again_is_not_a_focus_change() {
    let h = Harness::start(&[RULES]).await;
    let firefox = window("firefox", "Docs", "0x1");
    h.hyprland(HyprEvent::Focus(firefox.clone())).await;
    h.hyprland(HyprEvent::Focus(firefox)).await;
    h.settle().await;
    assert_eq!(h.fakes.notifier.sent().len(), 1);
}

#[tokio::test]
async fn matchers_are_lua_patterns_and_all_must_match() {
    let h = Harness::start(&[(
        "init.lua",
        "om.on_focus({class = 'chrom', title = '%- YouTube$'}, function(win) om.notify('yt', win.title) end)\n\
         om.on_focus({}, function(win) om.notify('any', win.class) end)",
    )])
    .await;
    h.hyprland(HyprEvent::Focus(window(
        "google-chrome",
        "Cats - YouTube",
        "0x1",
    )))
    .await;
    h.hyprland(HyprEvent::Focus(window("google-chrome", "Docs", "0x2")))
        .await;
    h.hyprland(HyprEvent::Focus(window("firefox", "Cats - YouTube", "0x3")))
        .await;
    h.settle().await;
    let titles: Vec<String> = h
        .fakes
        .notifier
        .sent()
        .into_iter()
        .map(|(t, b)| format!("{t}:{b}"))
        .collect();
    assert_eq!(
        titles,
        [
            "yt:Cats - YouTube",
            "any:google-chrome",
            "any:google-chrome",
            "any:firefox"
        ]
    );
}

#[tokio::test]
async fn a_bad_pattern_fails_at_the_rule_line() {
    let h = Harness::start(&[(
        "init.lua",
        "\nom.on_focus({class = '[oops'}, function() end)",
    )])
    .await;
    let errors = h.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].starts_with("init.lua:2: om.on_focus: bad class pattern '[oops': "),
        "{errors:?}"
    );
}

#[tokio::test]
async fn focus_triggers_are_listed_and_removable() {
    let h = Harness::start(&[RULES]).await;
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    let kinds: Vec<String> = rows
        .iter()
        .map(|r| format!("{} {}", r.kind, r.origin))
        .collect();
    assert_eq!(
        kinds,
        [
            "on_blur init.lua:4",
            "on_focus init.lua:1",
            "on_focus init.lua:7"
        ]
    );

    h.eval("h = om.on_focus({class = 'x'}, function() om.notify('x') end)")
        .await
        .unwrap();
    assert_eq!(h.status().await.triggers, 4);
    assert_eq!(h.eval("return h:remove()").await.unwrap(), ["true"]);
    assert_eq!(h.status().await.triggers, 3);
}

#[tokio::test]
async fn dispatch_key_and_type_reach_the_backends() {
    let h = Harness::start(&[]).await;
    h.eval("om.dispatch('hl.dsp.window.float()')")
        .await
        .unwrap();
    h.eval("om.key('ctrl+shift+t')").await.unwrap();
    h.eval("om.type('2026-09-30 ✓')").await.unwrap();
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.window.float()",
            "key CTRL+SHIFT+T",
            "type \"2026-09-30 ✓\"",
        ]
    );

    assert_eq!(
        h.eval("local r = om.dispatch('') return r")
            .await
            .unwrap_err(),
        "eval:1: om.dispatch: nothing to dispatch"
    );
    assert!(
        h.eval("local r = om.dispatch('hl.dsp.nil()') return r")
            .await
            .unwrap_err()
            .contains("nil value")
    );
    assert_eq!(
        h.eval("local r = om.key('HYPER+x') return r")
            .await
            .unwrap_err(),
        "eval:1: om.key: unknown modifier 'HYPER' in 'HYPER+x'"
    );
}

#[tokio::test]
async fn the_window_rules_plugin_floats_a_window_once() {
    let h = Harness::start(&[]).await;
    h.install_builtin("window-rules");
    assert!(
        h.save(&[("rules.d/wr.lua", "om.use('window-rules').setup({})")])
            .await
            .ok
    );
    h.hyprland(HyprEvent::Focus(window(
        "org.gnome.Calculator",
        "Calculator",
        "0x1",
    )))
    .await;
    h.settle().await;
    h.hyprland(HyprEvent::Focus(window("firefox", "Docs", "0x2")))
        .await;
    h.hyprland(HyprEvent::Focus(window(
        "org.gnome.Calculator",
        "Calculator",
        "0x1",
    )))
    .await;
    h.settle().await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    assert_eq!(
        h.fakes.journal.entries(),
        [
            "dispatch hl.dsp.window.float({ action = \"enable\", window = \"address:0x1\" })",
            "dispatch hl.dsp.window.center({ window = \"address:0x1\" })"
        ]
    );
}

#[tokio::test]
async fn the_text_tools_plugin_types_todays_date() {
    let h = Harness::start(&[]).await;
    h.install_builtin("text-tools");
    assert!(
        h.save(&[("rules.d/text.lua", "om.use('text-tools').setup({})")])
            .await
            .ok
    );
    assert!(h.trigger("hotkey:SUPER+ALT+D").await.ok);
    h.settle().await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let entries: Vec<String> = h
        .fakes
        .journal
        .entries()
        .into_iter()
        .filter(|e| e.starts_with("type "))
        .collect();
    assert_eq!(entries.len(), 1);
    // "type \"2026-09-30\"": a date, whatever today is.
    assert!(entries[0].starts_with("type \"2"), "{entries:?}");
    assert_eq!(entries[0].len(), "type \"2026-09-30\"".len(), "{entries:?}");
}
