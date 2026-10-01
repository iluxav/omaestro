//! `om doctor`: is this machine and session set up for the daemon, and did
//! an earlier daemon leave binds behind?

use std::path::Path;

use crate::backend::hypr::{HyprCtl, Instance};
use crate::backend::run::find_on_path;
use crate::backend::{BindInfo, Hypr};
use crate::chord::Chord;
use crate::ipc::{self, Request, Status, TriggerRow};

/// Tools the v1 API shells out to.
const TOOLS: [&str; 5] = ["hyprctl", "wtype", "wl-paste", "wl-copy", "notify-send"];

/// Prints one line per check. Returns false if something is broken enough
/// that the daemon cannot run.
pub async fn run(socket: &Path, clear: bool) -> bool {
    let mut healthy = true;

    let instance = match Instance::from_env() {
        Ok(instance) if instance.dir().is_dir() => {
            println!("ok    Hyprland instance {}", instance.signature());
            Some(instance)
        }
        Ok(instance) => {
            healthy = false;
            println!(
                "FAIL  HYPRLAND_INSTANCE_SIGNATURE is {} but that instance has no directory under \
                 $XDG_RUNTIME_DIR/hypr (stale environment?)",
                instance.signature()
            );
            None
        }
        Err(err) => {
            healthy = false;
            println!("FAIL  {err}");
            None
        }
    };

    for tool in TOOLS {
        match find_on_path(tool) {
            Some(found) => println!("ok    {tool} ({})", found.display()),
            None => println!(
                "warn  {tool} is not on PATH; the API functions that use it will raise an error"
            ),
        }
    }

    // The daemon, and which hotkeys it holds right now.
    let mut live = Vec::new();
    match ipc::request(socket, &Request::Status).await {
        Ok(response) => match response.data.map(serde_json::from_value::<Status>) {
            Some(Ok(status)) => {
                println!(
                    "ok    daemon {} answers on {} (pid {}, {} file(s), {} trigger(s))",
                    status.version,
                    socket.display(),
                    status.pid,
                    status.files.len(),
                    status.triggers
                );
                if let Some(error) = status.load_error {
                    println!("warn  the last load failed: {error}");
                }
                if let Ok(response) = ipc::request(socket, &Request::List).await
                    && let Some(Ok(rows)) =
                        response.data.map(serde_json::from_value::<Vec<TriggerRow>>)
                {
                    live = hotkey_chords(&rows);
                }
            }
            _ => println!(
                "warn  something answers on {} but it is not this version of omaestro",
                socket.display()
            ),
        },
        Err(_) => println!(
            "warn  daemon is not running (no answer on {})",
            socket.display()
        ),
    }

    if instance.is_some() {
        check_binds(&HyprCtl, &live, clear).await;
    }

    healthy
}

/// The chords of the daemon's hotkeys, from `om list`. A hotkey that is
/// switched off or refused holds no bind, so it is not counted; an app
/// hotkey's bind comes and goes with the focus, so it is.
fn hotkey_chords(rows: &[TriggerRow]) -> Vec<Chord> {
    rows.iter()
        .filter(|row| {
            (row.kind == "hotkey" || row.kind == "app_hotkey")
                && row.enabled
                && row.problem.is_none()
        })
        .filter_map(|row| {
            let chord = row.detail.split(" in ").next().unwrap_or(&row.detail);
            Chord::parse(chord).ok()
        })
        .collect()
}

/// Binds with our description that no running daemon accounts for, as
/// `(chord, description)`.
fn leftovers(binds: &[BindInfo], live: &[Chord]) -> Vec<(Chord, String)> {
    binds
        .iter()
        .filter(|bind| bind.description.starts_with("omaestro: ") && bind.submap.is_empty())
        .filter(|bind| !live.iter().any(|chord| chord.same_keys(&bind.chord)))
        .map(|bind| (bind.chord.clone(), bind.description.clone()))
        .collect()
}

