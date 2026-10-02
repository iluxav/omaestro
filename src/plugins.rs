//! `om plugin`: the Lua modules under `~/.config/omaestro/lib`, which rules
//! load with `om.use(name)`. A plugin is a directory with an `init.lua` that
//! returns a table. Every plugin is installed the same way, omaestro's own
//! included: from a git repository (its root, or a directory in it) or from
//! a directory on disk, copied into `lib/<name>` with a record of where it
//! came from (`fetch::Record`). Plugins you write yourself (`om plugin new`)
//! are git working copies in `lib/`. Everything here is files and git, no
//! daemon needed: the daemon watches `lib/` and `rules.d/` and reloads.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::process::Command;

mod check;
mod configure;
pub mod fetch;
mod form;
mod rule;
mod scaffold;
pub mod schema;
pub mod source;
#[cfg(test)]
mod tests;
mod update;

pub use configure::{Setup, configure, edit_in_editor};
use fetch::Record;
use rule::{drop_rule, write_rule};
pub use scaffold::new;
use source::{Official, check_name};
pub use update::update;

/// The rule file `om plugin add` writes, for the runtime's tests.
#[cfg(test)]
pub fn rule_text_for_tests(
    name: &str,
    description: &str,
    values: &[(String, serde_json::Value)],
) -> String {
    rule::rule_text(name, description, values)
}

/// The plugins of omaestro's own repository that a fresh install offers in
/// one click (see `welcome`).
pub const STARTERS: [&str; 2] = ["window-halves", "text-tools"];

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
    // A mistyped or private repository must fail, not ask for a password.
    command.env("GIT_TERMINAL_PROMPT", "0");
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

// ---- add / available / list / update / remove ----------------------------

