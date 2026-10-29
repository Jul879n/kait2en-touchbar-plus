// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MIN_TIMEOUT_MS: u64 = 5_000;
pub const DEFAULT_MAX_TIMEOUT_MS: u64 = 30_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub minimum_timeout_ms: u64,
    pub maximum_timeout_ms: u64,
    pub continuation_window_ms: u64,
    pub fn_long_press_ms: u64,
    pub active_brightness: u32,
    pub haptic_feedback: bool,
    pub haptic_touch_up: bool,
    pub key_color: String,
    pub two_finger_window_ms: u64,
    pub volume_swipe_step_px: u32,
    pub brightness_swipe_step_px: u32,
    pub mode_swipe_px: u32,
    pub follow_keyboard_backlight: bool,
    pub activity_backlight: bool,
    pub activity_timeout_ms: u64,
    pub show_track_changes: bool,
    pub always_on: bool,
    pub dim_after_ms: u64,
    pub dim_brightness: u32,
    pub pixel_shift: bool,
    pub pixel_shift_interval_ms: u64,
    pub screen_lock_dark: bool,
    pub notifications: bool,
    pub suggestions: bool,
    /// Show the apps of Super+Tab (or Alt+Tab) on the bar.
    pub app_switcher: bool,
    pub hold_slider_ms: u64,
    /// `auto` (system locale), `es` or `en`.
    pub language: String,
    /// `mono` (key color) or `full` (a color per icon, app icons in color).
    pub icon_color: String,
    /// Touch ID prompt: look `A` to `F`, a color per state (`auto` follows
    /// the key color), a text per state (empty uses the language) and motion.
    pub touch_id_style: String,
    pub touch_id_color_waiting: String,
    pub touch_id_color_retry: String,
    pub touch_id_color_failed: String,
    pub touch_id_color_matched: String,
    pub touch_id_text_waiting: String,
    pub touch_id_text_retry: String,
    pub touch_id_text_failed: String,
    pub touch_id_text_matched: String,
    /// `completa`, `sutil` or `ninguna`.
    pub touch_id_animation: String,
    /// Percent of the normal speed, 50..200.
    pub touch_id_speed: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            minimum_timeout_ms: DEFAULT_MIN_TIMEOUT_MS,
            maximum_timeout_ms: DEFAULT_MAX_TIMEOUT_MS,
            continuation_window_ms: 15_000,
            fn_long_press_ms: 600,
            active_brightness: 128,
            haptic_feedback: true,
            haptic_touch_up: true,
            key_color: "#dce6ff".into(),
            two_finger_window_ms: 100,
            volume_swipe_step_px: 40,
            brightness_swipe_step_px: 40,
            mode_swipe_px: 160,
            follow_keyboard_backlight: false,
            activity_backlight: true,
            activity_timeout_ms: 30_000,
            show_track_changes: true,
            always_on: true,
            dim_after_ms: 120_000,
            dim_brightness: 1,
            pixel_shift: true,
            pixel_shift_interval_ms: 180_000,
            screen_lock_dark: true,
            notifications: true,
            suggestions: true,
            app_switcher: true,
            hold_slider_ms: 400,
            language: "auto".into(),
            icon_color: "mono".into(),
            touch_id_style: "F".into(),
            touch_id_color_waiting: "auto".into(),
            touch_id_color_retry: "#ff9f0a".into(),
            touch_id_color_failed: "#ff453a".into(),
            touch_id_color_matched: "#34c759".into(),
            touch_id_text_waiting: String::new(),
            touch_id_text_retry: String::new(),
            touch_id_text_failed: String::new(),
            touch_id_text_matched: String::new(),
            touch_id_animation: "completa".into(),
            touch_id_speed: 100,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        if env::var_os("KAIT2EN_TOUCHBAR_CONFIG").is_none()
            && !plus_dir().join("config.toml").exists()
            && !config_home().join("kait2en-touchbar/config.toml").exists()
        {
            create_template();
        }
        let path = config_path();
        let mut config = match fs::read_to_string(&path) {
            Ok(body) => toml::from_str(&body)
                .with_context(|| format!("invalid configuration {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.minimum_timeout_ms >= DEFAULT_MIN_TIMEOUT_MS,
            "minimum_timeout_ms must be at least {DEFAULT_MIN_TIMEOUT_MS}"
        );
        anyhow::ensure!(
            self.maximum_timeout_ms >= self.minimum_timeout_ms,
            "maximum_timeout_ms must not be below minimum_timeout_ms"
        );
        anyhow::ensure!(
            self.fn_long_press_ms >= 300,
            "fn_long_press_ms is too short"
        );
        anyhow::ensure!(
            self.active_brightness <= 255,
            "active_brightness must be 0..255"
        );
        anyhow::ensure!(
            (30..=300).contains(&self.two_finger_window_ms),
            "two_finger_window_ms must be 30..300"
        );
        anyhow::ensure!(
            self.volume_swipe_step_px >= 10
                && self.brightness_swipe_step_px >= 10
                && self.mode_swipe_px >= 40,
            "swipe distances are too short"
        );
        anyhow::ensure!(
            (5_000..=300_000).contains(&self.activity_timeout_ms),
            "activity_timeout_ms must be 5000..300000"
        );
        anyhow::ensure!(
            self.pixel_shift_interval_ms >= 10_000,
            "pixel_shift_interval_ms must be at least 10000"
        );
        anyhow::ensure!(
            self.dim_after_ms == 0 || self.dim_after_ms >= 10_000,
            "dim_after_ms must be 0 (never) or at least 10000"
        );
        anyhow::ensure!(
            self.dim_brightness <= self.active_brightness,
            "dim_brightness must not exceed active_brightness"
        );
        anyhow::ensure!(
            (200..=1500).contains(&self.hold_slider_ms),
            "hold_slider_ms must be 200..1500"
        );
        anyhow::ensure!(
            matches!(self.language.as_str(), "auto" | "es" | "en"),
            "language must be auto, es or en"
        );
        anyhow::ensure!(
            matches!(self.icon_color.as_str(), "mono" | "full"),
            "icon_color must be mono or full"
        );
        anyhow::ensure!(
            matches!(self.touch_id_style.as_str(), "A" | "B" | "C" | "D" | "E" | "F"),
            "touch_id_style must be A to F"
        );
        anyhow::ensure!(
            matches!(self.touch_id_animation.as_str(), "completa" | "sutil" | "ninguna"),
            "touch_id_animation must be completa, sutil or ninguna"
        );
        anyhow::ensure!(
            (50..=200).contains(&self.touch_id_speed),
            "touch_id_speed must be 50..200"
        );
        for (name, value) in [
            ("touch_id_color_waiting", &self.touch_id_color_waiting),
            ("touch_id_color_retry", &self.touch_id_color_retry),
            ("touch_id_color_failed", &self.touch_id_color_failed),
            ("touch_id_color_matched", &self.touch_id_color_matched),
        ] {
            anyhow::ensure!(
                value == "auto" || parse_color(value).is_some(),
                "{name} must be auto or #rrggbb"
            );
        }
        for (name, value) in [
            ("touch_id_text_waiting", &self.touch_id_text_waiting),
            ("touch_id_text_retry", &self.touch_id_text_retry),
            ("touch_id_text_failed", &self.touch_id_text_failed),
            ("touch_id_text_matched", &self.touch_id_text_matched),
        ] {
            anyhow::ensure!(
                value.len() <= 24 && value.is_ascii(),
                "{name} must be at most 24 ASCII characters"
            );
        }
        parse_color(&self.key_color)
            .with_context(|| format!("key_color {:?} must be #rrggbb", self.key_color))?;
        self.continuation_window_ms = self.continuation_window_ms.max(1_000);
        Ok(())
    }

    /// The Touch ID prompt as configured.
    pub fn touch_id_look(&self) -> crate::renderer::TouchIdLook {
        let color = |value: &str| parse_color(value);
        crate::renderer::TouchIdLook {
            style: self.touch_id_style.chars().next().unwrap_or('F'),
            colors: [
                color(&self.touch_id_color_waiting),
                color(&self.touch_id_color_retry),
                color(&self.touch_id_color_failed),
                color(&self.touch_id_color_matched),
            ],
            texts: [
                self.touch_id_text_waiting.clone(),
                self.touch_id_text_retry.clone(),
                self.touch_id_text_failed.clone(),
                self.touch_id_text_matched.clone(),
            ],
            motion: match self.touch_id_animation.as_str() {
                "ninguna" => 0,
                "sutil" => 1,
                _ => 2,
            },
        }
    }

    pub fn key_color(&self) -> u32 {
        parse_color(&self.key_color).unwrap_or(0x00dce6ff)
    }
}

