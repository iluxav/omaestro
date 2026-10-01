//! `om plugin update`: the latest of each plugin from where it came from.

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::fetch::{self, Record};
use super::source::Official;
use super::{Installed, git, installed};

/// Takes the latest of each installed plugin (or the named one) from where
/// it came from. A plugin you edited since is left alone without `force`;
/// one you are writing yourself is `git pull`ed.
pub async fn update(
    lib: &Path,
    name: Option<&str>,
    force: bool,
    official: &Official,
) -> Result<()> {
    let plugins = installed(lib).await?;
    let chosen: Vec<&Installed> = match name {
        Some(name) => vec![
            plugins
                .iter()
                .find(|p| p.name == name)
                .with_context(|| format!("no plugin named '{name}' in {}", lib.display()))?,
        ],
        None => plugins.iter().filter(|p| p.source.is_some()).collect(),
    };
    if chosen.is_empty() {
        println!("nothing to update: no plugin here came from anywhere");
        return Ok(());
    }
    let mut failed = false;
    for plugin in chosen {
        let Some(dir) = plugin.path.parent() else {
            continue;
        };
        let outcome = if let Some(record) = Record::read(dir) {
            update_copy(lib, &plugin.name, dir, &record, force, official).await
        } else if dir.join(".git").exists() {
            update_working_copy(dir).await
        } else {
            Ok(format!(
                "not installed from anywhere; `om plugin remove {0} && om plugin add {0}` replaces it",
                plugin.name
            ))
        };
        match outcome {
            Ok(message) => println!("{}: {message}", plugin.name),
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

async fn update_copy(
    lib: &Path,
    name: &str,
    dir: &Path,
    record: &Record,
    force: bool,
    official: &Official,
) -> Result<String> {
    let changed = fetch::changed_files(dir, record);
    if !changed.is_empty() && !force {
        bail!(
            "changed since it was installed ({}); `om plugin update {name} --force` replaces your changes",
            changed.join(", ")
        );
    }
    let fetched = fetch::fetch(lib, &record.source, official).await?;
    let before = record.version_label();
    if fetched.record.files == record.files && changed.is_empty() {
        return Ok(format!("up to date ({before})"));
    }
    fetch::install(lib, name, &fetched)?;
    Ok(format!("{before} -> {}", fetched.record.version_label()))
}

async fn update_working_copy(dir: &Path) -> Result<String> {
    if git(Some(dir), &["symbolic-ref", "-q", "HEAD"])
        .await
        .is_err()
    {
        return Ok("a detached checkout; left as it is".to_string());
    }
    if git(Some(dir), &["rev-parse", "--abbrev-ref", "@{upstream}"])
        .await
        .is_err()
    {
        return Ok("yours, with no remote to pull from".to_string());
    }
    let before = git(Some(dir), &["describe", "--tags", "--always"])
        .await
        .unwrap_or_default();
    git(Some(dir), &["pull", "--ff-only", "--quiet"]).await?;
    let after = git(Some(dir), &["describe", "--tags", "--always"])
        .await
        .unwrap_or_default();
    Ok(if before == after {
        format!("up to date ({after})")
    } else {
        format!("{before} -> {after}")
    })
}
