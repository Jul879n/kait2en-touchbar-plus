// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop notifications, read by monitoring the session bus. This needs
//! nothing from the shell extension, so notices work from the first start.

use std::{collections::HashMap, thread, time::Duration};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::OwnedValue;

use crate::events::{Bus, Ext};

type NotifyArgs = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    HashMap<String, OwnedValue>,
    i32,
);

pub fn watch(bus: Bus) -> Result<()> {
    thread::Builder::new()
        .name("notifications".to_owned())
        .spawn(move || {
            let mut reported = false;
            loop {
                if let Err(error) = listen(&bus) {
                    if !reported {
                        eprintln!("kait2en-touchbar-plus: notification monitor: {error:#}");
                        reported = true;
                    }
                }
                thread::sleep(Duration::from_secs(10));
            }
        })?;
    Ok(())
}

/// Whether GNOME shows notification banners (not "do not disturb"). Read
/// through `gsettings`, at most once every two seconds.
fn banners_on() -> bool {
    use std::{sync::Mutex, time::Instant};
    static LAST: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, on)) = *last
        && at.elapsed() < Duration::from_secs(2)
    {
        return on;
    }
    let on = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.notifications", "show-banners"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_none_or(|output| String::from_utf8_lossy(&output.stdout).trim() != "false");
    *last = Some((Instant::now(), on));
    on
}

fn listen(bus: &Bus) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(
                vec!["type='method_call',interface='org.freedesktop.Notifications',member='Notify'"],
                0u32,
            ),
        )
        .context("become bus monitor")?;
    for message in MessageIterator::from(&connection) {
        let message = message?;
        let header = message.header();
        if header.member().map(|member| member.as_str()) != Some("Notify") {
            continue;
        }
        let Ok((app, replaces_id, app_icon, summary, body, _, hints, _)) = message.body().deserialize::<NotifyArgs>() else {
            continue;
        };
        // Banners switched off in GNOME's settings stay off here, but urgent
        // notices (urgency 2) still show.
        let urgency = hints
            .get("urgency")
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(1);
        if urgency != 2 && !banners_on() {
            continue;
        }
        // The desktop entry names the app best; the icon name is the fallback.
        let entry = hints
            .get("desktop-entry")
            .and_then(|value| <&str>::try_from(value).ok())
            .map(str::to_owned)
            .unwrap_or(app_icon);
        let icon = crate::apps::notice_icon(&app, &entry).map(|path| path.to_string_lossy().into_owned());
        bus.send(Ext::Notification {
            app,
            title: summary,
            text: body,
            icon,
            replaces: replaces_id != 0,
        });
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}
