//! `om plugin new`: a plugin of your own, ready to edit and to publish.

use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use super::source::check_name;
use super::{git, print_use, variable, write_rule};
use crate::backend::run::find_on_path;

/// `lib/<name>` with an `init.lua`, a README and a git repository, the rule
/// that loads it, then the editor: the first line of code a moment away.
pub async fn new(config_dir: &Path, name: &str, edit: bool, rule: bool) -> Result<()> {
    check_name(name)?;
    let dir = config_dir.join("lib").join(name);
    if dir.exists() {
        bail!("{} exists already", dir.display());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(dir.join("init.lua"), init_template(name))?;
    std::fs::write(dir.join("README.md"), readme_template(name))?;
    if let Err(err) = git(Some(&dir), &["init", "--quiet"]).await {
        eprintln!("om: {err:#}; the files are there, without a git repository");
    }
    println!("created {}", dir.display());
    if rule {
        write_rule(config_dir, name, "a plugin of your own")?;
    } else {
        print_use(name);
    }
    if edit {
        open_editor(&dir.join("init.lua")).await?;
    }
    Ok(())
}

pub fn init_template(name: &str) -> String {
    let var = variable(name);
    format!(
        "-- {name}: an omaestro plugin. A rule loads it with\n\
         --\n\
         --   local {var} = om.use(\"{name}\")\n\
         --   {var}.setup({{ chord = \"SUPER + ALT + X\" }})\n\
         --\n\
         -- and gets the table this file returns. Everything under om.* works here.\n\
         \n\
         local M = {{}}\n\
         \n\
         function M.setup(opts)\n\
         \x20 opts = opts or {{}}\n\
         \x20 om.hotkey(opts.chord or \"SUPER + ALT + X\", function()\n\
         \x20   om.notify(\"{name}\", \"hello from {name}\")\n\
         \x20 end)\n\
         \x20 return M\n\
         end\n\
         \n\
         return M\n"
    )
}

pub fn readme_template(name: &str) -> String {
    let var = variable(name);
    format!(
        "# {name}\n\
         \n\
         An omaestro plugin. What it does: say it here.\n\
         \n\
         ## Install\n\
         \n\
         ```sh\n\
         om plugin add https://github.com/<you>/{name}\n\
         ```\n\
         \n\
         Then, in a rule (`~/.config/omaestro/rules.d/{name}.lua`):\n\
         \n\
         ```lua\n\
         local {var} = om.use(\"{name}\")\n\
         {var}.setup({{ chord = \"SUPER + ALT + X\" }})\n\
         ```\n\
         \n\
         ## Options\n\
         \n\
         - `chord`: the hotkey (default `SUPER + ALT + X`).\n"
    )
}

/// `$VISUAL`, `$EDITOR`, or the first editor found on PATH, run in the
/// terminal until it exits.
async fn open_editor(path: &Path) -> Result<()> {
    let from_env = ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|e| !e.trim().is_empty()));
    let editor = from_env.or_else(|| {
        ["nvim", "vim", "vi", "nano"]
            .iter()
            .find(|e| find_on_path(e).is_some())
            .map(|e| e.to_string())
    });
    let Some(editor) = editor else {
        println!(
            "no editor found ($VISUAL, $EDITOR, nvim, vim, vi, nano); open {} yourself",
            path.display()
        );
        return Ok(());
    };
    // `$EDITOR` may carry arguments, `code --wait`.
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let status = Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .await
        .with_context(|| format!("running {editor}"))?;
    if !status.success() {
        bail!("{editor} exited with {status}");
    }
    Ok(())
}
