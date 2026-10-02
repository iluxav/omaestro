//! Tests of `plugins::configure` and the form.

use super::*;
use serde_json::json;

fn schema() -> Schema {
    serde_json::from_value(json!({ "options": [
        { "key": "chord", "type": "chord", "label": "Open it", "default": "SUPER + ALT + P",
          "description": "The chord that opens it." },
        { "key": "toggle", "type": "chord", "default": "SUPER + ALT + C", "optional": true },
        { "key": "keep", "type": "number", "default": 10 },
        { "key": "modes", "type": "bool", "default": false }
    ]}))
    .unwrap()
}

fn bind(chord: &str, description: &str) -> BindInfo {
    BindInfo {
        chord: Chord::parse(chord).unwrap(),
        description: description.to_string(),
        submap: String::new(),
    }
}

fn defaults() -> Resolved {
    resolve("p", &schema(), &Map::new(), &[]).unwrap()
}

/// One round of editing: the form in, the saved form out.
type Round = Box<dyn Fn(&str) -> String + Send>;
/// Every form the editor was shown, in order.
type Shown = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// An editor that applies each round in turn and records the forms it was
/// shown.
fn editor(rounds: Vec<Round>) -> (impl FnMut(&str) -> Result<String> + Send, Shown) {
    let shown: Shown = Default::default();
    let log = shown.clone();
    let mut rounds = rounds.into_iter();
    let edit = move |form: &str| {
        log.lock().unwrap().push(form.to_string());
        let round = rounds
            .next()
            .expect("the form came back more often than expected");
        Ok(round(form))
    };
    (edit, shown)
}

#[test]
fn the_form_shows_every_option_and_reads_back() {
    let form = form::render("p", &schema(), &defaults().values, &Map::new());
    assert!(form.starts_with("# p: change a value, then save and close the editor.\n"));
    assert!(form.contains("# Open it\n# The chord that opens it.\n# chord; default: SUPER + ALT + P\nchord = SUPER + ALT + P\n"), "{form}");
    assert!(
        form.contains("# chord, optional; default: SUPER + ALT + C\ntoggle = SUPER + ALT + C\n"),
        "{form}"
    );
    assert!(form.contains("modes = no\n"), "{form}");
    // Unchanged, it reads back as the same values.
    assert_eq!(
        form::parse(&form, &schema(), &Map::new()),
        Ok(Edited::Values(defaults().values))
    );
    // Emptied of options: cancel.
    assert_eq!(
        form::parse("# nothing\n", &schema(), &Map::new()),
        Ok(Edited::Cancelled)
    );
    // A removed line or an empty value is the default.
    let Ok(Edited::Values(values)) = form::parse("keep =\nmodes = yes\n", &schema(), &Map::new())
    else {
        panic!()
    };
    assert_eq!(values["keep"], json!(10));
    assert_eq!(values["chord"], json!("SUPER + ALT + P"));
    assert_eq!(values["modes"], json!(true));
}

#[test]
fn every_problem_in_a_form_is_reported_at_once() {
    let problems = form::parse(
        "chord = HYPER + J\nkeep = many\nwhat = 1\nnot a line\nmodes = yes\nmodes = no\n",
        &schema(),
        &Map::new(),
    )
    .unwrap_err();
    assert_eq!(problems.len(), 5, "{problems:?}");
    assert!(problems[0].starts_with("chord: not a chord"));
    assert_eq!(problems[1], "keep: a number");
    assert_eq!(problems[2], "line 3: there is no option `what`");
    assert_eq!(problems[3], "line 4: `not a line` is not `key = value`");
    assert_eq!(problems[4], "modes: given twice");
}

