//! `om.on_clipboard`, `om.on_file`, `om.layout`.

use super::*;
use crate::backend::{ClipContent, Window};

#[tokio::test]
async fn clipboard_changes_reach_the_handler_with_the_text() {
    let h = Harness::start(&[(
        "init.lua",
        "om.on_clipboard(function(text) om.notify('clip', text) end)",
    )])
    .await;
    assert_eq!(h.fakes.journal.entries(), ["watch clipboard"]);

    h.fakes.clipboard.copy(ClipContent::text("copied words"));
    assert!(h.events.send(Event::ClipboardChanged).await.is_ok());
    h.settle().await;
    h.fakes.clipboard.copy(ClipContent {
        mime: "image/png".into(),
        data: vec![1],
    });
    assert!(h.events.send(Event::ClipboardChanged).await.is_ok());
    h.settle().await;
    assert_eq!(
        h.fakes.notifier.sent(),
        [
            ("clip".to_string(), "copied words".to_string()),
            ("clip".to_string(), String::new())
        ]
    );

    // No rule watching: the watch is released and changes go nowhere.
    assert!(h.save(&[]).await.ok);
    assert!(h.events.send(Event::ClipboardChanged).await.is_ok());
    h.settle().await;
    assert_eq!(h.fakes.notifier.sent().len(), 2);
}

#[tokio::test]
async fn file_changes_reach_the_handler() {
    let dir = TempDir::new("on-file");
    let rule = format!(
        "om.on_file('{}', function(change) om.notify(change.kind, change.path) end)",
        dir.path().display()
    );
    let h = Harness::start(&[("init.lua", &rule)]).await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(rows[0].kind, "on_file");

    let file = dir.path().join("note.txt");
    std::fs::write(&file, "hello").unwrap();
    h.until("the write is noticed", |h| {
        h.fakes
            .notifier
            .sent()
            .iter()
            .any(|(_, body)| body == &file.display().to_string())
    })
    .await;
    // Editor droppings are ignored.
    std::fs::write(dir.path().join(".note.txt.swp"), "x").unwrap();
    std::fs::write(dir.path().join("note.txt~"), "x").unwrap();
    std::fs::remove_file(&file).unwrap();
    h.until("the removal is noticed", |h| {
        h.fakes
            .notifier
            .sent()
            .iter()
            .any(|(kind, _)| kind == "remove")
    })
    .await;
    assert!(
        !h.fakes
            .notifier
            .sent()
            .iter()
            .any(|(_, body)| body.contains(".swp") || body.ends_with('~')),
        "{:?}",
        h.fakes.notifier.sent()
    );
}

#[tokio::test]
async fn a_path_that_cannot_be_watched_is_reported() {
    let h = Harness::start(&[(
        "init.lua",
        "\nom.on_file('/nonexistent/nowhere', function() end)",
    )])
    .await;
    let errors = h.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].starts_with("init.lua:2: cannot watch /nonexistent/nowhere"),
        "{errors:?}"
    );
    assert_eq!(h.status().await.triggers, 0);
}

#[tokio::test(start_paused = true)]
async fn layout_places_matching_windows() {
    let h = Harness::start(&[]).await;
    let client = |address: &str, class: &str| Window {
        address: address.into(),
        class: class.into(),
        title: class.into(),
        initial_class: class.into(),
        workspace: "1".into(),
        workspace_id: 1,
        width: 800,
        height: 600,
        ..Window::default()
    };
    h.fakes.hypr.add_client(client("0xa", "firefox"));
    h.fakes.hypr.add_client(client("0xb", "code"));
    h.fakes.hypr.add_client(client("0xc", "firefox"));

    let placed = h
        .eval(
            "return om.layout({\n\
               {class = '^firefox$', place = 'left'},\n\
               {class = '^code$', workspace = 2, place = 'right', all = true},\n\
               {class = '^nothing$', place = 'max'},\n\
             })",
        )
        .await
        .unwrap();
    assert_eq!(placed, ["2"]);
    let entries = h.fakes.journal.entries();
    assert!(entries.contains(&"dispatch hl.dsp.window.move({ workspace = \"2\", follow = false, window = \"address:0xb\" })".to_string()), "{entries:?}");
    assert!(
        entries.contains(
            &"dispatch hl.dsp.window.move({ x = 0, y = 30, window = \"address:0xa\" })".to_string()
        ),
        "{entries:?}"
    );
    assert!(
        !entries.iter().any(|e| e.contains("0xc")),
        "only the first firefox: {entries:?}"
    );
}
