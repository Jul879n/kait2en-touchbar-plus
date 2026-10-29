// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether the screen is locked or the monitors are off. The bar follows
//! both: a dark screen means a dark bar.

use std::{collections::HashMap, thread, time::Duration};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::OwnedValue;
use zbus::{MatchRule, message::Type};

use crate::events::{Bus, Ext};

const MUTTER: &str = "org.gnome.Mutter.DisplayConfig";
const MUTTER_PATH: &str = "/org/gnome/Mutter/DisplayConfig";

pub fn watch(bus: Bus) -> Result<()> {
    let power = bus.clone();
    thread::Builder::new()
        .name("screensaver-signals".to_owned())
        .spawn(move || retry("screen lock listener", || lock_listen(&bus)))?;
    thread::Builder::new()
        .name("display-power".to_owned())
        .spawn(move || retry("display power listener", || power_listen(&power)))?;
    Ok(())
}

fn retry(name: &str, listen: impl Fn() -> Result<()>) {
    let mut reported = false;
    loop {
        if let Err(error) = listen() {
            if !reported {
                eprintln!("kait2en-touchbar-plus: {name}: {error:#}");
                reported = true;
            }
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn lock_listen(bus: &Bus) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    // The first state, so a lock that began before the daemon is not missed.
    let (active,): (bool,) = connection
        .call_method(
            Some("org.gnome.ScreenSaver"),
            "/org/gnome/ScreenSaver",
            Some("org.gnome.ScreenSaver"),
            "GetActive",
            &(),
        )?
        .body()
        .deserialize()?;
    bus.send(Ext::ScreenLocked(active));
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path("/org/gnome/ScreenSaver")?
        .interface("org.gnome.ScreenSaver")?
        .member("ActiveChanged")?
        .build();
    for message in MessageIterator::for_match_rule(rule, &connection, None)? {
        let Ok((active,)) = message?.body().deserialize::<(bool,)>() else {
            continue;
        };
        bus.send(Ext::ScreenLocked(active));
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}

/// Mutter's `PowerSaveMode` is 0 while the monitors are on.
fn power_listen(bus: &Bus) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    let (mode,): (OwnedValue,) = connection
        .call_method(
            Some(MUTTER),
            MUTTER_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(MUTTER, "PowerSaveMode"),
        )?
        .body()
        .deserialize()?;
    bus.send(Ext::ScreenPower(i32::try_from(mode)? == 0));
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(MUTTER_PATH)?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .build();
    for message in MessageIterator::for_match_rule(rule, &connection, None)? {
        let message = message?;
        let Ok((interface, changed, _)) =
            message.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
        else {
            continue;
        };
        if interface != MUTTER {
            continue;
        }
        if let Some(mode) = changed.get("PowerSaveMode").and_then(|value| i32::try_from(value).ok()) {
            bus.send(Ext::ScreenPower(mode == 0));
        }
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}
