//! The clock plugin: the time, the date and the weather on a card, the
//! weather fetched in the background.

use super::*;
use crate::backend::{HttpResponse, Position};

const RULE: &str = "om.use('clock').setup({})";
const PRESS: &str = "hotkey:SUPER+ALT+A";

fn answer(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.into(),
        ..HttpResponse::default()
    }
}

async fn loaded(rule: &str, setup: impl FnOnce(&Fakes)) -> Harness {
    let h = Harness::start_with(&[], setup).await;
    h.install_builtin("clock");
    let saved = h.save(&[("rules.d/clock.lua", rule)]).await;
    assert!(saved.ok, "{saved:?}");
    h
}

async fn press(h: &Harness) {
    assert!(h.trigger(PRESS).await.ok);
    h.settle().await;
}

fn is_clock(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 5
        && bytes[2] == b':'
        && text.replace(':', "").chars().all(|c| c.is_ascii_digit())
}

fn urls(h: &Harness) -> Vec<String> {
    h.fakes.http.requests().into_iter().map(|r| r.url).collect()
}

#[tokio::test(start_paused = true)]
async fn the_first_press_fetches_the_weather_and_adds_it_while_the_card_is_up() {
    let h = loaded(RULE, |f| f.http.answer(answer(200, "Sunny, +22°C\n"))).await;
    press(&h).await;
    let shown = h.fakes.ui.shown();
    assert_eq!(shown.len(), 1);
    assert!(is_clock(&shown[0].big), "{}", shown[0].big);
    assert_eq!(shown[0].duration, Some(Duration::from_secs(1)));
    assert_eq!(shown[0].position, Position::Center);
    // The date alone: no weather yet. "%e" pads the day; the pad is gone.
    let date = shown[0].lines.clone();
    assert_eq!(date.len(), 1);
    assert!(
        !date[0].contains("  ") && date[0].contains(", "),
        "{date:?}"
    );
    assert_eq!(urls(&h), ["https://wttr.in/?format=%C,+%t"]);
    let updated = h.fakes.ui.updated();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, shown[0].id);
    assert_eq!(
        updated[0].lines,
        [date[0].clone(), "Sunny, +22°C".to_string()]
    );

    // The next press has it at once.
    press(&h).await;
    assert_eq!(h.fakes.ui.shown()[1].lines[1], "Sunny, +22°C");
    assert_eq!(urls(&h).len(), 1);
    assert_eq!(h.fakes.ui.updated().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn the_weather_is_kept_fresh_in_the_background() {
    let h = loaded(RULE, |f| {
        f.http.answer(answer(200, "Sunny, +22°C"));
        f.http.answer(answer(200, "Light rain, +14°C"));
    })
    .await;
    tokio::time::sleep(Duration::from_secs(15 * 60 + 1)).await;
    h.settle().await;
    assert_eq!(urls(&h).len(), 1);
    press(&h).await;
    assert_eq!(h.fakes.ui.shown()[0].lines[1], "Sunny, +22°C");
    tokio::time::sleep(Duration::from_secs(15 * 60)).await;
    h.settle().await;
    press(&h).await;
    assert_eq!(h.fakes.ui.shown()[1].lines[1], "Light rain, +14°C");
    assert_eq!(urls(&h).len(), 2);
}

#[tokio::test(start_paused = true)]
async fn the_place_and_the_units_go_to_wttr_in() {
    let h = loaded(
        "om.use('clock').setup({location = ' São Paulo ', units = 'imperial'})",
        |_| {},
    )
    .await;
    press(&h).await;
    assert_eq!(urls(&h), ["https://wttr.in/S%C3%A3o+Paulo?format=%C,+%t&u"]);
}

#[tokio::test(start_paused = true)]
async fn a_failed_fetch_leaves_the_time_and_the_date() {
    let h = loaded(RULE, |f| {
        f.http.fail("connection refused");
        f.http
            .answer(answer(404, "Unknown location; please try ~48.85,2.35"));
    })
    .await;
    press(&h).await;
    press(&h).await;
    let shown = h.fakes.ui.shown();
    assert!(shown.iter().all(|card| card.lines.len() == 1));
    assert!(h.fakes.ui.updated().is_empty());
    assert_eq!(h.errors(), Vec::<String>::new());
}

#[tokio::test(start_paused = true)]
async fn without_weather_nothing_is_fetched() {
    let h = loaded(
        "om.use('clock').setup({weather = false, date = false, time = '%H', seconds = 3, position = 'top'})",
        |_| {},
    )
    .await;
    assert_eq!(h.status().await.triggers, 2);
    press(&h).await;
    let shown = h.fakes.ui.shown();
    assert_eq!(shown[0].big.len(), 2);
    assert!(shown[0].lines.is_empty());
    assert_eq!(shown[0].duration, Some(Duration::from_secs(3)));
    assert_eq!(shown[0].position, Position::Top);
    assert!(urls(&h).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_bad_format_fails_on_load() {
    let h = Harness::start(&[]).await;
    h.install_builtin("clock");
    let saved = h
        .save(&[("rules.d/clock.lua", "om.use('clock').setup({time = '%-H'})")])
        .await;
    assert!(!saved.ok);
    let errors = h.errors();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("invalid conversion specifier")),
        "{errors:?}"
    );
}

#[tokio::test]
async fn an_om_without_cards_is_told_on_load() {
    let h = Harness::start(&[]).await;
    h.install_builtin("clock");
    let saved = h
        .save(&[(
            "rules.d/clock.lua",
            "om.show = nil\nom.use('clock').setup({})",
        )])
        .await;
    assert!(!saved.ok);
    assert_eq!(
        h.errors(),
        [
            "rules.d/clock.lua:2: clock needs a newer omaestro, one with om.show and om.tap: \
             update om (reload failed, previous rules kept)"
        ]
    );
}

/// One quick tap of the key, as Hyprland's two binds report it.
async fn tap(h: &Harness, key: &str) -> Response {
    assert!(h.trigger(&format!("tap:{key}/down")).await.ok);
    tokio::time::sleep(Duration::from_millis(90)).await;
    h.trigger(&format!("tap:{key}")).await
}

#[tokio::test(start_paused = true)]
async fn a_double_tap_of_super_shows_it_and_a_single_one_does_not() {
    let h = loaded(RULE, |_| {}).await;
    assert_eq!(
        tap(&h, "Super_L").await,
        Response::ok("tap:Super_L: tap 1 of 2")
    );
    tokio::time::sleep(Duration::from_millis(1000)).await;
    h.settle().await;
    assert!(h.fakes.ui.shown().is_empty());
    assert!(tap(&h, "Super_L").await.ok);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(tap(&h, "Super_L").await, Response::ok("fired tap:Super_L"));
    h.settle().await;
    assert_eq!(h.fakes.ui.shown().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn the_tap_and_the_chord_can_each_be_switched_off() {
    let h = loaded(
        "om.use('clock').setup({tap = 'off', chord = false, weather = false})",
        |_| {},
    )
    .await;
    assert_eq!(h.status().await.triggers, 0);
    let h = loaded(
        "om.use('clock').setup({tap = 'ALT', taps = 1, within = 0.5, chord = false, weather = false})",
        |_| {},
    )
    .await;
    assert!(h.trigger("tap:Alt_L/down").await.ok);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        h.trigger("tap:Alt_L").await,
        Response::ok("fired tap:Alt_L")
    );
}
