//! `om.on_typed`: keys from the (fake) keyboard stream to a handler.

use super::*;
use crate::runtime::typed::Key;

const RULES: (&str, &str) = (
    "init.lua",
    "om.on_typed(':sig', function() om.type('Best, Ilya') end)\n\
     om.on_typed('::date', function() om.notify('typed', 'date') end)",
);

async fn type_keys(h: &Harness, text: &str) {
    for c in text.chars() {
        let key = match c {
            '\x08' => Key::Backspace,
            '\n' => Key::Reset,
            c => Key::Char(c),
        };
        assert!(h.events.send(Event::Typed(key)).await.is_ok());
    }
}

#[tokio::test]
async fn a_typed_text_is_erased_and_its_handler_runs() {
    let h = Harness::start(&[RULES]).await;
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(rows[0].kind, "on_typed");
    assert_eq!(rows[0].id, "on_typed:::date");

    type_keys(&h, "hello :sig").await;
    h.settle().await;
    assert_eq!(
        h.fakes.journal.entries(),
        ["watch keyboards", "erase 4", "type \"Best, Ilya\""]
    );

    // Backspace edits the buffer; Enter clears it.
    type_keys(&h, "::dat\x08te").await;
    h.settle().await;
    assert_eq!(h.titles(), ["typed"]);
    type_keys(&h, "::da\nte").await;
    h.settle().await;
    assert_eq!(h.titles(), ["typed"]);
}

#[tokio::test]
async fn typed_texts_are_checked_at_the_rule_line() {
    let h = Harness::start(&[("init.lua", "\nom.on_typed('', function() end)")]).await;
    assert_eq!(
        h.errors(),
        ["init.lua:2: om.on_typed: the text is empty (no rules loaded)"]
    );
    let h = Harness::start(&[("init.lua", "om.on_typed('ü', function() end)")]).await;
    assert!(h.errors()[0].contains("US layout"), "{:?}", h.errors());
    let h = Harness::start(&[(
        "init.lua",
        "om.on_typed(':a', function() end)\nom.on_typed(':a', function() end)",
    )])
    .await;
    assert!(
        h.errors()[0].contains("already registered"),
        "{:?}",
        h.errors()
    );
}

#[tokio::test]
async fn keys_go_nowhere_when_nothing_is_watched() {
    let h = Harness::start(&[]).await;
    type_keys(&h, ":sig").await;
    h.settle().await;
    assert!(h.fakes.journal.entries().is_empty());
}
