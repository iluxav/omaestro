//! Every built-in plugin loads with its defaults, all of them at once: no
//! errors, no two claiming the same chord, origins inside lib/, and the
//! README naming each one.

use super::*;
use crate::plugins::builtin;

#[tokio::test]
async fn all_builtin_plugins_load_together() {
    let h = Harness::start(&[]).await;
    assert!(
        builtin::ALL.len() >= 10,
        "an essential set was promised, found {}",
        builtin::ALL.len()
    );
    let mut files = Vec::new();
    for plugin in builtin::ALL {
        h.install_builtin(plugin.name);
        // downloads would watch the real ~/Downloads; give it a directory of its own.
        let options = if plugin.name == "downloads" {
            "{ dir = om.config_dir }"
        } else {
            "{}"
        };
        files.push((
            format!("rules.d/{}.lua", plugin.name),
            format!("om.use(\"{}\").setup({options})", plugin.name),
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
    for plugin in builtin::ALL {
        assert!(
            readme.contains(&format!("`{}`", plugin.name)),
            "README.md does not mention the {} plugin",
            plugin.name
        );
    }
}
