// SPDX-License-Identifier: GPL-3.0-or-later

//! Current volume and display brightness for the swipe feedback. The daemon
//! only sends keys. The desktop applies them, so these are read back.

use std::{
    fs,
    io::Write,
    os::unix::net::UnixStream,
    path::Path,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use anyhow::Result;

use crate::{layout::Control, renderer::LevelKind};

/// Reads levels on a worker thread: `wpctl` takes 10-20 ms, which would
/// otherwise stall touch handling in the middle of a swipe.
pub struct Reader {
    requests: Sender<LevelKind>,
    results: Receiver<(LevelKind, Level)>,
}

impl Reader {
    /// Returns the reader and a socket that becomes readable with results.
    pub fn spawn() -> Result<(Self, UnixStream)> {
        let (requests, jobs) = mpsc::channel::<LevelKind>();
        let (done, results) = mpsc::channel();
        let (mut wake_writer, wake_reader) = UnixStream::pair()?;
        thread::Builder::new()
            .name("level-reader".to_owned())
            .spawn(move || {
                while let Ok(mut kind) = jobs.recv() {
                    // Only the newest request matters.
                    while let Ok(newer) = jobs.try_recv() {
                        kind = newer;
                    }
                    let level = match kind {
                        LevelKind::Volume => volume(),
                        LevelKind::Brightness => brightness(),
                    };
                    if done.send((kind, level)).is_err() {
                        return;
                    }
                    let _ = wake_writer.write_all(&[1]);
                }
            })?;
        Ok((Self { requests, results }, wake_reader))
    }

    pub fn request(&self, kind: LevelKind) {
        let _ = self.requests.send(kind);
    }

    pub fn results(&self) -> impl Iterator<Item = (LevelKind, Level)> + '_ {
        self.results.try_iter()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Level {
    pub percent: Option<u32>,
    pub muted: bool,
}

pub fn volume() -> Level {
    Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or_else(Level::default, |text| parse_wpctl(&text))
}

fn parse_wpctl(text: &str) -> Level {
    let percent = text
        .strip_prefix("Volume:")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| (value * 100.0).round() as u32);
    Level {
        percent,
        muted: text.contains("[MUTED]"),
    }
}

/// The display backlight, chosen like desktops do: firmware before platform
/// before raw, never one of the Touch Bar's own controllers.
pub fn brightness() -> Level {
    let percent = screen_backlight().and_then(|path| {
        let current: u64 = read(&path, "brightness")?.parse().ok()?;
        let max: u64 = read(&path, "max_brightness")?.parse().ok()?;
        (max > 0).then(|| ((current * 100 + max / 2) / max) as u32)
    });
    Level {
        percent,
        muted: false,
    }
}

/// Current level of a panel slider, in percent.
pub fn read_control(control: Control) -> Option<u32> {
    match control {
        Control::Volume => volume().percent,
        Control::Brightness => brightness().percent,
        Control::Keyboard => {
            let max = crate::kbdlight::maximum_brightness().ok()?;
            let now = crate::kbdlight::brightness().ok()?;
            (max > 0).then(|| (u64::from(now) * 100 / u64::from(max)) as u32)
        }
    }
}

/// Sets a slider in the background; each command takes a few milliseconds.
pub fn set_control(control: Control, percent: u32) {
    let percent = percent.min(100);
    thread::spawn(move || {
        let status = match control {
            Control::Volume => Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{percent}%")])
                .status(),
            Control::Keyboard => Command::new("busctl")
                .args([
                    "--user",
                    "set-property",
                    "org.gnome.SettingsDaemon.Power",
                    "/org/gnome/SettingsDaemon/Power",
                    "org.gnome.SettingsDaemon.Power.Keyboard",
                    "Brightness",
                    "i",
                    &percent.to_string(),
                ])
                .status(),
            Control::Brightness => {
                let Some(path) = screen_backlight() else { return };
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
                let max: u64 = read(&path, "max_brightness").and_then(|v| v.parse().ok()).unwrap_or(0);
                // Never fully dark: the screen would be unreadable.
                let value = (max * u64::from(percent.max(1)) / 100).max(1);
                Command::new("busctl")
                    .args([
                        "--system",
                        "call",
                        "org.freedesktop.login1",
                        "/org/freedesktop/login1/session/auto",
                        "org.freedesktop.login1.Session",
                        "SetBrightness",
                        "ssu",
                        "backlight",
                        &name,
                        &value.to_string(),
                    ])
                    .stdout(std::process::Stdio::null())
                    .status()
            }
        };
        if let Err(error) = status {
            eprintln!("kait2en-touchbar-plus: set {control:?}: {error}");
        }
    });
}

fn screen_backlight() -> Option<std::path::PathBuf> {
    fs::read_dir("/sys/class/backlight")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name != "t2tb_backlight" && name != "appletb_backlight"
        })
        .filter_map(|path| Some((type_rank(&read(&path, "type")?)?, path)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, path)| path)
}

fn type_rank(kind: &str) -> Option<u8> {
    match kind {
        "firmware" => Some(0),
        "platform" => Some(1),
        "raw" => Some(2),
        _ => None,
    }
}

fn read(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name))
        .ok()
        .map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpctl_output_is_parsed() {
        assert_eq!(
            parse_wpctl("Volume: 0.36\n"),
            Level {
                percent: Some(36),
                muted: false
            }
        );
        assert_eq!(
            parse_wpctl("Volume: 1.20 [MUTED]\n"),
            Level {
                percent: Some(120),
                muted: true
            }
        );
        assert_eq!(parse_wpctl("garbage"), Level::default());
    }
}
