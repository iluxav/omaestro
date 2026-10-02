//! `plugin.json`: the options a plugin's `setup(opts)` takes, so `om` can
//! ask for them (`om plugin add`, `om plugin configure`) and write them into
//! the plugin's rule file instead of anyone editing Lua.
//!
//! ```json
//! { "options": [
//!   { "key": "chord", "type": "chord", "label": "Open the menu", "default": "SUPER + ALT + P" },
//!   { "key": "toggle", "type": "chord", "default": "SUPER + ALT + C", "optional": true },
//!   { "key": "prefix", "type": "modifiers", "keys": ["Left", "Right"], "default": "CTRL + ALT + " },
//!   { "key": "language", "type": "string", "default": "English" },
//!   { "key": "keep", "type": "number", "default": 10 },
//!   { "key": "sound", "type": "bool", "default": true },
//!   { "key": "every", "type": "interval", "default": "45m" },
//!   { "key": "at", "type": "time", "default": "17:30" },
//!   { "key": "place", "type": "enum", "options": ["left", "right"], "default": "left" },
//!   { "key": "file", "type": "path", "default": "~/notes/clips.md" }
//! ] }
//! ```
//!
//! `optional` lets the answer be `none`: `false` for a chord, modifiers, a
//! number, an interval or a time (the plugin switches that part off), and
//! no value at all for a string, a path or an enum (the plugin's default).

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::chord::Chord;
use crate::runtime::timers::{parse_clock, parse_interval};

