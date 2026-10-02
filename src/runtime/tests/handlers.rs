//! Dispatching triggers to handlers.

use super::*;

#[tokio::test]
async fn named_trigger_runs_its_handler() {
    let h = Harness::start(&[(
        "init.lua",
        "om.trigger('greet', function() om.notify('hello', 'from a rule') end)",
    )])
    .await;
    assert_eq!(h.trigger("greet").await, Response::ok("fired greet"));
    h.settle().await;
    assert_eq!(
        h.fakes.notifier.sent(),
        [("hello".to_string(), "from a rule".to_string())]
    );

    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(
        rows,
        [TriggerRow {
            id: "greet".into(),
            kind: "trigger".into(),
            detail: String::new(),
            label: None,
            origin: "init.lua:1".into(),
            enabled: true,
            problem: None,
            overrides: None,
            bound: None,
        }]
    );
    assert_eq!(h.status().await.triggers, 1);
}

#[tokio::test]
async fn unknown_trigger_is_an_error() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.trigger("nope").await,
        Response::err("no trigger named 'nope'")
    );
}

#[tokio::test]
async fn handler_error_is_notified_with_file_and_line() {
    let h = Harness::start(&[(
        "rules.d/foo.lua",
        "om.trigger('boom', function()\n  local t = nil\n  return t.field\nend)",
    )])
    .await;
    assert!(h.trigger("boom").await.ok);
    h.settle().await;
    let errors = h.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].starts_with("rules.d/foo.lua:3: attempt to index a nil value"),
        "{errors:?}"
    );
    // The daemon is still there and the trigger still registered.
    assert_eq!(h.status().await.triggers, 1);
}

#[tokio::test]
async fn backend_failure_points_at_the_calling_line() {
    let h = Harness::start(&[(
        "rules.d/foo.lua",
        "om.trigger('say', function()\n  local x = 1\n  om.notify('hi')\nend)",
    )])
    .await;
    h.fakes.notifier.fail_next(1);
    assert!(h.trigger("say").await.ok);
    h.settle().await;
    assert_eq!(
        h.errors(),
        ["rules.d/foo.lua:3: notify-send is not installed or not on PATH"]
    );
}

#[tokio::test]
async fn tail_call_failure_points_at_the_handler_registration() {
    // `return om.notify(...)` is a tail call: Lua keeps no frame for the
    // line, so the best position left is where the trigger was registered.
    let h = Harness::start(&[(
        "rules.d/foo.lua",
        "\nom.trigger('say', function()\n  return om.notify('hi')\nend)",
    )])
    .await;
    h.fakes.notifier.fail_next(1);
    assert!(h.trigger("say").await.ok);
    h.settle().await;
    assert_eq!(
        h.errors(),
        ["rules.d/foo.lua:2: notify-send is not installed or not on PATH"]
    );
}

#[tokio::test]
async fn bad_arguments_point_at_the_calling_line() {
    let h = Harness::start(&[(
        "init.lua",
        "om.trigger('say', function()\n  om.notify({})\nend)",
    )])
    .await;
    assert!(h.trigger("say").await.ok);
    h.settle().await;
    let errors = h.errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("init.lua:2: "), "{errors:?}");
}

#[tokio::test]
async fn reload_waits_for_running_handlers() {
    let h = Harness::start(&[(
        "init.lua",
        "generation = 1\nom.trigger('slow', function() om.notify('started') end)",
    )])
    .await;
    h.fakes.notifier.hold_next(1);
    assert!(h.trigger("slow").await.ok);
    h.until("the handler is mid-flight", |h| h.titles() == ["started"])
        .await;

    h.write(&[("init.lua", "generation = 2")]);
    let reload = h.send(Request::Reload).await;
    let status = h.status().await;
    assert!(status.reload_pending);
    assert_eq!(status.running, 1);
    // Nothing new starts on a state that is about to be replaced.
    assert!(!h.trigger("slow").await.ok);
    assert!(h.eval("return generation").await.is_err());

    h.fakes.notifier.release(1);
    assert_eq!(reload.await.unwrap(), Response::ok("reloaded 1 file(s)"));
    assert_eq!(h.eval("return generation").await.unwrap(), ["2"]);
    assert!(!h.status().await.reload_pending);
}

