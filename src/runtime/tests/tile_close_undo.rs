//! The tile-close-undo plugin: SUPER+W hides the window and closes it a few
//! seconds later, SUPER+Z in between brings it back.

use super::*;

const RULE: &str = "om.use('tile-close-undo').setup({})";
const TRASH: &str = "special:om-tile-close-undo";
const HIDE: &str = "dispatch hl.dsp.window.move({ workspace = \"special:om-tile-close-undo\", follow = false, window = \"address:0x1\" })";
const CLOSE: &str = "dispatch hl.dsp.window.close({ window = \"address:0x1\" })";
const BACK: &str =
    "dispatch hl.dsp.window.move({ workspace = \"1\", follow = true, window = \"address:0x1\" })";
const FOCUS: &str = "dispatch hl.dsp.focus({ window = \"address:0x1\" })";

/// The plugin loaded, a focused Firefox, and SUPER+W pressed.
async fn closed_firefox() -> Harness {
    let h = Harness::start(&[]).await;
    h.install_builtin("tile-close-undo");
    assert!(h.save(&[("rules.d/tile-close-undo.lua", RULE)]).await.ok);
    h.fakes.hypr.set_window("firefox", "Docs");
    h.fakes.hypr.forget_calls();
    assert!(h.trigger("hotkey:SUPER+W").await.ok);
    h.settle().await;
    h
}

fn dispatches(h: &Harness) -> Vec<String> {
    h.fakes
        .journal
        .entries()
        .into_iter()
        .filter(|e| e.starts_with("dispatch "))
        .collect()
}

async fn wait(h: &Harness, millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
    h.settle().await;
}

#[tokio::test(start_paused = true)]
async fn the_window_waits_hidden_then_closes() {
    let h = closed_firefox().await;
    assert_eq!(dispatches(&h), [HIDE]);
    wait(&h, 2900).await;
    assert_eq!(dispatches(&h), [HIDE], "not before the delay");
    wait(&h, 200).await;
    assert_eq!(dispatches(&h), [HIDE, CLOSE]);

    // It closed: nothing comes back.
    h.fakes.hypr.remove_client("0x1");
    wait(&h, 6000).await;
    assert_eq!(dispatches(&h), [HIDE, CLOSE]);
    assert!(h.titles().is_empty(), "{:?}", h.titles());
}

#[tokio::test(start_paused = true)]
async fn undo_brings_the_window_back_and_it_never_closes() {
    let h = closed_firefox().await;
    wait(&h, 1000).await;
    assert!(h.trigger("hotkey:SUPER+Z").await.ok);
    h.settle().await;
    assert_eq!(dispatches(&h), [HIDE, BACK, FOCUS]);
    wait(&h, 10_000).await;
    assert_eq!(
        dispatches(&h),
        [HIDE, BACK, FOCUS],
        "the close was cancelled"
    );

    // Nothing left to bring back.
    assert!(h.trigger("hotkey:SUPER+Z").await.ok);
    h.settle().await;
    assert_eq!(h.titles(), ["Nothing to bring back"]);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}

#[tokio::test(start_paused = true)]
async fn a_window_that_does_not_close_comes_back() {
    // The app asked "Save changes?" and stayed open, on the hidden workspace.
    let h = closed_firefox().await;
    h.fakes.hypr.move_client("0x1", TRASH, -98);
    wait(&h, 3100).await;
    assert_eq!(dispatches(&h), [HIDE, CLOSE]);
    wait(&h, 5000).await;
    assert_eq!(dispatches(&h), [HIDE, CLOSE, BACK, FOCUS]);
    assert_eq!(h.titles(), ["firefox did not close"]);
}

#[tokio::test(start_paused = true)]
async fn a_reload_puts_a_waiting_window_back_instead_of_closing_it() {
    let h = closed_firefox().await;
    h.fakes.hypr.move_client("0x1", TRASH, -98);
    // The reload drops the timer; setup finds the window waiting.
    let saved = h
        .save(&[("rules.d/tile-close-undo.lua", &format!("{RULE}\n-- edited"))])
        .await;
    assert!(saved.ok, "{saved:?}");
    h.settle().await;
    assert_eq!(
        dispatches(&h),
        [
            HIDE,
            "dispatch hl.dsp.window.move({ workspace = \"1\", follow = false, window = \"address:0x1\" })"
        ]
    );
    wait(&h, 10_000).await;
    assert!(!dispatches(&h).iter().any(|d| d == CLOSE));
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}
