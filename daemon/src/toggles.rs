// SPDX-License-Identifier: GPL-3.0-or-later

//! State and switching of the quick toggles. Every command runs on its own
//! short-lived thread, never in the render loop, and reports through the bus.

use std::{
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
};

use crate::{
    events::{Bus, Ext},
    layout::Toggle,
};

static CAFFEINE: Mutex<Option<Child>> = Mutex::new(None);

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn run(program: &str, args: &[&str]) {
    if let Err(error) = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .status()
    {
        eprintln!("kait2en-touchbar-plus: {program}: {error}");
    }
}

fn caffeine_on() -> bool {
    let mut child = CAFFEINE.lock().unwrap_or_else(|e| e.into_inner());
    match child.as_mut() {
        Some(process) => match process.try_wait() {
            Ok(None) => true,
            _ => {
                *child = None;
                false
            }
        },
        None => false,
    }
}

pub fn read(toggle: Toggle) -> bool {
    match toggle {
        Toggle::Dnd => output("gsettings", &["get", "org.gnome.desktop.notifications", "show-banners"])
            .is_some_and(|v| v == "false"),
        Toggle::NightLight => output(
            "gsettings",
            &["get", "org.gnome.settings-daemon.plugins.color", "night-light-enabled"],
        )
        .is_some_and(|v| v == "true"),
        Toggle::Caffeine => caffeine_on(),
        Toggle::Wifi => output("nmcli", &["radio", "wifi"]).is_some_and(|v| v == "enabled"),
        Toggle::Bluetooth => {
            output("bluetoothctl", &["show"]).is_some_and(|v| v.contains("Powered: yes"))
        }
    }
}

fn read_all() -> [bool; 5] {
    let mut states = [false; 5];
    for toggle in Toggle::ALL {
        states[toggle.index()] = read(toggle);
    }
    states
}

/// Reads every state in the background.
pub fn refresh(bus: &Bus) {
    let bus = bus.clone();
    thread::spawn(move || bus.send(Ext::Toggles(read_all())));
}

/// Switches `toggle` to `on` in the background, then reports all states.
pub fn set(toggle: Toggle, on: bool, bus: &Bus) {
    let bus = bus.clone();
    thread::spawn(move || {
        let word = if on { "on" } else { "off" };
        match toggle {
            Toggle::Dnd => run(
                "gsettings",
                &["set", "org.gnome.desktop.notifications", "show-banners", if on { "false" } else { "true" }],
            ),
            Toggle::NightLight => run(
                "gsettings",
                &["set", "org.gnome.settings-daemon.plugins.color", "night-light-enabled", if on { "true" } else { "false" }],
            ),
            Toggle::Caffeine => {
                let mut child = CAFFEINE.lock().unwrap_or_else(|e| e.into_inner());
                if on && child.is_none() {
                    *child = Command::new("systemd-inhibit")
                        .args([
                            "--what=idle:sleep",
                            "--who=kait2en-touchbar-plus",
                            "--why=Cafeina",
                            "sleep",
                            "infinity",
                        ])
                        .stdout(Stdio::null())
                        .spawn()
                        .map_err(|error| eprintln!("kait2en-touchbar-plus: systemd-inhibit: {error}"))
                        .ok();
                } else if !on && let Some(mut process) = child.take() {
                    let _ = process.kill();
                    let _ = process.wait();
                }
            }
            Toggle::Wifi => run("nmcli", &["radio", "wifi", word]),
            Toggle::Bluetooth => run("bluetoothctl", &["power", word]),
        }
        bus.send(Ext::Toggles(read_all()));
    });
}

/// Ends the caffeine inhibitor when the daemon exits.
pub fn shutdown() {
    if let Some(mut process) = CAFFEINE.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = process.kill();
        let _ = process.wait();
    }
}
