//! Asking for a plugin's options: on `om plugin add` (before anything is
//! installed) and on `om plugin configure`, from its `plugin.json`. Chords
//! are checked against what Hyprland already binds while you choose them,
//! not afterwards. The answers go into the plugin's rule file.

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value as Json};

use super::rule::{description_of, is_generated_rule, read_values, rule_path, write_rule};
use super::schema::{self, Opt, Schema, display};
use crate::backend::hypr::HyprCtl;
use crate::backend::{BindInfo, Hypr};
use crate::chord::Chord;

/// How options are chosen for one `om plugin add` or `configure`.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    /// `--set key=value`, applied before any question.
    pub sets: Vec<(String, String)>,
    /// Ask in the terminal (stdin and stdout are one, and no `--defaults`).
    pub interactive: bool,
    /// Hyprland's binds, to check chords against; empty: no check.
    pub binds: Vec<BindInfo>,
    /// Replace a rule file om did not write.
    pub force: bool,
}

impl Setup {
    /// Defaults, no questions: for scripts and the first-run offer.
    pub fn defaults() -> Self {
        Self::default()
    }

    /// From the command line: `--set` pairs, `--defaults`, `--force`; asks
    /// only in a terminal, and reads Hyprland's binds when it can.
    pub async fn from_cli(sets: &[String], defaults: bool, force: bool) -> Result<Self> {
        let mut pairs = Vec::new();
        for set in sets {
            let Some((key, value)) = set.split_once('=') else {
                bail!("--set takes key=value, not '{set}'");
            };
            pairs.push((key.trim().to_string(), value.to_string()));
        }
        let interactive =
            !defaults && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        let binds = HyprCtl.binds().await.unwrap_or_default();
        Ok(Self {
            sets: pairs,
            interactive,
            binds,
            force,
        })
    }

    /// Who holds a chord already, other than this plugin's own binds.
    fn holder(&self, plugin: &str, chord: &Chord) -> Option<String> {
        let own = format!("omaestro: lib/{plugin}/");
        self.binds
            .iter()
            .filter(|bind| bind.submap.is_empty() && !bind.description.starts_with(&own))
            .find(|bind| bind.chord.same_keys(chord))
            .map(|bind| match bind.description.strip_prefix("omaestro: ") {
                Some(origin) => format!("an omaestro rule ({origin})"),
                None if bind.description.is_empty() => "a bind without a description".to_string(),
                None => format!("\"{}\"", bind.description),
            })
    }
}

/// One line from the terminal; `None` at the end of input (Ctrl+D).
pub fn ask_stdin(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
    }
}

/// The chords in `values` someone else holds, or that two options share.
fn conflicts(
    plugin: &str,
    schema: &Schema,
    values: &Map<String, Json>,
    opt: &Opt,
    setup: &Setup,
) -> Vec<String> {
    let value = values.get(&opt.key).cloned().unwrap_or(Json::Null);
    let mut found = Vec::new();
    for chord in opt.chords(&value) {
        if let Some(holder) = setup.holder(plugin, &chord) {
            found.push(format!("{chord} is taken by {holder}"));
        }
        for other in schema.options.iter().filter(|o| o.key != opt.key) {
            let theirs = values.get(&other.key).cloned().unwrap_or(Json::Null);
            if other.chords(&theirs).iter().any(|c| c.same_keys(&chord)) {
                found.push(format!("{chord} is also {}", other.key));
            }
        }
    }
    found
}

