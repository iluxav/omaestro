//! `om.tap`: a modifier let go on its own, quickly. Hyprland runs the press
//! and the release binds; here they are fired as Hyprland would.

use super::*;

const TAP: (&str, &str) = (
    "init.lua",
    "om.tap('SUPER', function() om.notify('tapped') end, {label = 'Clock'})",
);
const ID: &str = "tap:Super_L";

/// The key held for `held`, as Hyprland's two binds report it.
async fn tap(h: &Harness, id: &str, held: Duration) -> Response {
    assert!(h.trigger(&format!("{id}/down")).await.ok);
    tokio::time::sleep(held).await;
    let answer = h.trigger(id).await;
    h.settle().await;
    answer
}

#[tokio::test(start_paused = true)]
async fn a_quick_tap_fires_and_its_key_is_bound_twice() {
    let h = Harness::start(&[TAP]).await;
    let binds: Vec<String> = h
        .fakes
        .journal
        .entries()
        .into_iter()
        .filter(|e| e.starts_with("bind "))
        .collect();
    assert_eq!(
        binds,
        [
            "bind Super_L -> om trigger 'tap:Super_L/down' [omaestro: init.lua:1] (non_consuming, ignore_mods)",
            "bind Super_L -> om trigger 'tap:Super_L' [omaestro: init.lua:1] (release, non_consuming, ignore_mods)",
        ]
    );
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(
        (
            rows[0].id.as_str(),
            rows[0].kind.as_str(),
            rows[0].detail.as_str()
        ),
        (ID, "tap", "Super_L")
    );
    assert_eq!(rows[0].bound, Some(true));

    assert_eq!(
        tap(&h, ID, Duration::from_millis(120)).await,
        Response::ok("fired tap:Super_L")
    );
    assert_eq!(h.titles(), ["tapped"]);
}

#[tokio::test(start_paused = true)]
async fn a_long_press_or_a_release_alone_does_not_fire() {
    let h = Harness::start(&[TAP]).await;
    assert_eq!(
        tap(&h, ID, Duration::from_millis(450)).await,
        Response::ok("tap:Super_L: held 450 ms, not a tap")
    );
    // The press was used up: a second release has nothing to time.
    assert_eq!(
        h.trigger(ID).await,
        Response::ok("tap:Super_L: no press seen, not a tap")
    );
    h.settle().await;
    assert!(h.titles().is_empty(), "{:?}", h.titles());
}

#[tokio::test(start_paused = true)]
async fn within_sets_how_long_the_key_may_be_down() {
    let h = Harness::start(&[(
        "init.lua",
        "om.tap('ctrl_r', function() om.notify('slow tap') end, {within = 1})",
    )])
    .await;
    assert!(
        tap(&h, "tap:Control_R", Duration::from_millis(800))
            .await
            .ok
    );
    assert_eq!(h.titles(), ["slow tap"]);
}

#[tokio::test(start_paused = true)]
async fn a_double_tap_fires_once_and_a_single_tap_does_not() {
    let h = Harness::start(&[(
        "init.lua",
        "om.tap('SUPER', function() om.notify('double') end, {count = 2})",
    )])
    .await;
    let ms = Duration::from_millis;
    assert_eq!(
        tap(&h, ID, ms(90)).await,
        Response::ok("tap:Super_L: tap 1 of 2")
    );
    // Too long a pause: the next tap starts over.
    tokio::time::sleep(ms(600)).await;
    assert_eq!(
        tap(&h, ID, ms(90)).await,
        Response::ok("tap:Super_L: tap 1 of 2")
    );
    tokio::time::sleep(ms(150)).await;
    assert_eq!(tap(&h, ID, ms(90)).await, Response::ok("fired tap:Super_L"));
    assert_eq!(h.titles(), ["double"]);
}

#[tokio::test]
async fn a_tap_shares_its_key_with_nothing() {
    let h = Harness::start(&[]).await;
    for (code, message) in [
        (
            "om.tap('J', function() end)",
            "om.tap: 'J' is not a key to tap",
        ),
        (
            "om.tap('SUPER', function() end, {within = 0})",
            "om.tap: within is a number of seconds above 0, not 0",
        ),
        (
            "om.tap('SUPER', function() end, {count = 0})",
            "om.tap: count is how many taps in a row, 1 to 5, not 0",
        ),
        (
            "om.hotkey('Super_L', function() end)\nom.tap('SUPER', function() end)",
            "om.tap: Super_L is already bound at eval:1",
        ),
        (
            "om.tap('SUPER', function() end)\nom.hotkey('Super_L', function() end)",
            "om.hotkey: Super_L is tapped at eval:1",
        ),
        (
            "om.tap('SUPER', function() end)\nom.tap('super_l', function() end)",
            "tap 'tap:Super_L' is already registered at eval:1",
        ),
    ] {
        let err = h.eval(code).await.unwrap_err();
        assert!(err.contains(message), "{code}: {err}");
        h.save(&[]).await;
    }
}
