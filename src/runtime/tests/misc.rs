//! `om.http`, notifications with actions, `om.spawn`.

use super::*;
use crate::backend::HttpResponse;

#[tokio::test]
async fn http_get_and_json_post() {
    let h = Harness::start(&[]).await;
    h.fakes.http.answer(HttpResponse {
        status: 200,
        headers: vec![("content-type".into(), "text/plain".into())],
        body: "hello".into(),
    });
    assert_eq!(
        h.eval("local r = om.http('http://x/hi') return r.status, r.ok, r.body, r.headers['content-type']")
            .await
            .unwrap(),
        ["200", "true", "hello", "text/plain"]
    );

    h.fakes.http.answer(HttpResponse {
        status: 201,
        headers: vec![("content-type".into(), "application/json".into())],
        body: r#"{"id": 7, "tags": ["a"]}"#.into(),
    });
    assert_eq!(
        h.eval("local r = om.http('http://x/items', {json = {name = 'n'}, headers = {authorization = 'Bearer t'}}) return r.json.id, r.json.tags[1]")
            .await
            .unwrap(),
        ["7", "a"]
    );
    let sent = h.fakes.http.requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, "GET");
    assert_eq!(sent[1].method, "POST");
    assert_eq!(sent[1].body.as_deref(), Some(r#"{"name":"n"}"#));
    assert!(
        sent[1]
            .headers
            .contains(&("content-type".to_string(), "application/json".to_string()))
    );
    assert!(
        sent[1]
            .headers
            .contains(&("authorization".to_string(), "Bearer t".to_string()))
    );

    // A JSON answer decodes even without asking; a non-JSON one with json=true is an error.
    h.fakes.http.answer(HttpResponse {
        status: 500,
        headers: vec![],
        body: "boom".into(),
    });
    let err = h
        .eval("local r = om.http('http://x/bad', {json = true}) return r")
        .await
        .unwrap_err();
    assert!(
        err.starts_with("eval:1: http: the answer is not JSON"),
        "{err}"
    );
    h.fakes
        .http
        .fail("http: could not connect to http://x/down");
    let err = h
        .eval("local r = om.http('http://x/down') return r")
        .await
        .unwrap_err();
    assert_eq!(err, "eval:1: http: could not connect to http://x/down");
}

#[tokio::test]
async fn notify_with_actions_returns_the_choice() {
    let h = Harness::start(&[]).await;
    h.fakes.notifier.choose("yes");
    assert_eq!(
        h.eval("return om.notify('Deploy?', 'to prod', {actions = {yes = 'Go', no = 'Wait'}, timeout = 30})")
            .await
            .unwrap(),
        ["yes"]
    );
    h.fakes.notifier.choose("");
    assert_eq!(
        h.eval("return om.notify('Deploy?', 'to prod', {actions = {yes = 'Go'}})")
            .await
            .unwrap(),
        ["nil"]
    );
    let asked = h.fakes.notifier.asked();
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[0].0, "Deploy?");
    assert_eq!(
        asked[0].2,
        [
            ("no".to_string(), "Wait".to_string()),
            ("yes".to_string(), "Go".to_string())
        ]
    );
    assert_eq!(asked[0].3, Some(Duration::from_secs(30)));
    // Plain notifications are unchanged.
    h.eval("om.notify('plain', 'one')").await.unwrap();
    assert_eq!(
        h.fakes.notifier.sent(),
        [("plain".to_string(), "one".to_string())]
    );
}

#[tokio::test]
async fn spawn_starts_a_command_and_does_not_wait() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("return om.spawn('sleep 30') > 0").await.unwrap(),
        ["true"]
    );
    assert_eq!(h.fakes.journal.entries(), ["spawn sleep 30"]);
    let err = h.eval("local p = om.spawn('') return p").await.unwrap_err();
    assert_eq!(err, "eval:1: spawn: the command is empty");
}
