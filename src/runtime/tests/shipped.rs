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

#[test]
fn every_plugin_json_loads_and_describes_options_its_setup_reads() {
    for name in shipped_plugins() {
        let dir = shipped_plugins_dir().join(&name);
        let Some(schema) = crate::plugins::schema::load(&dir).unwrap() else {
            continue;
        };
        let init = std::fs::read_to_string(dir.join("init.lua")).unwrap();
        for opt in &schema.options {
            assert!(
                init.contains(&format!("opts.{}", opt.key)),
                "{name}: plugin.json has {} but init.lua never reads opts.{}",
                opt.key,
                opt.key
            );
        }
    }
}

#[tokio::test]
async fn every_option_of_every_plugin_can_be_set_and_still_loads() {
    // Each plugin with its options changed through plugin.json (the first
    // enum choice, the other boolean, `none` where allowed, a fresh chord),
    // written as om plugin configure writes it, then loaded.
    use crate::plugins::schema::Kind;
    let h = Harness::start(&[]).await;
    let mut files = Vec::new();
    let mut next_key = b'A';
    for name in shipped_plugins() {
        let dir = shipped_plugins_dir().join(&name);
        let Some(schema) = crate::plugins::schema::load(&dir).unwrap() else {
            continue;
        };
        h.install_builtin(&name);
        let mut values = Vec::new();
        for opt in &schema.options {
            let answer = match opt.kind {
                Kind::Chord if opt.optional => "none".to_string(),
                Kind::Chord => {
                    next_key += 1;
                    format!("SUPER + CTRL + SHIFT + {}", next_key as char)
                }
                Kind::Modifiers => "SUPER + CTRL + SHIFT".to_string(),
                Kind::Bool => match opt.default_value() {
                    serde_json::Value::Bool(true) => "no".to_string(),
                    _ => "yes".to_string(),
                },
                Kind::Number => "3".to_string(),
                Kind::Interval => "2h".to_string(),
                Kind::Time => "08:15".to_string(),
                Kind::Enum => opt.options[0].clone(),
                Kind::Path => "~/omaestro-test-path".to_string(),
                Kind::String => opt
                    .default
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .unwrap_or("x")
                    .to_string(),
            };
            let value = opt.parse(&answer).unwrap();
            if !value.is_null() {
                values.push((opt.key.clone(), value));
            }
        }
        let rule = crate::plugins::rule_text_for_tests(&name, "test", &values);
        files.push((format!("rules.d/{name}.lua"), rule));
    }
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(n, c)| (n.as_str(), c.as_str()))
        .collect();
    let saved = h.save(&refs).await;
    assert!(saved.ok, "{saved:?} {:?}", h.errors());
    assert_eq!(h.errors(), Vec::<String>::new());
}
