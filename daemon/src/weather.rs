// SPDX-License-Identifier: GPL-3.0-or-later

//! Current weather from Open-Meteo. Coordinates come from GeoClue2 or from
//! the manual setting in layout.toml.

use std::{process::Command, thread, time::Duration};

use anyhow::{Context, Result, anyhow};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::{
    events::{Bus, Ext},
    layout::WeatherCfg,
    widgets::Weather,
};

/// Application id GeoClue checks against the installed .desktop file.
const DESKTOP_ID: &str = "org.kait2en.TouchbarPlus";
const LOCATION_WAIT: Duration = Duration::from_secs(20);

pub fn watch(bus: Bus, cfg: WeatherCfg) -> Result<()> {
    thread::Builder::new()
        .name("weather".to_owned())
        .spawn(move || {
            let mut coordinates = None;
            loop {
                let (latitude, longitude) = *coordinates.get_or_insert_with(|| locate(&cfg));
                match fetch(latitude, longitude) {
                    Ok(weather) => bus.send(Ext::Weather(weather)),
                    // Keeps the last value; a failed first fetch shows --.
                    Err(error) => eprintln!("kait2en-touchbar-plus: weather: {error:#}"),
                }
                thread::sleep(Duration::from_secs(cfg.refresh_min.max(1) * 60));
            }
        })?;
    Ok(())
}

fn locate(cfg: &WeatherCfg) -> (f64, f64) {
    if cfg.auto {
        match geoclue() {
            Ok(found) => {
                eprintln!("kait2en-touchbar-plus: weather location from geoclue");
                return found;
            }
            Err(error) => eprintln!(
                "kait2en-touchbar-plus: geoclue unavailable ({error:#}); manual fallback"
            ),
        }
    } else {
        eprintln!("kait2en-touchbar-plus: weather location manual");
    }
    (cfg.latitude, cfg.longitude)
}

fn geoclue() -> Result<(f64, f64)> {
    let connection = Connection::system().context("connect system bus")?;
    let manager = Proxy::new(
        &connection,
        "org.freedesktop.GeoClue2",
        "/org/freedesktop/GeoClue2/Manager",
        "org.freedesktop.GeoClue2.Manager",
    )?;
    let client_path: OwnedObjectPath = manager.call("GetClient", &())?;
    let client = Proxy::new(
        &connection,
        "org.freedesktop.GeoClue2",
        client_path.as_str().to_owned(),
        "org.freedesktop.GeoClue2.Client",
    )?;
    client.set_property("DesktopId", DESKTOP_ID)?;
    // 4 = city.
    client.set_property("RequestedAccuracyLevel", 4u32)?;
    client.call::<_, _, ()>("Start", &())?;
    let started = std::time::Instant::now();
    let result = loop {
        let location: OwnedObjectPath = client.get_property("Location")?;
        if location.as_str() != "/" {
            let proxy = Proxy::new(
                &connection,
                "org.freedesktop.GeoClue2",
                location.as_str().to_owned(),
                "org.freedesktop.GeoClue2.Location",
            )?;
            break Ok((proxy.get_property("Latitude")?, proxy.get_property("Longitude")?));
        }
        if started.elapsed() > LOCATION_WAIT {
            break Err(anyhow!("no location within {}s", LOCATION_WAIT.as_secs()));
        }
        thread::sleep(Duration::from_millis(500));
    };
    let _ = client.call::<_, _, ()>("Stop", &());
    result
}

fn fetch(latitude: f64, longitude: f64) -> Result<Weather> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}&current=temperature_2m,weather_code,is_day&timezone=auto"
    );
    let output = Command::new("curl")
        .args(["-sf", "--max-time", "10", &url])
        .output()
        .context("run curl")?;
    anyhow::ensure!(output.status.success(), "curl failed ({})", output.status);
    parse_open_meteo(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| anyhow!("unexpected Open-Meteo response"))
}

pub fn parse_open_meteo(body: &str) -> Option<Weather> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let current = json.get("current")?;
    let temperature = current.get("temperature_2m")?.as_f64()?;
    let code = current.get("weather_code")?.as_i64()?;
    let day = current.get("is_day")?.as_i64()? != 0;
    Some(Weather {
        temp_c: temperature.round() as i16,
        icon: wmo_glyph(code, day),
    })
}

/// Lucide icon for a WMO weather code.
pub fn wmo_glyph(code: i64, day: bool) -> &'static str {
    match code {
        0 if day => "sun",
        0 => "moon",
        1 | 2 => "cloud-sun",
        3 => "cloud",
        45 | 48 => "cloud-fog",
        51..=57 => "cloud-drizzle",
        61..=67 | 80..=82 => "cloud-rain",
        71..=77 | 85 | 86 => "cloud-snow",
        95..=99 => "cloud-lightning",
        _ => "cloud",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open_meteo_reads_current() {
        let body = r#"{"latitude":-33.45,"current":{"time":"2026-10-28T19:00","interval":900,"temperature_2m":22.7,"weather_code":1,"is_day":1}}"#;
        assert_eq!(
            parse_open_meteo(body),
            Some(Weather {
                temp_c: 23,
                icon: "cloud-sun"
            })
        );
        assert_eq!(parse_open_meteo("{}"), None);
    }

    #[test]
    fn wmo_glyph_table() {
        assert_eq!(wmo_glyph(0, true), "sun");
        assert_eq!(wmo_glyph(0, false), "moon");
        assert_eq!(wmo_glyph(3, true), "cloud");
        assert_eq!(wmo_glyph(48, true), "cloud-fog");
        assert_eq!(wmo_glyph(55, true), "cloud-drizzle");
        assert_eq!(wmo_glyph(81, true), "cloud-rain");
        assert_eq!(wmo_glyph(86, true), "cloud-snow");
        assert_eq!(wmo_glyph(96, true), "cloud-lightning");
        assert_eq!(wmo_glyph(1234, true), "cloud");
    }
}
