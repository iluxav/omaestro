//! Where a plugin comes from, read from what the user typed. Every plugin,
//! ours included, is a directory with an `init.lua`: at a repository's root,
//! at a path inside one, or on disk.
//!
//! ```text
//! window-halves                                   omaestro's own: <repo>/plugins/window-halves
//! you/my-plugin                                   https://github.com/you/my-plugin
//! you/plugins/clock                               the clock/ directory of you/plugins
//! https://github.com/you/plugins/tree/main/clock  the same, as copied from the browser
//! https://git.example.com/x.git --path clock      any git URL, with a directory in it
//! ./my-plugin  /home/me/my-plugin  ~/my-plugin    a directory on disk (a git repository root is cloned)
//! ```

use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Where omaestro's own plugins live; `OMAESTRO_PLUGIN_REPO` points it at a
/// fork or a local clone.
pub const OFFICIAL_REPO: &str = "https://github.com/iluxav/omaestro";
pub const OFFICIAL_DIR: &str = "plugins";

/// The repository a bare plugin name is looked up in.
#[derive(Debug, Clone, PartialEq)]
pub struct Official {
    pub repo: String,
    pub dir: String,
}

impl Official {
    pub fn from_env() -> Self {
        let repo = std::env::var("OMAESTRO_PLUGIN_REPO")
            .ok()
            .filter(|repo| !repo.trim().is_empty())
            .unwrap_or_else(|| OFFICIAL_REPO.to_string());
        Self {
            repo,
            dir: OFFICIAL_DIR.to_string(),
        }
    }

