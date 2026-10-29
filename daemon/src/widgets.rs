// SPDX-License-Identifier: GPL-3.0-or-later

//! Data behind the strip widgets and the readers that fill it.

use std::{fs, path::PathBuf};

use crate::renderer::WidgetKind;

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub temp_c: i16,
    pub icon: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct WidgetData {
    pub clock: String,
    /// Percent and whether it charges.
    pub battery: Option<(u8, bool)>,
    pub cpu: Option<u8>,
    pub memory: Option<u8>,
    pub temp_c: Option<i16>,
    pub fan_rpm: Option<u32>,
    /// Bytes per second received and sent.
    pub net: Option<(u64, u64)>,
    pub weather: Option<Weather>,
    pub next_event: Option<String>,
    pub pomodoro: Option<(&'static str, String)>,
    pub now_playing: Option<NowPlayingView>,
    pub panel: Option<PanelView>,
}

/// The open controls panel.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelView {
    pub zone: (u16, u16),
    pub rows: Vec<PanelRow>,
    /// The slider under the finger; the others dim.
    pub active: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelRow {
    pub icon: &'static str,
    pub percent: Option<u32>,
}

/// What the now playing widget draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlayingView {
    /// 52x52 grayscale cover.
    pub art: Option<Vec<u8>>,
    pub line: String,
    pub fraction: f32,
    /// Heights in px of the visualizer bars.
    pub bars: Option<[u8; 5]>,
}

/// Icon name (Lucide) and ASCII text of a widget.
pub fn view(kind: WidgetKind, data: &WidgetData) -> (Option<&'static str>, String) {
    let or_dashes = |text: Option<String>| text.unwrap_or_else(|| "--".to_owned());
    match kind {
        WidgetKind::Clock => (None, or_dashes(Some(data.clock.clone()).filter(|c| !c.is_empty()))),
        WidgetKind::Battery => match data.battery {
            Some((percent, charging)) => {
                let icon = if charging {
                    "battery-charging"
                } else if percent >= 60 {
                    "battery-full"
                } else if percent >= 25 {
                    "battery-medium"
                } else {
                    "battery-low"
                };
                (Some(icon), format!("{percent}%"))
            }
            None => (Some("battery-full"), "--".into()),
        },
        WidgetKind::Cpu => (Some("cpu"), or_dashes(data.cpu.map(|v| format!("{v}%")))),
        WidgetKind::Memory => (
            Some("memory-stick"),
            or_dashes(data.memory.map(|v| format!("{v}%"))),
        ),
        WidgetKind::Temperature => (
            Some("thermometer"),
            or_dashes(data.temp_c.map(|v| format!("{v}\u{b0}"))),
        ),
        WidgetKind::Fan => (Some("fan"), or_dashes(data.fan_rpm.map(|v| v.to_string()))),
        WidgetKind::Network => (
            Some("arrow-down-up"),
            or_dashes(data.net.map(|(rx, tx)| format!("{}/{}", rate(rx), rate(tx)))),
        ),
        WidgetKind::Weather => match &data.weather {
            Some(weather) => (Some(weather.icon), format!("{}\u{b0}", weather.temp_c)),
            None => (Some("cloud"), "--\u{b0}".into()),
        },
        WidgetKind::NextEvent => (Some("calendar"), or_dashes(data.next_event.clone())),
        WidgetKind::Pomodoro => match &data.pomodoro {
            Some((icon, text)) => (Some(icon), text.clone()),
            None => (Some("timer"), "--".into()),
        },
        WidgetKind::NowPlaying => (None, String::new()),
    }
}

/// Bytes per second as `80K` or `1.2M`.
pub fn rate(bytes: u64) -> String {
    let kb = bytes as f64 / 1024.0;
    if kb >= 1024.0 {
        format!("{:.1}M", kb / 1024.0)
    } else {
        format!("{}K", kb.round() as u64)
    }
}

/// Wall-clock time with a strftime format.
pub fn format_clock(format: &str) -> String {
    format_time(None, format)
}

/// Local time of a Unix timestamp (now when `None`) with a strftime format.
pub fn format_time(timestamp: Option<i64>, format: &str) -> String {
    let Ok(format) = std::ffi::CString::new(format) else {
        return String::new();
    };
    let mut buffer = [0u8; 64];
    unsafe {
        let now = timestamp.map_or_else(|| libc::time(std::ptr::null_mut()), |t| t as libc::time_t);
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::new();
        }
        let len = libc::strftime(buffer.as_mut_ptr().cast(), buffer.len(), format.as_ptr(), &tm);
        String::from_utf8_lossy(&buffer[..len]).into_owned()
    }
}

/// Milliseconds until the wall clock enters the next minute.
pub fn ms_to_next_minute() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    60_000 - now % 60_000
}

