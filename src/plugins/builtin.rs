//! The plugins that ship inside the binary: the directories under
//! `plugins/` in the repository, embedded at build time. `om plugin add
//! <name>` copies one into lib/, where it is the user's to read and change;
//! the sources double as the examples.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub struct Builtin {
    pub name: &'static str,
    /// One line, for `om plugin available` and the rule file.
    pub description: &'static str,
    /// `(file name, contents)`.
    pub files: &'static [(&'static str, &'static str)],
}

macro_rules! builtin {
    ($name:literal, $description:literal) => {
        Builtin {
            name: $name,
            description: $description,
            files: &[
                (
                    "init.lua",
                    include_str!(concat!("../../plugins/", $name, "/init.lua")),
                ),
                (
                    "README.md",
                    include_str!(concat!("../../plugins/", $name, "/README.md")),
                ),
            ],
        }
    };
}

/// In the order `om plugin available` lists them.
pub const ALL: &[Builtin] = &[
    builtin!(
        "ai-text",
        "rewrite, summarize or translate the selection with the local model"
    ),
    builtin!(
        "window-halves",
        "the window on a half, a third or the center with one chord; float-and-center toggle"
    ),
    builtin!(
        "window-mode",
        "a window mode: one chord, then h j k l c m f place the window until Esc"
    ),
    builtin!(
        "window-rules",
        "float, center or move a window to a workspace when it appears; monitor notices"
    ),
    builtin!(
        "apps",
        "bring an app to the front or start it; arrange a desk with one chord"
    ),
    builtin!(
        "text-tools",
        "type today's date; upper-case the selection; your own snippets"
    ),
    builtin!(
        "clipboard",
        "the last ten clips to paste from; the clipboard into a notes file"
    ),
    builtin!(
        "reminders",
        "remind me in N minutes; a stretch reminder; a daily note"
    ),
    builtin!("web-search", "ask for a query and open it in the browser"),
    builtin!(
        "system-events",
        "notifications for wake, USB devices, a low battery; network changes logged"
    ),
    builtin!(
        "downloads",
        "a notification when something lands in ~/Downloads"
    ),
    builtin!(
        "panel",
        "a hotkey that opens the rules panel of the Omarchy plugin"
    ),
];

pub fn find(name: &str) -> Option<&'static Builtin> {
    ALL.iter().find(|plugin| plugin.name == name)
}

/// Writes the plugin's files into `lib/<name>`. Refuses to overwrite.
pub fn install(lib: &Path, plugin: &Builtin) -> Result<PathBuf> {
    let dir = lib.join(plugin.name);
    if dir.exists() {
        bail!("{} exists already", dir.display());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    for (file, text) in plugin.files {
        std::fs::write(dir.join(file), text)
            .with_context(|| format!("writing {}/{file}", dir.display()))?;
    }
    Ok(dir)
}

/// Whether the copy under `lib/<name>` is the shipped one, byte for byte.
pub fn unchanged(lib: &Path, plugin: &Builtin) -> bool {
    plugin.files.iter().all(|(file, text)| {
        std::fs::read_to_string(lib.join(plugin.name).join(file))
            .is_ok_and(|on_disk| on_disk == *text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plugin_directory_is_listed_and_complete() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        on_disk.sort();
        let mut listed: Vec<String> = ALL.iter().map(|p| p.name.to_string()).collect();
        listed.sort();
        assert_eq!(on_disk, listed, "plugins/ and builtin::ALL disagree");

        for plugin in ALL {
            assert!(!plugin.description.is_empty());
            let (init, readme) = (plugin.files[0].1, plugin.files[1].1);
            assert!(
                init.starts_with(&format!("-- {}:", plugin.name)),
                "{}'s init.lua starts with its name",
                plugin.name
            );
            assert!(init.ends_with("return M\n"), "{} returns M", plugin.name);
            assert!(
                readme.starts_with(&format!("# {}\n", plugin.name)),
                "{}'s README is titled",
                plugin.name
            );
            assert!(
                readme.contains(&format!("om plugin add {}", plugin.name)),
                "{}'s README says how to install it",
                plugin.name
            );
        }
    }

    #[test]
    fn install_copies_the_files_once() {
        let tmp = crate::testutil::TempDir::new("builtin");
        let lib = tmp.path().join("lib");
        let panel = find("panel").unwrap();
        let dir = install(&lib, panel).unwrap();
        assert!(dir.join("init.lua").is_file() && dir.join("README.md").is_file());
        assert!(unchanged(&lib, panel));
        std::fs::write(dir.join("init.lua"), "-- edited\n").unwrap();
        assert!(!unchanged(&lib, panel));
        assert!(install(&lib, panel).is_err(), "never overwrites");
        assert!(find("nope").is_none());
    }
}
