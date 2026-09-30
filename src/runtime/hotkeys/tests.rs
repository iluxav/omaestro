//! The bind policy against a fake Hyprland.

use super::*;
use crate::backend::fake::FakeHypr;

fn wanted(chord: &str, origin: &str) -> Wanted {
    let chord = Chord::parse(chord).unwrap();
    Wanted {
        id: format!("hotkey:{}", chord.id()),
        chord,
        origin: origin.to_string(),
    }
}

fn hotkeys() -> Hotkeys {
    Hotkeys::new(trigger_command(Path::new("/usr/bin/om"), None))
}

#[test]
fn trigger_command_is_shell_safe() {
    assert_eq!(
        trigger_command(Path::new("/usr/bin/om"), None),
        "'/usr/bin/om' trigger"
    );
    assert_eq!(
        trigger_command(
            Path::new("/home/it's me/om"),
            Some(Path::new("/run/user/1000/x y.sock"))
        ),
        r"'/home/it'\''s me/om' --socket '/run/user/1000/x y.sock' trigger"
    );
}

#[tokio::test]
async fn nothing_wanted_nothing_asked() {
    let hypr = FakeHypr::default();
    assert_eq!(hotkeys().sync(&hypr, &[]).await, Report::default());
    assert!(hypr.calls().is_empty());
}

#[tokio::test]
async fn binds_what_is_wanted_and_unbinds_what_is_not() {
    let hypr = FakeHypr::default();
    let mut hotkeys = hotkeys();
    let (j, k) = (
        wanted("SUPER, J", "init.lua:1"),
        wanted("SUPER + K", "init.lua:2"),
    );

    assert_eq!(
        hotkeys.sync(&hypr, &[j.clone(), k.clone()]).await,
        Report::default()
    );
    assert_eq!(
        hypr.calls(),
        [
            "binds",
            "bind SUPER + J -> '/usr/bin/om' trigger 'hotkey:SUPER+J' [omaestro: init.lua:1]",
            "bind SUPER + K -> '/usr/bin/om' trigger 'hotkey:SUPER+K' [omaestro: init.lua:2]",
        ]
    );

    // Same set again: one look at the listing, no changes.
    hypr.forget_calls();
    assert_eq!(
        hotkeys.sync(&hypr, &[j.clone(), k]).await,
        Report::default()
    );
    assert_eq!(hypr.calls(), ["binds"]);

    // K is gone from the rules.
    hypr.forget_calls();
    assert_eq!(hotkeys.sync(&hypr, &[j]).await, Report::default());
    assert_eq!(hypr.calls(), ["binds", "unbind SUPER + K"]);
    assert_eq!(hypr.chords(), ["SUPER + J"]);
}

#[tokio::test]
async fn a_chord_somebody_else_bound_is_refused_and_left_alone() {
    let hypr = FakeHypr::default();
    hypr.add("SUPER + J", "Toggle window split");
    let mut hotkeys = hotkeys();

    let report = hotkeys
        .sync(&hypr, &[wanted("SUPER + J", "rules.d/a.lua:3")])
        .await;
    assert_eq!(
        report.rejected,
        [(
            "hotkey:SUPER+J".to_string(),
            "rules.d/a.lua:3: SUPER + J is already bound in Hyprland (Toggle window split); \
             remove that bind or pick another chord"
                .to_string()
        )]
    );
    assert_eq!(hypr.calls(), ["binds"]);

    // Nothing of ours was bound, so nothing is unbound at exit either.
    hotkeys.clear(&hypr).await;
    assert_eq!(hypr.calls(), ["binds"]);
    assert_eq!(hypr.chords(), ["SUPER + J"]);
}

