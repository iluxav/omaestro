//! The user's rule files: `init.lua` first, then `rules.d/*.lua` in name order.

use std::fs;
use std::io;
use std::path::Path;

use crate::config::Config;

pub const INIT: &str = "init.lua";
pub const RULES_DIR: &str = "rules.d";

/// One rule file. `name` is relative to the config directory
/// (`rules.d/foo.lua`) and is what error messages show.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub name: String,
    pub code: String,
}

impl Source {
    #[cfg(test)]
    pub fn new(name: &str, code: &str) -> Self {
        Self {
            name: name.to_string(),
            code: code.to_string(),
        }
    }
}

/// Everything a load needs: the rule files and the settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rules {
    pub sources: Vec<Source>,
    pub config: Config,
}

/// Reads the config directory: `omaestro.toml` and the rule files.
pub fn load(dir: &Path) -> Result<Rules, String> {
    Ok(Rules {
        config: Config::load(dir)?,
        sources: read_dir(dir).map_err(|err| format!("reading the rule files: {err}"))?,
    })
}

/// Reads the rule files under `dir`. A missing file or directory is not an
/// error: a daemon with no rules is a valid daemon.
pub fn read_dir(dir: &Path) -> io::Result<Vec<Source>> {
    let mut sources = Vec::new();
    if let Some(code) = read_optional(&dir.join(INIT))? {
        sources.push(Source {
            name: INIT.to_string(),
            code,
        });
    }

    let mut names = Vec::new();
    match fs::read_dir(dir.join(RULES_DIR)) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if name.ends_with(".lua") && !name.starts_with('.') && entry.path().is_file() {
                    names.push(name);
                }
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    names.sort();
    for name in names {
        // A file can vanish between the listing and the read (editors rename).
        if let Some(code) = read_optional(&dir.join(RULES_DIR).join(&name))? {
            sources.push(Source {
                name: format!("{RULES_DIR}/{name}"),
                code,
            });
        }
    }
    Ok(sources)
}

fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(code) => Ok(Some(code)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(io::Error::new(
            err.kind(),
            format!("{}: {err}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn missing_directory_means_no_rules() {
        let tmp = TempDir::new("source-missing");
        assert_eq!(read_dir(&tmp.path().join("nope")).unwrap(), vec![]);
    }

    #[test]
    fn init_first_then_rules_in_name_order() {
        let tmp = TempDir::new("source-order");
        let rules = tmp.path().join(RULES_DIR);
        fs::create_dir(&rules).unwrap();
        fs::write(rules.join("20-b.lua"), "b").unwrap();
        fs::write(rules.join("10-a.lua"), "a").unwrap();
        fs::write(rules.join("notes.txt"), "skipped").unwrap();
        fs::write(rules.join(".hidden.lua"), "skipped").unwrap();
        fs::create_dir(rules.join("dir.lua")).unwrap();
        fs::write(tmp.path().join(INIT), "init").unwrap();

        let names: Vec<_> = read_dir(tmp.path())
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["init.lua", "rules.d/10-a.lua", "rules.d/20-b.lua"]);
    }
}
