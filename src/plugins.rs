//! `om plugin`: the Lua modules under `~/.config/omaestro/lib`, which rules
//! load with `om.use(name)`. A plugin is a directory with an `init.lua` at
//! its root that returns a table: one of the built-in ones (copied out of
//! this binary), or a git repository, where the repository is the package
//! and a tag is a version. Everything here is files and git, no daemon
//! needed: the daemon watches `lib/` and `rules.d/` and reloads by itself.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::process::Command;

use crate::backend::run::find_on_path;

pub mod builtin;

/// `user/repo` is short for a GitHub repository.
pub fn expand_url(url: &str) -> String {
    let trimmed = url.trim();
    let shorthand = !trimmed.contains("://")
        && !trimmed.starts_with("git@")
        && !trimmed.starts_with('/')
        && !trimmed.starts_with('.')
        && trimmed.matches('/').count() == 1
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._/".contains(c));
    if shorthand {
        format!("https://github.com/{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// The directory a repository is installed under: the last path segment
/// of its URL, without `.git`.
pub fn name_from_url(url: &str) -> Result<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next().unwrap_or("");
    let name = last.strip_suffix(".git").unwrap_or(last);
    check_name(name)?;
    Ok(name.to_string())
}

/// A name is a directory name and a `require` argument.
fn check_name(name: &str) -> Result<()> {
    let plain = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if name.is_empty() || name.starts_with('.') || !plain {
        bail!("'{name}' is not a plugin name (letters, digits, '-', '_' and '.')");
    }
    Ok(())
}

/// The Lua variable a rule would hold the plugin in: `om-window-halves`
/// becomes `window_halves`.
fn variable(name: &str) -> String {
    let base = name
        .strip_prefix("om-")
        .or_else(|| name.strip_prefix("om_"))
        .unwrap_or(name);
    let mut var: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if var.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        var.insert(0, '_');
    }
    var
}

fn print_use(name: &str) {
    let var = variable(name);
    println!("load it from a rule:");
    println!("  local {var} = om.use(\"{name}\")");
    println!("  {var}.setup({{}})");
}

/// Runs git and returns its stdout. The error carries git's own message.
async fn git(cwd: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command
        .output()
        .await
        .context("running git (is it installed?)")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        bail!("git {}: {message}", args.first().unwrap_or(&""));
    }
    Ok(stdout)
}

// ---- the rule file that makes a plugin live --------------------------------

fn rule_path(config_dir: &Path, name: &str) -> PathBuf {
    config_dir.join("rules.d").join(format!("{name}.lua"))
}

/// What `om plugin add` writes into rules.d/, so the plugin runs with its
/// defaults right away and the options have a place to go.
fn rule_text(name: &str, description: &str) -> String {
    let var = variable(name);
    format!(
        "-- {name}: {description}\n\
         -- Options and what it does: ~/.config/omaestro/lib/{name}/README.md\n\
         local {var} = om.use(\"{name}\")\n\
         {var}.setup({{}})\n"
    )
}

