// SPDX-License-Identifier: GPL-3.0-or-later

use std::{thread, time::Duration};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::{MatchRule, message::Type};

use crate::events::{Bus, Ext};

const PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

pub fn watch(bus: Bus) -> Result<()> {
    thread::Builder::new()
        .name("upower-battery".to_owned())
        .spawn(move || {
            loop {
                if let Err(error) = listen(&bus) {
                    eprintln!("kait2en-touchbar-plus: battery listener: {error:#}");
                    thread::sleep(Duration::from_secs(10));
                }
            }
        })?;
    Ok(())
}

fn publish(proxy: &Proxy<'_>, bus: &Bus) -> Result<()> {
    let percent: f64 = proxy.get_property("Percentage")?;
    let state: u32 = proxy.get_property("State")?;
    bus.send(Ext::Battery(percent.round().clamp(0.0, 100.0) as u8, state == 1));
    Ok(())
}

fn listen(bus: &Bus) -> Result<()> {
    let connection = Connection::system().context("connect system bus")?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.UPower",
        PATH,
        "org.freedesktop.UPower.Device",
    )?;
    publish(&proxy, bus)?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(PATH)?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .build();
    for message in MessageIterator::for_match_rule(rule, &connection, None)? {
        message?;
        publish(&proxy, bus)?;
    }
    Err(anyhow::anyhow!("system bus iterator ended"))
}