#[test]
fn a_form_with_problems_comes_back_with_them_on_top() {
    let (mut edit, shown) = editor(vec![
        Box::new(|f: &str| f.replace("keep = 10", "keep = lots")),
        Box::new(|f: &str| f.replace("keep = lots", "keep = 25")),
    ]);
    let out = edit_loop("p", &schema(), &defaults(), &Setup::defaults(), &mut edit)
        .unwrap()
        .unwrap();
    assert_eq!(out.values["keep"], json!(25));
    let shown = shown.lock().unwrap();
    assert!(
        shown[1].starts_with("#! NOT SAVED. Fix these"),
        "{}",
        shown[1]
    );
    assert!(shown[1].contains("#!   keep: a number\n"), "{}", shown[1]);
    // The user's edit is still there to fix, not reset.
    assert!(shown[1].contains("\nkeep = lots\n"), "{}", shown[1]);
}

#[test]
fn a_taken_chord_is_shown_once_and_kept_when_saved_again() {
    let setup = Setup {
        binds: vec![
            bind("SUPER + ALT + X", "Omarchy: something"),
            bind("SUPER + ALT + C", "omaestro: lib/p/init.lua:3"),
        ],
        ..Setup::default()
    };
    let (mut edit, shown) = editor(vec![
        Box::new(|f: &str| f.replace("chord = SUPER + ALT + P", "chord = SUPER + ALT + X")),
        Box::new(|f: &str| f.to_string()),
    ]);
    let out = edit_loop("p", &schema(), &defaults(), &setup, &mut edit)
        .unwrap()
        .unwrap();
    assert_eq!(out.values["chord"], json!("SUPER + ALT + X"));
    let shown = shown.lock().unwrap();
    assert!(
        shown[1].contains("#!   chord: SUPER + ALT + X is taken by \"Omarchy: something\"\n"),
        "{}",
        shown[1]
    );
    // This plugin's own bind (toggle on SUPER+ALT+C) is no clash.
    let notes: Vec<&str> = shown[1].lines().filter(|l| l.starts_with("#!")).collect();
    assert!(!notes.iter().any(|l| l.contains("toggle")), "{notes:?}");
}

#[test]
fn nothing_changed_or_cancelled_writes_nothing() {
    let (mut same, _) = editor(vec![Box::new(|f: &str| f.to_string())]);
    assert_eq!(
        edit_loop("p", &schema(), &defaults(), &Setup::default(), &mut same).unwrap(),
        None
    );
    let (mut emptied, _) = editor(vec![Box::new(|_: &str| String::new())]);
    assert_eq!(
        edit_loop("p", &schema(), &defaults(), &Setup::default(), &mut emptied).unwrap(),
        None
    );
}

#[test]
fn sets_kept_code_and_what_gets_written() {
    let mut start = Map::new();
    start.insert("keep".into(), json!(25));
    start.insert("modes".into(), json!([["a", "b"]])); // a list where the schema says bool
    start.insert("extra".into(), json!("x"));
    let resolved = resolve("p", &schema(), &start, &[("toggle".into(), "none".into())]).unwrap();
    assert!(resolved.kept.contains_key("modes"));
    assert_eq!(
        to_write(&schema(), &resolved),
        [
            ("toggle".to_string(), json!(false)),
            ("keep".to_string(), json!(25)),
            ("modes".to_string(), json!([["a", "b"]])),
            ("extra".to_string(), json!("x")),
        ]
    );
    let form = form::render("p", &schema(), &resolved.values, &resolved.kept);
    assert!(
        form.contains("# Set in rules.d/p.lua as Lua code"),
        "{form}"
    );
    assert!(!form.contains("\nmodes ="), "{form}");
    let err = resolve("p", &schema(), &Map::new(), &[("nope".into(), "1".into())]).unwrap_err();
    assert!(
        err.to_string()
            .contains("has no option 'nope'; it has: chord, toggle, keep, modes")
    );
}

#[test]
fn the_summary_is_a_small_table_with_clashes_under_it() {
    let setup = Setup {
        binds: vec![bind("SUPER + ALT + P", "Omarchy: something")],
        ..Setup::default()
    };
    let text = summary("p", &schema(), &defaults(), &setup);
    assert_eq!(
        text,
        "  chord   SUPER + ALT + P  Open it\n\
         \x20 toggle  SUPER + ALT + C  \n\
         \x20 keep    10               \n\
         \x20 modes   no               \n\
         \x20 ! chord: SUPER + ALT + P is taken by \"Omarchy: something\"\n\
         change them: om plugin configure p\n"
    );
}

