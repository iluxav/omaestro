//! Where things live, and `omaestro.toml`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::backend::run;
use crate::chord::Chord;

pub const CONFIG_FILE: &str = "omaestro.toml";

/// `$XDG_CONFIG_HOME/omaestro`, falling back to `~/.config/omaestro`.
pub fn default_config_dir() -> Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env::var_os("HOME").filter(|v| !v.is_empty()).context(
            "neither XDG_CONFIG_HOME nor HOME is set, cannot find the config directory",
        )?)
        .join(".config"),
    };
    Ok(base.join("omaestro"))
}

/// `$XDG_STATE_HOME/omaestro`, falling back to `~/.local/state/omaestro`:
/// where `om.store` keeps its file.
pub fn default_state_dir() -> Result<PathBuf> {
    let base =
        match env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(env::var_os("HOME").filter(|v| !v.is_empty()).context(
                "neither XDG_STATE_HOME nor HOME is set, cannot find the state directory",
            )?)
            .join(".local")
            .join("state"),
        };
    Ok(base.join("omaestro"))
}

/// `$XDG_RUNTIME_DIR/omaestro.sock`.
pub fn default_socket() -> Result<PathBuf> {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .context("XDG_RUNTIME_DIR is unset, cannot find the control socket (pass --socket)")?;
    Ok(PathBuf::from(dir).join("omaestro.sock"))
}

/// `omaestro.toml`. Every key is optional.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub model: Model,
    pub paste: Paste,
    /// The dmenu-style command `om.prompt` runs; `{label}` is replaced by
    /// the prompt text, quoted. Unset: the first known tool on PATH.
    pub prompt_command: Option<String>,
    /// The command `om.choose` runs. `{label}` as above; with `{options}`
    /// the choices are passed as arguments, without it they go to stdin one
    /// per line. Unset: the first known tool on PATH.
    pub choose_command: Option<String>,
}

/// Chooser tools, in order of preference.
const CHOOSE_TOOLS: [(&str, &str); 5] = [
    (
        "omarchy-menu-select",
        "omarchy-menu-select {label} {options}",
    ),
    ("walker", "walker --dmenu -p {label}"),
    ("wofi", "wofi --dmenu -p {label}"),
    ("fuzzel", "fuzzel --dmenu --prompt {label}"),
    ("rofi", "rofi -dmenu -p {label}"),
];

