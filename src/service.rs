//! `om start`, `om stop`, `om restart`, and the supervisor part of `om
//! status`: who keeps the daemon running (the systemd user unit, the
//! Omarchy shell's plugin service, or nobody) and how to ask it.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::ipc::{self, Request, Status};

/// How long a start or stop may take before it is called a failure.
const PATIENCE: Duration = Duration::from_secs(10);

/// The systemd user unit. `OMAESTRO_UNIT` overrides it (the smoke test
/// runs a unit of its own).
pub fn unit_name() -> String {
    std::env::var("OMAESTRO_UNIT")
        .ok()
        .filter(|unit| !unit.trim().is_empty())
        .unwrap_or_else(|| "omaestro".to_string())
}

/// Who keeps the daemon running.
#[derive(Debug, Clone, PartialEq)]
pub enum Supervisor {
    /// The systemd user unit.
    Systemd,
    /// `Service.qml` of the Omarchy plugin, through `scripts/plugin-start.sh`.
    OmarchyShell,
    /// Nobody: a terminal or a script started it.
    None,
}

impl Supervisor {
    pub fn describe(&self) -> String {
        match self {
            Supervisor::Systemd => format!("the systemd user unit {}", unit_name()),
            Supervisor::OmarchyShell => "the Omarchy shell's plugin service".to_string(),
            Supervisor::None => "nothing (a terminal or a script started it)".to_string(),
        }
    }
}

async fn systemctl(args: &[&str]) -> Result<String> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .await
        .context("running systemctl")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        bail!("systemctl --user {}: {message}", args.join(" "));
    }
    Ok(stdout)
}

/// `loaded`, `not-found`, ...
async fn unit_load_state(unit: &str) -> String {
    systemctl(&["show", "-p", "LoadState", "--value", unit])
        .await
        .unwrap_or_else(|_| "unknown".to_string())
}

/// `active`, `inactive`, `failed`, ...
async fn unit_active_state(unit: &str) -> String {
    systemctl(&["show", "-p", "ActiveState", "--value", unit])
        .await
        .unwrap_or_else(|_| "unknown".to_string())
}

async fn unit_main_pid(unit: &str) -> Option<u32> {
    systemctl(&["show", "-p", "MainPID", "--value", unit])
        .await
        .ok()?
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid != 0)
}

