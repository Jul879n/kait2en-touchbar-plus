// SPDX-License-Identifier: GPL-3.0-or-later

//! Dialogs of the focused window over AT-SPI: libadwaita and GTK alerts live
//! inside their window, so the shell extension cannot see them. The
//! accessibility bus lists them with their text and buttons, and pressing a
//! button there answers the dialog.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, connection::Builder};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::events::{Bus, Ext};

const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const ACTION: &str = "org.a11y.atspi.Action";
/// `StateType::Active` of AT-SPI.
const STATE_ACTIVE: u32 = 1;
const POLL: Duration = Duration::from_millis(400);
const CALL_TIMEOUT: Duration = Duration::from_millis(300);
/// How deep below a window an alert may sit, and below an alert its buttons.
const ALERT_DEPTH: usize = 3;
const BUTTON_DEPTH: usize = 14;
const MAX_BUTTONS: usize = 4;
/// The walk of every app gives up after this long.
const WALK_BUDGET: Duration = Duration::from_millis(250);
/// Failed walks in a row before the connection is made again.
const MAX_ERRORS: u32 = 5;

type Object = (String, OwnedObjectPath);

#[derive(Clone, Debug, PartialEq)]
pub struct DialogButton {
    pub label: String,
    object: (String, String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub title: String,
    pub buttons: Vec<DialogButton>,
}

/// Polls the focused window for a dialog while `enabled` is set and reports
/// every change.
pub fn watch(bus: Bus, enabled: Arc<AtomicBool>) -> Result<()> {
    thread::Builder::new()
        .name("atspi-dialogs".to_owned())
        .spawn(move || {
            let mut connection: Option<Connection> = None;
            let mut reported = false;
            let mut last: Option<Dialog> = None;
            let mut errors = 0;
            let mut walked = Duration::ZERO;
            loop {
                // A fixed period: the walk's own time counts toward it.
                thread::sleep(POLL.saturating_sub(walked).max(Duration::from_millis(100)));
                walked = Duration::ZERO;
                if !enabled.load(Ordering::Relaxed) {
                    continue;
                }
                let started = Instant::now();
                if connection.is_none() {
                    match connect() {
                        Ok(found) => connection = Some(found),
                        Err(error) => {
                            if !reported {
                                eprintln!("kait2en-touchbar-plus: accessibility bus: {error:#}");
                                reported = true;
                            }
                            continue;
                        }
                    }
                }
                let Some(open) = connection.as_ref() else { continue };
                // A failed walk is not "no dialog": keep what the bar shows.
                let result = find_dialog(open, started);
                walked = started.elapsed();
                match result {
                    Ok(found) => {
                        errors = 0;
                        if found != last {
                            last = found.clone();
                            bus.send(Ext::Dialog(found));
                        }
                    }
                    Err(error) => {
                        errors += 1;
                        if errors == MAX_ERRORS {
                            eprintln!("kait2en-touchbar-plus: accessibility walk: {error:#}");
                            connection = None;
                        }
                    }
                }
            }
        })?;
    Ok(())
}

fn connect() -> Result<Connection> {
    let session = Connection::session().context("session bus")?;
    let address: String = session
        .call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.a11y.Bus"),
            "GetAddress",
            &(),
        )?
        .body()
        .deserialize()?;
    Ok(Builder::address(address.as_str())?
        .method_timeout(CALL_TIMEOUT)
        .build()?)
}

/// Presses a dialog button. Runs on its own thread: a hung app must not
/// stall the bar.
pub fn press(button: &DialogButton) {
    let (name, path) = button.object.clone();
    thread::spawn(move || {
        let result = connect().and_then(|connection| {
            connection
                .call_method(
                    Some(name.as_str()),
                    path.as_str(),
                    Some(ACTION),
                    "DoAction",
                    &(0i32,),
                )
                .map(|_| ())
                .map_err(Into::into)
        });
        if let Err(error) = result {
            eprintln!("kait2en-touchbar-plus: press dialog button: {error:#}");
        }
    });
}

fn children(connection: &Connection, object: &Object) -> Result<Vec<Object>> {
    Ok(connection
        .call_method(
            Some(object.0.as_str()),
            object.1.as_str(),
            Some(ACCESSIBLE),
            "GetChildren",
            &(),
        )?
        .body()
        .deserialize()?)
}

