//! `om.show`: a card on the screen, its handle, and the notification where
//! nothing can draw it.

use super::*;
use crate::backend::{Card, Position};

fn card(id: u64, big: &str, lines: &[&str], seconds: Option<u64>, position: Position) -> Card {
    Card {
        id,
        big: big.into(),
        lines: lines.iter().map(|line| line.to_string()).collect(),
        duration: seconds.map(Duration::from_secs),
        position,
    }
}

/// The ids are the daemon's, shared by every test in this process: compare
/// the rest.
fn without_ids(cards: Vec<Card>) -> Vec<Card> {
    cards
        .into_iter()
        .map(|card| Card { id: 0, ..card })
        .collect()
}

#[tokio::test]
async fn a_card_is_shown_with_its_options() {
    let h = Harness::start(&[]).await;
    h.eval(
        "om.show({big = '14:32', lines = {'Saturday, October 11', 18}}, {seconds = 1, position = 'top'})",
    )
    .await
    .unwrap();
    h.eval("om.show('Saved')").await.unwrap();
    h.eval("om.show({lines = 'one line'}, {seconds = 0})")
        .await
        .unwrap();
    assert_eq!(
        without_ids(h.fakes.ui.shown()),
        [
            card(
                0,
                "14:32",
                &["Saturday, October 11", "18"],
                Some(1),
                Position::Top
            ),
            card(0, "Saved", &[], Some(2), Position::Center),
            card(0, "", &["one line"], None, Position::Center),
        ]
    );
    let ids: Vec<u64> = h.fakes.ui.shown().iter().map(|c| c.id).collect();
    assert!(ids[0] < ids[1] && ids[1] < ids[2], "ids grow: {ids:?}");
    assert!(h.fakes.notifier.sent().is_empty());
}

#[tokio::test]
async fn the_handle_updates_and_closes_its_card() {
    let h = Harness::start(&[]).await;
    let answers = h
        .eval(
            "local c = om.show({big = '3'}, {seconds = 5, position = 'bottom'})\n\
             local up = c:update({big = '2', lines = {'left'}})\n\
             c:close()\n\
             return up, c:update('1')",
        )
        .await
        .unwrap();
    // Up when updated; closed, so gone for the second update.
    assert_eq!(answers, ["true", "false"]);
    let shown = h.fakes.ui.shown();
    assert_eq!(
        without_ids(shown.clone()),
        [card(0, "3", &[], Some(5), Position::Bottom)]
    );
    // The update keeps the card's time and place.
    assert_eq!(
        h.fakes.ui.updated(),
        [card(shown[0].id, "2", &["left"], Some(5), Position::Bottom)]
    );
    assert_eq!(h.fakes.ui.closed(), [shown[0].id]);
}

#[tokio::test]
async fn an_update_never_brings_back_a_card_that_went() {
    let h = Harness::start(&[]).await;
    h.eval("old = om.show('first')").await.unwrap();
    h.fakes.ui.expire();
    assert_eq!(
        h.eval("return old:update('late')").await.unwrap(),
        ["false"]
    );
    // Nor does it take the place of a newer card.
    h.eval("old = om.show('second') om.show('third')")
        .await
        .unwrap();
    assert_eq!(
        h.eval("return old:update('late')").await.unwrap(),
        ["false"]
    );
    assert!(h.fakes.ui.updated().is_empty());
    assert_eq!(h.fakes.ui.shown().len(), 3);
}

#[tokio::test]
async fn with_nothing_to_draw_it_is_a_notification() {
    let h = Harness::start_with(&[], |fakes| fakes.ui.remove()).await;
    h.eval(
        "local c = om.show({big = '14:32', lines = {'Saturday', 'sunny'}})\n\
         assert(c:update({big = '14:33'}) == false)\n\
         c:close()\n\
         om.show({lines = {'', 'only lines', 'here'}})",
    )
    .await
    .unwrap();
    // Once per card: an update has nothing to change.
    assert_eq!(
        h.fakes.notifier.sent(),
        [
            ("14:32".to_string(), "Saturday\nsunny".to_string()),
            ("only lines".to_string(), "here".to_string()),
        ]
    );
    assert!(h.fakes.ui.shown().is_empty());
}

#[tokio::test]
async fn a_bad_card_is_a_rule_error() {
    let h = Harness::start(&[]).await;
    for (code, message) in [
        ("om.show('')", "om.show: nothing to show"),
        ("om.show({lines = {}})", "om.show: nothing to show"),
        (
            "om.show({lines = {true}})",
            "om.show: a card holds text, not boolean",
        ),
        ("om.show(nil)", "om.show: a card holds text, not nil"),
        (
            "om.show('x', {position = 'left'})",
            "om.show: position is \"center\", \"top\" or \"bottom\", not \"left\"",
        ),
        (
            "om.show('x', {seconds = -1})",
            "om.show: seconds is 0 or more, not -1",
        ),
        ("om.show('x'):update({})", "card:update: nothing to show"),
    ] {
        let err = h.eval(code).await.unwrap_err();
        assert!(err.contains(message), "{code}: {err}");
    }
}