pub const FILE: &str = "plugin.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Chord,
    Modifiers,
    String,
    Path,
    Bool,
    Number,
    Interval,
    Time,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Opt {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<Json>,
    #[serde(default)]
    pub optional: bool,
    /// The choices of an `enum`.
    #[serde(default)]
    pub options: Vec<String>,
    /// For `modifiers`: the keys they go in front of, for checking chords.
    #[serde(default)]
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Schema {
    #[serde(default)]
    pub options: Vec<Opt>,
}

impl Opt {
    /// What a prompt calls it.
    pub fn title(&self) -> String {
        match &self.label {
            Some(label) => format!("{label} ({})", self.key),
            None => self.key.clone(),
        }
    }

    /// The default, or "unset" when there is none.
    pub fn default_value(&self) -> Json {
        self.default.clone().unwrap_or(Json::Null)
    }

    /// What `none` means for this option.
    fn none_value(&self) -> Json {
        match self.kind {
            Kind::String | Kind::Path | Kind::Enum => Json::Null,
            _ => Json::Bool(false),
        }
    }

    /// Reads an answer typed for this option.
    pub fn parse(&self, text: &str) -> Result<Json, String> {
        let text = text.trim();
        if self.optional && matches!(text, "none" | "-" | "off") {
            return Ok(self.none_value());
        }
        match self.kind {
            Kind::Chord => {
                Chord::parse(text).map_err(|err| format!("not a chord: {err}"))?;
                Ok(Json::String(normalize_chord(text)))
            }
            Kind::Modifiers => {
                let prefix = normalize_modifiers(text);
                for key in self.keys.iter().take(1) {
                    Chord::parse(&format!("{prefix}{key}"))
                        .map_err(|err| format!("not modifiers: {err}"))?;
                }
                if prefix.is_empty() {
                    return Err("give at least one modifier, like CTRL + ALT".to_string());
                }
                Ok(Json::String(prefix))
            }
            Kind::String | Kind::Path => {
                if text.is_empty() {
                    return Err("empty".to_string());
                }
                Ok(Json::String(text.to_string()))
            }
            Kind::Bool => match text.to_ascii_lowercase().as_str() {
                "y" | "yes" | "true" | "on" | "1" => Ok(Json::Bool(true)),
                "n" | "no" | "false" | "0" => Ok(Json::Bool(false)),
                _ => Err("yes or no".to_string()),
            },
            Kind::Number => {
                if let Ok(int) = text.parse::<i64>() {
                    return Ok(Json::from(int));
                }
                text.parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(Json::Number)
                    .ok_or_else(|| "a number".to_string())
            }
            Kind::Interval => {
                parse_interval(text)?;
                Ok(Json::String(text.to_string()))
            }
            Kind::Time => {
                parse_clock(text)?;
                Ok(Json::String(text.to_string()))
            }
            Kind::Enum => {
                if self.options.iter().any(|o| o == text) {
                    Ok(Json::String(text.to_string()))
                } else {
                    Err(format!("one of: {}", self.options.join(", ")))
                }
            }
        }
    }

    /// Whether a value (from the default or a rule file) fits this option.
    fn accepts(&self, value: &Json) -> bool {
        match value {
            Json::Null => true,
            Json::Bool(false) if self.optional => true,
            Json::Bool(_) => self.kind == Kind::Bool,
            Json::Number(_) => self.kind == Kind::Number,
            Json::String(text) => {
                self.kind != Kind::Bool && self.kind != Kind::Number && self.parse(text).is_ok()
            }
            _ => false,
        }
    }

    /// The chords a value binds, for checking them against what is taken.
    pub fn chords(&self, value: &Json) -> Vec<Chord> {
        let Json::String(text) = value else {
            return Vec::new();
        };
        match self.kind {
            Kind::Chord => Chord::parse(text).into_iter().collect(),
            Kind::Modifiers => self
                .keys
                .iter()
                .filter_map(|key| Chord::parse(&format!("{text}{key}")).ok())
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// `super+alt+j` → `SUPER + ALT + J`, the way the plugins' defaults read.
fn normalize_chord(text: &str) -> String {
    match Chord::parse(text) {
        Ok(chord) => chord.hyprland(),
        Err(_) => text.to_string(),
    }
}

/// `ctrl alt`, `CTRL+ALT`, `CTRL + ALT + ` → `CTRL + ALT + `, ready for a key.
fn normalize_modifiers(text: &str) -> String {
    let mods: Vec<String> = text
        .split(|c: char| c == '+' || c.is_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_uppercase)
        .collect();
    if mods.is_empty() {
        return String::new();
    }
    format!("{} + ", mods.join(" + "))
}

/// A value as a prompt shows it: `no` for a switch that is off, `none`
/// for an optional chord or timer turned off.
pub fn display(opt: &Opt, value: &Json) -> String {
    match value {
        Json::Null => "(unset)".to_string(),
        Json::Bool(false) if opt.kind == Kind::Bool => "no".to_string(),
        Json::Bool(false) => "none".to_string(),
        Json::Bool(true) => "yes".to_string(),
        Json::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// A JSON value as a Lua literal for the rule file.
pub fn lua_literal(value: &Json) -> String {
    match value {
        Json::Null => "nil".to_string(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => n.to_string(),
        Json::String(text) => {
            let mut out = String::from("\"");
            for c in text.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    c if c.is_control() => out.push_str(&format!("\\{}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Json::Array(items) => {
            let inner: Vec<String> = items.iter().map(lua_literal).collect();
            format!("{{ {} }}", inner.join(", "))
        }
        Json::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} = {}", lua_key(k), lua_literal(v)))
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

/// `chord` stays `chord`; anything that is not a Lua name becomes `["x-y"]`.
pub fn lua_key(key: &str) -> String {
    let identifier = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if identifier {
        key.to_string()
    } else {
        format!("[{}]", lua_literal(&Json::String(key.to_string())))
    }
}

/// Reads `plugin.json` from a plugin directory: none when the plugin has
/// none; an error naming the problem when it is there but wrong.
pub fn load(dir: &Path) -> Result<Option<Schema>> {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    let schema: Schema =
        serde_json::from_str(&text).with_context(|| format!("{}: not valid", path.display()))?;
    let mut seen = std::collections::HashSet::new();
    for opt in &schema.options {
        if !seen.insert(opt.key.clone()) {
            bail!("{}: option {} is there twice", path.display(), opt.key);
        }
        if opt.kind == Kind::Enum && opt.options.is_empty() {
            bail!("{}: {} is an enum without options", path.display(), opt.key);
        }
        if opt.kind == Kind::Modifiers && opt.keys.is_empty() {
            bail!(
                "{}: {} is modifiers without keys to put them in front of",
                path.display(),
                opt.key
            );
        }
        if let Some(default) = &opt.default
            && !opt.accepts(default)
        {
            bail!(
                "{}: the default of {} ({default}) does not fit its type",
                path.display(),
                opt.key
            );
        }
    }
    Ok(Some(schema))
}

impl Schema {
    pub fn get(&self, key: &str) -> Option<&Opt> {
        self.options.iter().find(|opt| opt.key == key)
    }

    /// Whether a value read back from a rule file fits the option it is for.
    pub fn accepts(&self, key: &str, value: &Json) -> bool {
        self.get(key).is_some_and(|opt| opt.accepts(value))
    }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