/// Whether a rule file is still what `rule_text` wrote, untouched.
fn is_generated_rule(name: &str, text: &str) -> bool {
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
fn write_rule(config_dir: &Path, name: &str, description: &str) -> Result<()> {
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
fn drop_rule(config_dir: &Path, name: &str) {
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

// ---- add / available / list / new / update / remove -----------------------

/// Installs a plugin into `lib/<name>`: a built-in one by name, or a
/// repository by URL (`reference` pins a tag or branch). With `rule`, also
/// the rule file that loads it.
pub async fn add(config_dir: &Path, what: &str, reference: Option<&str>, rule: bool) -> Result<()> {
    let lib = config_dir.join("lib");
    let (name, description) = match builtin::find(what.trim()) {
        Some(plugin) => {
            if reference.is_some() {
                bail!("{} is built in; --ref is for repositories", plugin.name);
            }
            let target = lib.join(plugin.name);
            if target.exists() {
                bail!(
                    "{} is already installed at {}",
                    plugin.name,
                    target.display()
                );
            }
            builtin::install(&lib, plugin)?;
            println!(
                "installed {} into {} (built in, omaestro {})",
                plugin.name,
                target.display(),
                env!("CARGO_PKG_VERSION")
            );
            (plugin.name.to_string(), plugin.description.to_string())
        }
        None => {
            let url = expand_url(what);
            let name = name_from_url(&url)?;
            clone(&lib, &url, &name, reference).await?;
            println!("installed {name} into {}", lib.join(&name).display());
            (name, format!("a plugin from {url}"))
        }
    };
    if rule {
        write_rule(config_dir, &name, &description)?;
    } else {
        print_use(&name);
    }
    Ok(())
}

/// Clones a repository into `lib/<name>`.
async fn clone(lib: &Path, url: &str, name: &str, reference: Option<&str>) -> Result<()> {
    let target = lib.join(name);
    if target.exists() {
        bail!(
            "{name} is already installed at {}; `om plugin update {name}` pulls the latest",
            target.display()
        );
    }
    std::fs::create_dir_all(lib).with_context(|| format!("creating {}", lib.display()))?;

    // Cloned next to its place and moved in once it looks like a plugin, so
    // a repository that is not one never lands in lib/.
    let staging = lib.join(format!(".adding-{name}"));
    let _ = std::fs::remove_dir_all(&staging);
    let staging_text = staging.to_string_lossy().to_string();
    let mut args = vec!["clone", "--quiet", "--depth", "1"];
    if let Some(reference) = reference {
        args.extend(["--branch", reference]);
    }
    args.extend([url, staging_text.as_str()]);
    if let Err(err) = git(None, &args).await {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err.context(format!("cloning {url}")));
    }
    if !staging.join("init.lua").is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        bail!("{url} is not an omaestro plugin: no init.lua at its root");
    }
    std::fs::rename(&staging, &target)
        .with_context(|| format!("moving the clone into {}", target.display()))
}

/// The built-in plugins, and which are installed.
pub fn available(config_dir: &Path) -> Result<()> {
    let lib = config_dir.join("lib");
    let width = builtin::ALL.iter().map(|p| p.name.len()).max().unwrap_or(0);
    for plugin in builtin::ALL {
        let state = if lib.join(plugin.name).join("init.lua").is_file() {
            "installed"
        } else {
            ""
        };
        println!(
            "{:width$}  {:9}  {}",
            plugin.name, state, plugin.description
        );
    }
    println!();
    println!(
        "`om plugin add <name>` installs one into {}; the sources are the examples: https://github.com/iluxav/omaestro/tree/main/plugins",
        lib.display()
    );
    Ok(())
}

/// One entry of `lib/`: a directory with an `init.lua`, or a lone `.lua` file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Installed {
    pub name: String,
    /// The file `require` loads.
    pub path: PathBuf,
    /// A tag or commit for a repository, `built-in` for a shipped copy,
    /// `local` for plain files.
    pub version: String,
    /// The repository it came from, if any.
    pub source: Option<String>,
}

pub async fn installed(lib: &Path) -> Result<Vec<Installed>> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(lib) else {
        return Ok(found);
    };
    for entry in entries {
        let path = entry?.path();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            let init = path.join("init.lua");
            if !init.is_file() {
                continue;
            }
            let (version, source) = if path.join(".git").exists() {
                let version = git(Some(&path), &["describe", "--tags", "--always"])
                    .await
                    .unwrap_or_else(|_| "no commits yet".to_string());
                let source = git(Some(&path), &["remote", "get-url", "origin"])
                    .await
                    .ok();
                (version, source)
            } else {
                let version = match builtin::find(&file_name) {
                    Some(plugin) if builtin::unchanged(lib, plugin) => {
                        format!("built-in {}", env!("CARGO_PKG_VERSION"))
                    }
                    Some(_) => "built-in, changed".to_string(),
                    None => "local".to_string(),
                };
                (version, None)
            };
            found.push(Installed {
                name: file_name,
                path: init,
                version,
                source,
            });
        } else if path.extension().is_some_and(|e| e == "lua") {
            found.push(Installed {
                name: file_name.trim_end_matches(".lua").to_string(),
                path,
                version: "local".to_string(),
                source: None,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

pub async fn list(lib: &Path, json: bool) -> Result<()> {
    let plugins = installed(lib).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&plugins)?);
        return Ok(());
    }
    if plugins.is_empty() {
        println!(
            "no plugins in {}; `om plugin available` lists the built-in ones, `om plugin add <name|git-url>` installs, `om plugin new <name>` starts one",
            lib.display()
        );
        return Ok(());
    }
    let name_width = plugins.iter().map(|p| p.name.len()).max().unwrap_or(0);
    let version_width = plugins.iter().map(|p| p.version.len()).max().unwrap_or(0);
    for plugin in plugins {
        let source = plugin.source.as_deref().unwrap_or("-");
        println!(
            "{:name_width$}  {:version_width$}  {}  {source}",
            plugin.name,
            plugin.version,
            plugin.path.display()
        );
    }
    Ok(())
}