/// Works out the options: what is there now (`start`, from a rule file),
/// else the defaults; then `--set`; then, in a terminal, a question per
/// option. Returns the options to write, in the schema's order, only those
/// that differ from the default, followed by anything the rule had that the
/// schema does not describe.
pub fn fill(
    plugin: &str,
    schema: &Schema,
    start: &Map<String, Json>,
    setup: &Setup,
    ask: &mut (dyn FnMut(&str) -> Option<String> + Send),
) -> Result<Vec<(String, Json)>> {
    let mut values = Map::new();
    // Values the schema cannot represent (a list where it expects a switch,
    // say) stay as written and are not asked about.
    let mut kept = Vec::new();
    for opt in &schema.options {
        match start.get(&opt.key) {
            Some(value) if schema.accepts(&opt.key, value) => {
                values.insert(opt.key.clone(), value.clone());
            }
            Some(value) => {
                kept.push(opt.key.clone());
                values.insert(opt.key.clone(), value.clone());
            }
            None => {
                values.insert(opt.key.clone(), opt.default_value());
            }
        }
    }
    let extras: Vec<(String, Json)> = start
        .iter()
        .filter(|(key, _)| schema.get(key).is_none())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

    for (key, text) in &setup.sets {
        let Some(opt) = schema.get(key) else {
            let keys: Vec<&str> = schema.options.iter().map(|o| o.key.as_str()).collect();
            bail!(
                "{plugin} has no option '{key}'; it has: {}",
                keys.join(", ")
            );
        };
        let value = opt
            .parse(text)
            .map_err(|err| anyhow::anyhow!("--set {key}: {err}"))?;
        values.insert(key.clone(), value);
        kept.retain(|k| k != key);
    }

    if setup.interactive && !schema.options.is_empty() {
        println!("{plugin}: Enter keeps a value; `none` turns an optional one off; Ctrl+D cancels");
        for opt in &schema.options {
            if kept.contains(&opt.key) {
                println!("  {}: kept as written in the rule file", opt.title());
                continue;
            }
            if let Some(description) = &opt.description {
                println!("  {description}");
            }
            loop {
                let current = values.get(&opt.key).cloned().unwrap_or(Json::Null);
                let Some(answer) =
                    ask(&format!("  {} [{}]: ", opt.title(), display(opt, &current)))
                else {
                    println!();
                    bail!("cancelled; nothing was changed");
                };
                if !answer.trim().is_empty() {
                    match opt.parse(&answer) {
                        Ok(value) => {
                            values.insert(opt.key.clone(), value);
                        }
                        Err(err) => {
                            println!("    {err}");
                            continue;
                        }
                    }
                }
                let clashes = conflicts(plugin, schema, &values, opt, setup);
                if clashes.is_empty() {
                    break;
                }
                for clash in &clashes {
                    println!("    {clash}");
                }
                let keep = ask("    keep it anyway? [y/N]: ").unwrap_or_default();
                if matches!(keep.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                    break;
                }
            }
        }
    } else {
        for opt in &schema.options {
            for clash in conflicts(plugin, schema, &values, opt, setup) {
                eprintln!(
                    "om: {plugin}: {clash}; it will be refused unless you pick another (om plugin configure {plugin})"
                );
            }
        }
    }

    let mut out: Vec<(String, Json)> = schema
        .options
        .iter()
        .filter_map(|opt| {
            let value = values.get(&opt.key)?.clone();
            let changed = value != opt.default_value() && !value.is_null();
            changed.then(|| (opt.key.clone(), value))
        })
        .collect();
    out.extend(extras);
    Ok(out)
}

