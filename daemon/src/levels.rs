// SPDX-License-Identifier: GPL-3.0-or-later

//! Current volume and display brightness for the swipe feedback. The daemon
//! only sends keys. The desktop applies them, so these are read back.

use std::{
    fs,
    io::Write,
    os::unix::net::UnixStream,
    path::Path,
    process::Command,
    sync::{
        OnceLock,
        mpsc::{self, Receiver, Sender},
    },
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

/// Corre `wpctl` y, si falla (p. ej. no resuelve el sink por defecto cuando
/// es un filter-chain), el equivalente de `pactl`, que sí lo resuelve.
fn sink_status(wpctl: &[&str], pactl: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    match Command::new("wpctl").args(wpctl).output() {
        Ok(out) if out.status.success() => return Ok(out.status),
        Ok(out) => eprintln!(
            "kait2en-touchbar-plus: wpctl {wpctl:?} -> {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(error) => eprintln!("kait2en-touchbar-plus: wpctl {wpctl:?}: {error}"),
    }
    let out = Command::new("pactl").env("LC_ALL", "C").args(pactl).output()?;
    if !out.status.success() {
        eprintln!(
            "kait2en-touchbar-plus: pactl {pactl:?} -> {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.status)
}

/// El silencio lo puso el 0 % del propio slider (no un toggle del usuario).
static MUTED_BY_ZERO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Lee el volumen. Si el 0 % del slider lo silenció y otro programa lo subió, quita ese silencio.
pub fn volume() -> Level {
    let mut level = read_volume();
    let flag = &MUTED_BY_ZERO;
    if flag.load(std::sync::atomic::Ordering::Relaxed) {
        if !level.muted {
            flag.store(false, std::sync::atomic::Ordering::Relaxed);
        } else if level.percent.is_some_and(|p| p > 0) {
            flag.store(false, std::sync::atomic::Ordering::Relaxed);
            let _ = sink_status(
                &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"],
                &["set-sink-mute", "@DEFAULT_SINK@", "0"],
            );
            level.muted = false;
        }
    }
    level
}

fn read_volume() -> Level {
    let wpctl = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| parse_wpctl(&text));
    if let Some(level) = wpctl {
        return level;
    }
    let pactl = |args: &[&str]| {
        Command::new("pactl")
            .env("LC_ALL", "C")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
    };
    let percent = pactl(&["get-sink-volume", "@DEFAULT_SINK@"]).and_then(|text| {
        let (before, _) = text.split_once('%')?;
        before.rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
    });
    let muted = pactl(&["get-sink-mute", "@DEFAULT_SINK@"]).is_some_and(|text| text.contains("yes"));
    Level { percent, muted }
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
        (max > 0).then(|| backlight_percent(current, max))
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

/// Flips the sound's mute in the background.
pub fn toggle_mute() {
    MUTED_BY_ZERO.store(false, std::sync::atomic::Ordering::Relaxed);
    thread::spawn(|| {
        let _ = sink_status(
            &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
            &["set-sink-mute", "@DEFAULT_SINK@", "toggle"],
        );
    });
}

/// Queues a slider change for the worker. One thread applies them in order
/// and keeps only the newest value per slider, so a slow command can never
/// land after a newer one and undo it.
pub fn set_control(control: Control, percent: u32) {
    static QUEUE: OnceLock<Sender<(Control, u32)>> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (sender, jobs) = mpsc::channel::<(Control, u32)>();
        thread::Builder::new()
            .name("level-writer".to_owned())
            .spawn(move || {
                while let Ok(first) = jobs.recv() {
                    let mut latest = [None; 3];
                    latest[first.0.index()] = Some(first);
                    while let Ok(job) = jobs.try_recv() {
                        latest[job.0.index()] = Some(job);
                    }
                    for (control, percent) in latest.into_iter().flatten() {
                        apply_control(control, percent.min(100));
                    }
                }
            })
            .expect("spawn level-writer");
        sender
    });
    let _ = queue.send((control, percent));
}

/// Primer repetido tras el toque en un botón −/+ del panel.
pub const REPEAT_FIRST_MS: u64 = 350;

/// Intervalo entre pasos de 1 % según cuánto lleva mantenido.
pub fn repeat_interval_ms(held_ms: u64) -> u64 {
    match held_ms {
        0..1000 => 150,
        1000..2000 => 90,
        2000..3000 => 50,
        _ => 25,
    }
}

/// `level` movido `delta` puntos, dentro de 0..=100 (valores >100 de wpctl se tratan como 100).
pub fn stepped(level: u32, delta: i32) -> u32 {
    (i64::from(level.min(100)) + i64::from(delta)).clamp(0, 100) as u32
}

/// Porcentaje de un valor de hardware: el mínimo visible (1) cuenta como 0 %, aunque `max` sea pequeño.
fn backlight_percent(current: u64, max: u64) -> u32 {
    if current <= 1 {
        return 0;
    }
    ((current * 100 + max / 2) / max) as u32
}

/// Valor de hardware para `percent`: 0 % es el mínimo visible (1), nunca negro.
fn backlight_value(max: u64, percent: u32) -> u64 {
    (max * u64::from(percent.min(100)) / 100).max(1)
}

fn apply_control(control: Control, percent: u32) {
    let status = match control {
        Control::Volume => {
            // 0 % es silencio (el filter-chain sigue sonando a 0 %); cualquier otro valor es querer oír.
            let mute = if percent == 0 { "1" } else { "0" };
            MUTED_BY_ZERO.store(percent == 0, std::sync::atomic::Ordering::Relaxed);
            let _ = sink_status(
                &["set-mute", "@DEFAULT_AUDIO_SINK@", mute],
                &["set-sink-mute", "@DEFAULT_SINK@", mute],
            );
            let value = format!("{percent}%");
            sink_status(
                &["set-volume", "@DEFAULT_AUDIO_SINK@", &value],
                &["set-sink-volume", "@DEFAULT_SINK@", &value],
            )
        }
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
            let value = backlight_value(max, percent);
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
    fn backlight_percent_floor() {
        assert_eq!(backlight_percent(1, 65535), 0);
        assert_eq!(backlight_percent(1, 100), 0);
        assert_eq!(backlight_percent(2, 100), 2);
        assert_eq!(backlight_percent(65535, 65535), 100);
    }

    #[test]
    fn backlight_value_floor() {
        assert_eq!(backlight_value(65535, 0), 1);
        assert_eq!(backlight_value(65535, 100), 65535);
        assert_eq!(backlight_value(65535, 1), 655);
        assert_eq!(backlight_value(0, 50), 1);
    }

    #[test]
    fn repeat_helpers() {
        assert_eq!(repeat_interval_ms(0), 150);
        assert_eq!(repeat_interval_ms(999), 150);
        assert_eq!(repeat_interval_ms(1000), 90);
        assert_eq!(repeat_interval_ms(2500), 50);
        assert_eq!(repeat_interval_ms(10_000), 25);
        assert_eq!(stepped(0, -1), 0);
        assert_eq!(stepped(100, 1), 100);
        assert_eq!(stepped(120, -1), 99);
        assert_eq!(stepped(41, 1), 42);
    }

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
