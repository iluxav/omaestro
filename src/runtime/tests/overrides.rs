//! `om override`: whether a rule may take a chord Hyprland already has.

use super::*;

const J: (&str, &str) = (
    "init.lua",
    "om.hotkey('SUPER + J', function() om.notify('pressed J') end)",
);

const REFUSAL: &str = "SUPER + J is already bound in Hyprland (Toggle window split); \
                       remove that bind, pick another chord, or `om override on`";

async fn rows(h: &Harness) -> Vec<TriggerRow> {
    serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap()
}

/// How many times our bind on SUPER + J was made.
fn binds_made(h: &Harness) -> usize {
    h.fakes
        .journal
        .entries()
        .iter()
        .filter(|e| e.starts_with("bind SUPER + J -> "))
        .count()
}

fn told(h: &Harness) -> Vec<String> {
    h.fakes
        .notifier
        .sent()
        .into_iter()
        .filter(|(title, _)| title == "omaestro override")
        .map(|(_, body)| body)
        .collect()
}

#[tokio::test]
async fn by_default_hyprlands_bind_wins_and_the_rule_says_why() {
    let h = Harness::start_with(&[J], |f| f.hypr.add("SUPER + J", "Toggle window split")).await;
    assert_eq!(h.errors(), [format!("init.lua:1: {REFUSAL}")]);
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);
    assert_eq!(binds_made(&h), 0, "theirs is left alone");

    let rows = rows(&h).await;
    assert_eq!(rows[0].problem.as_deref(), Some(REFUSAL));
    assert_eq!(rows[0].overrides, None);
    assert!(!h.status().await.override_binds);

    // A rules reload does not nag about the same chord again.
    assert!(h.save(&[J]).await.ok);
    assert_eq!(h.errors().len(), 1);
}

#[tokio::test]
async fn override_takes_the_chord_and_gives_it_back() {
    let h = Harness::start_with(&[J], |f| f.hypr.add("SUPER + J", "Toggle window split")).await;

    assert_eq!(
        h.ask(Request::Override { on: true }).await,
        Response::ok("override on; 1 chord(s) taken from other binds")
    );
    assert!(h.status().await.override_binds);
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);
    assert_eq!(binds_made(&h), 1, "ours is the one bind on the chord now");
    assert_eq!(
        told(&h),
        ["SUPER + J now runs init.lua:1 instead of Toggle window split"]
    );
    let listed = rows(&h).await;
    assert_eq!(listed[0].overrides.as_deref(), Some("Toggle window split"));
    assert_eq!(listed[0].problem, None);
    assert!(h.trigger("hotkey:SUPER+J").await.ok);
    h.settle().await;
    assert!(h.titles().contains(&"pressed J".to_string()));

    // Hyprland reloads its config: theirs is back, ours is gone, and the
    // chord is taken again without a second notice.
    h.fakes.hypr.reload_config();
    h.hyprland(HyprEvent::ConfigReloaded).await;
    h.until("the chord is taken again", |h| binds_made(h) == 2)
        .await;
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);
    assert_eq!(told(&h).len(), 1);

    // Off: Hyprland's config is reloaded to bring the bind back, and the
    // sync after that refuses the rule again.
    assert_eq!(
        h.ask(Request::Override { on: false }).await,
        Response::ok("override off; rules give way to Hyprland's own binds")
    );
    assert!(h.fakes.journal.entries().contains(&"reload".to_string()));
    h.hyprland(HyprEvent::ConfigReloaded).await;
    h.until("the rule is refused again", |h| h.errors().len() == 2)
        .await;
    assert_eq!(h.fakes.hypr.chords(), ["SUPER + J"]);
    assert_eq!(binds_made(&h), 2, "theirs, untouched");
    assert_eq!(rows(&h).await[0].problem.as_deref(), Some(REFUSAL));
}

#[tokio::test]
async fn override_with_nothing_to_take_reloads_nothing() {
    let h = Harness::start(&[J]).await;
    assert_eq!(
        h.ask(Request::Override { on: true }).await,
        Response::ok("override on; 0 chord(s) taken from other binds")
    );
    assert!(h.ask(Request::Override { on: false }).await.ok);
    assert!(!h.fakes.journal.entries().contains(&"reload".to_string()));
    assert_eq!(binds_made(&h), 1);
}

#[tokio::test]
async fn the_setting_outlives_a_restart_and_exit_gives_the_bind_back() {
    let h = Harness::start_with(&[J], |f| f.hypr.add("SUPER + J", "Toggle window split")).await;
    assert!(h.ask(Request::Override { on: true }).await.ok);

    let h = h
        .restart_with(|f| f.hypr.add("SUPER + J", "Toggle window split"))
        .await;
    assert!(h.status().await.override_binds);
    assert_eq!(binds_made(&h), 1);
    assert_eq!(
        rows(&h).await[0].overrides.as_deref(),
        Some("Toggle window split")
    );

    // A clean exit removes ours and reloads Hyprland's config for theirs.
    let (fakes, exit) = h.stop(Event::Shutdown).await;
    assert_eq!(exit, Exit::Shutdown);
    let entries = fakes.journal.entries();
    let tail: Vec<&str> = entries.iter().rev().take(2).map(String::as_str).collect();
    assert_eq!(tail, ["reload", "unbind SUPER + J"]);
}
