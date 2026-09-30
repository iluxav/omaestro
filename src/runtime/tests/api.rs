//! The `om` functions.

use super::*;

#[tokio::test]
async fn notify_body_is_optional() {
    let h = Harness::start(&[]).await;
    h.eval("om.notify('only a title')").await.unwrap();
    h.eval("om.notify('count', 3)").await.unwrap();
    assert_eq!(
        h.fakes.notifier.sent(),
        [
            ("only a title".to_string(), String::new()),
            ("count".to_string(), "3".to_string())
        ]
    );
}

#[tokio::test]
async fn log_takes_anything() {
    let h = Harness::start(&[]).await;
    h.eval("om.log('text', 1, 2.5, nil, true, {}, print)")
        .await
        .unwrap();
    h.eval("om.log()").await.unwrap();
}

#[tokio::test]
async fn handle_remove_unregisters_the_trigger() {
    let h = Harness::start(&[("init.lua", "handle = om.trigger('once', function() end)")]).await;
    assert!(h.trigger("once").await.ok);
    assert_eq!(h.eval("return handle:remove()").await.unwrap(), ["true"]);
    assert!(!h.trigger("once").await.ok);
    assert_eq!(h.eval("return handle:remove()").await.unwrap(), ["false"]);

    // A stale handle does not remove a newer registration of the same name.
    h.eval("om.trigger('once', function() end)").await.unwrap();
    assert_eq!(h.eval("return handle:remove()").await.unwrap(), ["false"]);
    assert!(h.trigger("once").await.ok);
}

#[tokio::test]
async fn duplicate_trigger_name_is_refused() {
    let h = Harness::start(&[
        ("init.lua", "om.trigger('same', function() end)"),
        ("rules.d/again.lua", "\nom.trigger('same', function() end)"),
    ])
    .await;
    assert_eq!(
        h.errors(),
        [
            "rules.d/again.lua:2: trigger 'same' is already registered at init.lua:1 (no rules loaded)"
        ]
    );
}

#[tokio::test]
async fn trigger_arguments_are_checked() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("om.trigger('', function() end)").await.unwrap_err(),
        "eval:1: om.trigger: the name is empty"
    );
    assert!(
        h.eval("om.trigger('x', 'not a function')")
            .await
            .unwrap_err()
            .starts_with("eval:1: ")
    );
}

#[tokio::test]
async fn on_typed_is_a_stub_in_v1() {
    let h = Harness::start(&[]).await;
    let error = h
        .eval("om.on_typed(':sig', function() end)")
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "eval:1: om.on_typed is not available in v1: typed triggers arrive in v2"
    );
}

#[tokio::test]
async fn prelude_string_helpers() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("return ('  a b \\n'):trim()").await.unwrap(),
        ["a b"]
    );
    assert_eq!(
        h.eval("return ('a,b,,c'):split(',')").await.unwrap(),
        [r#"{"a", "b", "", "c"}"#]
    );
    assert_eq!(
        h.eval("return (' one  two '):split()").await.unwrap(),
        [r#"{"one", "two"}"#]
    );
    assert_eq!(
        h.eval("return ('a.b'):split('.')").await.unwrap(),
        [r#"{"a", "b"}"#]
    );
    assert_eq!(
        h.eval("return ('hello'):starts_with('he'), ('hello'):starts_with('lo')")
            .await
            .unwrap(),
        ["true", "false"]
    );
    assert_eq!(
        h.eval(
            "return ('hello'):ends_with('lo'), ('hello'):ends_with(''), ('hello'):ends_with('he')"
        )
        .await
        .unwrap(),
        ["true", "true", "false"]
    );
}