#[tokio::test]
async fn our_bind_is_not_removed_once_somebody_else_shares_the_chord() {
    let hypr = FakeHypr::default();
    let mut hotkeys = hotkeys();
    hotkeys
        .sync(&hypr, &[wanted("SUPER + J", "init.lua:1")])
        .await;
    hypr.add("SUPER + J", "added by the user later");
    hypr.forget_calls();

    // Unbinding the chord would take the user's bind with it.
    hotkeys.sync(&hypr, &[]).await;
    assert_eq!(hypr.calls(), ["binds"]);
    assert_eq!(hypr.chords(), ["SUPER + J", "SUPER + J"]);
}

#[tokio::test]
async fn binds_lost_to_a_hyprland_reload_come_back() {
    let hypr = FakeHypr::default();
    let mut hotkeys = hotkeys();
    let j = wanted("SUPER + J", "init.lua:1");
    hotkeys.sync(&hypr, std::slice::from_ref(&j)).await;

    hypr.reload_config();
    assert!(hypr.chords().is_empty());
    hypr.forget_calls();
    assert_eq!(hotkeys.sync(&hypr, &[j]).await, Report::default());
    assert_eq!(hypr.chords(), ["SUPER + J"]);
    assert_eq!(
        hypr.calls().len(),
        2,
        "one listing, one bind: {:?}",
        hypr.calls()
    );
}

#[tokio::test]
async fn leftover_of_an_earlier_daemon_is_replaced() {
    let hypr = FakeHypr::default();
    hypr.add("SUPER + J", "omaestro: init.lua:9");
    let mut hotkeys = hotkeys();

    assert_eq!(
        hotkeys
            .sync(&hypr, &[wanted("SUPER + J", "init.lua:1")])
            .await,
        Report::default()
    );
    assert_eq!(
        hypr.calls(),
        [
            "binds",
            "unbind SUPER + J",
            "bind SUPER + J -> '/usr/bin/om' trigger 'hotkey:SUPER+J' [omaestro: init.lua:1]",
        ]
    );
}

#[tokio::test]
async fn a_chord_hyprland_refuses_is_reported_with_its_origin() {
    let hypr = FakeHypr::default();
    hypr.fail_next_bind("Unknown keysym: \"NOPE\"");
    let mut hotkeys = hotkeys();

    let report = hotkeys
        .sync(&hypr, &[wanted("SUPER + NOPE", "init.lua:4")])
        .await;
    assert_eq!(
        report.rejected,
        [(
            "hotkey:SUPER+NOPE".to_string(),
            "init.lua:4: could not bind SUPER + NOPE: hyprctl failed: Unknown keysym: \"NOPE\""
                .to_string()
        )]
    );
    assert!(hypr.chords().is_empty());
}

#[tokio::test]
async fn unreadable_bind_list_changes_nothing() {
    let hypr = FakeHypr::default();
    hypr.fail_next_listing();
    let report = hotkeys()
        .sync(&hypr, &[wanted("SUPER + J", "init.lua:1")])
        .await;
    assert!(report.rejected.is_empty());
    assert!(
        report
            .error
            .unwrap()
            .starts_with("could not read Hyprland's binds")
    );
    assert_eq!(hypr.calls(), ["binds"]);
}

#[tokio::test]
async fn clear_removes_only_our_binds() {
    let hypr = FakeHypr::default();
    hypr.add("SUPER + Q", "Close window");
    let mut hotkeys = hotkeys();
    hotkeys
        .sync(
            &hypr,
            &[
                wanted("SUPER + J", "init.lua:1"),
                wanted("SUPER + K", "init.lua:2"),
            ],
        )
        .await;
    hypr.forget_calls();

    hotkeys.clear(&hypr).await;
    assert_eq!(
        hypr.calls(),
        ["binds", "unbind SUPER + J", "unbind SUPER + K"]
    );
    assert_eq!(hypr.chords(), ["SUPER + Q"]);

    // A second clear has nothing left to do.
    hypr.forget_calls();
    hotkeys.clear(&hypr).await;
    assert!(hypr.calls().is_empty());
}
