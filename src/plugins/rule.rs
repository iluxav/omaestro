//! The rule file `om plugin add` writes into `rules.d/`, so a plugin runs
//! with its defaults at once and its options have a place to go; and its
//! removal with the plugin, when it is still untouched.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::variable;

pub(super) fn rule_path(config_dir: &Path, name: &str) -> PathBuf {
    config_dir.join("rules.d").join(format!("{name}.lua"))
}

/// What `om plugin add` writes into rules.d/, so the plugin runs with its
/// defaults right away and the options have a place to go.
pub(super) fn rule_text(name: &str, description: &str) -> String {
    let var = variable(name);
    format!(
        "-- {name}: {description}\n\
         -- Options and what it does: ~/.config/omaestro/lib/{name}/README.md\n\
         local {var} = om.use(\"{name}\")\n\
         {var}.setup({{}})\n"
    )
}

/// Whether a rule file is still what `rule_text` wrote, untouched.
pub(super) fn is_generated_rule(name: &str, text: &str) -> bool {
    let var = variable(name);
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == 4
        && lines[0].starts_with(&format!("-- {name}: "))
        && lines[1]
            == format!("-- Options and what it does: ~/.config/omaestro/lib/{name}/README.md")
        && lines[2] == format!("local {var} = om.use(\"{name}\")")
        && lines[3] == format!("{var}.setup({{}})")
}

/// Writes rules.d/<name>.lua unless there is one already.
pub(super) fn write_rule(config_dir: &Path, name: &str, description: &str) -> Result<()> {
    let path = rule_path(config_dir, name);
    if path.exists() {
        println!("{} exists already; left alone", path.display());
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&path, rule_text(name, description))
        .with_context(|| format!("writing {}", path.display()))?;
    println!(
        "wrote {}: the plugin is live with its defaults; its options go there",
        path.display()
    );
    Ok(())
}

/// After a plugin is gone: its generated rule goes too; a rule the user
/// changed stays, with a warning.
pub(super) fn drop_rule(config_dir: &Path, name: &str) {
    let path = rule_path(config_dir, name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if is_generated_rule(name, &text) {
        match std::fs::remove_file(&path) {
            Ok(()) => println!("removed {}, which only loaded it", path.display()),
            Err(err) => eprintln!("om: could not remove {}: {err}", path.display()),
        }
    } else if text.contains(&format!("om.use(\"{name}\")"))
        || text.contains(&format!("om.use('{name}')"))
    {
        println!(
            "{} still loads it: the rules will not load until you change that",
            path.display()
        );
    }
}
