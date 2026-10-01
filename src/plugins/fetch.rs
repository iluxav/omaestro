//! Getting a plugin's files: a shallow clone (or a copy from disk) into a
//! staging directory next to `lib/`, checked for an `init.lua`, then moved
//! into place with a small record of where it came from. That record,
//! `.om-source.json`, is what `update` follows and what tells a plugin you
//! changed from the one you installed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::git;
use super::source::{Official, Source};

pub const RECORD: &str = ".om-source.json";

/// Where an installed plugin came from, written next to its `init.lua`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub source: Source,
    /// The commit the files were taken from, for a repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// `git describe` at that commit: a tag when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Every file as installed, with a fingerprint of its contents.
    pub files: BTreeMap<String, String>,
}

impl Record {
    pub fn read(dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join(RECORD)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write(&self, dir: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(dir.join(RECORD), text)
            .with_context(|| format!("writing {}", dir.join(RECORD).display()))
    }

    /// `v1.2.0`, `a1b2c3d`, or `local copy`.
    pub fn version_label(&self) -> String {
        match (&self.version, &self.commit) {
            (Some(version), _) => version.clone(),
            (None, Some(commit)) => commit.chars().take(7).collect(),
            (None, None) => "local copy".to_string(),
        }
    }
}

/// A fetched plugin in its staging directory, removed when dropped.
pub struct Fetched {
    staging: PathBuf,
    /// The plugin's own directory inside `staging`.
    pub dir: PathBuf,
    pub record: Record,
}

impl Drop for Fetched {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.staging);
    }
}

/// 64-bit FNV-1a: stable across versions, enough to notice an edited file.
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Every file under `dir` (but `.git` and the record), by relative path.
pub fn fingerprints(dir: &Path) -> Result<BTreeMap<String, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name == ".git" || (dir == root && name == RECORD) {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out)?;
            } else if path.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let bytes =
                    std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
                out.insert(relative, fingerprint(&bytes));
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// The files of an installed plugin that differ from what was installed:
/// changed, added or gone.
pub fn changed_files(dir: &Path, record: &Record) -> Vec<String> {
    let Ok(now) = fingerprints(dir) else {
        return vec!["(unreadable)".to_string()];
    };
    let mut changed: Vec<String> = record
        .files
        .iter()
        .filter(|(path, print)| now.get(*path) != Some(print))
        .map(|(path, _)| path.clone())
        .collect();
    changed.extend(
        now.keys()
            .filter(|path| !record.files.contains_key(*path))
            .cloned(),
    );
    changed
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))?;
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let path = entry?.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        if name == ".git" || name == RECORD {
            continue;
        }
        let target = to.join(name);
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else if path.is_file() {
            std::fs::copy(&path, &target).with_context(|| format!("copying {}", path.display()))?;
        }
    }
    Ok(())
}

/// Gets the plugin's files into a staging directory under `lib/`.
pub async fn fetch(lib: &Path, source: &Source, official: &Official) -> Result<Fetched> {
    let name = source.name()?;
    std::fs::create_dir_all(lib).with_context(|| format!("creating {}", lib.display()))?;
    let staging = lib.join(format!(".fetch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).with_context(|| format!("creating {}", staging.display()))?;
    let mut fetched = Fetched {
        dir: staging.clone(),
        staging,
        record: Record {
            source: source.clone(),
            commit: None,
            version: None,
            files: BTreeMap::new(),
        },
    };

    match source {
        Source::Local { dir } => {
            fetched.dir = fetched.staging.join(&name);
            copy_dir(dir, &fetched.dir)?;
            if !fetched.dir.join("init.lua").is_file() {
                bail!(
                    "{} is not an omaestro plugin: no init.lua in it",
                    dir.display()
                );
            }
        }
        Source::Git {
            url,
            path,
            reference,
        } => {
            let clone = fetched.staging.join("repo");
            let clone_text = clone.to_string_lossy().to_string();
            let mut args = vec!["clone", "--quiet", "--depth", "1"];
            if let Some(reference) = reference {
                args.extend(["--branch", reference.as_str()]);
            }
            args.extend([url.as_str(), clone_text.as_str()]);
            if let Err(err) = git(None, &args).await {
                let text = format!("{err:#}");
                if text.contains("could not read Username") || text.contains("not found") {
                    bail!(
                        "{}: no such repository, or it is private",
                        source.describe()
                    );
                }
                return Err(err.context(format!("fetching {}", source.describe())));
            }
            let dir = match path {
                Some(path) => clone.join(path),
                None => clone.clone(),
            };
            if !dir.is_dir() {
                let hint = if url == &official.repo {
                    "; `om plugin available` lists the ones there"
                } else {
                    ""
                };
                bail!(
                    "{}: no directory {} in that repository{hint}",
                    source.describe(),
                    path.as_deref().unwrap_or("")
                );
            }
            if !dir.join("init.lua").is_file() {
                bail!(
                    "{} is not an omaestro plugin: no init.lua at its root",
                    source.describe()
                );
            }
            fetched.record.commit = git(Some(&clone), &["rev-parse", "HEAD"]).await.ok();
            fetched.record.version = git(Some(&clone), &["describe", "--tags", "--exact-match"])
                .await
                .ok();
            // Only the plugin's own files: a repository root keeps them at the
            // top, a path inside one is lifted out of the rest.
            let plugin = fetched.staging.join(&name);
            copy_dir(&dir, &plugin)?;
            fetched.dir = plugin;
        }
    }
    let init = std::fs::read_to_string(fetched.dir.join("init.lua")).unwrap_or_default();
    super::source::check_requirement(&name, &init)?;
    fetched.record.files = fingerprints(&fetched.dir)?;
    Ok(fetched)
}

/// Moves a fetched plugin to `lib/<name>`, replacing what is there.
pub fn install(lib: &Path, name: &str, fetched: &Fetched) -> Result<PathBuf> {
    let target = lib.join(name);
    let incoming = lib.join(format!(".incoming-{name}"));
    let _ = std::fs::remove_dir_all(&incoming);
    std::fs::rename(&fetched.dir, &incoming)
        .with_context(|| format!("moving {} into {}", name, lib.display()))?;
    fetched.record.write(&incoming)?;
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .with_context(|| format!("replacing {}", target.display()))?;
    }
    std::fs::rename(&incoming, &target)
        .with_context(|| format!("moving the plugin into {}", target.display()))?;
    Ok(target)
}

/// The first sentence of a plugin's README, for a listing: the first line
/// that is not a heading, a badge or empty, joined with the lines after it.
pub fn summary(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("README.md")).ok()?;
    let mut paragraph = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if paragraph.is_empty() {
            if line.is_empty() || line.starts_with(['#', '<', '[', '!', '|', '`', '-']) {
                continue;
            }
            paragraph.push(line);
        } else if line.is_empty() {
            break;
        } else {
            paragraph.push(line);
        }
    }
    let joined = paragraph.join(" ").replace("**", "").replace('`', "");
    if joined.is_empty() {
        return None;
    }
    let sentence = match joined.find(". ") {
        Some(end) => joined[..=end].to_string(),
        None => joined,
    };
    let mut sentence = sentence.trim().to_string();
    if sentence.chars().count() > 100 {
        sentence = sentence.chars().take(97).collect::<String>() + "...";
    }
    Some(sentence)
}
