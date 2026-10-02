//! `om.menu`: a chord, a menu of named actions, the last one first.

use super::*;

/// The menu goes through `choose_command`, which the fake shell answers.
const CONFIG: (&str, &str) = (
    "omaestro.toml",
    "choose_command = \"pick {label} {options}\"\n",
);

const MENU: (&str, &str) = (
    "init.lua",
    "om.menu('SUPER + ALT + P', {\n\
       { 'Say hello', function(ctx) om.notify('hello', ctx.window and ctx.window.class or 'none') end },\n\
       { label = 'Say bye', fn = function() om.notify('bye') end },\n\
       { 'Once', function() om.notify('once') end, remember = false },\n\
     }, { title = 'Do' })",
);

fn picks(h: &Harness) -> Vec<String> {
    h.fakes
        .journal
        .entries()
        .into_iter()
        .filter(|e| e.starts_with("sh pick"))
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_pick_runs_its_item_and_comes_first_next_time() {
    let h = Harness::start(&[CONFIG, MENU]).await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    // A hotkey like any other, registered at the rule's line, not the prelude's.
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].id.as_str(), rows[0].origin.as_str()),
        ("hotkey:SUPER+ALT+P", "init.lua:1")
    );

    h.fakes.hypr.set_window("firefox", "Docs");
    h.fakes.shell.answer("Say bye\n");
    assert!(h.trigger("hotkey:SUPER+ALT+P").await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["bye"]);
    // The window from before the menu got the focus back first.
    let journal = h.fakes.journal.entries();
    assert!(
        journal.contains(&"dispatch hl.dsp.focus({ window = \"address:0x1\" })".to_string()),
        "{journal:?}"
    );

    h.fakes.shell.answer("Say hello\n");
    assert!(h.trigger("hotkey:SUPER+ALT+P").await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["bye", "hello"]);
    assert_eq!(
        h.fakes.notifier.sent()[1].1,
        "firefox",
        "ctx.window is the window"
    );

    // An item with remember = false runs but does not move up.
    h.fakes.shell.answer("Once\n");
    assert!(h.trigger("hotkey:SUPER+ALT+P").await.ok);
    h.settle().await;
    assert_eq!(
        picks(&h),
        [
            "sh pick 'Do' 'Say hello' 'Say bye' 'Once'",
            "sh pick 'Do' 'Say bye' 'Say hello' 'Once'",
            "sh pick 'Do' 'Say hello' 'Say bye' 'Once'",
        ]
    );
    assert_eq!(h.titles(), ["bye", "hello", "once"]);

    // Cancelling runs nothing.
    h.fakes.shell.fail(Some(1), "");
    assert!(h.trigger("hotkey:SUPER+ALT+P").await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["bye", "hello", "once"]);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}

#[tokio::test(start_paused = true)]
async fn items_can_be_made_on_each_press_from_the_context() {
    let h = Harness::start(&[
        CONFIG,
        (
            "init.lua",
            "om.menu('SUPER + ALT + P', function(ctx)\n\
               if ctx.selection == '' then return nil end\n\
               return { { 'Shout', function(c) om.notify(c.selection:upper()) end } }\n\
             end, { selection = true, refocus = false, remember = false })",
        ),
    ])
    .await;
    // Nothing selected: the item function says no, and no menu opens.
    assert!(h.trigger("hotkey:SUPER+ALT+P").await.ok);
    h.settle().await;
    assert!(picks(&h).is_empty());

    h.fakes.clipboard.select("quiet");
    h.fakes.shell.answer("Shout\n");
    assert!(h.trigger("hotkey:SUPER+ALT+P").await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["QUIET"]);
    assert!(
        !h.fakes
            .journal
            .entries()
            .iter()
            .any(|e| e.starts_with("dispatch hl.dsp.focus")),
        "refocus = false"
    );
    assert!(
        h.eval("return om.store.get('om.menu SUPER + ALT + P')")
            .await
            .unwrap()
            == ["nil"]
    );
}

#[tokio::test]
async fn bad_items_are_reported_at_the_rule() {
    let h = Harness::start(&[(
        "init.lua",
        "\nom.menu('SUPER + ALT + P', { { 'No function' } })",
    )])
    .await;
    assert_eq!(
        h.errors(),
        ["init.lua:2: om.menu: item 1 needs a label and a function (no rules loaded)"]
    );

    // Made on a press: reported then, at the rule that made the menu.
    let h = Harness::start(&[(
        "rules.d/m.lua",
        "om.menu('SUPER + ALT + P', function() return { 42 } end)",
    )])
    .await;
    assert!(h.trigger("hotkey:SUPER+ALT+P").await.ok);
    h.settle().await;
    assert_eq!(
        h.errors(),
        ["rules.d/m.lua:1: om.menu: item 1 needs a label and a function"]
    );
}