/// `lib/<name>` with an `init.lua`, a README and a git repository, the rule
/// that loads it, then the editor: the first line of code a moment away.
pub async fn new(config_dir: &Path, name: &str, edit: bool, rule: bool) -> Result<()> {
    check_name(name)?;
    if builtin::find(name).is_some() {
        bail!("{name} is the name of a built-in plugin; pick another");
    }
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

fn init_template(name: &str) -> String {
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

fn readme_template(name: &str) -> String {
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

/// `git pull` for the plugins that came from a repository (all, or the
/// named one); built-in copies are checked against this binary.
pub async fn update(lib: &Path, name: Option<&str>) -> Result<()> {
    let plugins = installed(lib).await?;
    let chosen: Vec<&Installed> = match name {
        Some(name) => vec![
            plugins
                .iter()
                .find(|p| p.name == name)
                .with_context(|| format!("no plugin named '{name}' in {}", lib.display()))?,
        ],
        None => plugins
            .iter()
            .filter(|p| p.source.is_some() || p.version.starts_with("built-in"))
            .collect(),
    };
    if chosen.is_empty() {
        println!("nothing to update: no plugin here came from a repository or this binary");
        return Ok(());
    }
    let mut failed = false;
    for plugin in chosen {
        let Some(dir) = plugin.path.parent() else {
            continue;
        };
        if plugin.source.is_none() {
            if plugin.version.starts_with("built-in ") {
                println!("{}: up to date ({})", plugin.name, plugin.version);
            } else if plugin.version == "built-in, changed" {
                println!(
                    "{}: differs from the one in this binary (edited, or from an older version); `om plugin remove {0} && om plugin add {0}` for the current one",
                    plugin.name
                );
            } else {
                println!("{}: not from a repository, nothing to pull", plugin.name);
            }
            continue;
        }
        if git(Some(dir), &["symbolic-ref", "-q", "HEAD"])
            .await
            .is_err()
        {
            println!(
                "{}: pinned at {}; remove it and add it again to change that",
                plugin.name, plugin.version
            );
            continue;
        }
        match git(Some(dir), &["pull", "--ff-only", "--quiet"]).await {
            Ok(_) => {
                let version = git(Some(dir), &["describe", "--tags", "--always"])
                    .await
                    .unwrap_or_default();
                if version == plugin.version {
                    println!("{}: up to date ({version})", plugin.name);
                } else {
                    println!("{}: {} -> {version}", plugin.name, plugin.version);
                }
            }
            Err(err) => {
                failed = true;
                eprintln!("{}: {err:#}", plugin.name);
            }
        }
    }
    if failed {
        bail!("some plugins did not update");
    }
    Ok(())
}

/// Deletes `lib/<name>` and the rule file `add` wrote for it. Work that
/// exists nowhere else (uncommitted changes, commits without a remote or
/// not pushed) is refused without `force`.
pub async fn remove(config_dir: &Path, name: &str, force: bool) -> Result<()> {
    check_name(name)?;
    let lib = config_dir.join("lib");
    let dir = lib.join(name);
    let file = lib.join(format!("{name}.lua"));
    let path = if dir.is_dir() {
        dir
    } else if file.is_file() {
        file
    } else {
        bail!("no plugin named '{name}' in {}", lib.display());
    };
    if path.is_dir() && path.join(".git").exists() && !force {
        let changes = git(Some(&path), &["status", "--porcelain"])
            .await
            .unwrap_or_default();
        if !changes.is_empty() {
            bail!(
                "{name} has uncommitted changes; commit and push them, or `om plugin remove {name} --force`"
            );
        }
        let has_commits = git(Some(&path), &["rev-parse", "--verify", "-q", "HEAD"])
            .await
            .is_ok();
        if has_commits {
            if git(Some(&path), &["rev-parse", "--abbrev-ref", "@{upstream}"])
                .await
                .is_err()
            {
                bail!(
                    "{name} has commits but no remote to keep them; push it somewhere, or `om plugin remove {name} --force`"
                );
            }
            let ahead = git(Some(&path), &["log", "--oneline", "@{upstream}..HEAD"])
                .await
                .unwrap_or_default();
            if !ahead.is_empty() {
                bail!(
                    "{name} has commits that are not pushed; push them, or `om plugin remove {name} --force`"
                );
            }
        }
    }
    if path.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    }
    .with_context(|| format!("removing {}", path.display()))?;
    println!("removed {name} ({})", path.display());
    drop_rule(config_dir, name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn urls_and_names() {
        assert_eq!(
            expand_url("iluxav/om-window-halves"),
            "https://github.com/iluxav/om-window-halves"
        );
        for url in [
            "https://github.com/iluxav/om-window-halves.git",
            "git@github.com:iluxav/om-window-halves.git",
            "/home/me/om-window-halves/",
            "../om-window-halves",
        ] {
            assert_eq!(expand_url(url), url, "not shorthand");
            assert_eq!(name_from_url(url).unwrap(), "om-window-halves", "{url}");
        }
        assert!(name_from_url("").is_err());
        assert!(name_from_url("https://x/..").is_err());
        assert!(check_name("../x").is_err());
        assert!(check_name(".hidden").is_err());
        assert_eq!(variable("om-window-halves"), "window_halves");
        assert_eq!(variable("3d.thing"), "_3d_thing");
    }

    #[test]
    fn templates_and_rule_files_name_the_plugin() {
        let init = init_template("om-x");
        assert!(init.contains("local x = om.use(\"om-x\")"), "{init}");
        assert!(init.starts_with("-- om-x:"));
        assert!(init.ends_with("return M\n"));
        let readme = readme_template("om-x");
        assert!(readme.contains("om plugin add https://github.com/<you>/om-x"));

        let rule = rule_text("window-halves", "halves and thirds");
        assert_eq!(
            rule,
            "-- window-halves: halves and thirds\n\
             -- Options and what it does: ~/.config/omaestro/lib/window-halves/README.md\n\
             local window_halves = om.use(\"window-halves\")\n\
             window_halves.setup({})\n"
        );
        assert!(is_generated_rule("window-halves", &rule));
        assert!(!is_generated_rule(
            "window-halves",
            &rule.replace("({})", "({ chord = 'X' })")
        ));
        assert!(!is_generated_rule("other", &rule));
    }

    /// git with an identity, for the repositories the test makes.
    async fn git_in(dir: &Path, args: &[&str]) -> String {
        let mut full = vec![
            "-c",
            "user.name=omaestro",
            "-c",
            "user.email=omaestro@test",
            "-c",
            "init.defaultBranch=main",
        ];
        full.extend(args);
        git(Some(dir), &full).await.unwrap()
    }

    async fn repository(path: &Path, init_lua: Option<&str>) {
        std::fs::create_dir_all(path).unwrap();
        git_in(path, &["init", "--quiet"]).await;
        match init_lua {
            Some(code) => std::fs::write(path.join("init.lua"), code).unwrap(),
            None => std::fs::write(path.join("README.md"), "not a plugin").unwrap(),
        }
        git_in(path, &["add", "-A"]).await;
        git_in(path, &["commit", "--quiet", "-m", "first"]).await;
    }

    #[tokio::test]
    async fn builtin_plugins_install_with_a_rule_and_go_with_it() {
        let tmp = TempDir::new("plugins-builtin");
        let config = tmp.path().join("config");
        add(&config, "panel", None, true).await.unwrap();
        assert!(config.join("lib/panel/init.lua").is_file());
        let rule = config.join("rules.d/panel.lua");
        let text = std::fs::read_to_string(&rule).unwrap();
        assert!(text.contains("om.use(\"panel\")"), "{text}");
        assert!(text.starts_with("-- panel: a hotkey"), "{text}");

        let plugins = installed(&config.join("lib")).await.unwrap();
        assert_eq!(plugins[0].name, "panel");
        assert_eq!(
            plugins[0].version,
            format!("built-in {}", env!("CARGO_PKG_VERSION"))
        );
        let err = add(&config, "panel", None, true)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("already installed"), "{err}");
        assert!(add(&config, "panel", Some("v1"), true).await.is_err());
        assert!(new(&config, "panel", false, false).await.is_err());
        update(&config.join("lib"), Some("panel")).await.unwrap();

        // The user's own rule file is never deleted.
        std::fs::write(
            &rule,
            "om.use(\"panel\").setup({ chord = \"SUPER + ALT + P\" })\n",
        )
        .unwrap();
        remove(&config, "panel", false).await.unwrap();
        assert!(!config.join("lib/panel").exists());
        assert!(rule.is_file(), "an edited rule stays");

        // A generated one goes with the plugin.
        std::fs::remove_file(&rule).unwrap();
        add(&config, "panel", None, true).await.unwrap();
        remove(&config, "panel", false).await.unwrap();
        assert!(!rule.exists());
    }

    #[tokio::test]
    async fn add_list_update_and_remove_against_a_local_repository() {
        if find_on_path("git").is_none() {
            eprintln!("skipped: git is not installed");
            return;
        }
        let tmp = TempDir::new("plugins");
        let config = tmp.path().join("config");
        let lib = config.join("lib");

        // A repository standing in for GitHub.
        let origin = tmp.path().join("om-thing");
        repository(&origin, Some("return { setup = function() end }\n")).await;
        git_in(&origin, &["tag", "v1"]).await;
        let url = origin.to_string_lossy().to_string();

        add(&config, &url, None, false).await.unwrap();
        assert!(lib.join("om-thing/init.lua").is_file());
        assert!(
            !config.join("rules.d/om-thing.lua").exists(),
            "no rule asked for"
        );
        let err = add(&config, &url, None, false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("already installed"), "{err}");

        let plugins = installed(&lib).await.unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "om-thing");
        assert_eq!(plugins[0].version, "v1");
        assert_eq!(plugins[0].source.as_deref(), Some(url.as_str()));

        // A repository without init.lua is not a plugin and leaves nothing behind.
        let other = tmp.path().join("not-a-plugin");
        repository(&other, None).await;
        let err = add(&config, &other.to_string_lossy(), None, true)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no init.lua"), "{err}");
        assert!(!lib.join("not-a-plugin").exists());
        assert!(!lib.join(".adding-not-a-plugin").exists());
        assert!(!config.join("rules.d/not-a-plugin.lua").exists());

        // Upstream moves on; update follows.
        std::fs::write(
            origin.join("init.lua"),
            "return { setup = function() end, v = 2 }\n",
        )
        .unwrap();
        git_in(&origin, &["commit", "--quiet", "-am", "second"]).await;
        git_in(&origin, &["tag", "v2"]).await;
        update(&lib, Some("om-thing")).await.unwrap();
        assert_eq!(installed(&lib).await.unwrap()[0].version, "v2");
        assert!(
            std::fs::read_to_string(lib.join("om-thing/init.lua"))
                .unwrap()
                .contains("v = 2")
        );

        // A plugin of one's own: files, a repository, a rule, and nothing
        // lost by accident.
        new(&config, "mine", false, true).await.unwrap();
        assert!(lib.join("mine/init.lua").is_file());
        assert!(lib.join("mine/README.md").is_file());
        assert!(lib.join("mine/.git").is_dir());
        assert!(config.join("rules.d/mine.lua").is_file());
        let mine = installed(&lib)
            .await
            .unwrap()
            .into_iter()
            .find(|p| p.name == "mine")
            .unwrap();
        assert_eq!(mine.version, "no commits yet");
        assert_eq!(mine.source, None);
        let err = remove(&config, "mine", false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("uncommitted changes"), "{err}");
        git_in(&lib.join("mine"), &["add", "-A"]).await;
        git_in(&lib.join("mine"), &["commit", "--quiet", "-m", "start"]).await;
        let err = remove(&config, "mine", false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("no remote"), "{err}");
        remove(&config, "mine", true).await.unwrap();
        assert!(!lib.join("mine").exists());
        assert!(
            !config.join("rules.d/mine.lua").exists(),
            "the generated rule went too"
        );

        // The cloned one is clean and pushed: it goes without force.
        remove(&config, "om-thing", false).await.unwrap();
        assert!(installed(&lib).await.unwrap().is_empty());
        assert!(remove(&config, "om-thing", false).await.is_err());
    }
}