/// `om plugin configure`: the options of an installed plugin, asked again
/// with its rule file's values as the starting point, and written back.
pub async fn configure(
    config_dir: &Path,
    name: &str,
    setup: &Setup,
    ask: &mut (dyn FnMut(&str) -> Option<String> + Send),
) -> Result<()> {
    let dir = config_dir.join("lib").join(name);
    if !dir.join("init.lua").is_file() {
        bail!(
            "no plugin named '{name}' in {}",
            config_dir.join("lib").display()
        );
    }
    let Some(schema) = schema::load(&dir)? else {
        bail!(
            "{name} has no {}: its options are in lib/{name}/README.md and go in rules.d/{name}.lua",
            schema::FILE
        );
    };
    let path = rule_path(config_dir, name);
    let (start, description) = match std::fs::read_to_string(&path) {
        Ok(text) => {
            if !setup.force && !is_generated_rule(name, &text) {
                bail!(
                    "{} has more in it than om writes; change it there, or add --force to replace it",
                    path.display()
                );
            }
            let values = match read_values(&text) {
                Ok(values) => values,
                // --force replaces the file anyway: start from the defaults.
                Err(err) if setup.force => {
                    eprintln!("om: {}: {err}; starting from the defaults", path.display());
                    Map::new()
                }
                Err(err) => bail!("{}: {err}", path.display()),
            };
            (values, description_of(name, &text))
        }
        Err(_) => (Map::new(), None),
    };
    let description = description
        .or_else(|| super::fetch::summary(&dir))
        .unwrap_or_else(|| "a plugin".to_string());
    let values = fill(name, &schema, &start, setup, ask)?;
    write_rule(config_dir, name, &description, &values, true, setup.force)
        .with_context(|| format!("saving {name}'s options"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Schema {
        serde_json::from_value(json!({ "options": [
            { "key": "chord", "type": "chord", "label": "Open it", "default": "SUPER + ALT + P" },
            { "key": "toggle", "type": "chord", "default": "SUPER + ALT + C", "optional": true },
            { "key": "keep", "type": "number", "default": 10 },
            { "key": "modes", "type": "bool", "default": false }
        ]}))
        .unwrap()
    }

    fn scripted(answers: &[&str]) -> impl FnMut(&str) -> Option<String> {
        let mut answers: std::collections::VecDeque<String> =
            answers.iter().map(|a| a.to_string()).collect();
        move |_prompt| answers.pop_front()
    }

    fn bind(chord: &str, description: &str) -> BindInfo {
        BindInfo {
            chord: Chord::parse(chord).unwrap(),
            description: description.to_string(),
            submap: String::new(),
        }
    }

    #[test]
    fn defaults_write_nothing_and_answers_write_what_changed() {
        let none = Setup::defaults();
        let out = fill("p", &schema(), &Map::new(), &none, &mut scripted(&[])).unwrap();
        assert!(out.is_empty());

        let ask = Setup {
            interactive: true,
            ..Setup::default()
        };
        let out = fill(
            "p",
            &schema(),
            &Map::new(),
            &ask,
            &mut scripted(&["super+ctrl+p", "none", "", "yes"]),
        )
        .unwrap();
        assert_eq!(
            out,
            [
                ("chord".to_string(), json!("SUPER + CTRL + P")),
                ("toggle".to_string(), json!(false)),
                ("modes".to_string(), json!(true)),
            ]
        );
    }

    #[test]
    fn a_bad_answer_is_asked_again_and_ctrl_d_cancels() {
        let ask = Setup {
            interactive: true,
            ..Setup::default()
        };
        let out = fill(
            "p",
            &schema(),
            &Map::new(),
            &ask,
            &mut scripted(&["", "", "ten", "12", ""]),
        )
        .unwrap();
        assert_eq!(out, [("keep".to_string(), json!(12))]);
        let err = fill("p", &schema(), &Map::new(), &ask, &mut scripted(&[""])).unwrap_err();
        assert!(err.to_string().starts_with("cancelled"));
    }

    #[test]
    fn a_taken_chord_is_caught_while_choosing() {
        let setup = Setup {
            interactive: true,
            binds: vec![
                bind("SUPER + ALT + P", "Omarchy: something"),
                bind("SUPER + ALT + C", "omaestro: lib/p/init.lua:3"),
            ],
            ..Setup::default()
        };
        // The default is taken: decline it, pick another; its own bind is not a clash.
        let out = fill(
            "p",
            &schema(),
            &Map::new(),
            &setup,
            &mut scripted(&["", "n", "SUPER + ALT + X", "", "", ""]),
        )
        .unwrap();
        assert_eq!(out, [("chord".to_string(), json!("SUPER + ALT + X"))]);
        // Or keep it anyway.
        let out = fill(
            "p",
            &schema(),
            &Map::new(),
            &setup,
            &mut scripted(&["", "y", "", "", ""]),
        )
        .unwrap();
        assert!(out.is_empty());
        assert_eq!(
            setup
                .holder("p", &Chord::parse("SUPER + ALT + P").unwrap())
                .as_deref(),
            Some("\"Omarchy: something\"")
        );
        assert_eq!(
            setup.holder("p", &Chord::parse("SUPER + ALT + C").unwrap()),
            None
        );
        assert_eq!(
            setup
                .holder("q", &Chord::parse("SUPER + ALT + C").unwrap())
                .as_deref(),
            Some("an omaestro rule (lib/p/init.lua:3)")
        );
    }

    #[test]
    fn sets_and_what_the_rule_already_had() {
        let mut start = Map::new();
        start.insert("keep".into(), json!(25));
        start.insert("modes".into(), json!([["a", "b"]])); // a list where the schema says bool
        start.insert("extra".into(), json!("x"));
        let setup = Setup {
            sets: vec![("toggle".into(), "none".into())],
            ..Setup::default()
        };
        let out = fill("p", &schema(), &start, &setup, &mut scripted(&[])).unwrap();
        assert_eq!(
            out,
            [
                ("toggle".to_string(), json!(false)),
                ("keep".to_string(), json!(25)),
                ("modes".to_string(), json!([["a", "b"]])),
                ("extra".to_string(), json!("x")),
            ]
        );
        let bad = Setup {
            sets: vec![("nope".into(), "1".into())],
            ..Setup::default()
        };
        let err = fill("p", &schema(), &Map::new(), &bad, &mut scripted(&[])).unwrap_err();
        assert!(
            err.to_string()
                .contains("has no option 'nope'; it has: chord, toggle, keep, modes")
        );
    }
}
