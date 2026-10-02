//! `om.selection()` ignores a selection the user is no longer looking at.

use super::*;
use crate::backend::hypr::events::WinRef;

fn focus(address: &str) -> HyprEvent {
    HyprEvent::Focus(Some(WinRef {
        class: "app".into(),
        title: "A window".into(),
        address: address.into(),
        workspace: String::new(),
    }))
}

async fn selected(h: &Harness, text: &str) {
    h.fakes.clipboard.select(text);
    assert!(h.events.send(Event::SelectionChanged).await.is_ok());
    h.settle().await;
}

#[tokio::test]
async fn a_selection_made_in_another_window_reads_as_none() {
    let h = Harness::start(&[]).await;
    h.hyprland(focus("0x1")).await;
    selected(&h, "from the editor").await;
    assert_eq!(
        h.eval("return om.selection()").await.unwrap(),
        ["from the editor"]
    );

    h.hyprland(focus("0x2")).await;
    h.settle().await;
    assert_eq!(h.eval("return om.selection()").await.unwrap(), [""]);

    // Back in the window it was made in, it counts again.
    h.hyprland(focus("0x1")).await;
    h.settle().await;
    assert_eq!(
        h.eval("return om.selection()").await.unwrap(),
        ["from the editor"]
    );
}

#[tokio::test]
async fn a_selection_a_rule_pasted_over_reads_as_none_until_the_next() {
    let h = Harness::start(&[]).await;
    h.hyprland(focus("0x1")).await;
    selected(&h, "me wants cofee").await;
    h.eval("om.paste(om.selection():upper())").await.unwrap();
    assert_eq!(h.eval("return om.selection()").await.unwrap(), [""]);

    selected(&h, "the next one").await;
    assert_eq!(
        h.eval("return om.selection()").await.unwrap(),
        ["the next one"]
    );
    h.eval("om.type('typed over it')").await.unwrap();
    assert_eq!(h.eval("return om.selection()").await.unwrap(), [""]);
}

#[tokio::test]
async fn a_selection_from_before_the_daemon_is_used() {
    // No change seen yet: nothing is known, the selection is returned as is.
    let h = Harness::start(&[]).await;
    h.fakes.clipboard.select("older than the daemon");
    h.hyprland(focus("0x2")).await;
    h.settle().await;
    assert_eq!(
        h.eval("return om.selection()").await.unwrap(),
        ["older than the daemon"]
    );
}