/// Installs plugins: for each spec, fetch it, check its shape, copy it into
/// `lib/<name>`, and (with `rule`) write the rule file that loads it. One
/// that fails does not stop the others; the error lists every failure.
pub async fn add(
    config_dir: &Path,
    specs: &[String],
    reference: Option<&str>,
    path: Option<&str>,
    rule: bool,
    official: &Official,
    setup: &Setup,
) -> Result<()> {
    if specs.len() > 1 && (reference.is_some() || path.is_some() || !setup.sets.is_empty()) {
        bail!("--ref, --path and --set go with one plugin at a time");
    }
    if !rule && !setup.sets.is_empty() {
        bail!("--set writes the rule file, which --no-rule leaves out");
    }
    let mut failed = Vec::new();
    for spec in specs {
        if let Err(err) = add_one(config_dir, spec, reference, path, rule, official, setup).await {
            eprintln!("om: {spec}: {err:#}");
            failed.push(spec.clone());
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        bail!("not installed: {}", failed.join(", "))
    }
}

async fn add_one(
    config_dir: &Path,
    spec: &str,
    reference: Option<&str>,
    path: Option<&str>,
    rule: bool,
    official: &Official,
    setup: &Setup,
) -> Result<()> {
    let lib = config_dir.join("lib");
    let source = source::parse(spec, reference, path, official)?;
    let name = source.name()?;
    let target = lib.join(&name);
    if target.exists() {
        bail!(
            "{name} is already installed at {}; `om plugin update {name}` takes the latest",
            target.display()
        );
    }
    let fetched = fetch::fetch(&lib, &source, official).await?;
    if rule && !fetched.shape.setup {
        bail!(
            "{name}'s init.lua returns no table with a setup function, which the rule om writes calls; \
             `om plugin add {spec} --no-rule` installs it to load your own way"
        );
    }
    let description = fetch::summary(&fetched.dir)
        .unwrap_or_else(|| format!("a plugin from {}", source.describe()));
    // Its options: the defaults, with --set on top. A bad --set stops here,
    // before anything is installed.
    let options = if rule {
        match schema::load(&fetched.dir) {
            Ok(Some(schema)) => {
                let resolved =
                    configure::resolve(&name, &schema, &Default::default(), &setup.sets)?;
                Some((schema, resolved))
            }
            Ok(None) => {
                if !setup.sets.is_empty() {
                    bail!(
                        "{name} has no {}, so --set has nothing to set",
                        schema::FILE
                    );
                }
                None
            }
            Err(err) => {
                eprintln!("om: {name}: {err:#}; installing it with its defaults");
                None
            }
        }
    } else {
        None
    };
    let installed = fetch::install(&lib, &name, &fetched)?;
    println!(
        "installed {name} {} into {}",
        fetched.record.version_label(),
        installed.display()
    );
    if rule {
        let values = options
            .as_ref()
            .map(|(schema, resolved)| configure::to_write(schema, resolved))
            .unwrap_or_default();
        write_rule(config_dir, &name, &description, &values, false, false)?;
        if let Some((schema, resolved)) = &options {
            print!("{}", configure::summary(&name, schema, resolved, setup));
        }
    } else {
        print_use(&name);
    }
    Ok(())
}

/// The plugins in omaestro's own repository, fetched fresh, and which of
/// them are installed.
pub async fn available(config_dir: &Path, official: &Official) -> Result<()> {
    let lib = config_dir.join("lib");
    std::fs::create_dir_all(&lib).with_context(|| format!("creating {}", lib.display()))?;
    let staging = lib.join(format!(".available-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let staging_text = staging.to_string_lossy().to_string();
    let cloned = git(
        None,
        &[
            "clone",
            "--quiet",
            "--depth",
            "1",
            &official.repo,
            &staging_text,
        ],
    )
    .await;
    let result = (|| -> Result<Vec<(String, String)>> {
        cloned.with_context(|| {
            format!(
                "could not reach {} (offline?); the README there lists the plugins too",
                official.repo
            )
        })?;
        let dir = staging.join(&official.dir);
        let mut found = Vec::new();
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let path = entry?.path();
            if !path.join("init.lua").is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let summary = fetch::summary(&path).unwrap_or_default();
            found.push((name, summary));
        }
        found.sort();
        Ok(found)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    let found = result?;
    let width = found.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    for (name, summary) in &found {
        let state = if lib.join(name).join("init.lua").is_file() {
            "installed"
        } else {
            ""
        };
        println!("{name:width$}  {state:9}  {summary}");
    }
    println!();
    println!(
        "`om plugin add <name>` installs one (several at once: om plugin add {}); from {}",
        STARTERS.join(" "),
        official.repo
    );
    Ok(())
}

/// One entry of `lib/`: a directory with an `init.lua`, or a lone `.lua` file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Installed {
    pub name: String,
    /// The file `require` loads.
    pub path: PathBuf,
    /// A tag or commit, `local copy`, `yours` for a plugin you are writing,
    /// `local` for plain files; `, changed` when edited since installed.
    pub version: String,
    /// Where it came from, if anywhere.
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
            let (version, source) = if let Some(record) = Record::read(&path) {
                let mut version = record.version_label();
                if !fetch::changed_files(&path, &record).is_empty() {
                    version.push_str(", changed");
                }
                (version, Some(record.source.describe()))
            } else if path.join(".git").exists() {
                let commit = git(Some(&path), &["describe", "--tags", "--always"])
                    .await
                    .unwrap_or_else(|_| "no commits yet".to_string());
                let source = git(Some(&path), &["remote", "get-url", "origin"])
                    .await
                    .ok();
                (format!("yours, {commit}"), source)
            } else {
                ("local".to_string(), None)
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
            "no plugins in {}; `om plugin available` lists omaestro's own, `om plugin add <name|url>` installs, `om plugin new <name>` starts one",
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

/// Deletes `lib/<name>` and the rule file `add` wrote for it. A copy you
/// changed since installing it, or work that exists nowhere else in a
/// plugin you are writing (uncommitted, unpushed), is refused without
/// `force`.
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
    if path.is_dir() && !force {
        if let Some(record) = Record::read(&path) {
            let changed = fetch::changed_files(&path, &record);
            if !changed.is_empty() {
                bail!(
                    "{name} was changed since it was installed ({}); `om plugin remove {name} --force` deletes your changes too",
                    changed.join(", ")
                );
            }
        } else if path.join(".git").exists() {
            check_working_copy(&path, name).await?;
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

/// `om plugin remove --all`: every plugin in `lib/`, with the rule files om
/// wrote for them, to start over. Each goes through `remove`, so a plugin
/// you changed, or one of your own with work nowhere else, stays unless
/// `force`; rule files you wrote yourself are not touched.
pub async fn remove_all(config_dir: &Path, force: bool) -> Result<()> {
    let lib = config_dir.join("lib");
    let plugins = installed(&lib).await?;
    if plugins.is_empty() {
        println!("no plugins in {}; nothing to remove", lib.display());
        return Ok(());
    }
    let mut kept = Vec::new();
    for plugin in &plugins {
        if let Err(err) = remove(config_dir, &plugin.name, force).await {
            eprintln!("om: {err:#}");
            kept.push(plugin.name.clone());
        }
    }
    if !kept.is_empty() {
        bail!(
            "kept {}: see above; `om plugin remove --all --force` removes them too",
            kept.join(", ")
        );
    }
    println!("removed every plugin; `om plugin available` lists the ones to start again with");
    Ok(())
}

/// Refuses to delete work in a plugin's git repository that exists nowhere else.
async fn check_working_copy(path: &Path, name: &str) -> Result<()> {
    let changes = git(Some(path), &["status", "--porcelain"])
        .await
        .unwrap_or_default();
    if !changes.is_empty() {
        bail!(
            "{name} has uncommitted changes; commit and push them, or `om plugin remove {name} --force`"
        );
    }
    let has_commits = git(Some(path), &["rev-parse", "--verify", "-q", "HEAD"])
        .await
        .is_ok();
    if has_commits {
        if git(Some(path), &["rev-parse", "--abbrev-ref", "@{upstream}"])
            .await
            .is_err()
        {
            bail!(
                "{name} has commits but no remote to keep them; push it somewhere, or `om plugin remove {name} --force`"
            );
        }
        let ahead = git(Some(path), &["log", "--oneline", "@{upstream}..HEAD"])
            .await
            .unwrap_or_default();
        if !ahead.is_empty() {
            bail!(
                "{name} has commits that are not pushed; push them, or `om plugin remove {name} --force`"
            );
        }
    }
    Ok(())
}