fn parse_color(value: &str) -> Option<u32> {
    let hex = value.strip_prefix('#')?;
    (hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

fn config_home() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        })
}

/// Directory of the fork's own files (config, layout, keys).
pub fn plus_dir() -> PathBuf {
    config_home().join("kait2en-touchbar-plus")
}

/// The fork never writes to KaiT2en's paths: upstream rejects unknown keys.
pub fn config_path() -> PathBuf {
    if let Some(path) = env::var_os("KAIT2EN_TOUCHBAR_CONFIG") {
        return path.into();
    }
    [
        plus_dir().join("config.toml"),
        config_home().join("kait2en-touchbar/config.toml"),
    ]
    .into_iter()
    .find(|path| path.exists())
    .unwrap_or_else(|| PathBuf::from("/etc/kait2en/touchbar.toml"))
}

const TEMPLATE: &str = include_str!("../config-template.toml");

fn create_template() {
    let target = plus_dir().join("config.toml");
    let created = fs::create_dir_all(plus_dir())
        .and_then(|()| OpenOptions::new().write(true).create_new(true).open(&target))
        .and_then(|mut file| file.write_all(TEMPLATE.as_bytes()));
    if let Err(error) = created {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            eprintln!(
                "kait2en-touchbar-plus: could not create {}: {error}",
                target.display()
            );
        }
    }
}

