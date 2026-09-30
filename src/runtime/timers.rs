//! `om.every`: one tokio task per timer, ticking into the event loop. The
//! loop fires the handler, so timers queue behind everything else like any
//! other event and a reload can wait for them.

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

/// A timer the rules want.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub id: String,
    pub interval: Duration,
}

pub struct Timers {
    events: mpsc::Sender<Event>,
    running: HashMap<String, (Duration, JoinHandle<()>)>,
}

impl Timers {
    pub fn new(events: mpsc::Sender<Event>) -> Self {
        Self {
            events,
            running: HashMap::new(),
        }
    }

    /// Starts and stops timers so exactly `wanted` are ticking. A timer that
    /// is already running with the same interval keeps its phase.
    pub fn sync(&mut self, wanted: &[Wanted]) {
        let keep = |id: &str, interval: Duration| {
            wanted.iter().any(|w| w.id == id && w.interval == interval)
        };
        self.running.retain(|id, (interval, task)| {
            let stays = keep(id, *interval);
            if !stays {
                task.abort();
            }
            stays
        });
        for timer in wanted {
            if self.running.contains_key(&timer.id) {
                continue;
            }
            let events = self.events.clone();
            let id = timer.id.clone();
            let interval = timer.interval;
            let task = tokio::spawn(async move {
                loop {
                    sleep(interval).await;
                    if events.send(Event::Timer(id.clone())).await.is_err() {
                        return;
                    }
                }
            });
            self.running.insert(timer.id.clone(), (interval, task));
        }
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
    }
}
