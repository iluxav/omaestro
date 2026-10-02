//! `om plugin` against local git repositories standing in for GitHub: the
//! same code path a URL takes, without the network.

use std::path::{Path, PathBuf};

use super::Setup;
use super::rule::{is_generated_rule, read_values, rule_text};
use super::scaffold::{init_template, readme_template};
use super::source::{Official, Source};
use super::*;
use crate::backend::run::find_on_path;
use crate::testutil::TempDir;

/// git with an identity, for the repositories the tests make.
async fn git_in(dir: &Path, args: &[&str]) -> String {
    let mut full = vec![
        "-c",
        "user.name=omaestro",
        "-c",
        "user.email=omaestro@test",
        "-c",
        "init.defaultBranch=main",
    ];
    full.extend(args);
    git(Some(dir), &full).await.unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn plugin_files(dir: &Path, name: &str, what: &str) {
    write(
        &dir.join("init.lua"),
        &format!("-- {name}\nreturn {{ setup = function() end }}\n"),
    );
    write(
        &dir.join("README.md"),
        &format!("# {name}\n\n{what} More detail here.\n"),
    );
}

/// A repository like omaestro's: plugins/alpha and plugins/beta.
async fn catalog(root: &Path) -> PathBuf {
    let repo = root.join("catalog");
    std::fs::create_dir_all(&repo).unwrap();
    git_in(&repo, &["init", "--quiet"]).await;
    plugin_files(&repo.join("plugins/alpha"), "alpha", "Does alpha things.");
    plugin_files(&repo.join("plugins/beta"), "beta", "Does beta things.");
    write(&repo.join("README.md"), "# catalog\n");
    git_in(&repo, &["add", "-A"]).await;
    git_in(&repo, &["commit", "--quiet", "-m", "first"]).await;
    repo
}

fn official(repo: &Path) -> Official {
    Official {
        repo: repo.to_string_lossy().to_string(),
        dir: "plugins".to_string(),
    }
}

fn names(specs: &[&str]) -> Vec<String> {
    specs.iter().map(|s| s.to_string()).collect()
}

fn no_leftovers(lib: &Path) {
    let hidden: Vec<String> = std::fs::read_dir(lib)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    assert!(hidden.is_empty(), "staging left behind: {hidden:?}");
}

fn has_git() -> bool {
    if find_on_path("git").is_none() {
        eprintln!("skipped: git is not installed");
        return false;
    }
    true
}

#[test]
fn rule_files_and_templates_name_the_plugin() {
    assert_eq!(variable("om-window-halves"), "window_halves");
    assert_eq!(variable("3d.thing"), "_3d_thing");
    let init = init_template("om-x");
    assert!(init.contains("local x = om.use(\"om-x\")"), "{init}");
    assert!(init.ends_with("return M\n"));
    assert!(readme_template("om-x").contains("om plugin add https://github.com/<you>/om-x"));

    let rule = rule_text("window-halves", "halves and thirds", &[]);
    assert_eq!(
        rule,
        "-- window-halves: halves and thirds\n\
         -- Options and what it does: ~/.config/omaestro/lib/window-halves/README.md\n\
         local window_halves = om.use(\"window-halves\")\n\
         window_halves.setup({})\n"
    );
    assert!(is_generated_rule("window-halves", &rule));
    assert!(!is_generated_rule(
        "window-halves",
        &rule.replace("({})", "({ chord = 'X' })")
    ));
}

#[tokio::test]
async fn plugins_by_name_come_from_a_directory_of_the_official_repository() {
    if !has_git() {
        return;
    }
    let tmp = TempDir::new("plugins-official");
    let repo = catalog(tmp.path()).await;
    let official = official(&repo);
    let config = tmp.path().join("config");
    let lib = config.join("lib");

    add(
        &config,
        &names(&["alpha", "beta"]),
        None,
        None,
        true,
        &official,
        &Setup::defaults(),
    )
    .await
    .unwrap();
    for name in ["alpha", "beta"] {
        assert!(lib.join(name).join("init.lua").is_file());
        assert!(!lib.join(name).join(".git").exists(), "a copy, not a clone");
    }
    // Only the plugin's directory, not the rest of the repository.
    assert!(!lib.join("alpha").join("plugins").exists());
    assert!(!lib.join("alpha").join("README.md").read_link().is_ok());
    let rule = std::fs::read_to_string(config.join("rules.d/alpha.lua")).unwrap();
    assert!(rule.starts_with("-- alpha: Does alpha things.\n"), "{rule}");
    no_leftovers(&lib);

    let listed = installed(&lib).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].name, "alpha");
    assert_eq!(
        listed[0].version.len(),
        7,
        "a short commit: {}",
        listed[0].version
    );
    assert!(
        listed[0]
            .source
            .as_deref()
            .unwrap()
            .ends_with("catalog plugins/alpha")
    );
    let record = Record::read(&lib.join("alpha")).unwrap();
    assert_eq!(record.source, official.plugin("alpha"));

    available(&config, &official).await.unwrap();
    no_leftovers(&lib);

    let err = add(
        &config,
        &names(&["alpha"]),
        None,
        None,
        true,
        &official,
        &Setup::defaults(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("not installed: alpha"), "{err}");

    let err = add(
        &config,
        &names(&["gamma"]),
        None,
        None,
        true,
        &official,
        &Setup::defaults(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("gamma"), "{err}");
    assert!(!lib.join("gamma").exists());
    assert!(!config.join("rules.d/gamma.lua").exists());
    no_leftovers(&lib);

    remove(&config, "alpha", false).await.unwrap();
    assert!(!lib.join("alpha").exists());
    assert!(
        !config.join("rules.d/alpha.lua").exists(),
        "the generated rule goes too"
    );
}

#[tokio::test]
async fn update_follows_upstream_and_never_loses_your_edits_quietly() {
    if !has_git() {
        return;
    }
    let tmp = TempDir::new("plugins-update");
    let repo = catalog(tmp.path()).await;
    let official = official(&repo);
    let config = tmp.path().join("config");
    let lib = config.join("lib");
    add(
        &config,
        &names(&["alpha"]),
        None,
        None,
        false,
        &official,
        &Setup::defaults(),
    )
    .await
    .unwrap();

    // Nothing new upstream.
    update(&lib, Some("alpha"), false, &official).await.unwrap();

    // Upstream changes.
    write(
        &repo.join("plugins/alpha/init.lua"),
        "-- alpha v2\nreturn {}\n",
    );
    write(&repo.join("plugins/alpha/data.json"), "{}");
    git_in(&repo, &["add", "-A"]).await;
    git_in(&repo, &["commit", "--quiet", "-m", "second"]).await;
    update(&lib, None, false, &official).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(lib.join("alpha/init.lua")).unwrap(),
        "-- alpha v2\nreturn {}\n"
    );
    assert!(
        lib.join("alpha/data.json").is_file(),
        "new files come along"
    );
    assert!(installed(&lib).await.unwrap()[0].version.len() == 7);

    // Edited here: update and remove refuse, --force goes ahead.
    write(&lib.join("alpha/init.lua"), "-- mine\nreturn {}\n");
    assert!(
        installed(&lib).await.unwrap()[0]
            .version
            .ends_with(", changed")
    );
    let err = update(&lib, Some("alpha"), false, &official)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("did not update"));
    let err = remove(&config, "alpha", false)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("changed since it was installed (init.lua)"),
        "{err}"
    );
    update(&lib, Some("alpha"), true, &official).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(lib.join("alpha/init.lua")).unwrap(),
        "-- alpha v2\nreturn {}\n"
    );
    no_leftovers(&lib);
    remove(&config, "alpha", false).await.unwrap();
}