/// The name of the process that started `pid`, from /proc.
fn parent_comm(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (comm) state ppid ...`; the comm may hold spaces and parens,
    // so the fields start after the last `)`.
    let after = stat.rsplit(')').next()?;
    let ppid: u32 = after.split_whitespace().nth(1)?.parse().ok()?;
    std::fs::read_to_string(format!("/proc/{ppid}/comm"))
        .ok()
        .map(|comm| comm.trim().to_string())
}

pub async fn supervisor_of(pid: u32) -> Supervisor {
    if unit_main_pid(&unit_name()).await == Some(pid) {
        return Supervisor::Systemd;
    }
    match parent_comm(pid).as_deref() {
        Some("quickshell") | Some("qs") => Supervisor::OmarchyShell,
        _ => Supervisor::None,
    }
}

/// What `om status` says about the unit when no daemon answers.
pub async fn unit_hint() -> String {
    let unit = unit_name();
    match unit_load_state(&unit).await.as_str() {
        "loaded" => match unit_active_state(&unit).await.as_str() {
            "failed" => format!(
                "{unit} failed; `journalctl --user -u {unit} -n 20` says why, `om start` tries again"
            ),
            "activating" => format!("{unit} is starting"),
            state => format!("{unit} is {state}; `om start` starts it"),
        },
        _ => format!(
            "no systemd unit {unit} (`make install-systemd` adds it); or run `om daemon --foreground`"
        ),
    }
}

/// The daemon's status, if one answers on the socket.
async fn running(socket: &Path) -> Option<Status> {
    let response = ipc::request(socket, &Request::Status).await.ok()?;
    serde_json::from_value(response.data?).ok()
}

/// Polls until a daemon answers (one other than `other_than`, if given),
/// for `PATIENCE` at most.
async fn wait_for_daemon(socket: &Path, other_than: Option<u32>) -> Option<Status> {
    let started = std::time::Instant::now();
    while started.elapsed() < PATIENCE {
        if let Some(status) = running(socket).await
            && Some(status.pid) != other_than
        {
            return Some(status);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    None
}

/// Polls until nothing answers, for `PATIENCE` at most.
async fn wait_gone(socket: &Path) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < PATIENCE {
        if running(socket).await.is_none() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// SIGTERM: the daemon unbinds its hotkeys and exits.
async fn terminate(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .await
        .context("running kill")?;
    if !status.success() {
        bail!("could not signal pid {pid}");
    }
    Ok(())
}

pub async fn start(socket: &Path) -> Result<()> {
    if let Some(status) = running(socket).await {
        println!(
            "already running: pid {}, under {}",
            status.pid,
            supervisor_of(status.pid).await.describe()
        );
        return Ok(());
    }
    let unit = unit_name();
    if unit_load_state(&unit).await != "loaded" {
        bail!(
            "no systemd unit {unit}. Install it with `make install-systemd` (or copy \
             systemd/omaestro.service to ~/.config/systemd/user and `systemctl --user \
             enable --now omaestro`), enable the Omarchy plugin, or run `om daemon --foreground`"
        );
    }
    systemctl(&["start", &unit]).await?;
    match wait_for_daemon(socket, None).await {
        Some(status) => println!(
            "started: omaestro {} pid {}, under the systemd user unit {unit}",
            status.version, status.pid
        ),
        None => bail!(
            "the unit started but no daemon answers on {}; `journalctl --user -u {unit} -n 20` says why",
            socket.display()
        ),
    }
    Ok(())
}

pub async fn stop(socket: &Path) -> Result<()> {
    let Some(status) = running(socket).await else {
        println!("not running");
        return Ok(());
    };
    match supervisor_of(status.pid).await {
        Supervisor::Systemd => {
            let unit = unit_name();
            systemctl(&["stop", &unit]).await?;
            if !wait_gone(socket).await {
                bail!("the unit was stopped but pid {} still answers", status.pid);
            }
            println!("stopped (the systemd user unit {unit}); `om start` starts it again");
        }
        Supervisor::OmarchyShell => bail!(
            "the daemon runs under the Omarchy shell's plugin service, which would start it \
             again. `omarchy plugin disable io.github.iluxav.omaestro` stops it for good; \
             `om restart` restarts it"
        ),
        Supervisor::None => {
            terminate(status.pid).await?;
            if !wait_gone(socket).await {
                bail!("pid {} did not exit", status.pid);
            }
            println!(
                "stopped pid {} (nothing supervised it; `om start` needs the systemd unit)",
                status.pid
            );
        }
    }
    Ok(())
}

/// Stop and start again: after `cargo install`, say.
pub async fn restart(socket: &Path) -> Result<()> {
    let Some(status) = running(socket).await else {
        return start(socket).await;
    };
    let old = status.pid;
    match supervisor_of(old).await {
        Supervisor::Systemd => {
            let unit = unit_name();
            systemctl(&["restart", &unit]).await?;
            match wait_for_daemon(socket, Some(old)).await {
                Some(status) => println!(
                    "restarted: omaestro {} pid {} -> {}, under the systemd user unit {unit}",
                    status.version, old, status.pid
                ),
                None => bail!(
                    "the unit restarted but no new daemon answers on {}; `journalctl --user -u {unit} -n 20` says why",
                    socket.display()
                ),
            }
        }
        Supervisor::OmarchyShell => {
            // A clean exit, which Service.qml follows with a fresh start.
            terminate(old).await?;
            match wait_for_daemon(socket, Some(old)).await {
                Some(status) => println!(
                    "restarted: omaestro {} pid {} -> {}, under the Omarchy shell's plugin service",
                    status.version, old, status.pid
                ),
                None => bail!(
                    "pid {old} stopped but the Omarchy shell did not start a new daemon; `journalctl --user -u omarchy-shell` or the shell log says why"
                ),
            }
        }
        Supervisor::None => {
            terminate(old).await?;
            if !wait_gone(socket).await {
                bail!("pid {old} did not exit");
            }
            let unit = unit_name();
            if unit_load_state(&unit).await == "loaded" {
                println!("stopped pid {old}; starting the systemd user unit {unit} instead");
                return start(socket).await;
            }
            bail!(
                "stopped pid {old}; nothing supervised it, so start it again where it ran \
                 (`om daemon --foreground`), or `make install-systemd` for the systemd unit"
            );
        }
    }
    Ok(())
}

/// The Omarchy plugin's id, which is also the panel's.
const PLUGIN_ID: &str = "io.github.iluxav.omaestro";

/// Toggles the rules panel through the Omarchy shell.
pub async fn panel() -> Result<()> {
    let output = match Command::new("omarchy-shell")
        .args(["shell", "toggle", PLUGIN_ID, "{}"])
        .output()
        .await
    {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => bail!(
            "omarchy-shell is not on PATH: the panel comes with the Omarchy shell and this \
             plugin (`omarchy plugin add https://github.com/iluxav/omaestro --enable`)"
        ),
        Err(err) => return Err(err).context("running omarchy-shell"),
    };
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if message.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        message
    };
    if message.contains("not running") || message.contains("not responding") {
        bail!("{message}");
    }
    bail!(
        "{message}; the panel needs the plugin enabled in the Omarchy shell: \
         `omarchy plugin enable {PLUGIN_ID}` (or `make plugin` from a checkout)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parent_of_this_process_is_known() {
        // Whatever runs the tests, /proc names it.
        let me = std::process::id();
        let parent = parent_comm(me).unwrap();
        assert!(!parent.is_empty());
        assert_eq!(parent_comm(u32::MAX), None);
    }

    #[test]
    fn supervisors_describe_themselves() {
        assert!(Supervisor::Systemd.describe().contains("systemd user unit"));
        assert!(
            Supervisor::OmarchyShell
                .describe()
                .contains("Omarchy shell")
        );
        assert!(Supervisor::None.describe().starts_with("nothing"));
    }
}