/// Prompt tools, in order of preference, with the command that shows a
/// label and prints the typed line.
const PROMPT_TOOLS: [(&str, &str); 5] = [
    ("omarchy-menu-input", "omarchy-menu-input {label}"),
    ("walker", "walker --dmenu -p {label}"),
    ("wofi", "wofi --dmenu -p {label}"),
    ("fuzzel", "fuzzel --dmenu --prompt {label}"),
    ("rofi", "rofi -dmenu -p {label}"),
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Model {
    /// Chat endpoint: Ollama's `/api/chat` or an OpenAI-compatible
    /// `/v1/chat/completions`.
    pub endpoint: String,
    pub name: String,
    /// Name of the environment variable holding the API key, if the
    /// endpoint wants one. The key itself never goes in the file.
    pub api_key_env: Option<String>,
    pub timeout_secs: u64,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434/api/chat".to_string(),
            name: "llama3.2".to_string(),
            api_key_env: None,
            timeout_secs: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paste {
    /// The chord that pastes in most apps.
    pub chord: String,
    /// How long the pasted text stays in the clipboard before the previous
    /// content is put back.
    pub restore_ms: u64,
    /// Chords for apps that paste differently, by window class. Entries here
    /// add to the built-in terminal list.
    pub apps: BTreeMap<String, String>,
}

/// Terminals paste with ctrl+shift+v.
const TERMINALS: [&str; 6] = [
    "Alacritty",
    "kitty",
    "foot",
    "footclient",
    "com.mitchellh.ghostty",
    "org.wezfurlong.wezterm",
];
const TERMINAL_PASTE: &str = "ctrl+shift+v";

impl Default for Paste {
    fn default() -> Self {
        Self {
            chord: "ctrl+v".to_string(),
            restore_ms: 300,
            apps: BTreeMap::new(),
        }
    }
}

impl Paste {
    /// The paste chord for a window of class `class` (compared without
    /// regard to case), or the default one.
    pub fn chord_for(&self, class: Option<&str>) -> Result<Chord, String> {
        let text = class
            .and_then(|class| {
                self.apps
                    .iter()
                    .find(|(app, _)| app.eq_ignore_ascii_case(class))
                    .map(|(_, chord)| chord.as_str())
                    .or_else(|| {
                        TERMINALS
                            .iter()
                            .any(|terminal| terminal.eq_ignore_ascii_case(class))
                            .then_some(TERMINAL_PASTE)
                    })
            })
            .unwrap_or(&self.chord);
        Chord::parse(text)
    }
}

impl Config {
    /// The command template `om.prompt` runs.
    pub fn prompt_command(&self) -> Result<String, String> {
        if let Some(command) = &self.prompt_command {
            return Ok(command.clone());
        }
        PROMPT_TOOLS
            .iter()
            .find(|(tool, _)| run::find_on_path(tool).is_some())
            .map(|(_, command)| command.to_string())
            .ok_or_else(|| {
                "no prompt tool found: install walker or wofi, or set prompt_command in omaestro.toml"
                    .to_string()
            })
    }

    /// The command template `om.choose` runs.
    pub fn choose_command(&self) -> Result<String, String> {
        if let Some(command) = &self.choose_command {
            return Ok(command.clone());
        }
        CHOOSE_TOOLS
            .iter()
            .find(|(tool, _)| run::find_on_path(tool).is_some())
            .map(|(_, command)| command.to_string())
            .ok_or_else(|| {
                "no chooser tool found: install walker or wofi, or set choose_command in omaestro.toml"
                    .to_string()
            })
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Config =
            toml::from_str(text).map_err(|err| err.to_string().trim_end().to_string())?;
        // A chord that cannot be parsed should fail the load, not the first paste.
        for chord in std::iter::once(&config.paste.chord).chain(config.paste.apps.values()) {
            Chord::parse(chord).map_err(|err| format!("[paste]: {err}"))?;
        }
        Ok(config)
    }

    /// Reads `omaestro.toml` from the config directory. No file means defaults.
    pub fn load(dir: &Path) -> Result<Self, String> {
        match fs::read_to_string(dir.join(CONFIG_FILE)) {
            Ok(text) => Self::parse(&text).map_err(|err| format!("{CONFIG_FILE}: {err}")),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(format!("{CONFIG_FILE}: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn defaults_without_a_file() {
        let tmp = TempDir::new("config-defaults");
        let config = Config::load(tmp.path()).unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.model.endpoint, "http://127.0.0.1:11434/api/chat");
        assert_eq!(config.model.name, "llama3.2");
        assert_eq!(config.model.timeout_secs, 60);
        assert_eq!(config.paste.restore_ms, 300);
    }

    #[test]
    fn partial_file_keeps_the_other_defaults() {
        let config = Config::parse("[model]\nname = \"qwen3\"\n").unwrap();
        assert_eq!(config.model.name, "qwen3");
        assert_eq!(config.model.endpoint, "http://127.0.0.1:11434/api/chat");
        assert_eq!(config.paste, Paste::default());
    }

    #[test]
    fn mistakes_fail_the_load() {
        let tmp = TempDir::new("config-mistakes");
        fs::write(tmp.path().join(CONFIG_FILE), "[model]\nnmae = \"typo\"\n").unwrap();
        let err = Config::load(tmp.path()).unwrap_err();
        assert!(err.starts_with("omaestro.toml: "), "{err}");
        assert!(err.contains("nmae"), "{err}");

        let err = Config::parse("[paste]\nchord = \"hyper+v\"\n").unwrap_err();
        assert_eq!(err, "[paste]: unknown modifier 'hyper' in 'hyper+v'");
    }

    #[test]
    fn prompt_command_from_the_file_wins() {
        let config = Config::parse("prompt_command = \"mymenu --ask {label}\"\n").unwrap();
        assert_eq!(config.prompt_command().unwrap(), "mymenu --ask {label}");
    }

    #[test]
    fn paste_chord_by_window_class() {
        let config =
            Config::parse("[paste.apps]\nEmacs = \"ctrl+y\"\nkitty = \"ctrl+v\"\n").unwrap();
        let chord = |class| config.paste.chord_for(class).unwrap().id();
        assert_eq!(chord(None), "CTRL+V");
        assert_eq!(chord(Some("firefox")), "CTRL+V");
        assert_eq!(chord(Some("Alacritty")), "CTRL+SHIFT+V");
        assert_eq!(chord(Some("alacritty")), "CTRL+SHIFT+V");
        assert_eq!(chord(Some("emacs")), "CTRL+Y");
        // The user's table wins over the built-in terminal list.
        assert_eq!(chord(Some("kitty")), "CTRL+V");
    }
}
