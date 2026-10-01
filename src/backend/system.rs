//! System events without a D-Bus library: sleep and wake from
//! `gdbus monitor` on logind, USB from `udevadm monitor`, the battery from
//! `/sys/class/power_supply`, the network from `nmcli monitor`. Each source
//! runs only while a rule listens.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{System, SystemEvent, SystemSource, Watching};
use crate::runtime::Event;

const POWER_SUPPLY: &str = "/sys/class/power_supply";
const BATTERY_POLL: Duration = Duration::from_secs(30);

pub struct Tools;

impl System for Tools {
    fn watch(&self, source: SystemSource, events: mpsc::Sender<Event>) -> Result<Watching, String> {
        let task = match source {
            SystemSource::Login1 => lines(
                "gdbus",
                &["monitor", "--system", "--dest", "org.freedesktop.login1"],
                events,
                parse_login1,
            )?,
            SystemSource::Usb => lines(
                "udevadm",
                &["monitor", "--udev", "--subsystem-match=usb"],
                events,
                parse_udev,
            )?,
            SystemSource::Network => lines("nmcli", &["monitor"], events, parse_nmcli)?,
            SystemSource::Battery => tokio::spawn(poll_battery(events)),
        };
        Ok(Watching::new(AbortOnDrop(task)))
    }
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Runs `tool` and feeds every line of its output through `parse`.
fn lines(
    tool: &'static str,
    args: &[&str],
    events: mpsc::Sender<Event>,
    parse: fn(&str) -> Option<SystemEvent>,
) -> Result<JoinHandle<()>, String> {
    let mut child = tokio::process::Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| format!("cannot start {tool}: {err}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("no output from {tool}"))?;
    Ok(tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(event) = parse(&line)
                && events.send(Event::System(event)).await.is_err()
            {
                break;
            }
        }
        drop(child);
    }))
}

/// `gdbus monitor` prints logind's signal as
/// `/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (true,)`.
pub fn parse_login1(line: &str) -> Option<SystemEvent> {
    let rest = line.split("PrepareForSleep").nth(1)?;
    if rest.contains("true") {
        Some(SystemEvent::Sleep)
    } else if rest.contains("false") {
        Some(SystemEvent::Wake)
    } else {
        None
    }
}

/// `udevadm monitor` prints `UDEV  [123.456] add      /devices/.../usb1/1-3 (usb)`.
/// Interfaces (`1-3:1.0`) are skipped so a device counts once.
pub fn parse_udev(line: &str) -> Option<SystemEvent> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "UDEV" {
        return None;
    }
    let _timestamp = parts.next()?;
    let action = parts.next()?;
    let path = parts.next()?;
    if !matches!(action, "add" | "remove")
        || path
            .rsplit('/')
            .next()
            .is_some_and(|last| last.contains(':'))
    {
        return None;
    }
    Some(SystemEvent::Usb {
        action: action.to_string(),
        device: path.to_string(),
    })
}

/// `nmcli monitor` prints one line per change, such as `wlan0: connected`
/// or `Connectivity is now 'full'`.
pub fn parse_nmcli(line: &str) -> Option<SystemEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(SystemEvent::Network {
        line: line.to_string(),
    })
}

/// The first real battery under `dir`: `(percent, status)`.
pub fn read_battery(dir: &Path) -> Option<(i64, String)> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        let kind = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        // Peripherals (a mouse, headphones) report through the same class
        // but have no `energy_now`/`charge_now` alongside a real capacity.
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.starts_with("BAT") {
            continue;
        }
        let percent: i64 = std::fs::read_to_string(path.join("capacity"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let status = std::fs::read_to_string(path.join("status"))
            .unwrap_or_default()
            .trim()
            .to_string();
        return Some((percent, status));
    }
    None
}

async fn poll_battery(events: mpsc::Sender<Event>) {
    let mut last = None;
    loop {
        let now = read_battery(Path::new(POWER_SUPPLY));
        if now.is_some() && now != last {
            if let Some((percent, status)) = &now
                && events
                    .send(Event::System(SystemEvent::Battery {
                        percent: *percent,
                        status: status.clone(),
                    }))
                    .await
                    .is_err()
            {
                return;
            }
            last = now;
        }
        tokio::time::sleep(BATTERY_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logind_sleep_signals() {
        assert_eq!(
            parse_login1(
                "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (true,)"
            ),
            Some(SystemEvent::Sleep)
        );
        assert_eq!(
            parse_login1(
                "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)"
            ),
            Some(SystemEvent::Wake)
        );
        assert_eq!(
            parse_login1(
                "/org/freedesktop/login1: org.freedesktop.login1.Manager.SessionNew ('3', ...)"
            ),
            None
        );
    }

    #[test]
    fn udev_devices_not_interfaces() {
        assert_eq!(
            parse_udev(
                "UDEV  [12345.678901] add      /devices/pci0000:00/0000:00:14.0/usb1/1-3 (usb)"
            ),
            Some(SystemEvent::Usb {
                action: "add".into(),
                device: "/devices/pci0000:00/0000:00:14.0/usb1/1-3".into()
            })
        );
        assert_eq!(
            parse_udev(
                "UDEV  [12345.678901] add      /devices/pci0000:00/0000:00:14.0/usb1/1-3/1-3:1.0 (usb)"
            ),
            None
        );
        assert_eq!(
            parse_udev(
                "KERNEL[12345.678901] add      /devices/pci0000:00/0000:00:14.0/usb1/1-3 (usb)"
            ),
            None
        );
        assert_eq!(
            parse_udev(
                "UDEV  [12345.678901] bind     /devices/pci0000:00/0000:00:14.0/usb1/1-3 (usb)"
            ),
            None
        );
        assert!(matches!(
            parse_udev("UDEV  [1.0] remove   /devices/pci0000:00/0000:00:14.0/usb1/1-3 (usb)"),
            Some(SystemEvent::Usb { action, .. }) if action == "remove"
        ));
    }

    #[test]
    fn battery_from_sysfs() {
        let tmp = crate::testutil::TempDir::new("battery");
        let mouse = tmp.path().join("hidpp_battery_0");
        std::fs::create_dir(&mouse).unwrap();
        std::fs::write(mouse.join("type"), "Battery\n").unwrap();
        std::fs::write(mouse.join("capacity"), "5\n").unwrap();
        assert_eq!(
            read_battery(tmp.path()),
            None,
            "a peripheral is not the battery"
        );
        let bat = tmp.path().join("BAT0");
        std::fs::create_dir(&bat).unwrap();
        std::fs::write(bat.join("type"), "Battery\n").unwrap();
        std::fs::write(bat.join("capacity"), "73\n").unwrap();
        std::fs::write(bat.join("status"), "Discharging\n").unwrap();
        assert_eq!(read_battery(tmp.path()), Some((73, "Discharging".into())));
    }
}