#[tokio::test]
async fn one_run_at_a_time_per_trigger() {
    let h = Harness::start(&[(
        "init.lua",
        "om.trigger('job', function() om.notify('start') om.notify('end') end)\n\
         om.trigger('other', function() om.notify('other') end)",
    )])
    .await;
    h.fakes.notifier.hold_next(2);
    assert!(h.trigger("job").await.ok);
    assert!(h.trigger("job").await.ok);
    h.until("the first run parks", |h| h.titles() == ["start"])
        .await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        h.titles(),
        ["start"],
        "the second run must wait for the first"
    );

    // A different trigger is not held up by it.
    h.fakes.notifier.release(1);
    h.until("the first run reaches its end", |h| {
        h.titles() == ["start", "end"]
    })
    .await;
    assert!(h.trigger("other").await.ok);
    h.until("the other trigger ran", |h| {
        h.titles() == ["start", "end", "other"]
    })
    .await;

    h.fakes.notifier.release(1);
    h.settle().await;
    assert_eq!(h.titles(), ["start", "end", "other", "start", "end"]);
}

#[tokio::test(start_paused = true)]
async fn slow_handler_is_not_killed() {
    let h = Harness::start(&[(
        "init.lua",
        "om.trigger('slow', function() om.notify('started') om.notify('finished') end)",
    )])
    .await;
    h.fakes.notifier.hold_next(1);
    assert!(h.trigger("slow").await.ok);
    // Well past the slow-handler mark.
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert_eq!(h.titles(), ["started"]);
    h.fakes.notifier.release(1);
    h.settle().await;
    assert_eq!(h.titles(), ["started", "finished"]);
    assert!(h.errors().is_empty());
}

#[tokio::test]
async fn a_rule_names_its_trigger_with_a_label() {
    let rules = "om.hotkey('SUPER + ALT + J', function() end, { label = 'Rewrite the selection' })\n\
                 om.hotkey('SUPER + ALT + O', om.panel)\n\
                 om.every('5m', function() end, { label = ' Stretch ' })\n\
                 om.on_usb(function() end, { label = 'USB plugged in' })\n\
                 om.menu('SUPER + ALT + P', { { 'a', function() end } }, { title = 'Projects' })\n\
                 om.mode('SUPER + ALT + W', { h = function() end }, { label = 'Window mode' })\n\
                 om.trigger('plain', function() end)\n";
    let h = Harness::start(&[("init.lua", rules)]).await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    let label = |id: &str| {
        rows.iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("no {id} in {rows:?}"))
            .label
            .clone()
    };
    assert_eq!(
        label("hotkey:SUPER+ALT+J").as_deref(),
        Some("Rewrite the selection")
    );
    assert_eq!(
        label("hotkey:SUPER+ALT+O").as_deref(),
        Some("omaestro menu")
    );
    assert_eq!(label("every:5m").as_deref(), Some("Stretch"));
    assert_eq!(label("on_usb").as_deref(), Some("USB plugged in"));
    assert_eq!(label("hotkey:SUPER+ALT+P").as_deref(), Some("Projects"));
    assert_eq!(label("mode:SUPER+ALT+W").as_deref(), Some("Window mode"));
    assert_eq!(label("plain"), None);

    let h = Harness::start(&[(
        "init.lua",
        "om.hotkey('SUPER + ALT + J', function() end, { label = true })",
    )])
    .await;
    assert_eq!(
        h.errors(),
        ["init.lua:1: om.hotkey: label is text, not boolean (no rules loaded)"]
    );
}
