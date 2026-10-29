// SPDX-License-Identifier: GPL-3.0-or-later

//! The configurable media row: `layout.toml` lists the items of the app zone
//! (per app profile) and of the fixed control strip on the right.

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::{config::plus_dir, keys::CustomAction, lucide};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Toggle {
    Dnd,
    NightLight,
    Caffeine,
    Wifi,
    Bluetooth,
}

impl Toggle {
    pub const ALL: [Toggle; 5] = [
        Self::Dnd,
        Self::NightLight,
        Self::Caffeine,
        Self::Wifi,
        Self::Bluetooth,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    fn parse(what: &str) -> Option<Self> {
        Some(match what {
            "dnd" => Self::Dnd,
            "night_light" => Self::NightLight,
            "caffeine" => Self::Caffeine,
            "wifi" => Self::Wifi,
            "bluetooth" => Self::Bluetooth,
            _ => return None,
        })
    }
}

/// The levels the controls panel can set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Volume,
    Brightness,
    Keyboard,
}

impl Control {
    pub const ALL: [Control; 3] = [Self::Volume, Self::Brightness, Self::Keyboard];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Volume => "volume-2",
            Self::Brightness => "sun",
            Self::Keyboard => "keyboard",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "volume" => Self::Volume,
            "brightness" => Self::Brightness,
            "keyboard" => Self::Keyboard,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ControlsCfg {
    pub items: Vec<Control>,
    /// Panel width; 0 takes the whole app zone.
    pub width_px: u16,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeySpec {
    /// PNG of an application icon, drawn in color.
    pub app_icon: Option<&'static str>,
    pub label: Option<&'static str>,
    pub icon: Option<&'static str>,
    pub action: CustomAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Brightness,
    KeyboardLight,
    Playback,
    Volume,
    Key(KeySpec),
    Screenshot,
    Toggle(Toggle),
    NowPlaying,
    Clock,
    Weather,
    Battery,
    Cpu,
    Memory,
    Temperature,
    Fan,
    Network,
    NextEvent,
    Pomodoro,
    Emoji,
    Windows,
    /// Blank columns of the grid.
    Spacer,
    /// One button that opens the volume, brightness and keyboard sliders.
    Controls,
    /// Keys of the temporary rows (suggestions, emoji, windows).
    Transient(TransientKey),
}

/// Role of a dialog key, so each reads at a glance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogTone {
    Title,
    Neutral,
    Accent,
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TransientGlyph {
    Icon(&'static str),
    Text(&'static str),
    Tone(&'static str, DialogTone),
    /// Icon file of an app and whether it is the selected one.
    App(&'static str, bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TransientAction {
    Suggest(u32),
    /// Index into the emoji favorites.
    Emoji(usize),
    Window(u32),
    NextPage,
    Close,
    /// Index into the buttons of the open dialog of the focused app.
    Dialog(usize),
    /// Text only: the title of a dialog.
    Label,
    /// Index into the apps of the open Super+Tab switcher.
    SwitchApp(usize),
    /// A window of the selected app in the switcher.
    SwitchWindow(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransientKey {
    pub glyph: TransientGlyph,
    pub action: TransientAction,
}

/// One copy of every distinct label (short ones, at most a few dozen bytes),
/// kept for the life of the process. Rows built from live text reuse them.
pub fn intern(text: &str) -> &'static str {
    use std::{collections::HashSet, sync::Mutex};
    static POOL: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let pool = pool.get_or_insert_with(HashSet::new);
    if let Some(found) = pool.get(text) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    pool.insert(leaked);
    leaked
}

/// Where an item sits on the 24-column grid. Missing values follow the
/// previous item and the item's natural width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slot {
    /// First column, 0-based (1-based in the file).
    pub col: Option<u16>,
    pub span: Option<u16>,
}

pub const GRID_COLS: u16 = 24;

#[derive(Clone, Debug)]
pub struct Profile {
    #[allow(dead_code)]
    pub name: String,
    pub apps: Vec<String>,
    pub items: Vec<ItemKind>,
    pub slots: Vec<Slot>,
}

#[derive(Clone, Debug)]
pub struct WeatherCfg {
    pub auto: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub refresh_min: u64,
}

#[derive(Clone, Debug)]
pub struct Favorite {
    pub icon: &'static str,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct LayoutConfig {
    pub strip: Vec<ItemKind>,
    pub strip_slots: Vec<Slot>,
    pub default: Vec<ItemKind>,
    pub default_slots: Vec<Slot>,
    pub profiles: Vec<Profile>,
    pub clock_format: String,
    pub weather: WeatherCfg,
    pub work_min: u64,
    pub break_min: u64,
    pub visualizer: bool,
    pub controls: ControlsCfg,
    pub favorites: Vec<Favorite>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    strip: Option<Vec<toml::Value>>,
    default: Option<Vec<toml::Value>>,
    #[serde(default)]
    profile: Vec<ProfileRaw>,
    #[serde(default)]
    clock: ClockRaw,
    #[serde(default)]
    weather: WeatherRaw,
    #[serde(default)]
    pomodoro: PomodoroRaw,
    #[serde(default)]
    now_playing: NowPlayingRaw,
    #[serde(default)]
    controls: ControlsRaw,
    emoji: Option<EmojiRaw>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileRaw {
    name: String,
    #[serde(default)]
    apps: Vec<String>,
    #[serde(default)]
    items: Vec<toml::Value>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ClockRaw {
    format: String,
}

impl Default for ClockRaw {
    fn default() -> Self {
        Self {
            format: "%H:%M".into(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WeatherRaw {
    location: String,
    latitude: f64,
    longitude: f64,
    refresh_min: u64,
}

impl Default for WeatherRaw {
    fn default() -> Self {
        Self {
            location: "auto".into(),
            latitude: -33.45,
            longitude: -70.67,
            refresh_min: 15,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PomodoroRaw {
    work_min: u64,
    break_min: u64,
}

impl Default for PomodoroRaw {
    fn default() -> Self {
        Self {
            work_min: 25,
            break_min: 5,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ControlsRaw {
    items: Vec<String>,
    width_px: u16,
    timeout_ms: u64,
}

impl Default for ControlsRaw {
    fn default() -> Self {
        Self {
            items: vec!["volume".into(), "brightness".into(), "keyboard".into()],
            width_px: 0,
            timeout_ms: 3000,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NowPlayingRaw {
    visualizer: bool,
}

impl Default for NowPlayingRaw {
    fn default() -> Self {
        Self { visualizer: true }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmojiRaw {
    #[serde(default)]
    favorites: Vec<FavoriteRaw>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FavoriteRaw {
    icon: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemRaw {
    #[serde(rename = "type")]
    kind: String,
    label: Option<String>,
    icon: Option<String>,
    send: Option<String>,
    run: Option<String>,
    what: Option<String>,
    /// Desktop id of an application to launch, e.g. `org.gnome.Nautilus.desktop`.
    app: Option<String>,
    /// First column (1 to 24) and width in columns of the item on the grid.
    col: Option<u16>,
    span: Option<u16>,
}

const TEMPLATE: &str = include_str!("../layout-template.toml");

pub fn path() -> PathBuf {
    env::var_os("KAIT2EN_TOUCHBAR_LAYOUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| plus_dir().join("layout.toml"))
}

fn create_template(path: &std::path::Path) {
    let created = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| OpenOptions::new().write(true).create_new(true).open(path))
        .and_then(|mut file| file.write_all(TEMPLATE.as_bytes()));
    if let Err(error) = created {
        eprintln!(
            "kait2en-touchbar-plus: could not create {}: {error}",
            path.display()
        );
    }
}

/// Defaults are the shipped template.
pub fn defaults() -> LayoutConfig {
    parse(TEMPLATE, false).expect("bundled layout template is valid")
}

/// Runtime load: a missing file is created from the template, a broken item
/// is reported and skipped, an unreadable file means the defaults.
pub fn load() -> LayoutConfig {
    let path = path();
    let body = match fs::read_to_string(&path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_template(&path);
            return defaults();
        }
        Err(error) => {
            eprintln!("kait2en-touchbar-plus: read {}: {error}", path.display());
            return defaults();
        }
    };
    parse(&body, false).unwrap_or_else(|error| {
        eprintln!("kait2en-touchbar-plus: {}: {error:#}", path.display());
        defaults()
    })
}

/// For `--check-config`: any problem is an error. A missing file is fine.
pub fn check() -> Result<()> {
    let path = path();
    match fs::read_to_string(&path) {
        Ok(body) => parse(&body, true)
            .map(|_| ())
            .with_context(|| format!("invalid layout {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn items(raw: Vec<toml::Value>, strict: bool, scope: &str) -> Result<(Vec<ItemKind>, Vec<Slot>)> {
    let mut out = Vec::new();
    let mut slots = Vec::new();
    for (index, value) in raw.into_iter().enumerate() {
        match item(value) {
            Ok((kind, slot)) => {
                out.push(kind);
                slots.push(slot);
            }
            Err(error) if strict => bail!("{scope} item {}: {error:#}", index + 1),
            Err(error) => eprintln!(
                "kait2en-touchbar-plus: {scope} item {}: {error:#}",
                index + 1
            ),
        }
    }
    Ok((out, slots))
}

fn item(value: toml::Value) -> Result<(ItemKind, Slot)> {
    let raw: ItemRaw = value.try_into().context("invalid item")?;
    let slot = place(&raw)?;
    let kind = kind(raw)?;
    // With only a column, the item still has to fit its natural width.
    if let (Some(col), None) = (slot.col, slot.span) {
        let natural = crate::renderer::natural_cols(&kind);
        if natural > 0 && col + natural > GRID_COLS {
            bail!("col {} leaves the {GRID_COLS} columns for an item {natural} columns wide", col + 1);
        }
    }
    Ok((kind, slot))
}

fn place(raw: &ItemRaw) -> Result<Slot> {
    let in_grid = |value: u16| (1..=GRID_COLS).contains(&value);
    if raw.col.is_some_and(|col| !in_grid(col)) || raw.span.is_some_and(|span| !in_grid(span)) {
        bail!("col and span must be 1 to {GRID_COLS}");
    }
    if let (Some(col), Some(span)) = (raw.col, raw.span) {
        if col + span - 1 > GRID_COLS {
            bail!("col {col} with span {span} leaves the {GRID_COLS} columns");
        }
    }
    Ok(Slot { col: raw.col.map(|col| col - 1), span: raw.span })
}

fn kind(raw: ItemRaw) -> Result<ItemKind> {
    let simple = |kind: ItemKind| {
        if raw.label.is_some()
            || raw.icon.is_some()
            || raw.send.is_some()
            || raw.run.is_some()
            || raw.what.is_some()
            || raw.app.is_some()
        {
            Err(anyhow!("type {:?} takes no other fields", raw.kind))
        } else {
            Ok(kind)
        }
    };
    match raw.kind.as_str() {
        "brightness" => simple(ItemKind::Brightness),
        "keyboard_light" => simple(ItemKind::KeyboardLight),
        "playback" => simple(ItemKind::Playback),
        "volume" => simple(ItemKind::Volume),
        "screenshot" => simple(ItemKind::Screenshot),
        "now_playing" => simple(ItemKind::NowPlaying),
        "clock" => simple(ItemKind::Clock),
        "weather" => simple(ItemKind::Weather),
        "battery" => simple(ItemKind::Battery),
        "cpu" => simple(ItemKind::Cpu),
        "memory" => simple(ItemKind::Memory),
        "temperature" => simple(ItemKind::Temperature),
        "fan" => simple(ItemKind::Fan),
        "network" => simple(ItemKind::Network),
        "next_event" => simple(ItemKind::NextEvent),
        "pomodoro" => simple(ItemKind::Pomodoro),
        "emoji" => simple(ItemKind::Emoji),
        "windows" => simple(ItemKind::Windows),
        "spacer" => simple(ItemKind::Spacer),
        "controls" => simple(ItemKind::Controls),
        "toggle" => {
            if raw.label.is_some() || raw.icon.is_some() || raw.send.is_some() || raw.run.is_some() || raw.app.is_some() {
                bail!("a toggle takes only what");
            }
            let what = raw.what.as_deref().unwrap_or_default();
            Toggle::parse(what)
                .map(ItemKind::Toggle)
                .ok_or_else(|| anyhow!("unknown toggle {what:?}"))
        }
        "key" => key(raw),
        other => bail!("unknown type {other:?}"),
    }
}

fn key(mut raw: ItemRaw) -> Result<ItemKind> {
    anyhow::ensure!(raw.what.is_none(), "a key takes no what");
    let label = match raw.label.as_deref().map(str::trim) {
        Some(label) => {
            if label.is_empty() || label.chars().count() > 12 || !label.is_ascii() {
                bail!("label must be 1 to 12 ASCII characters");
            }
            Some(&*Box::leak(label.to_owned().into_boxed_str()))
        }
        None => None,
    };
    let icon = match raw.icon.as_deref() {
        Some(name) => Some(
            lucide::names()
                .find(|icon| *icon == name)
                .ok_or_else(|| anyhow!("unknown icon {name:?}"))?,
        ),
        None => None,
    };
    let app_icon = match raw.app.as_deref() {
        Some(id) => {
            if raw.send.is_some() || raw.run.is_some() {
                bail!("an app item launches the app: drop send and run");
            }
            anyhow::ensure!(
                crate::apps::is_desktop_id(id),
                "app must be a desktop id such as org.gnome.Nautilus.desktop"
            );
            let path = crate::apps::icon_path(id);
            if path.is_none() {
                eprintln!("kait2en-touchbar-plus: no icon file for {id}");
            }
            raw.run = Some(format!("gtk-launch {id}"));
            path.map(|path| &*Box::leak(path.to_string_lossy().into_owned().into_boxed_str()))
        }
        None => None,
    };
    // An app without an icon file shows the start of its id instead of an empty key.
    let label = label.or_else(|| {
        let id = raw.app.as_deref().filter(|_| icon.is_none() && app_icon.is_none())?;
        let name: String = id.trim_end_matches(".desktop").rsplit('.').next()?.to_lowercase().chars().take(12).collect();
        (!name.is_empty()).then(|| &*Box::leak(name.into_boxed_str()))
    });
    if label.is_none() && icon.is_none() && app_icon.is_none() && raw.app.is_none() {
        bail!("key needs a label or an icon");
    }
    let action = match (raw.send, raw.run) {
        (Some(send), None) => CustomAction::Send(crate::keys::combination(&send)?),
        (None, Some(run)) if !run.trim().is_empty() => CustomAction::Run(run),
        _ => bail!("set exactly one of send or run"),
    };
    Ok(ItemKind::Key(KeySpec {
        app_icon,
        label,
        icon,
        action,
    }))
}

fn parse(body: &str, strict: bool) -> Result<LayoutConfig> {
    let file: File = toml::from_str(body).context("invalid layout file")?;
    let fallback = || -> Result<LayoutConfig> {
        let template: File = toml::from_str(TEMPLATE)?;
        build(template, false)
    };
    let mut config = build(file, strict)?;
    if config.default.is_empty() {
        // An empty app zone would leave the bar without media keys.
        let template = fallback()?;
        config.default = template.default;
        config.default_slots = template.default_slots;
    }
    Ok(config)
}

fn weather_ok(weather: &WeatherRaw) -> bool {
    matches!(weather.location.as_str(), "auto" | "manual")
        && (-90.0..=90.0).contains(&weather.latitude)
        && (-180.0..=180.0).contains(&weather.longitude)
        && weather.refresh_min >= 1
}

fn pomodoro_ok(pomodoro: &PomodoroRaw) -> bool {
    pomodoro.work_min >= 1 && pomodoro.break_min >= 1
}

fn controls_ok(controls: &ControlsRaw) -> bool {
    controls.items.iter().any(|name| Control::parse(name).is_some())
        && (controls.width_px == 0 || (300..=MAX_CONTROLS_PX).contains(&controls.width_px))
        && (1000..=30_000).contains(&controls.timeout_ms)
}

/// Widest the controls panel may be: the bar is 2170 px.
const MAX_CONTROLS_PX: u16 = 2170;

/// A broken section falls back to its defaults without costing the user's
/// profiles and keys, as a broken item only costs itself.
fn sanitize(file: &mut File) {
    let Ok(template) = toml::from_str::<File>(TEMPLATE) else { return };
    if !weather_ok(&file.weather) {
        eprintln!("kait2en-touchbar-plus: invalid [weather], using defaults");
        file.weather = template.weather;
    }
    if !pomodoro_ok(&file.pomodoro) {
        eprintln!("kait2en-touchbar-plus: invalid [pomodoro], using defaults");
        file.pomodoro = template.pomodoro;
    }
    if !controls_ok(&file.controls) {
        eprintln!("kait2en-touchbar-plus: invalid [controls], using defaults");
        file.controls = template.controls;
    }
}

fn build(mut file: File, strict: bool) -> Result<LayoutConfig> {
    if !strict {
        sanitize(&mut file);
    }
    let bundled = |pick: fn(&File) -> &Option<Vec<toml::Value>>| -> Result<Vec<toml::Value>> {
        let template: File = toml::from_str(TEMPLATE)?;
        Ok(pick(&template).clone().unwrap_or_default())
    };
    let strip = match file.strip {
        Some(raw) => raw,
        None => bundled(|f| &f.strip)?,
    };
    let default = match file.default {
        Some(raw) => raw,
        None => bundled(|f| &f.default)?,
    };
    let mut profiles = Vec::new();
    for profile in file.profile {
        let scope = format!("profile {:?}", profile.name);
        let (items, slots) = items(profile.items, strict, &scope)?;
        profiles.push(Profile {
            items,
            slots,
            name: profile.name,
            apps: profile.apps,
        });
    }
    anyhow::ensure!(
        matches!(file.weather.location.as_str(), "auto" | "manual"),
        "weather location must be \"auto\" or \"manual\""
    );
    anyhow::ensure!(
        (-90.0..=90.0).contains(&file.weather.latitude)
            && (-180.0..=180.0).contains(&file.weather.longitude),
        "weather coordinates are out of range"
    );
    anyhow::ensure!(
        file.weather.refresh_min >= 1,
        "weather refresh_min must be at least 1"
    );
    anyhow::ensure!(
        file.pomodoro.work_min >= 1 && file.pomodoro.break_min >= 1,
        "pomodoro minutes must be at least 1"
    );
    let mut controls = Vec::new();
    for name in &file.controls.items {
        match Control::parse(name) {
            Some(control) if !controls.contains(&control) => controls.push(control),
            Some(_) => {}
            None if strict => bail!("controls: unknown control {name:?}"),
            None => eprintln!("kait2en-touchbar-plus: controls: unknown control {name:?}"),
        }
    }
    anyhow::ensure!(!controls.is_empty(), "controls needs at least one control");
    anyhow::ensure!(
        file.controls.width_px == 0 || (300..=MAX_CONTROLS_PX).contains(&file.controls.width_px),
        "controls width_px must be 0 (whole zone) or 300..2170"
    );
    anyhow::ensure!(
        (1000..=30_000).contains(&file.controls.timeout_ms),
        "controls timeout_ms must be 1000..30000"
    );
    let mut favorites = Vec::new();
    let raw_favorites = match file.emoji {
        Some(emoji) => emoji.favorites,
        None => {
            let template: File = toml::from_str(TEMPLATE)?;
            template.emoji.map(|e| e.favorites).unwrap_or_default()
        }
    };
    for favorite in raw_favorites {
        match lucide::names().find(|icon| *icon == favorite.icon) {
            Some(icon) if !favorite.text.is_empty() => favorites.push(Favorite {
                icon,
                text: favorite.text,
            }),
            _ if strict => bail!("emoji favorite {:?}: unknown icon or empty text", favorite.icon),
            _ => eprintln!(
                "kait2en-touchbar-plus: emoji favorite {:?}: unknown icon or empty text",
                favorite.icon
            ),
        }
    }
    let (strip, strip_slots) = items(strip, strict, "strip")?;
    let (default, default_slots) = items(default, strict, "default")?;
    Ok(LayoutConfig {
        strip,
        strip_slots,
        default,
        default_slots,
        profiles,
        clock_format: file.clock.format,
        weather: WeatherCfg {
            auto: file.weather.location == "auto",
            latitude: file.weather.latitude,
            longitude: file.weather.longitude,
            refresh_min: file.weather.refresh_min,
        },
        work_min: file.pomodoro.work_min,
        break_min: file.pomodoro.break_min,
        visualizer: file.now_playing.visualizer,
        controls: ControlsCfg {
            items: controls,
            width_px: file.controls.width_px,
            timeout_ms: file.controls.timeout_ms,
        },
        favorites,
    })
}

impl LayoutConfig {
    /// Items of the first profile that lists `app_id`, else the default zone.
    pub fn zone_for(&self, app_id: &str) -> (&[ItemKind], &[Slot]) {
        self.profiles
            .iter()
            .find(|profile| profile.apps.iter().any(|app| app == app_id))
            .map_or((&self.default[..], &self.default_slots[..]), |profile| {
                (&profile.items[..], &profile.slots[..])
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn col_and_span_stay_inside_the_24_columns() {
        let ok = |body: &str| parse(body, true).is_ok();
        assert!(ok("default = [ { type = \"clock\", col = 5, span = 3 } ]"));
        assert!(ok("default = [ { type = \"clock\", col = 23, span = 2 } ]"));
        assert!(!ok("default = [ { type = \"clock\", col = 24, span = 2 } ]"));
        assert!(!ok("default = [ { type = \"clock\", col = 0 } ]"));
        assert!(!ok("default = [ { type = \"clock\", span = 25 } ]"));
        let config = parse("default = [ { type = \"spacer\", col = 5, span = 3 } ]", true).unwrap();
        assert_eq!(config.default, vec![ItemKind::Spacer]);
        assert_eq!(config.default_slots, vec![Slot { col: Some(4), span: Some(3) }]);
    }

    #[test]
    fn col_without_span_must_fit_its_natural_width() {
        let ok = |body: &str| parse(body, true).is_ok();
        assert!(!ok("default = [ { type = \"clock\", col = 24 } ]"));
        assert!(ok("default = [ { type = \"clock\", col = 23 } ]"));
    }

    #[test]
    fn broken_sections_keep_the_rest_when_not_strict() {
        let body = "default = [ { type = \"clock\" } ]\n[pomodoro]\nwork_min = 0\n[[profile]]\nname = \"X\"\napps = [\"a.desktop\"]\nitems = [ { type = \"clock\" } ]";
        assert!(parse(body, true).is_err());
        let config = parse(body, false).unwrap();
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.work_min, 25);
    }

    #[test]
    fn controls_default_has_volume_brightness_keyboard() {
        let expected = vec![Control::Volume, Control::Brightness, Control::Keyboard];
        assert_eq!(defaults().controls.items, expected);
        assert_eq!(parse("", false).unwrap().controls.items, expected);
    }

    #[test]
    fn template_parses_and_bad_items_are_skipped() {
        let config = defaults();
        assert_eq!(config.strip.len(), 3);
        assert_eq!(config.profiles.len(), 4);
        assert_eq!(config.favorites.len(), 7);
        let body = r#"
default = [ { type = "playback" }, { type = "key", label = "waytoolongtobevalid", send = "a" }, { type = "nope" }, { type = "key", icon = "nope", send = "a" } ]
"#;
        let config = parse(body, false).unwrap();
        assert_eq!(config.default, vec![ItemKind::Playback]);
        assert!(parse(body, true).is_err());
    }

    #[test]
    fn profile_matches_app_id_or_default() {
        let config = defaults();
        assert_eq!(config.zone_for("org.mozilla.firefox.desktop").0.len(), 6);
        assert_eq!(config.zone_for("unknown.desktop").0, &config.default[..]);
        assert_eq!(config.zone_for("").0, &config.default[..]);
    }
}
