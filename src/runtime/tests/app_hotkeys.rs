//! `om.app_hotkey`: a chord bound only while a matching window has focus,
//! so every other app keeps the chord for itself.

use super::*;
use crate::backend::hypr::events::WinRef;

fn window(class: &str, title: &str, address: &str) -> Option<WinRef> {
    Some(WinRef {
        class: class.into(),
        title: title.into(),
        address: address.into(),
        workspace: String::new(),
    })
}

const RULES: (&str, &str) = (
    "init.lua",
    "om.app_hotkey('^firefox$', 'CTRL + S', function() om.notify('firefox', 'saved') end)\n\
     om.app_hotkey({ class = '^code$', title = 'Untitled' }, 'CTRL + S', function() om.notify('code', 'saved') end)\n\
     om.hotkey('SUPER + K', function() end)",
);

const FIREFOX: &str = "app_hotkey:CTRL+S:class=^firefox$";

fn chords(h: &Harness) -> Vec<String> {
    h.fakes.hypr.chords()
}

#[tokio::test]
async fn bound_only_while_a_matching_window_has_focus() {
    let h = Harness::start(&[RULES]).await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    assert_eq!(chords(&h), ["SUPER + K"], "no app hotkey without a focus");

    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    let app: Vec<(&str, &str, &str)> = rows
        .iter()
        .filter(|r| r.kind == "app_hotkey")
        .map(|r| (r.id.as_str(), r.detail.as_str(), r.origin.as_str()))
        .collect();
    assert_eq!(
        app,
        [
            (
                "app_hotkey:CTRL+S:class=^code$,title=Untitled",
                "CTRL + S in class=^code$,title=Untitled",
                "init.lua:2"
            ),
            (FIREFOX, "CTRL + S in class=^firefox$", "init.lua:1"),
        ]
    );

    // Firefox gets focus: the chord is bound and its handler runs.
    h.hyprland(HyprEvent::Focus(window("firefox", "Docs", "0x1")))
        .await;
    h.until("the chord is bound", |h| {
        chords(h) == ["SUPER + K", "CTRL + S"]
    })
    .await;
    assert!(h.trigger(FIREFOX).await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["firefox"]);

    // Another app: the bind goes, the app gets its own CTRL+S.
    h.hyprland(HyprEvent::Focus(window("foot", "~", "0x2")))
        .await;
    h.until("the chord is gone", |h| chords(h) == ["SUPER + K"])
        .await;

    // Code with the right title: the other rule's bind. Another title: none.
    h.hyprland(HyprEvent::Focus(window("code", "Untitled - Code", "0x3")))
        .await;
    h.until("bound for code", |h| chords(h) == ["SUPER + K", "CTRL + S"])
        .await;
    assert!(
        h.fakes.journal.entries().last().unwrap().contains(
            "om trigger 'app_hotkey:CTRL+S:class=^code$,title=Untitled' [omaestro: init.lua:2]"
        ),
        "{:?}",
        h.fakes.journal.entries().last()
    );
    h.hyprland(HyprEvent::Focus(window("code", "main.rs - Code", "0x3")))
        .await;
    h.until("gone for another title", |h| chords(h) == ["SUPER + K"])
        .await;

    // The focused window closing drops it too.
    h.hyprland(HyprEvent::Focus(window("firefox", "Docs", "0x1")))
        .await;
    h.until("bound again", |h| chords(h).len() == 2).await;
    h.hyprland(HyprEvent::Closed {
        address: "0x1".into(),
    })
    .await;
    h.until("gone with the window", |h| chords(h) == ["SUPER + K"])
        .await;
}

