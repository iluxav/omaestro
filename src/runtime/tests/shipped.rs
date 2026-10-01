//! Every plugin in the repository's `plugins/` loads with its defaults, all
//! of them at once: no errors, no two claiming the same chord, origins
//! inside lib/, each with a README that says how to install it, and the
//! README of omaestro naming each one.

use super::*;

#[tokio::test]
async fn all_builtin_plugins_load_together() {
    let h = Harness::start(&[]).await;
    let plugins = shipped_plugins();
    assert!(
        plugins.len() >= 10,
        "an essential set was promised, found {}",
        plugins.len()
    );
    for starter in crate::plugins::STARTERS {
        assert!(
            plugins.iter().any(|p| p == starter),
            "starter {starter} is not in plugins/"
        );
    }
    let mut files = Vec::new();
    for name in &plugins {
        h.install_builtin(name);
        // downloads would watch the real ~/Downloads; give it a directory of its own.
        let options = if name == "downloads" {
            "{ dir = om.config_dir }"
        } else {
            "{}"
        };
        files.push((
            format!("rules.d/{}.lua", name),
            format!("om.use(\"{}\").setup({options})", name),
        ));
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(n, c)| (n.as_str(), c.as_str()))
        .collect();
    assert_eq!(
        h.save(&refs).await,
        Response::ok(format!("reloaded {} file(s)", refs.len()))
    );
    assert_eq!(h.errors(), Vec::<String>::new());
    let status = h.status().await;
    assert_eq!(status.files.len(), refs.len());
    assert_eq!(status.load_error, None);

    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    let mut kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(
        kinds,
        [
            "at",
            "every",
            "hotkey",
            "mode",
            "mode_exit",
            "mode_key",
            "on_battery",
            "on_clipboard",
            "on_file",
            "on_focus",
            "on_monitor",
            "on_network",
            "on_open",
            "on_sleep",
            "on_usb",
            "on_wake",
        ]
    );
    // Nothing was refused: every hotkey has its bind.
    assert!(
        rows.iter().all(|r| r.problem.is_none()),
        "{:?}",
        rows.iter()
            .filter_map(|r| r.problem.as_ref())
            .collect::<Vec<_>>()
    );
    // Origins point into lib/, relative to the config directory.
    let origins: Vec<&str> = rows.iter().map(|r| r.origin.as_str()).collect();
    assert!(
        origins
            .iter()
            .all(|o| o.starts_with("lib/") && o.contains("/init.lua:")),
        "{origins:?}"
    );
}

#[test]
fn the_readme_names_every_builtin_plugin() {
    let readme = include_str!("../../../README.md");
    for name in shipped_plugins() {
        assert!(
            readme.contains(&format!("`{name}`")),
            "README.md does not mention the {} plugin",
            name
        );
    }
}
