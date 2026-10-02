//! A plugin's options, from its `plugin.json`: `om plugin add` installs with
//! the defaults (or `--set`) and shows what it chose; `om plugin configure`
//! opens every option as a form in your editor and checks it on save. Chords
//! are checked against what Hyprland already binds. The result goes into
//! the plugin's rule file, never its code.

use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value as Json};

use super::form::{self, Edited};
use super::rule::{description_of, is_generated_rule, read_values, rule_path, write_rule};
use super::schema::{self, Schema};
use crate::backend::hypr::HyprCtl;
use crate::backend::run::find_on_path;
use crate::backend::{BindInfo, Hypr};
use crate::chord::Chord;

/// How options are chosen for one `om plugin add` or `configure`.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    /// `--set key=value`.
    pub sets: Vec<(String, String)>,
    /// Hyprland's binds, to check chords against; empty: no check.
    pub binds: Vec<BindInfo>,
    /// Replace a rule file om did not write.
    pub force: bool,
}

impl Setup {
    /// Defaults, no checks: for the first-run offer.
    pub fn defaults() -> Self {
        Self::default()
    }

    /// From the command line, with Hyprland's binds when it can read them.
    pub async fn from_cli(sets: &[String], force: bool) -> Result<Self> {
        let mut pairs = Vec::new();
        for set in sets {
            let Some((key, value)) = set.split_once('=') else {
                bail!("--set takes key=value, not '{set}'");
            };
            pairs.push((key.trim().to_string(), value.to_string()));
        }
        let binds = HyprCtl.binds().await.unwrap_or_default();
        Ok(Self {
            sets: pairs,
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

/// A plugin's options as they stand.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resolved {
    /// Every option of the schema.
    pub values: Map<String, Json>,
    /// Options holding something the schema's type cannot show (Lua code);
    /// left as they are.
    pub kept: Map<String, Json>,
    /// Options the rule file passes that the schema does not describe.
    pub extras: Vec<(String, Json)>,
}

/// What is there now (`start`, from a rule file), else the defaults; then
/// `--set` on top.
pub fn resolve(
    plugin: &str,
    schema: &Schema,
    start: &Map<String, Json>,
    sets: &[(String, String)],
) -> Result<Resolved> {
    let mut resolved = Resolved::default();
    for opt in &schema.options {
        let value = match start.get(&opt.key) {
            Some(value) if schema.accepts(&opt.key, value) => value.clone(),
            Some(value) => {
                resolved.kept.insert(opt.key.clone(), value.clone());
                value.clone()
            }
            None => opt.default_value(),
        };
        resolved.values.insert(opt.key.clone(), value);
    }
    resolved.extras = start
        .iter()
        .filter(|(key, _)| schema.get(key).is_none())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    for (key, text) in sets {
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
        resolved.values.insert(key.clone(), value);
        resolved.kept.remove(key);
    }
    Ok(resolved)
}

/// The options to write into the rule file: those that differ from the
/// default, in the schema's order, then anything the schema does not
/// describe.
pub fn to_write(schema: &Schema, resolved: &Resolved) -> Vec<(String, Json)> {
    let mut out: Vec<(String, Json)> = schema
        .options
        .iter()
        .filter_map(|opt| {
            let value = resolved.values.get(&opt.key)?.clone();
            (value != opt.default_value() && !value.is_null()).then(|| (opt.key.clone(), value))
        })
        .collect();
    out.extend(resolved.extras.iter().cloned());
    out
}

/// Every chord in the options that Hyprland or another option already has.
pub fn conflicts(
    plugin: &str,
    schema: &Schema,
    values: &Map<String, Json>,
    setup: &Setup,
) -> Vec<String> {
    let mut found = Vec::new();
    for opt in schema.options.iter().filter(|o| o.active(values)) {
        let value = values.get(&opt.key).cloned().unwrap_or(Json::Null);
        for chord in opt.chords(&value) {
            if let Some(holder) = setup.holder(plugin, &chord) {
                found.push(format!("{}: {chord} is taken by {holder}", opt.key));
            }
            for other in schema
                .options
                .iter()
                .filter(|o| o.key > opt.key && o.active(values))
            {
                let theirs = values.get(&other.key).cloned().unwrap_or(Json::Null);
                if other.chords(&theirs).iter().any(|c| c.same_keys(&chord)) {
                    found.push(format!("{} and {} are both {chord}", opt.key, other.key));
                }
            }
        }
    }
    found
}

/// The options as a small table, then any clash, then how to change them.
pub fn summary(plugin: &str, schema: &Schema, resolved: &Resolved, setup: &Setup) -> String {
    if schema.options.is_empty() {
        return String::new();
    }
    let rows: Vec<(String, String, String)> = schema
        .options
        .iter()
        .map(|opt| {
            let value = resolved.values.get(&opt.key).cloned().unwrap_or(Json::Null);
            let shown = if resolved.kept.contains_key(&opt.key) {
                "(Lua code)".to_string()
            } else {
                let shown = form::form_value(opt, &value);
                if shown.is_empty() {
                    "(default)".to_string()
                } else {
                    shown
                }
            };
            let mut label = opt.label.clone().unwrap_or_default();
            if !opt.active(&resolved.values)
                && let Some(reason) = opt.inactive_reason()
            {
                label = format!("{label} ({reason})");
            }
            (opt.key.clone(), shown, label)
        })
        .collect();
    let key_width = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    let value_width = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0);
    let mut out = String::new();
    for (key, value, label) in rows {
        out.push_str(&format!(
            "  {key:key_width$}  {value:value_width$}  {label}\n"
        ));
    }
    for clash in conflicts(plugin, schema, &resolved.values, setup) {
        out.push_str(&format!("  ! {clash}\n"));
    }
    out.push_str(&format!("change them: om plugin configure {plugin}\n"));
    out
}

/// An editor: given the form, returns it as saved.
pub type Editor<'a> = &'a mut (dyn FnMut(&str) -> Result<String> + Send);

/// Opens `text` in your editor, in this terminal or as a window, waits for
/// you to close it, and returns the file as it was saved.
pub fn edit_in_editor(text: &str) -> Result<String> {
    let mut command = editor_command().context(
        "no editor found: set $EDITOR (nvim, vim, nano, code, ...), or use --set key=value",
    )?;
    let terminal = !is_gui(&command[0]);
    if terminal && !std::io::stdin().is_terminal() {
        bail!(
            "the options form needs a terminal for {}; without one, use --set key=value",
            command[0]
        );
    }
    // A rule file (Lua) or the options form (key = value lines), named so
    // the editor highlights it.
    let extension = if text.starts_with("--") { "lua" } else { "ini" };
    let path = std::env::temp_dir().join(format!(
        "omaestro-options-{}.{extension}",
        std::process::id()
    ));
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    if !terminal {
        println!(
            "opened the form in {}; save it and close its tab to continue",
            command[0]
        );
    }
    let started = std::time::Instant::now();
    command.push(path.to_string_lossy().to_string());
    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .status()
        .with_context(|| format!("running {}", command[0]));
    let saved = std::fs::read_to_string(&path);
    let status = match status {
        Ok(status) => status,
        Err(err) => {
            let _ = std::fs::remove_file(&path);
            return Err(err);
        }
    };
    // An editor that hands the file to a window and returns at once (a GUI
    // editor without its wait flag) would leave the form empty: keep the
    // file and say so instead of reading nothing.
    if started.elapsed() < std::time::Duration::from_millis(800)
        && saved.as_deref().ok() == Some(text)
    {
        bail!(
            "{} returned before the form could be edited (it does not wait for the file to close); \
             set $VISUAL to an editor that waits, like VISUAL=\"code --wait\" or VISUAL=nvim. \
             The form is in {}",
            command[0],
            path.display()
        );
    }
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        bail!("{} exited with {status}; nothing was changed", command[0]);
    }
    saved.with_context(|| format!("reading {}", path.display()))
}

/// GUI editors, and the flag that makes each wait until the file is closed.
const GUI_EDITORS: &[(&str, &str)] = &[
    ("code", "--wait"),
    ("code-insiders", "--wait"),
    ("codium", "--wait"),
    ("cursor", "--wait"),
    ("windsurf", "--wait"),
    ("zed", "--wait"),
    ("zeditor", "--wait"),
    ("subl", "--wait"),
    ("sublime_text", "--wait"),
    ("gedit", "--wait"),
    ("kate", "--block"),
    ("gvim", "--nofork"),
];

/// VS Code and its forks hand a file to the window used last, on whatever
/// workspace that is; the form opens in a window of its own instead, where
/// you are, unless the command already says which window.
const NEW_WINDOW_EDITORS: &[&str] = &["code", "code-insiders", "codium", "cursor", "windsurf"];
const WINDOW_FLAGS: &[&str] = &["-n", "--new-window", "-r", "--reuse-window"];

fn base(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

fn is_gui(program: &str) -> bool {
    GUI_EDITORS.iter().any(|(name, _)| *name == base(program))
}

/// The editor command to run, from the configured one (`$VISUAL` or
/// `$EDITOR`) and the editor Omarchy's launcher would pick
/// (`omarchy_default`): `omarchy-launch-editor` is replaced by that editor
/// (nvim when there is none), and a GUI editor gets the flag that makes it
/// wait for the file to be closed, unless it has it already; VS Code and
/// its forks also get a new window (see `NEW_WINDOW_EDITORS`).
fn waiting(configured: &str, omarchy_default: Option<String>) -> String {
    let mut parts: Vec<String> = configured.split_whitespace().map(str::to_string).collect();
    if parts
        .first()
        .is_some_and(|p| base(p) == "omarchy-launch-editor")
    {
        let chosen = omarchy_default.unwrap_or_else(|| "nvim".to_string());
        parts = chosen.split_whitespace().map(str::to_string).collect();
    }
    if let Some(program) = parts.first()
        && let Some((_, flag)) = GUI_EDITORS.iter().find(|(name, _)| *name == base(program))
        && !parts.iter().any(|p| p == flag || p == "-w")
    {
        parts.push(flag.to_string());
    }
    if let Some(program) = parts.first()
        && NEW_WINDOW_EDITORS.contains(&base(program))
        && !parts.iter().any(|p| WINDOW_FLAGS.contains(&p.as_str()))
    {
        parts.push("--new-window".to_string());
    }
    parts.join(" ")
}

/// The editor to run, as program and arguments: `$VISUAL`, `$EDITOR`, or
/// the first of nvim, vim, vi, nano on PATH, made to wait (see `waiting`).
pub fn editor_command() -> Option<Vec<String>> {
    let configured = ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|e| !e.trim().is_empty()))
        .or_else(|| {
            ["nvim", "vim", "vi", "nano"]
                .iter()
                .find(|e| find_on_path(e).is_some())
                .map(|e| e.to_string())
        })?;
    // What omarchy-launch-editor would start: the first line of Omarchy's
    // default-editor file, when that editor is installed.
    let omarchy_default = std::env::var_os("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".local/state/omarchy/defaults/editor"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.lines().next().map(|l| l.trim().to_string()))
        .filter(|e| {
            e.split_whitespace()
                .next()
                .is_some_and(|program| find_on_path(program).is_some())
        });
    let command = waiting(&configured, omarchy_default);
    Some(command.split_whitespace().map(str::to_string).collect())
}

/// The form, round after round, until it is saved without problems (a
/// clash, once shown, is accepted when saved again as it is). None when
/// nothing changed or the form was emptied.
fn edit_loop(
    plugin: &str,
    schema: &Schema,
    resolved: &Resolved,
    setup: &Setup,
    edit: Editor,
) -> Result<Option<Resolved>> {
    let mut text = form::render(plugin, schema, &resolved.values, &resolved.kept);
    let mut warned: Vec<String> = Vec::new();
    loop {
        let saved = edit(&text)?;
        match form::parse(&saved, schema, &resolved.kept) {
            Err(problems) => {
                let mut notes =
                    vec!["NOT SAVED. Fix these, then save and close again (or delete every option line to cancel):".to_string()];
                notes.extend(problems.into_iter().map(|p| format!("  {p}")));
                text = form::with_notes(&saved, &notes);
            }
            Ok(Edited::Cancelled) => return Ok(None),
            Ok(Edited::Values(values)) => {
                if values == resolved.values {
                    return Ok(None);
                }
                let clashes = conflicts(plugin, schema, &values, setup);
                if !clashes.is_empty() && clashes != warned {
                    let mut notes = vec![
                        "NOT SAVED YET. These chords are taken; change them, or save and close again as they are to keep them:"
                            .to_string(),
                    ];
                    notes.extend(clashes.iter().map(|c| format!("  {c}")));
                    text = form::with_notes(&saved, &notes);
                    warned = clashes;
                    continue;
                }
                let mut changed = resolved.clone();
                for key in values.keys() {
                    if resolved.values.get(key) != values.get(key) {
                        changed.kept.remove(key);
                    }
                }
                changed.values = values;
                return Ok(Some(changed));
            }
        }
    }
}

/// A plugin with no `plugin.json`: its rule file itself in the editor (the
/// one `add` writes, when there is none yet). Its options are Lua, written
/// as its README says.
fn edit_rule(config_dir: &Path, name: &str, dir: &Path, edit: Editor<'_>) -> Result<()> {
    let path = rule_path(config_dir, name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        let description = super::fetch::summary(dir).unwrap_or_else(|| "a plugin".to_string());
        super::rule::rule_text(name, &description, &[])
    });
    let saved = edit(&text)?;
    if saved == text && path.exists() {
        println!("{name}: nothing changed");
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, saved).with_context(|| format!("writing {}", path.display()))?;
    println!("saved {}; the rules reload", path.display());
    Ok(())
}