#[tokio::test]
async fn a_repository_root_a_tag_and_a_directory_on_disk() {
    if !has_git() {
        return;
    }
    let tmp = TempDir::new("plugins-root");
    let official = official(&tmp.path().join("nowhere"));
    let config = tmp.path().join("config");
    let lib = config.join("lib");

    // A repository whose root is the plugin, tagged.
    let origin = tmp.path().join("om-thing");
    std::fs::create_dir_all(&origin).unwrap();
    git_in(&origin, &["init", "--quiet"]).await;
    plugin_files(&origin, "om-thing", "A thing.");
    git_in(&origin, &["add", "-A"]).await;
    git_in(&origin, &["commit", "--quiet", "-m", "first"]).await;
    git_in(&origin, &["tag", "v1"]).await;
    let url = origin.to_string_lossy().to_string();
    add(
        &config,
        std::slice::from_ref(&url),
        None,
        None,
        false,
        &official,
        &Setup::defaults(),
    )
    .await
    .unwrap();
    let thing = &installed(&lib).await.unwrap()[0];
    assert_eq!(
        (thing.name.as_str(), thing.version.as_str()),
        ("om-thing", "v1")
    );
    assert!(!lib.join("om-thing/.git").exists());

    // A repository that is not a plugin leaves nothing behind.
    let other = tmp.path().join("not-a-plugin");
    std::fs::create_dir_all(&other).unwrap();
    git_in(&other, &["init", "--quiet"]).await;
    write(&other.join("README.md"), "not a plugin");
    git_in(&other, &["add", "-A"]).await;
    git_in(&other, &["commit", "--quiet", "-m", "first"]).await;
    let err = add(
        &config,
        &[other.to_string_lossy().to_string()],
        None,
        None,
        true,
        &official,
        &Setup::defaults(),
    )
    .await;
    assert!(err.is_err());
    assert!(!lib.join("not-a-plugin").exists());
    no_leftovers(&lib);

    // A plain directory (yours, being written) is copied as it is, and
    // update copies it again.
    let mine = tmp.path().join("work").join("clock");
    plugin_files(&mine, "clock", "Tells the time.");
    let spec = format!("{}/", mine.display());
    add(
        &config,
        &[spec],
        None,
        None,
        true,
        &official,
        &Setup::defaults(),
    )
    .await
    .unwrap();
    let record = Record::read(&lib.join("clock")).unwrap();
    assert!(matches!(record.source, Source::Local { .. }));
    assert_eq!(record.version_label(), "local copy");
    write(&mine.join("init.lua"), "-- clock v2\nreturn {}\n");
    update(&lib, Some("clock"), false, &official).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(lib.join("clock/init.lua")).unwrap(),
        "-- clock v2\nreturn {}\n"
    );
}