#[test]
fn the_editor_waits_for_the_form() {
    // Omarchy's launcher sends a windowed editor to the background: what it
    // would start is run instead, told to wait.
    assert_eq!(
        waiting("omarchy-launch-editor --inline", Some("code".into())),
        "code --wait"
    );
    assert_eq!(
        waiting("/usr/bin/omarchy-launch-editor", Some("nvim".into())),
        "nvim"
    );
    assert_eq!(waiting("omarchy-launch-editor --inline", None), "nvim");
    assert_eq!(waiting("kate", None), "kate --block");
    assert_eq!(waiting("/opt/zed/zed", None), "/opt/zed/zed --wait");
    // Already waiting, or a terminal editor: as it is.
    assert_eq!(waiting("code --wait", None), "code --wait");
    assert_eq!(waiting("code -w", None), "code -w");
    assert_eq!(waiting("nvim -u NONE", None), "nvim -u NONE");
}

#[test]
fn options_that_only_apply_with_a_switch_do_not_clash_without_it() {
    // ai-text's shape: rewrite turns off with modes on; menu only exists with it.
    let schema: Schema = serde_json::from_value(json!({ "options": [
        { "key": "rewrite", "type": "chord", "default": "SUPER + ALT + J", "optional": true, "unless": "modes" },
        { "key": "modes", "type": "bool", "default": false },
        { "key": "menu", "type": "chord", "default": "SUPER + ALT + J", "when": "modes" }
    ]}))
    .unwrap();
    let off = resolve("ai", &schema, &Map::new(), &[]).unwrap();
    assert!(conflicts("ai", &schema, &off.values, &Setup::default()).is_empty());
    let on = resolve(
        "ai",
        &schema,
        &Map::new(),
        &[("modes".into(), "yes".into())],
    )
    .unwrap();
    assert!(conflicts("ai", &schema, &on.values, &Setup::default()).is_empty());
    // With modes on and rewrite given the same chord on purpose, it is a clash.
    let both = resolve(
        "ai",
        &schema,
        &Map::new(),
        &[
            ("modes".into(), "yes".into()),
            ("rewrite".into(), "SUPER + ALT + J".into()),
        ],
    )
    .unwrap();
    // Still the default value, so the plugin turns it off: no clash.
    assert!(conflicts("ai", &schema, &both.values, &Setup::default()).is_empty());
    let other = resolve(
        "ai",
        &schema,
        &Map::new(),
        &[
            ("modes".into(), "yes".into()),
            ("menu".into(), "SUPER + ALT + K".into()),
            ("rewrite".into(), "SUPER + ALT + K".into()),
        ],
    )
    .unwrap();
    assert_eq!(
        conflicts("ai", &schema, &other.values, &Setup::default()),
        ["menu and rewrite are both SUPER + ALT + K"]
    );
    // The summary and the form say why an option is idle.
    let text = summary("ai", &schema, &off, &Setup::default());
    assert!(text.contains("(only used with modes = yes)"), "{text}");
    let text = summary("ai", &schema, &on, &Setup::default());
    assert!(
        text.contains("(off with modes = yes, unless given a value of its own)"),
        "{text}"
    );
    let form = form::render("ai", &schema, &off.values, &Map::new());
    assert!(form.contains("# (only used with modes = yes)\n"), "{form}");
    // A dependency on something that is not a switch is a broken plugin.json.
    let dir = crate::testutil::TempDir::new("schema-when");
    std::fs::write(
        dir.path().join(schema::FILE),
        r#"{ "options": [ { "key": "a", "type": "chord", "when": "b" } ] }"#,
    )
    .unwrap();
    assert!(
        schema::load(dir.path())
            .unwrap_err()
            .to_string()
            .contains("depends on b")
    );
}
