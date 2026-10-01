//! `om.every`, `om.after`, `om.at`: one tokio task per timer, ticking into
//! the event loop. The loop fires the handler, so timers queue behind
//! everything else like any other event and a reload can wait for them.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;

use super::Event;

/// `30s`, `5m`, `1h`, or a sum like `1h30m`. At least a second.
pub fn parse_interval(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("the interval is empty".to_string());
    }
    let mut total = Duration::ZERO;
    let mut number = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let per_unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            other => return Err(format!("'{text}': unknown unit '{other}', use s, m or h")),
        };
        let count: u64 = number
            .parse()
            .map_err(|_| format!("'{text}': a number must come before '{c}'"))?;
        number.clear();
        total += Duration::from_secs(count * per_unit);
    }
    if !number.is_empty() {
        return Err(format!(
            "'{text}': missing a unit after '{number}', use s, m or h"
        ));
    }
    if total < Duration::from_secs(1) {
        return Err(format!("'{text}': the interval must be at least 1s"));
    }
    Ok(total)
}

/// The shortest spelling `parse_interval` reads back: `5m`, `1h30m`, `90s`.
pub fn format_interval(interval: Duration) -> String {
    let secs = interval.as_secs();
    let (hours, minutes, seconds) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let mut text = String::new();
    if hours > 0 {
        text.push_str(&format!("{hours}h"));
    }
    if minutes > 0 {
        text.push_str(&format!("{minutes}m"));
    }
    if seconds > 0 || text.is_empty() {
        text.push_str(&format!("{seconds}s"));
    }
    text
}

/// `HH:MM` on a 24-hour clock.
pub fn parse_clock(text: &str) -> Result<(u32, u32), String> {
    let (hour, minute) = text
        .trim()
        .split_once(':')
        .ok_or_else(|| format!("'{text}': a time looks like 09:30"))?;
    let hour: u32 = hour
        .parse()
        .map_err(|_| format!("'{text}': a time looks like 09:30"))?;
    let minute: u32 = minute
        .parse()
        .map_err(|_| format!("'{text}': a time looks like 09:30"))?;
    if hour > 23 || minute > 59 {
        return Err(format!("'{text}': hours go to 23 and minutes to 59"));
    }
    Ok((hour, minute))
}

/// A timer the rules want.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub id: String,
    /// Until the next tick.
    pub delay: Duration,
    /// Tick again every `delay` (`om.every`), or once (`om.after`, `om.at`;
    /// `om.at` is re-armed by the runtime after each tick).
    pub repeat: bool,
}

struct Running {
    delay: Duration,
    repeat: bool,
    task: JoinHandle<()>,
}

pub struct Timers {
    events: mpsc::Sender<Event>,
    running: HashMap<String, Running>,
}

impl Timers {
    pub fn new(events: mpsc::Sender<Event>) -> Self {
        Self {
            events,
            running: HashMap::new(),
        }
    }

    /// Starts and stops timers so exactly `wanted` are ticking. A repeating
    /// timer already running with the same interval keeps its phase; a
    /// one-shot already running is left alone whatever its delay says now.
    pub fn sync(&mut self, wanted: &[Wanted]) {
        self.running.retain(|id, running| {
            let stays = wanted.iter().any(|w| {
                w.id == *id && w.repeat == running.repeat && (!w.repeat || w.delay == running.delay)
            });
            if !stays {
                running.task.abort();
            }
            stays
        });
        for timer in wanted {
            if !self.running.contains_key(&timer.id) {
                self.start(timer);
            }
        }
    }

    /// Restarts one timer with a fresh delay, for `om.at` after a tick.
    pub fn rearm(&mut self, timer: &Wanted) {
        if let Some(running) = self.running.remove(&timer.id) {
            running.task.abort();
        }
        self.start(timer);
    }

    fn start(&mut self, timer: &Wanted) {
        let events = self.events.clone();
        let id = timer.id.clone();
        let delay = timer.delay;
        let repeat = timer.repeat;
        let task = tokio::spawn(async move {
            loop {
                sleep(delay).await;
                if events.send(Event::Timer(id.clone())).await.is_err() || !repeat {
                    return;
                }
            }
        });
        self.running.insert(
            timer.id.clone(),
            Running {
                delay,
                repeat,
                task,
            },
        );
    }

    pub fn clear(&mut self) {
        self.sync(&[]);
    }
}

impl Drop for Timers {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals() {
        let secs = |t| parse_interval(t).map(|d| d.as_secs());
        assert_eq!(secs("30s"), Ok(30));
        assert_eq!(secs("5m"), Ok(300));
        assert_eq!(secs("1h"), Ok(3600));
        assert_eq!(secs("1h30m"), Ok(5400));
        assert_eq!(secs(" 2m10s "), Ok(130));
        assert_eq!(
            secs("0s").unwrap_err(),
            "'0s': the interval must be at least 1s"
        );
        assert_eq!(
            secs("5").unwrap_err(),
            "'5': missing a unit after '5', use s, m or h"
        );
        assert_eq!(
            secs("5x").unwrap_err(),
            "'5x': unknown unit 'x', use s, m or h"
        );
        assert_eq!(secs("m").unwrap_err(), "'m': a number must come before 'm'");
        assert_eq!(secs("").unwrap_err(), "the interval is empty");

        // Spelled back the way a rule would write it.
        for (text, back) in [("30s", "30s"), ("5m", "5m"), ("1h", "1h"), ("90m", "1h30m")] {
            assert_eq!(format_interval(parse_interval(text).unwrap()), back);
        }
        assert_eq!(format_interval(Duration::ZERO), "0s");
    }

    #[test]
    fn clock_times() {
        assert_eq!(parse_clock("09:30"), Ok((9, 30)));
        assert_eq!(parse_clock(" 23:59 "), Ok((23, 59)));
        assert_eq!(parse_clock("0:05"), Ok((0, 5)));
        assert_eq!(
            parse_clock("24:00").unwrap_err(),
            "'24:00': hours go to 23 and minutes to 59"
        );
        assert_eq!(
            parse_clock("noon").unwrap_err(),
            "'noon': a time looks like 09:30"
        );
    }
}