#[tokio::test]
async fn follows_hyprland_reloads_and_the_off_switch() {
    let h = Harness::start(&[RULES]).await;
    h.hyprland(HyprEvent::Focus(window("firefox", "Docs", "0x1")))
        .await;
    h.until("bound", |h| chords(h).len() == 2).await;

    h.fakes.hypr.reload_config();
    h.hyprland(HyprEvent::ConfigReloaded).await;
    h.until("back after a Hyprland reload", |h| chords(h).len() == 2)
        .await;

    assert!(h.ask(Request::Disable { id: FIREFOX.into() }).await.ok);
    h.until("off: unbound though focused", |h| {
        chords(h) == ["SUPER + K"]
    })
    .await;
    assert!(h.ask(Request::Enable { id: FIREFOX.into() }).await.ok);
    h.until("on: bound again", |h| chords(h).len() == 2).await;
}

#[tokio::test]
async fn a_chord_is_global_or_per_app_not_both() {
    let h = Harness::start(&[(
        "init.lua",
        "om.hotkey('CTRL + S', function() end)\nom.app_hotkey('^firefox$', 'CTRL + S', function() end)",
    )])
    .await;
    assert_eq!(
        h.errors(),
        [
            "init.lua:2: om.app_hotkey: CTRL + S is a global hotkey at init.lua:1; \
             an app hotkey cannot share its chord (no rules loaded)"
        ]
    );
    let h = Harness::start(&[(
        "init.lua",
        "om.app_hotkey('^firefox$', 'CTRL + S', function() end)\nom.hotkey('CTRL + S', function() end)",
    )])
    .await;
    assert_eq!(
        h.errors(),
        [
            "init.lua:2: om.hotkey: CTRL + S is an app hotkey at init.lua:1; \
             a global hotkey cannot share its chord (no rules loaded)"
        ]
    );
    let h = Harness::start(&[(
        "init.lua",
        "om.app_hotkey('^firefox$', 'CTRL + S', function() end)\n\
         om.app_hotkey('^firefox$', 'CTRL + S', function() end)",
    )])
    .await;
    assert_eq!(
        h.errors(),
        [
            "init.lua:2: app hotkey CTRL + S for class=^firefox$ is already registered at init.lua:1 (no rules loaded)"
        ]
    );
    let h = Harness::start(&[("init.lua", "om.app_hotkey(42, 'CTRL + S', function() end)")]).await;
    assert_eq!(
        h.errors(),
        [
            "init.lua:1: om.app_hotkey: a class pattern or a table {class=, title=}, not integer (no rules loaded)"
        ]
    );
}

#[tokio::test]
async fn two_rules_matching_one_window_on_one_chord_bind_the_first() {
    let h = Harness::start(&[(
        "init.lua",
        "om.app_hotkey('^fire', 'CTRL + S', function() om.notify('first', '') end)\n\
         om.app_hotkey('fox$', 'CTRL + S', function() om.notify('second', '') end)",
    )])
    .await;
    h.hyprland(HyprEvent::Focus(window("firefox", "Docs", "0x1")))
        .await;
    h.until("one bind", |h| chords(h) == ["CTRL + S"]).await;
    assert!(
        h.fakes
            .journal
            .entries()
            .iter()
            .any(|e| e.contains("'app_hotkey:CTRL+S:class=^fire'")),
        "{:?}",
        h.fakes.journal.entries()
    );
    // Both stay registered; the second simply has no bind right now.
    assert_eq!(h.status().await.triggers, 2);
}

#[tokio::test]
async fn rows_say_whether_an_app_hotkey_is_bound_right_now() {
    let h = Harness::start(&[RULES]).await;
    async fn bound(h: &Harness, id: &str) -> Option<bool> {
        let rows: Vec<TriggerRow> =
            serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
        rows.iter().find(|r| r.id == id).unwrap().bound
    }
    assert_eq!(bound(&h, FIREFOX).await, Some(false));
    assert_eq!(bound(&h, "hotkey:SUPER+K").await, Some(true));

    h.hyprland(HyprEvent::Focus(window("firefox", "Docs", "0x1")))
        .await;
    h.until("bound", |h| chords(h).len() == 2).await;
    assert_eq!(bound(&h, FIREFOX).await, Some(true));
    assert_eq!(
        bound(&h, "app_hotkey:CTRL+S:class=^code$,title=Untitled").await,
        Some(false)
    );
}