fn role_name(connection: &Connection, object: &Object) -> Result<String> {
    Ok(connection
        .call_method(
            Some(object.0.as_str()),
            object.1.as_str(),
            Some(ACCESSIBLE),
            "GetRoleName",
            &(),
        )?
        .body()
        .deserialize()?)
}

fn name(connection: &Connection, object: &Object) -> Result<String> {
    let value: OwnedValue = connection
        .call_method(
            Some(object.0.as_str()),
            object.1.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(ACCESSIBLE, "Name"),
        )?
        .body()
        .deserialize()?;
    Ok(String::try_from(value)?)
}

fn is_active(connection: &Connection, object: &Object) -> bool {
    let states: Result<Vec<u32>> = connection
        .call_method(
            Some(object.0.as_str()),
            object.1.as_str(),
            Some(ACCESSIBLE),
            "GetState",
            &(),
        )
        .map_err(Into::into)
        .and_then(|reply| Ok(reply.body().deserialize()?));
    states.is_ok_and(|bits| bits.first().is_some_and(|word| word >> STATE_ACTIVE & 1 == 1))
}

fn has_action(connection: &Connection, object: &Object) -> bool {
    let actions: Result<Vec<(String, String, String)>> = connection
        .call_method(
            Some(object.0.as_str()),
            object.1.as_str(),
            Some(ACTION),
            "GetActions",
            &(),
        )
        .map_err(Into::into)
        .and_then(|reply| Ok(reply.body().deserialize()?));
    // GTK translates the action name ("click" becomes "Pulsar"), so any action will do.
    actions.is_ok_and(|list| !list.is_empty())
}

/// The alert of the focused window, if one is open.
fn find_dialog(connection: &Connection, started: Instant) -> Result<Option<Dialog>> {
    let root: Object = (REGISTRY.to_owned(), OwnedObjectPath::try_from(ROOT)?);
    for app in children(connection, &root)? {
        // An app that vanished mid-walk is not an error.
        for frame in children(connection, &app).unwrap_or_default() {
            if started.elapsed() > WALK_BUDGET {
                anyhow::bail!("accessibility walk over budget");
            }
            if !is_active(connection, &frame) {
                continue;
            }
            if let Some(alert) = find_alert(connection, &frame, 0)? {
                // Windows with a dialog role but nothing to press are not asking anything.
                let dialog = read_alert(connection, &alert)?;
                if !dialog.buttons.is_empty() {
                    return Ok(Some(dialog));
                }
            }
        }
    }
    Ok(None)
}

fn find_alert(connection: &Connection, object: &Object, depth: usize) -> Result<Option<Object>> {
    // Inside the active window an error is a real error, not an empty window.
    if depth > 0 && matches!(role_name(connection, object)?.as_str(), "alert" | "alert dialog" | "dialog") {
        return Ok(Some(object.clone()));
    }
    if depth >= ALERT_DEPTH {
        return Ok(None);
    }
    for child in children(connection, object)? {
        if let Some(found) = find_alert(connection, &child, depth + 1)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn read_alert(connection: &Connection, alert: &Object) -> Result<Dialog> {
    let mut buttons = Vec::new();
    collect_buttons(connection, alert, 0, &mut buttons)?;
    Ok(Dialog {
        title: name(connection, alert).unwrap_or_default(),
        buttons,
    })
}

fn collect_buttons(connection: &Connection, object: &Object, depth: usize, out: &mut Vec<DialogButton>) -> Result<()> {
    if depth > BUTTON_DEPTH || out.len() >= MAX_BUTTONS {
        return Ok(());
    }
    if matches!(role_name(connection, object)?.as_str(), "push button" | "button") && has_action(connection, object) {
        let label = name(connection, object).unwrap_or_default();
        if !label.is_empty() {
            out.push(DialogButton {
                label,
                object: (object.0.clone(), object.1.to_string()),
            });
        }
        return Ok(());
    }
    for child in children(connection, object)? {
        collect_buttons(connection, &child, depth + 1, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Needs a session with an open dialog: `cargo test live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live() {
        let connection = super::connect().unwrap();
        let dialog = super::find_dialog(&connection, std::time::Instant::now()).unwrap();
        println!("{dialog:?}");
        if let Some(button) = dialog.and_then(|d| d.buttons.first().cloned()) {
            super::press(&button);
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
    }
}