#[tokio::test]
async fn a_plugin_needing_a_newer_om_is_refused() {
    if !has_git() {
        return;
    }
    let tmp = TempDir::new("plugins-requires");
    let mine = tmp.path().join("future");
    write(
        &mine.join("init.lua"),
        "-- future\n-- requires om >= 99.0.0\nreturn {}\n",
    );
    let config = tmp.path().join("config");
    let err = add(
        &config,
        &[mine.to_string_lossy().to_string()],
        None,
        None,
        true,
        &official(tmp.path()),
        &Setup::defaults(),
    )
    .await;
    assert!(err.is_err());
    assert!(!config.join("lib/future").exists());
    assert!(!config.join("rules.d/future.lua").exists());
    no_leftovers(&config.join("lib"));
}

#[tokio::test]
async fn your_own_plugin_keeps_its_git_work_safe() {
    if !has_git() {
        return;
    }
    let tmp = TempDir::new("plugins-new");
    let config = tmp.path().join("config");
    let lib = config.join("lib");
    new(&config, "mine", false, true).await.unwrap();
    assert!(lib.join("mine/init.lua").is_file());
    assert!(lib.join("mine/.git").is_dir());
    assert!(config.join("rules.d/mine.lua").is_file());
    let listed = installed(&lib).await.unwrap();
    assert_eq!(listed[0].version, "yours, no commits yet");
    let err = remove(&config, "mine", false)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("uncommitted changes"), "{err}");
    git_in(&lib.join("mine"), &["add", "-A"]).await;
    git_in(&lib.join("mine"), &["commit", "--quiet", "-m", "start"]).await;
    let err = remove(&config, "mine", false)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("no remote"), "{err}");
    remove(&config, "mine", true).await.unwrap();
    assert!(!lib.join("mine").exists());
    assert!(!config.join("rules.d/mine.lua").exists());
}

#[test]
fn rule_files_carry_options_and_read_back() {
    use serde_json::json;
    let values = vec![
        ("chord".to_string(), json!("SUPER + CTRL + ")),
        ("toggle".to_string(), json!(false)),
        ("keep".to_string(), json!(25)),
    ];
    let rule = rule_text("window-halves", "halves", &values);
    assert_eq!(
        rule,
        "-- window-halves: halves\n\
         -- Options and what it does: ~/.config/omaestro/lib/window-halves/README.md\n\
         local window_halves = om.use(\"window-halves\")\n\
         window_halves.setup({\n\
         \x20 chord = \"SUPER + CTRL + \",\n\
         \x20 toggle = false,\n\
         \x20 keep = 25,\n\
         })\n"
    );
    assert!(is_generated_rule("window-halves", &rule));
    // A value changed by hand keeps the shape; code added does not.
    assert!(is_generated_rule(
        "window-halves",
        &rule.replace("25", "30")
    ));
    assert!(!is_generated_rule(
        "window-halves",
        &format!("{rule}om.notify('x')\n")
    ));

    let back = read_values(&rule).unwrap();
    assert_eq!(back.get("chord"), Some(&json!("SUPER + CTRL + ")));
    assert_eq!(back.get("toggle"), Some(&json!(false)));
    assert_eq!(back.get("keep"), Some(&json!(25)));
    assert!(
        read_values("local m = om.use('x')\nm.setup({ f = function() end })")
            .unwrap_err()
            .contains("code")
    );
    assert!(
        read_values("os.execute('true')").is_err(),
        "no os.execute in there"
    );
    assert_eq!(
        read_values("local m = om.use('x') m.setup({ file = os.getenv('HOME') .. '/n' })")
            .unwrap()
            .get("file")
            .and_then(|v| v.as_str())
            .map(|s| s.ends_with("/n")),
        Some(true)
    );
}

