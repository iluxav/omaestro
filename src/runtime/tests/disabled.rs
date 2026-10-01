//! `om disable` and `om enable`: switching rules off without editing them.

use super::*;
use crate::backend::hypr::events::WinRef;

const RULES: (&str, &str) = (
    "init.lua",
    "om.hotkey('SUPER + J', function() om.notify('pressed J') end)\n\
     om.every('1s', function() om.notify('tick') end)\n\
     om.on_focus({class = 'firefox'}, function() om.notify('focus') end)",
);

fn disabled_ids(rows: &[TriggerRow]) -> Vec<&str> {
    rows.iter()
        .filter(|r| !r.enabled)
        .map(|r| r.id.as_str())
        .collect()
}

async fn list(h: &Harness) -> Vec<TriggerRow> {
    serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap()
}

#[tokio::test]
async fn a_disabled_hotkey_is_unbound_and_cannot_be_fired() {
    let h = Harness::start(&[RULES]).await;
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);

    let id = "hotkey:SUPER+J".to_string();
    assert_eq!(
        h.ask(Request::Disable { id: id.clone() }).await,
        Response::ok("disabled hotkey:SUPER+J")
    );
    assert!(h.fakes.hypr.chords().is_empty(), "the bind goes away");
    assert_eq!(
        h.trigger(&id).await,
        Response::err(
            "'hotkey:SUPER+J' is switched off; `om enable 'hotkey:SUPER+J'` turns it back on"
        )
    );
    let status = h.status().await;
    assert_eq!((status.triggers, status.disabled), (3, 1));
    assert_eq!(disabled_ids(&list(&h).await), ["hotkey:SUPER+J"]);

    assert_eq!(
        h.ask(Request::Enable { id: id.clone() }).await,
        Response::ok("enabled hotkey:SUPER+J")
    );
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"], "and comes back");
    assert!(h.trigger(&id).await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["pressed J"]);
    assert_eq!(h.status().await.disabled, 0);
}

#[tokio::test]
async fn unknown_ids_are_refused() {
    let h = Harness::start(&[RULES]).await;
    for request in [
        Request::Disable { id: "nope".into() },
        Request::Enable { id: "nope".into() },
    ] {
        assert_eq!(
            h.ask(request).await,
            Response::err("no trigger named 'nope'")
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_disabled_timer_stops_and_a_disabled_focus_rule_is_skipped() {
    let h = Harness::start(&[RULES]).await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["tick"]);

    h.ask(Request::Disable {
        id: "every:1s".into(),
    })
    .await;
    h.ask(Request::Disable {
        id: "on_focus:class=firefox".into(),
    })
    .await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    h.hyprland(HyprEvent::Focus(Some(WinRef {
        class: "firefox".into(),
        title: "Inbox".into(),
        address: "0x1".into(),
        workspace: String::new(),
    })))
    .await;
    h.settle().await;
    assert_eq!(h.titles(), ["tick"], "nothing fires while off");

    h.ask(Request::Enable {
        id: "every:1s".into(),
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["tick", "tick"], "the timer runs again");
}

#[tokio::test]
async fn the_choice_outlives_edits_reloads_and_restarts() {
    let h = Harness::start(&[RULES]).await;
    assert!(
        h.ask(Request::Disable {
            id: "every:1s".into()
        })
        .await
        .ok
    );

    // A rule added above the others does not shift their ids.
    let edited = format!("om.trigger('first', function() end)\n{}", RULES.1);
    assert!(h.save(&[("init.lua", &edited)]).await.ok);
    assert_eq!(disabled_ids(&list(&h).await), ["every:1s"]);

    let h = h.restart().await;
    assert_eq!(disabled_ids(&list(&h).await), ["every:1s"]);
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);
    assert_eq!(saved(&h)["disabled"], serde_json::json!(["every:1s"]));

    // An id that is only remembered (its rule is gone for now) can still be
    // switched back on, which forgets it.
    assert!(h.save(&[("init.lua", "")]).await.ok);
    assert_eq!(
        h.ask(Request::Enable {
            id: "every:1s".into()
        })
        .await,
        Response::ok("enabled every:1s")
    );
    assert_eq!(saved(&h)["disabled"], serde_json::json!([]));
}

/// What the settings file holds.
fn saved(h: &Harness) -> serde_json::Value {
    let text = std::fs::read_to_string(h.state_dir().join("settings.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn disabling_a_mode_takes_its_keys_with_it() {
    let h = Harness::start(&[(
        "init.lua",
        "om.mode('SUPER + ALT + W', { h = function() om.notify('left') end })",
    )])
    .await;
    assert_eq!(h.fakes.hypr.chords().len(), 3, "entry, h, Escape");

    h.ask(Request::Disable {
        id: "mode:SUPER+ALT+W".into(),
    })
    .await;
    assert!(h.fakes.hypr.chords().is_empty());
    assert!(!h.trigger("mode:SUPER+ALT+W/H").await.ok);
    assert_eq!(
        disabled_ids(&list(&h).await),
        [
            "mode:SUPER+ALT+W",
            "mode:SUPER+ALT+W/H",
            "mode:SUPER+ALT+W/exit:Escape"
        ]
    );

    h.ask(Request::Enable {
        id: "mode:SUPER+ALT+W".into(),
    })
    .await;
    assert_eq!(h.fakes.hypr.chords().len(), 3);
    assert!(h.trigger("mode:SUPER+ALT+W/H").await.ok);
}

#[tokio::test]
async fn ids_come_from_the_rule_not_its_position() {
    let h = Harness::start(&[(
        "init.lua",
        "om.every('5m', function() end)\n\
         om.every('5m', function() end)\n\
         om.on_focus({class = 'a', title = 'b'}, function() end)\n\
         om.on_open({}, function() end)\n\
         om.at('07:30', function() end)\n\
         om.on_clipboard(function() end)\n\
         om.on_clipboard(function() end)\n\
         om.after('90m', function() end)",
    )])
    .await;
    let ids: Vec<String> = list(&h)
        .await
        .iter()
        .map(|r| format!("{} [{}]", r.id, r.detail))
        .collect();
    assert_eq!(
        ids,
        [
            "after:1h30m [1h30m]",
            "at:07:30 [07:30]",
            "every:5m [5m]",
            "every:5m#2 [5m]",
            "on_clipboard []",
            "on_clipboard#2 []",
            "on_focus:class=a,title=b [class=a,title=b]",
            "on_open:* [*]",
        ]
    );
}