pub fn state_path() -> PathBuf {
    if let Some(path) = env::var_os("KAIT2EN_TOUCHBAR_STATE") {
        return path.into();
    }
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/state")
        })
        .join("kait2en-touchbar-plus/state.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_id_settings_are_validated() {
        let check = |body: &str| toml::from_str::<Config>(body).unwrap().validate().map(|()| ());
        assert!(check("touch_id_style = \"B\"\ntouch_id_color_waiting = \"#4da3ff\"").is_ok());
        assert!(check("touch_id_style = \"G\"").is_err());
        assert!(check("touch_id_color_failed = \"red\"").is_err());
        assert!(check("touch_id_color_failed = \"#+12345\"").is_err());
        assert!(check("touch_id_speed = 300").is_err());
        assert!(check("touch_id_animation = \"mucha\"").is_err());
        assert!(check("touch_id_text_retry = \"café\"").is_err());
        assert!(check("touch_id_text_retry = \"Otra vez\"").is_ok());
    }

    #[test]
    fn plus_fields_default_and_validate() {
        let mut config: Config = toml::from_str("").unwrap();
        config.validate().unwrap();
        assert!(config.always_on);
        assert_eq!(config.dim_after_ms, 120_000);
        assert_eq!(config.hold_slider_ms, 400);
        let mut bad: Config = toml::from_str("dim_after_ms = 500").unwrap();
        let error = bad.validate().unwrap_err().to_string();
        assert!(error.contains("dim_after_ms must be"), "{error}");
    }
}