#[tokio::test]
async fn options_are_chosen_on_add_and_changed_with_configure() {
    if !has_git() {
        return;
    }
    let tmp = TempDir::new("plugins-options");
    let mine = tmp.path().join("work").join("clock");
    plugin_files(&mine, "clock", "Tells the time.");
    write(
        &mine.join("plugin.json"),
        r#"{ "options": [
             { "key": "chord", "type": "chord", "default": "SUPER + ALT + X" },
             { "key": "every", "type": "interval", "default": "1h", "optional": true }
           ] }"#,
    );
    let config = tmp.path().join("config");
    let rule_file = config.join("rules.d/clock.lua");
    let off = official(tmp.path());

    // --set picks one; the other stays at its default and is not written.
    let setup = Setup {
        sets: vec![("chord".into(), "super+ctrl+x".into())],
        ..Setup::defaults()
    };
    add(
        &config,
        &[mine.to_string_lossy().to_string()],
        None,
        None,
        true,
        &off,
        &setup,
    )
    .await
    .unwrap();
    let text = std::fs::read_to_string(&rule_file).unwrap();
    assert!(text.contains("  chord = \"SUPER + CTRL + X\",\n"), "{text}");
    assert!(!text.contains("every"), "{text}");

    // Configure in the editor: the form shows both; turning the timer off is
    // one edit, and what the form left alone stays.
    let mut seen = String::new();
    configure(&config, "clock", &Setup::defaults(), &mut |form: &str| {
        seen = form.to_string();
        Ok(form.replace("every = 1h", "every = none"))
    })
    .await
    .unwrap();
    assert!(seen.contains("\nchord = SUPER + CTRL + X\n"), "{seen}");
    assert!(
        seen.contains("# interval like 30s, 5m, 1h30m, optional; default: 1h\nevery = 1h\n"),
        "{seen}"
    );
    let text = std::fs::read_to_string(&rule_file).unwrap();
    assert!(
        text.contains("  chord = \"SUPER + CTRL + X\",\n  every = false,\n"),
        "{text}"
    );
    assert!(text.starts_with("-- clock: Tells the time.\n"), "{text}");

    // A rule with more in it is the owner's: configure refuses without --force.
    std::fs::write(&rule_file, format!("{text}om.log('mine')\n")).unwrap();
    let err = configure(&config, "clock", &Setup::defaults(), &mut |f: &str| {
        Ok(f.to_string())
    })
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("has more in it than om writes"),
        "{err}"
    );
    let force = Setup {
        force: true,
        sets: vec![("every".into(), "2h".into())],
        ..Setup::defaults()
    };
    configure(&config, "clock", &force, &mut |_: &str| {
        unreachable!("--set needs no form")
    })
    .await
    .unwrap();
    assert!(
        !std::fs::read_to_string(&rule_file)
            .unwrap()
            .contains("om.log")
    );

    // --set on a plugin without a plugin.json says so, and installs nothing.
    let plain = tmp.path().join("work").join("plain");
    plugin_files(&plain, "plain", "No options.");
    let err = add(
        &config,
        &[plain.to_string_lossy().to_string()],
        None,
        None,
        true,
        &off,
        &setup,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("not installed: "), "{err}");
    assert!(!config.join("lib/plain").exists());
    assert!(
        configure(&config, "nope", &Setup::defaults(), &mut |f: &str| Ok(
            f.to_string()
        ))
        .await
        .is_err()
    );

    // Removing the plugin takes its generated rule, options and all.
    remove(&config, "clock", false).await.unwrap();
    assert!(!rule_file.exists());
}

#[tokio::test]
async fn remove_all_starts_over_but_keeps_what_would_be_lost() {
    let tmp = TempDir::new("plugins-remove-all");
    let config = tmp.path().join("config");
    let off = official(tmp.path());
    for name in ["one", "two"] {
        let dir = tmp.path().join("work").join(name);
        plugin_files(&dir, name, "A plugin.");
        add(
            &config,
            &[dir.to_string_lossy().to_string()],
            None,
            None,
            true,
            &off,
            &Setup::defaults(),
        )
        .await
        .unwrap();
    }
    // A rule of the user's own stays; so does a plugin they changed.
    write(
        &config.join("rules.d/mine.lua"),
        "om.trigger('x', function() end)\n",
    );
    write(&config.join("lib/two/init.lua"), "-- changed\nreturn {}\n");
    let err = remove_all(&config, false).await.unwrap_err().to_string();
    assert!(err.contains("kept two"), "{err}");
    assert!(!config.join("lib/one").exists() && !config.join("rules.d/one.lua").exists());
    assert!(config.join("lib/two").exists() && config.join("rules.d/two.lua").exists());
    remove_all(&config, true).await.unwrap();
    assert!(installed(&config.join("lib")).await.unwrap().is_empty());
    assert!(!config.join("rules.d/two.lua").exists());
    assert!(
        config.join("rules.d/mine.lua").exists(),
        "the user's own rule is not a plugin's"
    );
    remove_all(&config, false).await.unwrap();
}