/// Busy and total jiffies of the aggregate `cpu` line.
pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|line| line.starts_with("cpu "))?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    if values.len() < 5 {
        return None;
    }
    let total: u64 = values.iter().take(8).sum();
    let idle = values[3] + values[4];
    Some((total - idle, total))
}

/// Used memory in percent.
pub fn parse_meminfo(text: &str) -> Option<u8> {
    let field = |name: &str| -> Option<u64> {
        text.lines()
            .find(|line| line.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    let total = field("MemTotal:")?;
    let available = field("MemAvailable:")?;
    (total > 0).then(|| ((total.saturating_sub(available)) * 100 / total) as u8)
}

/// Received and sent bytes summed over every interface but `lo`.
pub fn parse_net_dev(text: &str) -> (u64, u64) {
    let mut totals = (0, 0);
    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        if fields.len() >= 9 {
            totals.0 += fields[0];
            totals.1 += fields[8];
        }
    }
    totals
}

fn hwmon(name: &str, file: &str) -> Option<PathBuf> {
    fs::read_dir("/sys/class/hwmon").ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        let found = fs::read_to_string(path.join("name")).ok()?;
        (found.trim() == name && path.join(file).exists()).then(|| path.join(file))
    })
}

fn read_number(path: PathBuf) -> Option<i64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

pub fn read_temperature() -> Option<i16> {
    Some((read_number(hwmon("coretemp", "temp1_input")?)? / 1000) as i16)
}

pub fn read_fan() -> Option<u32> {
    read_number(hwmon("t2smc", "fan1_input")?).map(|v| v.max(0) as u32)
}

/// Keeps the previous readings to turn counters into rates.
#[derive(Default)]
pub struct Sampler {
    cpu: Option<(u64, u64)>,
    net: Option<(u64, u64, u64)>,
}

impl Sampler {
    /// Refreshes `data`; returns whether any shown value changed.
    pub fn sample(&mut self, data: &mut WidgetData, now_ms: u64) -> bool {
        let before = (
            data.cpu,
            data.memory,
            data.temp_c,
            data.fan_rpm,
            data.net.map(|(rx, tx)| (rate(rx), rate(tx))),
        );
        if let Some((busy, total)) = fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|t| parse_proc_stat(&t))
        {
            if let Some((old_busy, old_total)) = self.cpu
                && total > old_total
            {
                data.cpu = Some(((busy - old_busy) * 100 / (total - old_total)).min(100) as u8);
            }
            self.cpu = Some((busy, total));
        }
        data.memory = fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|t| parse_meminfo(&t));
        if let Ok(text) = fs::read_to_string("/proc/net/dev") {
            let (rx, tx) = parse_net_dev(&text);
            if let Some((old_rx, old_tx, old_ms)) = self.net
                && now_ms > old_ms
            {
                let ms = now_ms - old_ms;
                data.net = Some((
                    rx.saturating_sub(old_rx) * 1000 / ms,
                    tx.saturating_sub(old_tx) * 1000 / ms,
                ));
            }
            self.net = Some((rx, tx, now_ms));
        }
        data.temp_c = read_temperature();
        data.fan_rpm = read_fan();
        before
            != (
                data.cpu,
                data.memory,
                data.temp_c,
                data.fan_rpm,
                data.net.map(|(rx, tx)| (rate(rx), rate(tx))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_stat_sums_busy_and_total() {
        let text = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 1 1 1 1 1 1 1 1 1\n";
        assert_eq!(parse_proc_stat(text), Some((150, 1000)));
        assert_eq!(parse_proc_stat("nothing"), None);
    }

    #[test]
    fn parse_meminfo_reports_used_percent() {
        let text = "MemTotal:       1000 kB\nMemFree:  10 kB\nMemAvailable:    250 kB\n";
        assert_eq!(parse_meminfo(text), Some(75));
    }

    #[test]
    fn parse_net_dev_skips_loopback() {
        let text = "Inter-|   Receive\n face |bytes\n    lo: 500 1 0 0 0 0 0 0 500 1 0 0 0 0 0 0\nwlp5s0: 1000 1 0 0 0 0 0 0 200 1 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(text), (1000, 200));
    }

    #[test]
    fn rate_switches_to_megabytes() {
        assert_eq!(rate(81_920), "80K");
        assert_eq!(rate(1_258_291), "1.2M");
    }

    #[test]
    fn battery_icon_follows_level_and_charge() {
        let mut data = WidgetData::default();
        data.battery = Some((87, false));
        assert_eq!(view(WidgetKind::Battery, &data), (Some("battery-full"), "87%".into()));
        data.battery = Some((30, false));
        assert_eq!(view(WidgetKind::Battery, &data).0, Some("battery-medium"));
        data.battery = Some((10, false));
        assert_eq!(view(WidgetKind::Battery, &data).0, Some("battery-low"));
        data.battery = Some((10, true));
        assert_eq!(view(WidgetKind::Battery, &data).0, Some("battery-charging"));
    }
}
