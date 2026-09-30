//! Loading, reloading and `om eval`.

use super::*;

#[tokio::test]
async fn eval_returns_what_the_chunk_returns() {
    let h = Harness::start(&[]).await;
    assert_eq!(h.eval("return 1+1").await.unwrap(), ["2"]);
    assert_eq!(h.eval("1+1").await.unwrap(), ["2"]);
    assert_eq!(
        h.eval("return 1, 'two', {3}").await.unwrap(),
        ["1", "two", "{3}"]
    );
    assert_eq!(h.eval("x = 5").await.unwrap(), Vec::<String>::new());
    assert_eq!(h.eval("return x").await.unwrap(), ["5"]);
}

#[tokio::test]
async fn eval_errors_come_back_to_the_caller_not_the_desktop() {
    let h = Harness::start(&[]).await;
    assert_eq!(h.eval("error('nope')").await.unwrap_err(), "eval:1: nope");
    assert!(h.eval("return +").await.unwrap_err().starts_with("eval:1:"));
    assert!(h.fakes.notifier.sent().is_empty());
}

#[tokio::test]
async fn files_load_in_order_into_one_state() {
    let h = Harness::start(&[
        ("init.lua", "order = {'init'}"),
        ("rules.d/10-a.lua", "order[#order + 1] = 'a'"),
        ("rules.d/20-b.lua", "order[#order + 1] = 'b'"),
    ])
    .await;
    assert_eq!(
        h.eval("return table.concat(order, ',')").await.unwrap(),
        ["init,a,b"]
    );
    let status = h.status().await;
    assert_eq!(
        status.files,
        ["init.lua", "rules.d/10-a.lua", "rules.d/20-b.lua"]
    );
    assert_eq!(status.load_error, None);
}

#[tokio::test]
async fn load_error_is_notified_with_file_and_line() {
    let h = Harness::start(&[
        ("init.lua", "loaded = true"),
        ("rules.d/bad.lua", "local x = 1\nerror('boom')"),
    ])
    .await;
    assert_eq!(h.errors(), ["rules.d/bad.lua:2: boom (no rules loaded)"]);
    let status = h.status().await;
    assert_eq!(status.files, Vec::<String>::new());
    assert_eq!(
        status.load_error.as_deref(),
        Some("rules.d/bad.lua:2: boom (no rules loaded)")
    );
    // The half-loaded state was thrown away.
    assert_eq!(h.eval("return loaded").await.unwrap(), ["nil"]);
}

#[tokio::test]
async fn syntax_errors_name_the_file_and_line() {
    let h = Harness::start(&[("rules.d/typo.lua", "local ok = true\nlocal = 3")]).await;
    let errors = h.errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("rules.d/typo.lua:2: "), "{errors:?}");
}

#[tokio::test]
async fn errors_without_a_position_still_name_the_file() {
    let h = Harness::start(&[("rules.d/odd.lua", "\nerror('bare', 0)")]).await;
    assert_eq!(h.errors(), ["rules.d/odd.lua:2: bare (no rules loaded)"]);
}

#[tokio::test]
async fn failed_reload_keeps_the_previous_rules() {
    let good = (
        "init.lua",
        "om.trigger('hi', function() om.notify('hi') end)",
    );
    let h = Harness::start(&[good]).await;

    h.write(&[("init.lua", "om.trigger('other', function() end)\nnot lua")]);
    let response = h.ask(Request::Reload).await;
    assert!(!response.ok);
    let error = response.error.unwrap();
    assert!(error.starts_with("init.lua:2: "), "{error}");
    assert!(
        error.ends_with("(reload failed, previous rules kept)"),
        "{error}"
    );
    assert_eq!(h.errors(), std::slice::from_ref(&error));
    assert_eq!(h.status().await.load_error, Some(error));

    // The old trigger still fires; the one from the broken file does not exist.
    assert!(h.trigger("hi").await.ok);
    assert!(!h.trigger("other").await.ok);
    h.settle().await;
    assert!(h.titles().contains(&"hi".to_string()));

    h.write(&[("init.lua", "om.trigger('other', function() end)")]);
    assert_eq!(
        h.ask(Request::Reload).await,
        Response::ok("reloaded 1 file(s)")
    );
    assert!(h.trigger("other").await.ok);
    assert!(!h.trigger("hi").await.ok);
    assert_eq!(h.status().await.load_error, None);
}

#[tokio::test]
async fn files_changed_event_reloads_into_a_fresh_state() {
    let h = Harness::start(&[("init.lua", "generation = 1 leftover = true")]).await;
    h.write(&[("init.lua", "generation = 2")]);
    assert!(h.events.send(Event::FilesChanged).await.is_ok());
    assert_eq!(
        h.eval("return generation, leftover").await.unwrap(),
        ["2", "nil"]
    );
}

#[tokio::test(start_paused = true)]
async fn load_that_never_finishes_is_abandoned() {
    // Top-level code that parks forever: the load times out instead of
    // hanging the daemon.
    let h = Harness::start_with(&[("init.lua", "om.notify('stuck')")], |fakes| {
        fakes.notifier.hold_next(1);
    })
    .await;
    assert_eq!(
        h.errors(),
        ["the rule files took longer than 30s to load (no rules loaded)"]
    );
    assert_eq!(h.eval("return 1").await.unwrap(), ["1"]);
}