/// `om plugin configure`: with `--set`, those options; without, the form
/// in the editor. Either way the rule file is rewritten and the result
/// shown.
pub async fn configure(
    config_dir: &Path,
    name: &str,
    setup: &Setup,
    edit: Editor<'_>,
) -> Result<()> {
    let dir = config_dir.join("lib").join(name);
    if !dir.join("init.lua").is_file() {
        bail!(
            "no plugin named '{name}' in {}",
            config_dir.join("lib").display()
        );
    }
    let Some(schema) = schema::load(&dir)? else {
        if !setup.sets.is_empty() {
            bail!(
                "{name} has no {}, so --set has nothing to set; its options are in lib/{name}/README.md",
                schema::FILE
            );
        }
        return edit_rule(config_dir, name, &dir, edit);
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
    let current = resolve(name, &schema, &start, &[])?;
    let resolved = if setup.sets.is_empty() {
        match edit_loop(name, &schema, &current, setup, edit)? {
            Some(resolved) => resolved,
            None => {
                println!("{name}: nothing changed");
                return Ok(());
            }
        }
    } else {
        resolve(name, &schema, &start, &setup.sets)?
    };
    write_rule(
        config_dir,
        name,
        &description,
        &to_write(&schema, &resolved),
        true,
        setup.force,
    )
    .with_context(|| format!("saving {name}'s options"))?;
    print!("{}", summary(name, &schema, &resolved, setup));
    Ok(())
}

#[cfg(test)]
#[path = "configure_tests.rs"]
mod tests;
