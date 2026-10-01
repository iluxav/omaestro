//! `om skill`: the omaestro skill for AI coding agents, shipped inside the
//! binary: what the API is, where the files live, how to check a rule.
//! `install` puts it where Claude Code looks; `show` prints it for any
//! other agent or a quick read.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const SKILL: &str = include_str!("../skills/omaestro/SKILL.md");

/// Where Claude Code finds user skills.
fn default_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".claude/skills/omaestro"))
}

pub fn show() {
    print!("{SKILL}");
}

/// Writes `SKILL.md` into `dir` (default: `~/.claude/skills/omaestro`).
pub fn install(dir: Option<&Path>) -> Result<PathBuf> {
    let dir = match dir {
        Some(dir) => dir.to_path_buf(),
        None => default_dir()?,
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, SKILL).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_covers_the_api_and_the_plugins() {
        for name in [
            "om.hotkey",
            "om.app_hotkey",
            "om.on_focus",
            "om.every",
            "om.mode",
            "om.paste",
            "om.llm",
            "om.use",
            "om plugin new",
            "om list",
        ] {
            assert!(SKILL.contains(name), "the skill does not mention {name}");
        }
        for plugin in crate::plugins::builtin::ALL {
            assert!(
                SKILL.contains(plugin.name),
                "the skill does not list {}",
                plugin.name
            );
        }
        assert!(SKILL.starts_with("---\nname: omaestro\n"));
    }

    #[test]
    fn install_writes_the_file() {
        let dir = crate::testutil::TempDir::new("skill");
        let path = install(Some(dir.path())).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), SKILL);
    }
}
