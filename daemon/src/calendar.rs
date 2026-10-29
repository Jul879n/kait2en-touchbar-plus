// SPDX-License-Identifier: GPL-3.0-or-later

//! Calendar events from the GNOME Shell calendar server. The time range is
//! the shell's own (`Since`/`Until`), so its calendar menu keeps working.

use std::{collections::HashMap, thread, time::Duration};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::OwnedValue;
use zbus::{MatchRule, message::Type};

use crate::events::{Bus, Ext};

const DEST: &str = "org.gnome.Shell.CalendarServer";
const PATH: &str = "/org/gnome/Shell/CalendarServer";

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub summary: String,
    pub start: i64,
    pub end: i64,
}

pub fn watch(bus: Bus) -> Result<()> {
    thread::Builder::new()
        .name("calendar".to_owned())
        .spawn(move || {
            loop {
                if let Err(error) = listen(&bus) {
                    eprintln!("kait2en-touchbar-plus: calendar: {error:#}");
                    thread::sleep(Duration::from_secs(30));
                }
            }
        })?;
    Ok(())
}

fn refresh(proxy: &Proxy<'_>) -> Result<()> {
    let since: i64 = proxy.get_property("Since")?;
    let until: i64 = proxy.get_property("Until")?;
    proxy.call::<_, _, ()>("SetTimeRange", &(since, until, true))?;
    Ok(())
}

fn listen(bus: &Bus) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    let proxy = Proxy::new(&connection, DEST, PATH, DEST)?;
    let has: bool = proxy.get_property("HasCalendars").unwrap_or(false);
    if !has {
        bus.send(Ext::Events(Vec::new()));
    }
    // Events arrive on the signal after the range is (re)issued.
    let hourly = Proxy::new(&connection, DEST, PATH, DEST)?;
    thread::Builder::new()
        .name("calendar-refresh".to_owned())
        .spawn(move || {
            loop {
                if let Err(error) = refresh(&hourly) {
                    eprintln!("kait2en-touchbar-plus: calendar refresh: {error:#}");
                }
                thread::sleep(Duration::from_secs(3600));
            }
        })?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(DEST)?
        .build();
    let mut events: HashMap<String, Event> = HashMap::new();
    for message in MessageIterator::for_match_rule(rule, &connection, None)? {
        let message = message?;
        let header = message.header();
        match header.member().map(|m| m.as_str()) {
            Some("EventsAddedOrUpdated") => {
                let list: Vec<(String, String, i64, i64, HashMap<String, OwnedValue>)> =
                    message.body().deserialize()?;
                for (id, summary, start, end, _) in list {
                    events.insert(id, Event { summary, start, end });
                }
            }
            Some("EventsRemoved") => {
                let ids: Vec<String> = message.body().deserialize()?;
                for id in ids {
                    events.remove(&id);
                }
            }
            Some("ClientDisappeared") => events.clear(),
            _ => continue,
        }
        bus.send(Ext::Events(events.values().cloned().collect()));
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}

/// `HH:MM Summary` of the first event that has not ended, or `None`.
pub fn next_label(events: &[Event], now: i64, time: impl Fn(i64) -> String) -> Option<String> {
    events
        .iter()
        .filter(|event| event.end > now && event.start < now + 86_400)
        .min_by_key(|event| event.start)
        .map(|event| format!("{} {}", time(event.start), event.summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_label_picks_earliest_unfinished_within_a_day() {
        let events = vec![
            Event { summary: "past".into(), start: 0, end: 50 },
            Event { summary: "later".into(), start: 5000, end: 6000 },
            Event { summary: "soon".into(), start: 300, end: 400 },
            Event { summary: "far".into(), start: 200_000, end: 201_000 },
        ];
        let label = next_label(&events, 100, |t| format!("t{t}"));
        assert_eq!(label.as_deref(), Some("t300 soon"));
        assert_eq!(next_label(&[], 100, |t| t.to_string()), None);
    }
}