/// Whether every bind on `chord` is ours, so unbinding it takes nobody else's.
fn only_ours(binds: &[BindInfo], chord: &Chord) -> bool {
    binds
        .iter()
        .filter(|bind| bind.submap.is_empty() && bind.chord.same_keys(chord))
        .all(|bind| bind.description.starts_with("omaestro: "))
}

async fn check_binds(hypr: &dyn Hypr, live: &[Chord], clear: bool) {
    let binds = match hypr.binds().await {
        Ok(binds) => binds,
        Err(err) => {
            println!("warn  could not read Hyprland's binds: {err}");
            return;
        }
    };
    let left = leftovers(&binds, live);
    if left.is_empty() {
        println!("ok    no leftover omaestro binds in Hyprland");
        return;
    }
    for (chord, description) in &left {
        if !clear {
            println!("warn  leftover bind {chord} ({description}); `om doctor --clear` removes it");
        } else if !only_ours(&binds, chord) {
            println!(
                "warn  leftover bind {chord} ({description}) shares its chord with a bind that is not ours; left alone"
            );
        } else {
            match hypr.unbind(chord, "").await {
                Ok(()) => println!("ok    removed leftover bind {chord} ({description})"),
                Err(err) => println!("warn  could not remove leftover bind {chord}: {err}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeHypr;

    fn bind(chord: &str, description: &str) -> BindInfo {
        BindInfo {
            chord: Chord::parse(chord).unwrap(),
            description: description.to_string(),
            submap: String::new(),
        }
    }

    fn row(kind: &str, id: &str) -> TriggerRow {
        TriggerRow {
            id: id.to_string(),
            kind: kind.to_string(),
            detail: id.split_once(':').map(|(_, d)| d).unwrap_or("").to_string(),
            origin: "init.lua:1".to_string(),
            enabled: true,
            problem: None,
            overrides: None,
            bound: None,
        }
    }

    #[test]
    fn leftovers_are_our_binds_the_daemon_does_not_hold() {
        let binds = [
            bind("SUPER + J", "Toggle window split"),
            bind("SUPER + ALT + J", "omaestro: rules.d/a.lua:3"),
            bind("SUPER + ALT + K", "omaestro: rules.d/b.lua:1"),
        ];
        let live = hotkey_chords(&[row("hotkey", "hotkey:SUPER+ALT+J"), row("trigger", "hello")]);
        assert_eq!(live.len(), 1);
        let left = leftovers(&binds, &live);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].0.hyprland(), "SUPER + ALT + K");
        assert_eq!(left[0].1, "omaestro: rules.d/b.lua:1");

        // No daemon: every bind of ours is a leftover.
        assert_eq!(leftovers(&binds, &[]).len(), 2);
    }

    #[test]
    fn a_shared_chord_is_not_only_ours() {
        let binds = [
            bind("SUPER + J", "Toggle window split"),
            bind("SUPER + J", "omaestro: rules.d/a.lua:3"),
            bind("SUPER + K", "omaestro: rules.d/b.lua:1"),
        ];
        assert!(!only_ours(&binds, &Chord::parse("SUPER + J").unwrap()));
        assert!(only_ours(&binds, &Chord::parse("SUPER + K").unwrap()));
    }

    #[tokio::test]
    async fn clear_removes_only_unshared_leftovers() {
        let hypr = FakeHypr::default();
        hypr.add("SUPER + J", "Toggle window split");
        hypr.add("SUPER + J", "omaestro: rules.d/a.lua:3");
        hypr.add("SUPER + K", "omaestro: rules.d/b.lua:1");
        hypr.add("SUPER + L", "omaestro: rules.d/c.lua:1");

        check_binds(&hypr, &[Chord::parse("SUPER + L").unwrap()], false).await;
        assert_eq!(hypr.calls(), ["binds"]);
        check_binds(&hypr, &[Chord::parse("SUPER + L").unwrap()], true).await;
        assert_eq!(hypr.chords(), ["SUPER + J", "SUPER + J", "SUPER + L"]);
    }
}
