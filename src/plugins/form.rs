//! The options form `om plugin configure` opens in your editor: every option
//! of a plugin as `key = value`, with what it is, its type and default in
//! comments above it. Saved and closed, it is read back and checked; a form
//! with problems comes back with them written at the top.

use serde_json::{Map, Value as Json};

use super::schema::{Kind, Opt, Schema};

/// Lines `om` writes into the form to explain a problem start with this;
/// they are dropped before the next round.
const NOTE: &str = "#!";

/// A value as the form shows it: empty for unset, `none` for an optional
/// chord or timer that is off, `yes`/`no` for a switch, modifiers without
/// the trailing `+`.
pub fn form_value(opt: &Opt, value: &Json) -> String {
    match value {
        Json::Null => String::new(),
        Json::Bool(false) if opt.kind != Kind::Bool => "none".to_string(),
        Json::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
        Json::String(text) if opt.kind == Kind::Modifiers => {
            text.trim_end().trim_end_matches('+').trim_end().to_string()
        }
        Json::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Chord => "chord",
        Kind::Modifiers => "modifiers",
        Kind::String => "text",
        Kind::Path => "path",
        Kind::Bool => "yes or no",
        Kind::Number => "number",
        Kind::Interval => "interval like 30s, 5m, 1h30m",
        Kind::Time => "time like 09:30",
        Kind::Enum => "one of",
    }
}

/// The form for `values`. Options in `kept` hold something the form cannot
/// show (Lua code); they are listed as comments and left alone.
pub fn render(
    plugin: &str,
    schema: &Schema,
    values: &Map<String, Json>,
    kept: &Map<String, Json>,
) -> String {
    let mut out = format!(
        "# {plugin}: change a value, then save and close the editor.\n\
         # An empty value means the default; `none` turns an optional one off.\n\
         # To leave everything as it was, close without saving.\n"
    );
    for opt in &schema.options {
        out.push('\n');
        let label = opt.label.as_deref().unwrap_or(&opt.key);
        out.push_str(&format!("# {label}\n"));
        if let Some(description) = &opt.description {
            for line in wrap(description, 74) {
                out.push_str(&format!("# {line}\n"));
            }
        }
        if let Some(reason) = opt.inactive_reason() {
            out.push_str(&format!("# ({reason})\n"));
        }
        if kept.contains_key(&opt.key) {
            out.push_str(&format!(
                "# Set in rules.d/{plugin}.lua as Lua code, which only an editor of that file changes.\n"
            ));
            continue;
        }
        let mut kind = kind_name(opt.kind).to_string();
        if opt.kind == Kind::Enum {
            kind = format!("{kind} {}", opt.options.join(", "));
        }
        if opt.kind == Kind::Modifiers && !opt.keys.is_empty() {
            kind = format!("{kind}, put before {}", opt.keys.join(", "));
        }
        if opt.optional {
            kind.push_str(", optional");
        }
        let default = form_value(opt, &opt.default_value());
        let default = if default.is_empty() {
            "(none)".to_string()
        } else {
            default
        };
        out.push_str(&format!("# {kind}; default: {default}\n"));
        let value = values.get(&opt.key).cloned().unwrap_or(Json::Null);
        out.push_str(&format!("{} = {}\n", opt.key, form_value(opt, &value)));
    }
    out
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The form with `notes` (problems, warnings) at its top, replacing the
/// ones a previous round put there.
pub fn with_notes(text: &str, notes: &[String]) -> String {
    let body: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with(NOTE))
        .collect();
    let mut out = String::new();
    for note in notes {
        out.push_str(&format!("{NOTE} {note}\n"));
    }
    if !notes.is_empty() {
        out.push_str(&format!("{NOTE}\n"));
    }
    out.push_str(&body.join("\n"));
    out.push('\n');
    out
}

/// What came back from the editor.
#[derive(Debug, PartialEq)]
pub enum Edited {
    /// No `key = value` line left: leave everything as it was.
    Cancelled,
    /// Every option's value: what the form said, the default where it said
    /// nothing, and the kept ones as they were.
    Values(Map<String, Json>),
}

/// Reads a saved form. All problems come back together, one per line.
pub fn parse(text: &str, schema: &Schema, kept: &Map<String, Json>) -> Result<Edited, Vec<String>> {
    let mut problems = Vec::new();
    let mut given = Map::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            problems.push(format!(
                "line {}: `{line}` is not `key = value`",
                number + 1
            ));
            continue;
        };
        let key = key.trim();
        let Some(opt) = schema.get(key) else {
            problems.push(format!("line {}: there is no option `{key}`", number + 1));
            continue;
        };
        if given.contains_key(key) {
            problems.push(format!("{key}: given twice"));
            continue;
        }
        let value = value.trim();
        let parsed = if value.is_empty() {
            Ok(opt.default_value())
        } else {
            opt.parse(value)
        };
        match parsed {
            Ok(value) => {
                given.insert(key.to_string(), value);
            }
            Err(err) => problems.push(format!("{key}: {err}")),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    if given.is_empty() {
        return Ok(Edited::Cancelled);
    }
    let mut values = Map::new();
    for opt in &schema.options {
        let value = given
            .get(&opt.key)
            .or_else(|| kept.get(&opt.key))
            .cloned()
            .unwrap_or_else(|| opt.default_value());
        values.insert(opt.key.clone(), value);
    }
    Ok(Edited::Values(values))
}
