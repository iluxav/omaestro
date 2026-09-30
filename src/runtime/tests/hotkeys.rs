//! `om.hotkey`: from the rule to a bind in (fake) Hyprland and back.

use super::*;

const J: (&str, &str) = (
    "init.lua",
    "om.hotkey('SUPER, J', function() om.notify('pressed J') end)",
);

#[tokio::test]
async fn hotkey_is_bound_in_hyprland_and_fires_its_handler() {
    let h = Harness::start(&[J]).await;
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);
    assert!(
        h.fakes.journal.entries().contains(
            &"bind SUPER + J -> om trigger 'hotkey:SUPER+J' [omaestro: init.lua:1]".to_string()
        ),
        "{:?}",
        h.fakes.journal.entries()
    );

    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(
        rows,
        [TriggerRow {
            id: "hotkey:SUPER+J".into(),
            kind: "hotkey".into(),
            origin: "init.lua:1".into()
        }]
    );

    // What Hyprland runs when the chord is pressed.
    assert!(h.trigger("hotkey:SUPER+J").await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["pressed J"]);
}

#[tokio::test]
async fn reload_replaces_the_binds() {
    let h = Harness::start(&[J]).await;
    let saved = h
        .save(&[(
            "init.lua",
            "om.hotkey('SUPER + K', function() end)\nom.hotkey('ctrl+alt+l', function() end)",
        )])
        .await;
    assert!(saved.ok);
    assert_eq!(h.fakes.hypr.chords(), ["CTRL + ALT + L", "SUPER + K"]);
    assert!(!h.trigger("hotkey:SUPER+J").await.ok);
}

#[tokio::test]
async fn failed_reload_keeps_the_binds() {
    let h = Harness::start(&[J]).await;
    assert!(
        !h.save(&[(
            "init.lua",
            "om.hotkey('SUPER + K', function() end)\nnot lua"
        )])
        .await
        .ok
    );
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);
    assert!(h.trigger("hotkey:SUPER+J").await.ok);
}

#[tokio::test]
async fn chord_the_user_already_bound_is_refused() {
    let files = [(
        "rules.d/keys.lua",
        "om.hotkey('SUPER + J', function() end)\nom.hotkey('SUPER + K', function() end)",
    )];
    let h = Harness::start_with(&files, |fakes| {
        fakes.hypr.add("SUPER + J", "Toggle window split")
    })
    .await;

    assert_eq!(
        h.errors(),
        [
            "rules.d/keys.lua:1: SUPER + J is already bound in Hyprland (Toggle window split); \
          remove that bind or pick another chord"
        ]
    );
    // The user's bind is untouched, the other hotkey works, and the refused
    // one is not listed as if it did.
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J", "SUPER + K"]);
    assert!(
        !h.fakes
            .journal
            .entries()
            .iter()
            .any(|e| e.starts_with("unbind"))
    );
    assert!(!h.trigger("hotkey:SUPER+J").await.ok);
    assert!(h.trigger("hotkey:SUPER+K").await.ok);
    assert_eq!(h.status().await.triggers, 1);
}

#[tokio::test]
async fn unreadable_chord_fails_the_load_at_its_line() {
    let h = Harness::start(&[("init.lua", "\nom.hotkey('HYPER + J', function() end)")]).await;
    assert_eq!(
        h.errors(),
        ["init.lua:2: om.hotkey: unknown modifier 'HYPER' in 'HYPER + J' (no rules loaded)"]
    );
    assert!(h.fakes.hypr.chords().is_empty());
}

#[tokio::test]
async fn one_handler_per_chord() {
    let h = Harness::start(&[
        J,
        ("rules.d/again.lua", "om.hotkey('super+j', function() end)"),
    ])
    .await;
    assert_eq!(
        h.errors(),
        [
            "rules.d/again.lua:1: hotkey SUPER + J is already registered at init.lua:1 (no rules loaded)"
        ]
    );
}

#[tokio::test]
async fn hotkeys_added_and_removed_while_running_follow_through() {
    let h = Harness::start(&[]).await;
    h.eval("handle = om.hotkey('SUPER + L', function() end)")
        .await
        .unwrap();
    h.until("the new hotkey is bound", |h| {
        h.fakes.hypr.chords() == ["SUPER + L"]
    })
    .await;

    assert_eq!(h.eval("return handle:remove()").await.unwrap(), ["true"]);
    h.until("the removed hotkey is unbound", |h| {
        h.fakes.hypr.chords().is_empty()
    })
    .await;
}

#[tokio::test]
async fn binds_come_back_after_hyprland_reloads_its_config() {
    let h = Harness::start(&[J]).await;
    h.fakes.hypr.reload_config();
    assert!(h.fakes.hypr.chords().is_empty());

    h.hyprland(HyprEvent::ConfigReloaded).await;
    h.until("the hotkey is bound again", |h| {
        h.fakes.hypr.chords() == ["SUPER + J"]
    })
    .await;
}

#[tokio::test]
async fn shutdown_removes_our_binds_and_only_ours() {
    let h = Harness::start_with(&[J], |fakes| fakes.hypr.add("SUPER + Q", "Close window")).await;
    let (fakes, exit) = h.stop(Event::Shutdown).await;
    assert_eq!(exit, Exit::Shutdown);
    assert_eq!(fakes.hypr.chords(), ["SUPER + Q"]);
}

#[tokio::test]
async fn hyprland_going_away_ends_the_daemon_without_touching_it() {
    let h = Harness::start(&[J]).await;
    h.fakes.journal.clear();
    let (fakes, exit) = h.stop(Event::HyprGone).await;
    assert_eq!(exit, Exit::HyprGone);
    assert!(
        fakes.journal.entries().is_empty(),
        "{:?}",
        fakes.journal.entries()
    );
}

#[tokio::test]
async fn rules_without_hotkeys_never_ask_hyprland_for_its_binds() {
    let h = Harness::start(&[("init.lua", "om.trigger('plain', function() end)")]).await;
    assert!(
        h.save(&[("init.lua", "om.trigger('other', function() end)")])
            .await
            .ok
    );
    let (fakes, _) = h.stop(Event::Shutdown).await;
    assert!(
        fakes.journal.entries().is_empty(),
        "{:?}",
        fakes.journal.entries()
    );
}
