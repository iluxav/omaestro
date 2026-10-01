//! The bind policy against a fake Hyprland.

use super::*;
use crate::backend::fake::FakeHypr;

fn wanted(chord: &str, origin: &str) -> Wanted {
    let chord = Chord::parse(chord).unwrap();
    Wanted {
        id: format!("hotkey:{}", chord.id()),
        chord,
        origin: origin.to_string(),
        submap: String::new(),
        action: Action::Trigger,
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
             remove that bind, pick another chord, or `om override on`"
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
async fn a_refused_chord_stays_known_and_binds_once_it_is_free() {
    let hypr = FakeHypr::default();
    hypr.add("SUPER + J", "Toggle window split");
    let mut hotkeys = hotkeys();
    let j = wanted("SUPER + J", "init.lua:1");

    assert_eq!(
        hotkeys
            .sync(&hypr, std::slice::from_ref(&j))
            .await
            .rejected
            .len(),
        1
    );
    assert_eq!(
        hotkeys.refused().get("hotkey:SUPER+J").map(String::as_str),
        Some(
            "SUPER + J is already bound in Hyprland (Toggle window split); \
             remove that bind, pick another chord, or `om override on`"
        )
    );
    // The same refusal again is not reported twice.
    assert_eq!(
        hotkeys.sync(&hypr, std::slice::from_ref(&j)).await,
        Report::default()
    );

    // The user drops their bind: the next look binds ours.
    hypr.unbind(&j.chord, "").await.unwrap();
    assert_eq!(
        hotkeys.sync(&hypr, std::slice::from_ref(&j)).await,
        Report::default()
    );
    assert!(hotkeys.refused().is_empty());
    assert_eq!(hypr.chords(), ["SUPER + J"]);
}

#[tokio::test]
async fn with_override_a_foreign_chord_is_taken_and_given_back() {
    let hypr = FakeHypr::default();
    hypr.add("SUPER + J", "Toggle window split");
    let mut hotkeys = hotkeys();
    hotkeys.set_override(true);
    let j = wanted("SUPER + J", "rules.d/a.lua:3");

    let report = hotkeys.sync(&hypr, std::slice::from_ref(&j)).await;
    assert_eq!(
        report.taken,
        [(
            "hotkey:SUPER+J".to_string(),
            "SUPER + J now runs rules.d/a.lua:3 instead of Toggle window split".to_string()
        )]
    );
    assert!(report.rejected.is_empty() && !report.restore);
    assert_eq!(
        hypr.calls(),
        [
            "binds",
            "unbind SUPER + J",
            "bind SUPER + J -> '/usr/bin/om' trigger 'hotkey:SUPER+J' [omaestro: rules.d/a.lua:3]",
        ]
    );
    assert_eq!(
        hotkeys
            .displaced()
            .get("hotkey:SUPER+J")
            .map(String::as_str),
        Some("Toggle window split")
    );

    // After a Hyprland reload the chord is taken again, without a second notice.
    hypr.reload_config();
    assert_eq!(hypr.chords(), ["SUPER + J"], "theirs is back");
    hypr.forget_calls();
    assert_eq!(
        hotkeys.sync(&hypr, std::slice::from_ref(&j)).await,
        Report::default()
    );
    assert_eq!(hypr.calls().len(), 3, "{:?}", hypr.calls());
    assert_eq!(hypr.chords(), ["SUPER + J"], "ours again");

    // The rule goes away: ours is unbound and the displaced one needs a
    // config reload to come back.
    hypr.forget_calls();
    let report = hotkeys.sync(&hypr, &[]).await;
    assert!(report.restore);
    assert_eq!(hypr.calls(), ["binds", "unbind SUPER + J"]);
    assert!(hotkeys.displaced().is_empty());
}

#[tokio::test]
async fn turning_override_off_asks_for_a_reload_only_when_something_was_displaced() {
    let hypr = FakeHypr::default();
    let mut hotkeys = hotkeys();
    hotkeys.set_override(true);
    hotkeys
        .sync(&hypr, &[wanted("SUPER + J", "init.lua:1")])
        .await;
    assert!(!hotkeys.set_override(false), "nothing was displaced");

    hypr.add("SUPER + K", "Theirs");
    hotkeys.set_override(true);
    hotkeys
        .sync(
            &hypr,
            &[
                wanted("SUPER + J", "init.lua:1"),
                wanted("SUPER + K", "init.lua:2"),
            ],
        )
        .await;
    assert_eq!(hotkeys.displaced().len(), 1);
    assert!(hotkeys.set_override(false));
    assert!(hotkeys.displaced().is_empty());

    // Shutdown with a displaced bind asks for the reload too.
    hotkeys.set_override(true);
    hypr.add("SUPER + K", "Theirs");
    hotkeys
        .sync(&hypr, &[wanted("SUPER + K", "init.lua:2")])
        .await;
    assert!(hotkeys.clear(&hypr).await);
    assert!(hypr.chords().is_empty());
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