    /// The source of one of its plugins.
    pub fn plugin(&self, name: &str) -> Source {
        Source::Git {
            url: self.repo.clone(),
            path: Some(format!("{}/{name}", self.dir)),
            reference: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A git repository, and the plugin's directory in it (none: the root).
    Git {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// A tag or branch; none is the default branch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    /// A directory on disk, copied as it is.
    Local { dir: PathBuf },
}

impl Source {
    /// The directory name the plugin is installed under, and the name rules
    /// `om.use`.
    pub fn name(&self) -> Result<String> {
        let name = match self {
            Source::Git {
                path: Some(path), ..
            } => last_segment(path),
            Source::Git { url, .. } => {
                let last = last_segment(url.rsplit(':').next().unwrap_or(url));
                last.strip_suffix(".git").unwrap_or(&last).to_string()
            }
            Source::Local { dir } => dir
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
        };
        check_name(&name)?;
        Ok(name)
    }

    /// One line for `om plugin list`: `github.com/you/plugins/clock@v1`.
    pub fn describe(&self) -> String {
        match self {
            Source::Git {
                url,
                path,
                reference,
            } => {
                let base = url
                    .strip_prefix("https://")
                    .unwrap_or(url)
                    .trim_end_matches(".git");
                let mut text = base.to_string();
                if let Some(path) = path {
                    if base.starts_with("github.com/") {
                        text.push('/');
                    } else {
                        text.push(' ');
                    }
                    text.push_str(path);
                }
                if let Some(reference) = reference {
                    text.push('@');
                    text.push_str(reference);
                }
                text
            }
            Source::Local { dir } => dir.display().to_string(),
        }
    }
}

fn last_segment(text: &str) -> String {
    text.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// A name is a directory name and a `require` argument.
pub fn check_name(name: &str) -> Result<()> {
    let plain = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if name.is_empty() || name.starts_with('.') || !plain {
        bail!("'{name}' is not a plugin name (letters, digits, '-', '_' and '.')");
    }
    Ok(())
}

/// A path inside a repository: relative, no `..`, no empty segments.
fn check_path(path: &str) -> Result<String> {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty()
        || trimmed
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        bail!("'{path}' is not a directory inside a repository");
    }
    Ok(trimmed.to_string())
}

fn is_local(spec: &str) -> bool {
    spec.starts_with('/')
        || spec.starts_with("./")
        || spec.starts_with("../")
        || spec.starts_with("~/")
        || spec == "."
        || spec == ".."
}

fn expand_home(spec: &str) -> PathBuf {
    match spec.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|| PathBuf::from(spec)),
        None => PathBuf::from(spec),
    }
}

/// `owner/repo[/path...]` after `github.com/`, with `tree/<ref>/` or
/// `blob/<ref>/` as the browser shows them.
fn github(rest: &str) -> Result<Source> {
    let segments: Vec<&str> = rest.trim_matches('/').split('/').collect();
    if segments.len() < 2 || segments[0].is_empty() || segments[1].is_empty() {
        bail!("'github.com/{rest}' does not name a repository (github.com/owner/repo)");
    }
    let repo = segments[1].strip_suffix(".git").unwrap_or(segments[1]);
    let url = format!("https://github.com/{}/{repo}", segments[0]);
    let (reference, path) = match segments.get(2) {
        Some(&"tree") | Some(&"blob") => {
            let Some(reference) = segments.get(3) else {
                bail!("'github.com/{rest}' names no branch after /tree/");
            };
            (Some(reference.to_string()), segments[4..].join("/"))
        }
        _ => (None, segments[2..].join("/")),
    };
    let path = if path.is_empty() {
        None
    } else {
        Some(check_path(&path)?)
    };
    Ok(Source::Git {
        url,
        path,
        reference,
    })
}

/// Reads what the user typed. `reference` (`--ref`) and `path` (`--path`)
/// override what the spec itself says.
pub fn parse(
    spec: &str,
    reference: Option<&str>,
    path: Option<&str>,
    official: &Official,
) -> Result<Source> {
    let spec = spec.trim();
    if spec.is_empty() {
        bail!("no plugin given");
    }
    let mut source = if is_local(spec) {
        let dir = expand_home(spec);
        if !dir.is_dir() {
            bail!("{} is not a directory", dir.display());
        }
        let dir = dir.canonicalize().unwrap_or(dir);
        // A repository root is cloned, so updates follow its commits; any
        // other directory is copied as it is, uncommitted edits included.
        if dir.join(".git").exists() && (path.is_some() || dir.join("init.lua").is_file()) {
            Source::Git {
                url: dir.to_string_lossy().to_string(),
                path: None,
                reference: None,
            }
        } else {
            if reference.is_some() || path.is_some() {
                bail!(
                    "--ref and --path are for repositories; {} is copied as it is",
                    dir.display()
                );
            }
            return Ok(Source::Local { dir });
        }
    } else if let Some(rest) = spec
        .strip_prefix("https://github.com/")
        .or_else(|| spec.strip_prefix("http://github.com/"))
        .or_else(|| spec.strip_prefix("github.com/"))
    {
        github(rest)?
    } else if spec.contains("://") || spec.starts_with("git@") {
        Source::Git {
            url: spec.to_string(),
            path: None,
            reference: None,
        }
    } else if spec.contains('/') {
        github(spec)?
    } else {
        check_name(spec)?;
        official.plugin(spec)
    };
    if let Source::Git {
        path: p,
        reference: r,
        ..
    } = &mut source
    {
        if let Some(path) = path {
            *p = Some(check_path(path)?);
        }
        if let Some(reference) = reference {
            *r = Some(reference.to_string());
        }
    }
    Ok(source)
}

/// The version `init.lua` says it needs, from a line like
/// `-- requires om >= 0.2.0` near its top.
pub fn required_version(init: &str) -> Option<(u64, u64, u64)> {
    init.lines().take(40).find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("--")?.trim();
        let rest = rest.strip_prefix("requires om")?.trim();
        let rest = rest.strip_prefix(">=")?.trim();
        parse_version(rest.split_whitespace().next()?)
    })
}

pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim_start_matches('v').split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next().unwrap_or("0").parse().ok()?,
    );
    Some(version)
}

/// Refuses a plugin that needs a newer `om` than this one.
pub fn check_requirement(name: &str, init: &str) -> Result<()> {
    let Some(needed) = required_version(init) else {
        return Ok(());
    };
    let current = parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0));
    if needed > current {
        bail!(
            "{name} needs om {}.{}.{} or newer and this is {}: update om first",
            needed.0,
            needed.1,
            needed.2,
            env!("CARGO_PKG_VERSION")
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
