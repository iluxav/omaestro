//! `om.mode`: leader chords and the submaps behind them.

use super::*;

const MODE: (&str, &str) = (
    "init.lua",
    "handle = om.mode('SUPER + ALT + W', {\n\
       h = function() om.notify('mode', 'left') end,\n\
       ['SHIFT + h'] = function() om.notify('mode', 'far left') end,\n\
     }, { hint = 'Window mode: h, H, Esc', exit = {'q'} })",
);

#[tokio::test]
async fn a_mode_is_an_entry_bind_plus_a_submap_of_binds() {
    let h = Harness::start(&[MODE]).await;
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let mut binds: Vec<String> = h
        .fakes
        .journal
        .entries()
        .into_iter()
        .filter(|e| e.starts_with("bind ") || e.starts_with("bind@"))
        .collect();
    binds.sort();
    assert_eq!(
        binds,
        [
            "bind SUPER + ALT + W -> submap om-super+alt+w [omaestro: init.lua:1]",
            "bind@om-super+alt+w Escape -> submap reset [omaestro: init.lua:1]",
            "bind@om-super+alt+w H -> om trigger 'mode:SUPER+ALT+W/H' [omaestro: init.lua:1]",
            "bind@om-super+alt+w Q -> submap reset [omaestro: init.lua:1]",
            "bind@om-super+alt+w SHIFT + H -> om trigger 'mode:SUPER+ALT+W/SHIFT+H' [omaestro: init.lua:1]",
        ]
    );

    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    let kinds: Vec<String> = rows
        .iter()
        .map(|r| format!("{} {}", r.kind, r.id))
        .collect();
    assert_eq!(
        kinds,
        [
            "mode mode:SUPER+ALT+W",
            "mode_key mode:SUPER+ALT+W/H",
            "mode_key mode:SUPER+ALT+W/SHIFT+H",
            "mode_exit mode:SUPER+ALT+W/exit:Escape",
            "mode_exit mode:SUPER+ALT+W/exit:Q",
        ]
    );

    // A key press in the mode is `om trigger` with the key's id.
    assert!(h.trigger("mode:SUPER+ALT+W/SHIFT+H").await.ok);
    h.settle().await;
    assert_eq!(
        h.fakes.notifier.sent(),
        [("mode".to_string(), "far left".to_string())]
    );
}

#[tokio::test]
async fn entering_the_mode_shows_the_hint_and_the_handle_removes_everything() {
    let h = Harness::start(&[MODE]).await;
    h.hyprland(HyprEvent::Submap("om-super+alt+w".into())).await;
    h.hyprland(HyprEvent::Submap(String::new())).await;
    h.hyprland(HyprEvent::Submap("somebody-elses".into())).await;
    h.settle().await;
    assert_eq!(
        h.fakes.notifier.sent(),
        [("Window mode: h, H, Esc".to_string(), String::new())]
    );

    assert_eq!(h.eval("return handle:remove()").await.unwrap(), ["true"]);
    h.until("the submap binds are gone", |h| {
        h.fakes.hypr.chords().is_empty()
    })
    .await;
    assert_eq!(h.status().await.triggers, 0);
}

#[tokio::test]
async fn once_leaves_the_mode_after_a_key() {
    let h = Harness::start(&[(
        "init.lua",
        "om.mode('SUPER + ALT + O', { j = function() om.notify('mode', 'j') end }, { once = true })",
    )])
    .await;
    h.fakes.journal.clear();
    assert!(h.trigger("mode:SUPER+ALT+O/J").await.ok);
    h.settle().await;
    assert_eq!(
        h.fakes.journal.entries(),
        ["dispatch hl.dsp.submap(\"reset\")"]
    );
    assert_eq!(
        h.fakes.notifier.sent(),
        [("mode".to_string(), "j".to_string())]
    );
}

#[tokio::test]
async fn a_mode_cannot_take_a_hotkey_chord_and_bad_keys_fail_at_the_line() {
    let h = Harness::start(&[(
        "init.lua",
        "om.hotkey('SUPER + ALT + W', function() end)\nom.mode('super+alt+w', { h = function() end })",
    )])
    .await;
    assert_eq!(
        h.errors(),
        [
            "init.lua:2: om.mode: SUPER + ALT + W is already registered at init.lua:1 (no rules loaded)"
        ]
    );
    let h = Harness::start(&[(
        "init.lua",
        "om.mode('SUPER + ALT + W', { ['HYPER + h'] = function() end })",
    )])
    .await;
    assert_eq!(
        h.errors(),
        ["init.lua:1: om.mode: key unknown modifier 'HYPER' in 'HYPER + h' (no rules loaded)"]
    );
    let h = Harness::start(&[("init.lua", "om.mode('SUPER + ALT + W', {})")]).await;
    assert_eq!(
        h.errors(),
        ["init.lua:1: om.mode: no keys (no rules loaded)"]
    );
}

#[tokio::test]
async fn a_key_bound_by_someone_else_in_the_submap_is_refused_but_the_rest_works() {
    let h = Harness::start_with(&[MODE], |fakes| {
        fakes.hypr.add_in("om-super+alt+w", "h", "theirs")
    })
    .await;
    let errors = h.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].starts_with(
            "init.lua:1: H in mode om-super+alt+w is already bound in Hyprland (theirs)"
        ),
        "{errors:?}"
    );
    // The refused key stays registered, listed with the reason; the rest of
    // the mode is bound.
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    let h_key = rows.iter().find(|r| r.id == "mode:SUPER+ALT+W/H").unwrap();
    assert!(
        h_key
            .problem
            .as_deref()
            .is_some_and(|p| p.starts_with("H in mode om-super+alt+w is already bound")),
        "{h_key:?}"
    );
    assert!(
        rows.iter()
            .filter(|r| r.id != "mode:SUPER+ALT+W/H")
            .all(|r| r.problem.is_none())
    );
    assert!(h.trigger("mode:SUPER+ALT+W/SHIFT+H").await.ok);
}
