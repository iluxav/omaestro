//! What the user set from the panel or the command line, kept in
//! `<state>/settings.json` so it outlives reloads and restarts: the
//! triggers switched off, and whether rules may take chords that Hyprland
//! already has.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "settings.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    disabled: BTreeSet<String>,
    /// `om override on`: a rule's chord displaces a bind that is not ours.
    #[serde(default, rename = "override")]
    override_binds: bool,
}

pub struct Settings {
    path: PathBuf,
    stored: Stored,
}

impl Settings {
    /// Reads the file. No file means the defaults. A file that cannot be
    /// read counts as the defaults and the problem comes back for reporting.
    pub fn load(state_dir: &Path) -> (Self, Option<String>) {
        let path = state_dir.join(FILE);
        let (stored, problem) = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Stored>(&text) {
                Ok(stored) => (stored, None),
                Err(err) => (
                    Stored::default(),
                    Some(format!(
                        "{}: {err} (every rule is enabled, override is off)",
                        path.display()
                    )),
                ),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Stored::default(), None),
            Err(err) => (
                Stored::default(),
                Some(format!("cannot read {}: {err}", path.display())),
            ),
        };
        (Self { path, stored }, problem)
    }

    /// The trigger ids switched off. Kept whether or not a trigger with the
    /// id is registered right now: the rule may be back after a reload.
    pub fn disabled(&self) -> &BTreeSet<String> {
        &self.stored.disabled
    }

    /// Switches an id on or off and saves. The value is updated even when
    /// the save fails: the choice then holds until the daemon restarts.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let changed = if enabled {
            self.stored.disabled.remove(id)
        } else {
            self.stored.disabled.insert(id.to_string())
        };
        if !changed {
            return Ok(());
        }
        self.save()
    }

    pub fn override_binds(&self) -> bool {
        self.stored.override_binds
    }

    pub fn set_override(&mut self, on: bool) -> Result<(), String> {
        if self.stored.override_binds == on {
            return Ok(());
        }
        self.stored.override_binds = on;
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        self.write()
            .map_err(|err| format!("cannot save {}: {err}", self.path.display()))
    }

    fn write(&self) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(&self.stored)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Written whole, then moved into place: a crash mid-write cannot
        // leave a half file behind.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn round_trips_through_the_file() {
        let dir = TempDir::new("settings");
        let (mut settings, problem) = Settings::load(dir.path());
        assert_eq!(problem, None);
        assert!(settings.disabled().is_empty());
        assert!(!settings.override_binds());

        settings.set_enabled("hotkey:SUPER+J", false).unwrap();
        settings.set_enabled("every:5m", false).unwrap();
        settings.set_enabled("hotkey:SUPER+J", true).unwrap();
        // Switching something off twice changes nothing and writes nothing.
        settings.set_enabled("every:5m", false).unwrap();
        settings.set_override(true).unwrap();

        let (again, problem) = Settings::load(dir.path());
        assert_eq!(problem, None);
        assert_eq!(again.disabled().iter().collect::<Vec<_>>(), ["every:5m"]);
        assert!(again.override_binds());
        assert!(!dir.path().join("settings.json.tmp").exists());
    }

    #[test]
    fn a_broken_file_is_reported_and_ignored() {
        let dir = TempDir::new("settings-broken");
        std::fs::write(dir.path().join(FILE), "{not json").unwrap();
        let (settings, problem) = Settings::load(dir.path());
        assert!(settings.disabled().is_empty());
        assert!(!settings.override_binds());
        let problem = problem.unwrap();
        assert!(
            problem.ends_with("(every rule is enabled, override is off)"),
            "{problem}"
        );
        assert!(problem.contains("settings.json"), "{problem}");
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let dir = TempDir::new("settings-partial");
        std::fs::write(dir.path().join(FILE), r#"{"override": true}"#).unwrap();
        let (settings, problem) = Settings::load(dir.path());
        assert_eq!(problem, None);
        assert!(settings.disabled().is_empty());
        assert!(settings.override_binds());
    }
}
