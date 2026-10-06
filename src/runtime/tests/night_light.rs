//! The night-light plugin: the screen warm at night and normal by day
//! through hyprsunset, a hotkey to switch now, true colors while an image
//! editor has focus.

use super::*;
use crate::backend::SystemEvent;
use crate::backend::hypr::events::WinRef;

fn window(class: &str, title: &str, address: &str) -> Option<WinRef> {
    Some(WinRef {
        class: class.into(),
        title: title.into(),
        address: address.into(),
        workspace: String::new(),
    })
}

/// Night for the next hour, by the wall clock the rule sees.
const RULE: &str = "local now = os.time()\n\
    om.use('night-light').setup({\n\
      warm_at = os.date('%H:%M', now - 3600),\n\
      normal_at = os.date('%H:%M', now + 3600),\n\
    })";

/// What hyprsunset was told, in order.
fn sets(h: &Harness) -> Vec<String> {
    h.fakes
        .journal
        .entries()
        .into_iter()
        .filter(|e| e.starts_with("sh "))
        .map(|e| e.rsplit("hyprctl hyprsunset ").next().unwrap().to_string())
        .collect()
}

async fn loaded() -> Harness {
    let h = Harness::start(&[]).await;
    h.install_builtin("night-light");
    assert!(h.save(&[("rules.d/night-light.lua", RULE)]).await.ok);
    h.settle().await;
    h
}

#[tokio::test(start_paused = true)]
async fn the_screen_is_warm_at_night_and_normal_in_the_morning() {
    let h = loaded().await;
    assert_eq!(sets(&h), ["temperature 4000"]);
    // The morning, an hour away: normal colors, no notification by default.
    tokio::time::sleep(Duration::from_secs(3601)).await;
    h.settle().await;
    assert_eq!(sets(&h), ["temperature 4000", "identity"]);
    assert!(h.titles().is_empty(), "{:?}", h.titles());
}

#[tokio::test(start_paused = true)]
async fn the_hotkey_switches_now_and_a_wake_goes_back_to_the_schedule() {
    let h = loaded().await;
    assert!(h.trigger("hotkey:SUPER+ALT+S").await.ok);
    h.settle().await;
    assert_eq!(sets(&h), ["temperature 4000", "identity"]);
    assert_eq!(h.titles(), ["Night light"]);
    assert!(
        h.events
            .send(Event::System(SystemEvent::Wake))
            .await
            .is_ok()
    );
    h.settle().await;
    assert_eq!(
        sets(&h),
        ["temperature 4000", "identity", "temperature 4000"]
    );
}

#[tokio::test(start_paused = true)]
async fn an_image_editor_gets_true_colors_while_it_has_focus() {
    let h = loaded().await;
    h.hyprland(HyprEvent::Focus(window("Gimp", "photo.png", "0x1")))
        .await;
    h.settle().await;
    assert_eq!(sets(&h), ["temperature 4000", "identity"]);
    // From one editor to another: nothing to change.
    h.hyprland(HyprEvent::Focus(window(
        "org.inkscape.Inkscape",
        "drawing.svg",
        "0x2",
    )))
    .await;
    h.settle().await;
    assert_eq!(sets(&h).len(), 2);
    h.hyprland(HyprEvent::Focus(window("firefox", "Docs", "0x3")))
        .await;
    h.settle().await;
    assert_eq!(
        sets(&h),
        ["temperature 4000", "identity", "temperature 4000"]
    );
}
