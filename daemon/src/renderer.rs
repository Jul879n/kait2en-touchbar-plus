// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;

use ab_glyph::{Font, FontVec, OutlineCurve, VariableFont};
use anyhow::{Context, Result, anyhow};
use input_linux::Key;
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pixmap,
    Point, Rect, SpreadMode, Stroke, Transform,
};

use crate::{
    layout::{DialogTone, GRID_COLS, ItemKind, Slot, Toggle, TransientAction, TransientGlyph},
    lucide,
    policy::Mode,
    widgets::{self, NowPlayingView, PanelView, WidgetData},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Key(Key),
    /// Index into the personal keys from keys.toml.
    Custom(usize),
    /// Index into the resolved items of the media row (app zone, then strip).
    Item(usize),
    /// Shuffle and repeat of the active player.
    Player(PlayerCmd),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerCmd {
    Shuffle,
    Repeat,
}

#[derive(Clone, Copy)]
enum Glyph {
    Text(&'static str),
    /// Dialog text: title, neutral, accent or destructive key.
    Tone(&'static str, DialogTone),
    Icon(Icon),
    Lucide(&'static str),
    /// PNG of an application, drawn in color.
    App(&'static str),
    Widget(WidgetKind),
    /// Takes room but draws nothing.
    Blank,
}

/// Items drawn as plain text and icons without a key outline, like the
/// control strip of a MacBook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetKind {
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
    NowPlaying,
}

/// What the layout needs to know about the world to pick its keys.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutCtx {
    pub player_active: bool,
    /// On state of each `Toggle`, indexed by `Toggle::index`.
    pub toggles: [bool; 5],
    /// The windows key shows `--` because the shell extension is missing.
    pub windows_missing: bool,
    /// Shuffle on and repeat mode (0 off, 1 playlist, 2 track) of the player.
    pub shuffle: bool,
    pub repeat: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LevelKind {
    Volume,
    Brightness,
}

#[derive(Clone, Copy)]
enum Icon {
    Brightness(bool),
    Microphone,
    Keyboard(bool),
    Previous,
    PlayPause,
    Next,
    Mute,
    Volume(bool),
}

const KEY_GAP: u16 = 10;
const KEY_GAP_F: f32 = KEY_GAP as f32;
/// Swipe level feedback: icon slot, gap and meter.
const LEVEL_ICON_SLOT: f32 = 60.0;
const LEVEL_SPACING: f32 = 22.0;
const LEVEL_METER: f32 = 320.0;
const GROUP_GAP: u16 = 56;
const ART: usize = 52;
const PANEL_GAP: f32 = 24.0;

/// Left and right edge of slider `index` of `count` in the panel `zone`.
pub fn panel_slice(zone: (u16, u16), count: usize, index: usize) -> (f32, f32) {
    let (left, right) = (f32::from(zone.0), f32::from(zone.1));
    let count = count.max(1) as f32;
    let width = (right - left - PANEL_GAP * (count - 1.0)) / count;
    let start = left + index as f32 * (width + PANEL_GAP);
    (start, start + width)
}

/// The bar of a slider: after the icon, before the percentage.
pub fn panel_bar(slice_left: f32, slice_right: f32) -> (f32, f32) {
    (slice_left + 52.0, (slice_right - 92.0).max(slice_left + 60.0))
}

/// Which slider `x` falls on and the level (0..=100) it sets.
pub fn panel_hit(zone: (u16, u16), count: usize, x: f64) -> Option<(usize, u32)> {
    (0..count).find_map(|index| {
        let (left, right) = panel_slice(zone, count, index);
        let reach = PANEL_GAP / 2.0;
        if x < f64::from(left - reach) || x >= f64::from(right + reach) {
            return None;
        }
        Some((index, panel_level(zone, count, index, x)))
    })
}

pub fn panel_level(zone: (u16, u16), count: usize, index: usize, x: f64) -> u32 {
    let (left, right) = panel_slice(zone, count, index);
    let (bar_left, bar_right) = panel_bar(left, right);
    (((x as f32 - bar_left) / (bar_right - bar_left)).clamp(0.0, 1.0) * 100.0).round() as u32
}
const NP_FONT_SIZE: f32 = 20.0;
const NP_LINE_Y: f32 = 20.0;
const NP_BAR_Y: f32 = 42.0;
const NP_BAR_HEIGHT: f32 = 4.0;

/// Left and right edge of the progress bar of the now playing widget.
pub fn now_playing_bar(button: &Button, has_art: bool) -> (f32, f32) {
    let left = f32::from(button.left) + if has_art { ART as f32 + 10.0 } else { 0.0 };
    (left, f32::from(button.right))
}

struct Cell {
    /// Columns of the 24-column grid.
    cols: u16,
    glyph: Glyph,
    action: Action,
    on: bool,
}

impl Cell {
    fn key(glyph: Glyph, action: Action) -> Self {
        Self {
            cols: 2,
            glyph,
            action,
            on: false,
        }
    }

    fn button(self, left: u16, right: u16) -> Button {
        Button {
            left,
            right,
            glyph: self.glyph,
            action: self.action,
            on: self.on,
        }
    }
}

/// Columns a widget spans: wide enough for its text.
fn widget_cols(kind: WidgetKind) -> u16 {
    match kind {
        WidgetKind::Clock
        | WidgetKind::Battery
        | WidgetKind::Pomodoro
        | WidgetKind::Weather
        | WidgetKind::Cpu
        | WidgetKind::Memory
        | WidgetKind::Temperature
        | WidgetKind::Fan => 2,
        WidgetKind::Network | WidgetKind::NextEvent => 4,
        WidgetKind::NowPlaying => 8,
    }
}

/// Player buttons beside the now playing widget by the span the item gets:
/// 10 columns add play/pause, 14 previous and next, 18 shuffle and repeat.
fn now_playing_cells(span: Option<u16>, at: Action, ctx: &LayoutCtx) -> Vec<Cell> {
    let width = widget_cols(WidgetKind::NowPlaying);
    let span = span.unwrap_or(width).clamp(2, GRID_COLS);
    let key = |icon, key| Cell::key(Glyph::Icon(icon), Action::Key(key));
    let mut buttons = Vec::new();
    if span >= 14 {
        buttons.push(key(Icon::Previous, Key::PreviousSong));
    }
    if span >= 10 {
        buttons.push(key(Icon::PlayPause, Key::PlayPause));
    }
    if span >= 14 {
        buttons.push(key(Icon::Next, Key::NextSong));
    }
    if span >= 18 {
        let mut shuffle = Cell::key(Glyph::Lucide("shuffle"), Action::Player(PlayerCmd::Shuffle));
        shuffle.on = ctx.shuffle;
        let icon = if ctx.repeat == 2 { "repeat-1" } else { "repeat" };
        let mut repeat = Cell::key(Glyph::Lucide(icon), Action::Player(PlayerCmd::Repeat));
        repeat.on = ctx.repeat != 0;
        buttons.push(shuffle);
        buttons.push(repeat);
    }
    let taken: u16 = buttons.iter().map(|cell| cell.cols).sum();
    let main = Cell {
        cols: span.saturating_sub(taken).max(2),
        glyph: Glyph::Widget(WidgetKind::NowPlaying),
        action: at,
        on: false,
    };
    std::iter::once(main).chain(buttons).collect()
}

/// Columns an item takes by itself, 0 when it shows nothing yet.
pub(crate) fn natural_cols(kind: &ItemKind) -> u16 {
    item_cells(std::slice::from_ref(kind), &[Slot::default()], 0, &LayoutCtx::default())
        .first()
        .map_or(0, |(_, cells)| cells.iter().map(|cell| cell.cols).sum())
}

/// The cells of every visible item, with the item's index. `base` offsets
/// `Action::Item`.
fn item_cells(items: &[ItemKind], slots: &[Slot], base: usize, ctx: &LayoutCtx) -> Vec<(usize, Vec<Cell>)> {
    let mut groups: Vec<(usize, Vec<Cell>)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let at = Action::Item(base + index);
        let icon_pair = |a: (Icon, Key), b: (Icon, Key), rest: &[(Icon, Key)]| -> Vec<Cell> {
            [a, b]
                .into_iter()
                .chain(rest.iter().copied())
                .map(|(icon, key)| Cell::key(Glyph::Icon(icon), Action::Key(key)))
                .collect()
        };
        let widget = |kind: WidgetKind| {
            vec![Cell {
                cols: widget_cols(kind),
                glyph: Glyph::Widget(kind),
                action: at,
                on: false,
            }]
        };
        let cells: Vec<Cell> = match item {
            ItemKind::Brightness => icon_pair(
                    (Icon::Brightness(false), Key::BrightnessDown),
                    (Icon::Brightness(true), Key::BrightnessUp),
                    &[],
                ),
            ItemKind::KeyboardLight => icon_pair(
                    (Icon::Keyboard(false), Key::IllumDown),
                    (Icon::Keyboard(true), Key::IllumUp),
                    &[],
                ),
            ItemKind::Playback => {
                // Nothing to control while no player is running.
                if !ctx.player_active {
                    continue;
                }
                icon_pair(
                    (Icon::Previous, Key::PreviousSong),
                    (Icon::PlayPause, Key::PlayPause),
                    &[(Icon::Next, Key::NextSong)],
                )
            }
            ItemKind::Volume => icon_pair(
                    (Icon::Microphone, Key::MicMute),
                    (Icon::Mute, Key::Mute),
                    &[
                        (Icon::Volume(false), Key::VolumeDown),
                        (Icon::Volume(true), Key::VolumeUp),
                    ],
                ),
            ItemKind::Key(spec) => vec![Cell::key(
                    spec.app_icon
                        .map(Glyph::App)
                        .or(spec.icon.map(Glyph::Lucide))
                        .or(spec.label.map(Glyph::Text))
                        .unwrap_or(Glyph::Blank),
                    at,
                )],
            ItemKind::Screenshot => vec![Cell::key(Glyph::Lucide("camera"), Action::Key(Key::Sysrq))],
            ItemKind::Toggle(toggle) => {
                let on = ctx.toggles[toggle.index()];
                let icon = match (toggle, on) {
                    (Toggle::Dnd, _) => "bell-off",
                    (Toggle::NightLight, _) => "sun-moon",
                    (Toggle::Caffeine, _) => "coffee",
                    (Toggle::Wifi, true) => "wifi",
                    (Toggle::Wifi, false) => "wifi-off",
                    (Toggle::Bluetooth, true) => "bluetooth",
                    (Toggle::Bluetooth, false) => "bluetooth-off",
                };
                let mut cell = Cell::key(Glyph::Lucide(icon), at);
                // Radios show their state in the icon, the rest by lighting up.
                cell.on = on && !matches!(toggle, Toggle::Wifi | Toggle::Bluetooth);
                vec![cell]
            }
            ItemKind::Emoji => vec![Cell::key(Glyph::Lucide("smile"), at)],
            ItemKind::Controls => vec![Cell::key(Glyph::Lucide("sliders-horizontal"), at)],
            ItemKind::Windows => vec![Cell::key(
                    if ctx.windows_missing {
                        Glyph::Text("--")
                    } else {
                        Glyph::Lucide("app-window")
                    },
                    at,
                )],
            ItemKind::Transient(key) => {
                let mut cell = Cell::key(
                    match key.glyph {
                        TransientGlyph::Icon(name) => Glyph::Lucide(name),
                        TransientGlyph::Text(text) => Glyph::Text(text),
                        TransientGlyph::Tone(text, tone) => Glyph::Tone(text, tone),
                        TransientGlyph::App(path, _) => Glyph::App(path),
                    },
                    at,
                );
                // Switcher: apps take one column, their windows four.
                match (&key.glyph, &key.action) {
                    (TransientGlyph::App(_, selected), _) => {
                        cell.cols = 1;
                        cell.on = *selected;
                    }
                    (TransientGlyph::Tone(_, DialogTone::Accent), TransientAction::SwitchApp(_)) => {
                        cell.cols = 1;
                        cell.on = true;
                    }
                    (TransientGlyph::Tone(..), TransientAction::SwitchApp(_)) => cell.cols = 1,
                    (_, TransientAction::SwitchWindow(_)) => cell.cols = 4,
                    _ => {}
                }
                vec![cell]
            }
            ItemKind::Spacer => vec![Cell {
                    cols: 2,
                    glyph: Glyph::Blank,
                    action: at,
                    on: false,
                }],
            ItemKind::NowPlaying => {
                if !ctx.player_active {
                    continue;
                }
                let span = slots.get(index).and_then(|slot| slot.span);
                now_playing_cells(span, at, ctx)
            }
            ItemKind::Clock => widget(WidgetKind::Clock),
            ItemKind::Weather => widget(WidgetKind::Weather),
            ItemKind::Battery => widget(WidgetKind::Battery),
            ItemKind::Cpu => widget(WidgetKind::Cpu),
            ItemKind::Memory => widget(WidgetKind::Memory),
            ItemKind::Temperature => widget(WidgetKind::Temperature),
            ItemKind::Fan => widget(WidgetKind::Fan),
            ItemKind::Network => widget(WidgetKind::Network),
            ItemKind::NextEvent => widget(WidgetKind::NextEvent),
            ItemKind::Pomodoro => widget(WidgetKind::Pomodoro),
        };
        groups.push((index, cells));
    }
    groups
}

/// Columns each item takes, by item index: the slot's span
/// shared among the item's cells, else the natural width.
fn item_cols(groups: &mut [(usize, Vec<Cell>)], slots: &[Slot]) {
    for (item, cells) in groups {
        let natural: u16 = cells.iter().map(|cell| cell.cols).sum();
        let Some(span) = slots.get(*item).and_then(|slot| slot.span) else {
            continue;
        };
        // The now playing widget always keeps room for its cover and a line.
        let floor = if matches!(cells.first().map(|cell| &cell.glyph), Some(Glyph::Widget(WidgetKind::NowPlaying))) {
            2
        } else {
            cells.len() as u16
        };
        let span = span.clamp(floor, GRID_COLS);
        if span != natural {
            let count = cells.len() as u16;
            for (at, cell) in cells.iter_mut().enumerate() {
                cell.cols = span / count + u16::from((at as u16) < span % count);
            }
        }
    }
}

/// Starting column of every item, in order and never overlapping: a slot's
/// column wins unless it would overlap the previous item.
fn item_starts(groups: &[(usize, Vec<Cell>)], slots: &[Slot], first: u16) -> Vec<u16> {
    let mut cursor = first;
    groups
        .iter()
        .map(|(item, cells)| {
            let at = slots
                .get(*item)
                .and_then(|slot| slot.col)
                .map_or(cursor, |col| col.max(cursor));
            cursor = at + cells.iter().map(|cell| cell.cols).sum::<u16>();
            at
        })
        .collect()
}

/// Previous and next key spans (left, right) of the track overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct TrackLayout {
    pub prev: (u16, u16),
    pub next: (u16, u16),
}

#[derive(Clone, Copy)]
pub struct Button {
    pub left: u16,
    pub right: u16,
    glyph: Glyph,
    pub action: Action,
    /// Drawn pressed, for switches that are on.
    pub on: bool,
}

pub struct Layout {
    pub buttons: Vec<Button>,
    /// Left and right edge of the area the app zone fills.
    pub zone: (u16, u16),
}

impl Layout {
    pub fn new(mode: Mode, width: u16, physical_escape: bool) -> Self {
        Self::with_custom(mode, width, physical_escape, &[])
    }

    /// Like `new`, but the special row also shows `custom` labels in its
    /// free space next to esc, as many as fit.
    pub fn with_custom(
        mode: Mode,
        width: u16,
        physical_escape: bool,
        custom: &[&'static str],
    ) -> Self {
        // Keys inside a group sit KEY_GAP apart. Groups are separated by the
        // wider GROUP_GAP, like the clusters of a hardware function row.
        let mut groups: Vec<Vec<(Glyph, Key)>> = Vec::new();
        match mode {
            Mode::Function => {
                const LABELS: [&str; 12] = [
                    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
                ];
                let keys = [
                    Key::F1,
                    Key::F2,
                    Key::F3,
                    Key::F4,
                    Key::F5,
                    Key::F6,
                    Key::F7,
                    Key::F8,
                    Key::F9,
                    Key::F10,
                    Key::F11,
                    Key::F12,
                ];
                groups.push(
                    LABELS
                        .into_iter()
                        .zip(keys)
                        .map(|(label, key)| (Glyph::Text(label), key))
                        .collect(),
                );
            }
            Mode::Media => groups.extend([
                vec![
                    (Glyph::Icon(Icon::Brightness(false)), Key::BrightnessDown),
                    (Glyph::Icon(Icon::Brightness(true)), Key::BrightnessUp),
                ],
                vec![
                    (Glyph::Icon(Icon::Keyboard(false)), Key::IllumDown),
                    (Glyph::Icon(Icon::Keyboard(true)), Key::IllumUp),
                ],
                vec![
                    (Glyph::Icon(Icon::Previous), Key::PreviousSong),
                    (Glyph::Icon(Icon::PlayPause), Key::PlayPause),
                    (Glyph::Icon(Icon::Next), Key::NextSong),
                ],
                vec![
                    (Glyph::Icon(Icon::Microphone), Key::MicMute),
                    (Glyph::Icon(Icon::Mute), Key::Mute),
                    (Glyph::Icon(Icon::Volume(false)), Key::VolumeDown),
                    (Glyph::Icon(Icon::Volume(true)), Key::VolumeUp),
                ],
            ]),
            Mode::Special => groups.extend([
                vec![(Glyph::Text("print"), Key::Sysrq)],
                vec![
                    (Glyph::Text("insert"), Key::Insert),
                    (Glyph::Text("delete"), Key::Delete),
                ],
                vec![
                    (Glyph::Text("home"), Key::Home),
                    (Glyph::Text("end"), Key::End),
                    (Glyph::Text("page up"), Key::PageUp),
                    (Glyph::Text("page down"), Key::PageDown),
                ],
            ]),
        }

        // esc keeps the same slot in every mode, so switching layers only
        // swaps the keys to its right and never moves or resizes it.
        let mut buttons = Vec::new();
        let mut start = KEY_GAP;
        if !physical_escape {
            let right = start + width / 14;
            buttons.push(Button {
                left: start,
                right,
                glyph: Glyph::Text("esc"),
                action: Action::Key(Key::Esc),
                on: false,
            });
            start = right + GROUP_GAP;
        }

        if mode == Mode::Special {
            // Fewer keys than fit: keep the media key width and align them
            // to the right edge, leaving the space next to esc free.
            let media = Self::new(Mode::Media, width, physical_escape);
            let cell = media
                .buttons
                .last()
                .map_or(width / 14, |button| button.right - button.left);
            let mut right = width.saturating_sub(KEY_GAP);
            let mut placed = Vec::new();
            for group in groups.into_iter().rev() {
                for (glyph, key) in group.into_iter().rev() {
                    let left = right.saturating_sub(cell);
                    placed.push(Button {
                        left,
                        right,
                        glyph,
                        action: Action::Key(key),
                        on: false,
                    });
                    right = left.saturating_sub(KEY_GAP);
                }
                right = right.saturating_sub(GROUP_GAP - KEY_GAP);
            }
            // Personal keys form one more group in the space that is left.
            let space = right.saturating_sub(start);
            let fit = usize::from((space + KEY_GAP) / (cell + KEY_GAP));
            for (index, label) in custom.iter().enumerate().take(fit).rev() {
                let left = right.saturating_sub(cell);
                placed.push(Button {
                    left,
                    right,
                    glyph: Glyph::Text(label),
                    action: Action::Custom(index),
                    on: false,
                });
                right = left.saturating_sub(KEY_GAP);
            }
            buttons.extend(placed.into_iter().rev());
            return Self {
                buttons,
                zone: (0, width),
            };
        }

        // The remaining span is filled edge to edge. Rounding leftovers widen
        // the first keys by one pixel instead of shifting the row.
        let count = groups.iter().map(Vec::len).sum::<usize>().max(1) as u16;
        let group_count = groups.len().max(1) as u16;
        let spacing = KEY_GAP * (count - group_count) + GROUP_GAP * (group_count - 1);
        let usable = width.saturating_sub(start + KEY_GAP + spacing);
        let cell = usable / count;
        let mut extra = usable - cell * count;
        let mut left = start;
        for group in groups {
            for (glyph, key) in group {
                let mut right = left + cell;
                if extra > 0 {
                    right += 1;
                    extra -= 1;
                }
                buttons.push(Button {
                    left,
                    right,
                    glyph,
                    action: Action::Key(key),
                    on: false,
                });
                left = right + KEY_GAP;
            }
            left += GROUP_GAP - KEY_GAP;
        }
        Self {
            buttons,
            zone: (0, width),
        }
    }

    /// The media row built from configured items: the app zone fills the
    /// space left of the control strip, which stays right-aligned.
    #[cfg(test)]
    pub fn from_items(
        app: &[ItemKind],
        strip: &[ItemKind],
        width: u16,
        physical_escape: bool,
        ctx: &LayoutCtx,
    ) -> Self {
        Self::from_placed(app, &[], strip, &[], width, physical_escape, ctx)
    }

    /// Like `from_items`, with each item's grid slot (column and width).
    pub fn from_placed(
        app: &[ItemKind],
        app_slots: &[Slot],
        strip: &[ItemKind],
        strip_slots: &[Slot],
        width: u16,
        physical_escape: bool,
        ctx: &LayoutCtx,
    ) -> Self {
        let mut buttons = Vec::new();
        let mut start = KEY_GAP;
        if !physical_escape {
            let right = start + width / 14;
            buttons.push(Button {
                left: start,
                right,
                glyph: Glyph::Text("esc"),
                action: Action::Key(Key::Esc),
                on: false,
            });
            start = right + GROUP_GAP;
        }
        let edge = width.saturating_sub(KEY_GAP);

        // 24-column grid: items sit where their slot says, or after the
        // previous one; the control strip defaults to flush right.
        let mut zone_groups = item_cells(app, app_slots, 0, ctx);
        item_cols(&mut zone_groups, app_slots);
        let zone_starts = item_starts(&zone_groups, app_slots, 0);
        let zone_end = zone_groups
            .iter()
            .zip(&zone_starts)
            .map(|((_, cells), at)| at + cells.iter().map(|cell| cell.cols).sum::<u16>())
            .max()
            .unwrap_or(0);
        let mut strip_groups = item_cells(strip, strip_slots, app.len(), ctx);
        item_cols(&mut strip_groups, strip_slots);
        let strip_cols: u16 = strip_groups.iter().flat_map(|(_, cells)| cells).map(|cell| cell.cols).sum();
        let strip_first = GRID_COLS.saturating_sub(strip_cols).max(zone_end);
        let strip_starts = item_starts(&strip_groups, strip_slots, strip_first);
        let strip_end = strip_groups
            .iter()
            .zip(&strip_starts)
            .map(|((_, cells), at)| at + cells.iter().map(|cell| cell.cols).sum::<u16>())
            .max()
            .unwrap_or(0);
        // Too many items squeeze the columns instead of overlapping.
        let columns = GRID_COLS.max(zone_end).max(strip_end);

        let pitch = f32::from(edge.saturating_sub(start) + KEY_GAP) / f32::from(columns);
        let x = |column: u16| f32::from(start) + pitch * f32::from(column);
        let span = |from: u16, cols: u16| {
            (x(from).round() as u16, (x(from + cols) - f32::from(KEY_GAP)).round() as u16)
        };
        let strip_left = strip_starts.first().map(|at| span(*at, 1).0);
        let zone_limit = strip_left.map_or(edge, |left| left.saturating_sub(GROUP_GAP));
        for (groups, starts) in [(zone_groups, zone_starts), (strip_groups, strip_starts)] {
            for ((_, cells), at) in groups.into_iter().zip(starts) {
                let mut column = at;
                for cell in cells {
                    let (left, right) = span(column, cell.cols);
                    column += cell.cols;
                    buttons.push(cell.button(left, right));
                }
            }
        }
        Self {
            buttons,
            zone: (start, zone_limit),
        }
    }

    /// Index of the now playing widget, if the row has one.
    pub fn now_playing_index(&self) -> Option<usize> {
        self.buttons
            .iter()
            .position(|button| matches!(button.glyph, Glyph::Widget(WidgetKind::NowPlaying)))
    }

    pub fn hit(&self, x: f64, y: f64, height: u16) -> Option<usize> {
        if y < 5.0 || y > f64::from(height.saturating_sub(5)) {
            return None;
        }
        self.buttons
            .iter()
            .position(|button| {
                // Let the virtual Esc key respond all the way to the left edge
                let left = if button.action == Action::Key(Key::Esc) {
                    0.0
                } else {
                    f64::from(button.left)
                };
                x >= left && x < f64::from(button.right)
            })
    }
}

/// Variable sans fonts that match the San Francisco legends of the keyboard
/// closely enough. Adwaita Sans ships with GNOME on Fedora.
const FONT_PATHS: [&str; 3] = [
    "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
    "/usr/share/fonts/rsms-inter-vf-fonts/InterVariable.ttf",
    "/usr/share/fonts/abattis-cantarell-vf-fonts/Cantarell-VF.otf",
];
const FONT_WEIGHT: f32 = 500.0;
/// Emphasis in the Touch ID prompt ("**Unlock** with Touch ID").
const STRONG_WEIGHT: f32 = 700.0;
const TRACK_FONT_SIZE: f32 = 24.0;
const TRACK_GAP: f32 = 40.0;
const PROMPT_FONT_SIZE: f32 = 26.0;
/// Space between the prompt and the right edge, where the sensor is.
const PROMPT_RIGHT: f32 = 96.0;
const PROMPT_ICON_SLOT: f32 = 44.0;
const PROMPT_ICON_GAP: f32 = 14.0;
const KEY_FONT_SIZE: f32 = 23.0;

mod prompt;
pub use prompt::{STATES as TOUCH_ID_STATES, TouchIdLook};
mod notice;
pub use notice::{NoticeCard, NoticeIcon, NoticeMotion};

/// Icons are drawn on a 24 px design grid and centered by their bounds so
/// every key gets the same optical margin.
const ICON_SCALE: f32 = 1.3;
const ICON_STROKE: f32 = 2.3;
const KEY_INSET: f32 = 6.5;
const KEY_RADIUS: f32 = 8.0;
const KEY_OUTLINE: f32 = 1.5;

pub struct Canvas {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u32>,
    pixmap: Pixmap,
    font: FontVec,
    /// The configured key color at full intensity.
    base: Rgb,
    /// `base` dimmed to the current key level.
    color: Rgb,
    /// Draw icons with their own colors.
    pub full_color: bool,
    app_icons: std::collections::HashMap<&'static str, Option<std::rc::Rc<[u8]>>>,
    notice_fits: std::collections::HashMap<(String, String), notice::NoticeFit>,
}

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8);

impl Rgb {
    fn from_u32(color: u32) -> Self {
        Self((color >> 16) as u8, (color >> 8) as u8, color as u8)
    }

    fn scale(self, percent: u32) -> Self {
        let channel = |value: u8| (u32::from(value) * percent / 100).min(255) as u8;
        Self(channel(self.0), channel(self.1), channel(self.2))
    }

    fn paint(self) -> Paint<'static> {
        let mut paint = Paint::default();
        paint.set_color_rgba8(self.0, self.1, self.2, 255);
        paint.anti_alias = true;
        paint
    }
}

const BLACK: Rgb = Rgb(0, 0, 0);

/// Color of a Lucide icon in full-color mode, at the current key level.
fn full_tint(name: &str, level: Rgb, base: Rgb) -> Rgb {
    let hue = match name {
        "volume-2" | "bluetooth" | "bluetooth-off" | "sliders-horizontal" | "wifi" | "wifi-off" => Rgb(0x4d, 0xa3, 0xff),
        "sun" | "sun-moon" | "coffee" | "cloud-sun" | "keyboard" | "moon" => Rgb(0xff, 0xc8, 0x3d),
        "play" | "pause" | "timer" => Rgb(0xff, 0x9f, 0x45),
        "battery-full" | "battery-medium" | "battery-charging" | "check" | "thumbs-up" => Rgb(0x4c, 0xd9, 0x64),
        "camera" | "smile" | "laugh" | "heart" | "flame" | "party-popper" | "thermometer" | "battery-low" => Rgb(0xff, 0x6b, 0x81),
        "bell-off" | "calendar" | "app-window" | "cpu" | "memory-stick" => Rgb(0xb8, 0x8c, 0xff),
        "cloud" | "cloud-fog" | "cloud-drizzle" | "cloud-rain" | "cloud-snow" | "cloud-lightning" | "fan" | "arrow-down-up" => Rgb(0x7f, 0xd4, 0xff),
        _ => return level,
    };
    at_level(hue, level, base)
}

/// `hue` dimmed like the key color `level` is dimmed from `base`.
fn at_level(hue: Rgb, level: Rgb, base: Rgb) -> Rgb {
    let percent = u32::from(level.0.max(level.1).max(level.2)) * 100 / u32::from(base.0.max(base.1).max(base.2)).max(1);
    hue.scale(percent)
}

impl Canvas {
    pub fn new(width: u16, height: u16, key_color: u32) -> Result<Self> {
        let pixmap = Pixmap::new(u32::from(width), u32::from(height))
            .ok_or_else(|| anyhow!("invalid canvas size {width}x{height}"))?;
        let mut canvas = Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize],
            pixmap,
            font: load_font()?,
            base: Rgb::from_u32(key_color),
            color: Rgb::from_u32(key_color),
            full_color: false,
            app_icons: std::collections::HashMap::new(),
            notice_fits: std::collections::HashMap::new(),
        };
        canvas.clear();
        Ok(canvas)
    }

    /// Color of an icon: its own hue in full-color mode, else the key color.
    fn tint(&self, name: &str) -> Rgb {
        if self.full_color {
            full_tint(name, self.color, self.base)
        } else {
            self.color
        }
    }

    /// Dims everything drawn in the key color, in percent of full intensity.
    pub fn set_level(&mut self, percent: u32) {
        self.color = self.base.scale(percent.min(100));
    }

    pub fn clear(&mut self) {
        self.pixmap.fill(Color::BLACK);
        self.pixels.fill(0);
    }

    pub fn keys(&mut self, layout: &Layout, active: Option<usize>) {
        self.keys_with(layout, active, None);
    }

    /// Like `keys`, plus the strip widgets when `data` is given.
    pub fn keys_with(&mut self, layout: &Layout, active: Option<usize>, data: Option<&WidgetData>) {
        self.pixmap.fill(Color::BLACK);
        for (index, button) in layout.buttons.iter().enumerate() {
            if matches!(button.glyph, Glyph::Widget(_) | Glyph::Blank) {
                continue;
            }
            self.button(button, active == Some(index));
        }
        if let Some(data) = data {
            for button in &layout.buttons {
                if let Glyph::Widget(kind) = button.glyph {
                    self.widget(button, kind, data);
                }
            }
            if let Some(panel) = &data.panel {
                self.panel(panel);
            }
        }
        self.flush();
    }

    /// Icon and ASCII value centered in the button's span, no outline.
    fn widget(&mut self, button: &Button, kind: WidgetKind, data: &WidgetData) {
        if kind == WidgetKind::NowPlaying {
            if let Some(view) = &data.now_playing {
                self.now_playing(button, view);
            }
            return;
        }
        let (icon, text) = widgets::view(kind, data);
        let cy = f32::from(self.height) / 2.0;
        let size = 22.0;
        let text_width = self.text_width(&text, size);
        let icon_slot = if icon.is_some() { 30.0 } else { 0.0 };
        let gap = if icon.is_some() && !text.is_empty() { 4.0 } else { 0.0 };
        let total = icon_slot + gap + text_width;
        let cx = (f32::from(button.left) + f32::from(button.right)) / 2.0;
        let left = cx - total / 2.0;
        if let Some(shapes) = icon.and_then(lucide::shapes) {
            let saved = self.color;
            self.color = self.tint(icon.unwrap_or_default());
            self.shapes_sized(left + icon_slot / 2.0, cy, shapes, BLACK, 0.8);
            self.color = saved;
        }
        self.center_text(left + icon_slot + gap + text_width / 2.0, cy, &text, size, self.color);
    }

    /// The sliders that replace the app zone while the controls are open.
    fn panel(&mut self, panel: &PanelView) {
        let (left, right) = panel.zone;
        // Clear the zone: the keys beneath are not drawn while it is open.
        if let Some(rect) = Rect::from_ltrb(f32::from(left), 0.0, f32::from(right), f32::from(self.height)) {
            self.pixmap.fill_rect(rect, &BLACK.paint(), Transform::identity(), None);
        }
        let cy = f32::from(self.height) / 2.0;
        for (index, row) in panel.rows.iter().enumerate() {
            let (slice_left, slice_right) = panel_slice(panel.zone, panel.rows.len(), index);
            let (bar_left, bar_right) = panel_bar(slice_left, slice_right);
            let dim = if panel.active.is_some_and(|active| active != index) { 55 } else { 100 };
            let color = self.color.scale(dim);
            if let Some(shapes) = lucide::shapes(row.icon) {
                let saved = self.color;
                self.color = if self.full_color { full_tint(row.icon, color, self.base) } else { color };
                self.shapes_sized(slice_left + 20.0, cy, shapes, BLACK, 0.85);
                self.color = saved;
            }
            self.fill(&rounded_rect(bar_left, cy - 4.0, bar_right, cy + 4.0, 4.0), color.scale(25));
            let percent = row.percent.unwrap_or(0).min(100);
            let filled = (bar_right - bar_left) * percent as f32 / 100.0;
            if filled >= 1.0 {
                self.fill(&rounded_rect(bar_left, cy - 4.0, bar_left + filled, cy + 4.0, 4.0), color);
            }
            let mut knob = PathBuilder::new();
            knob.push_circle(bar_left + filled, cy, 9.0);
            if let Some(path) = knob.finish() {
                self.fill(&path, color);
            }
            let text = row.percent.map_or_else(|| "--".to_owned(), |p| format!("{p}%"));
            self.center_text(bar_right + 48.0, cy, &text, 20.0, color);
        }
    }

    /// Centered `App: text` message, for notifications and timer alerts.

    /// Cover, `Artist - Title`, progress bar and the optional visualizer.
    fn now_playing(&mut self, button: &Button, view: &NowPlayingView) {
        let (left, right) = (f32::from(button.left), f32::from(button.right));
        let height = f32::from(self.height);
        let (text_left, _) = now_playing_bar(button, view.art.is_some());
        if let Some(art) = &view.art {
            self.draw_art(left as usize, ((height - ART as f32) / 2.0) as usize, art);
        }
        let viz = if view.bars.is_some() { 35.0 } else { 0.0 };
        let line = self.fit_ascii(&view.line, NP_FONT_SIZE, right - text_left - viz, FONT_WEIGHT);
        let baseline = (NP_LINE_Y + self.cap_height(NP_FONT_SIZE) / 2.0).round();
        let mut path = PathBuilder::new();
        self.append_text(&mut path, text_left, baseline, &line, NP_FONT_SIZE, FONT_WEIGHT);
        if let Some(path) = path.finish() {
            self.fill(&path, self.color);
        }
        let (top, bottom) = (NP_BAR_Y, NP_BAR_Y + NP_BAR_HEIGHT);
        self.fill(&rounded_rect(text_left, top, right, bottom, 2.0), self.color.scale(25));
        let filled = (right - text_left) * view.fraction.clamp(0.0, 1.0);
        if filled >= 1.0 {
            self.fill(&rounded_rect(text_left, top, text_left + filled, bottom, 2.0), self.color);
        }
        if let Some(bars) = view.bars {
            for (index, bar) in bars.iter().enumerate() {
                let x = right - 27.0 + index as f32 * 6.0;
                let half = f32::from((*bar).max(2)) / 2.0;
                self.fill(
                    &rounded_rect(x, NP_LINE_Y - half, x + 3.0, NP_LINE_Y + half, 1.5),
                    self.color,
                );
            }
        }
    }

    /// Application icon in its own colors (dimmed with the key level).
    fn draw_app_icon(&mut self, cx: f32, cy: f32, path: &'static str) {
        const SIZE: u32 = 34;
        if self.app_icons.len() >= 128 {
            self.app_icons.clear();
        }
        let pixels = self
            .app_icons
            .entry(path)
            .or_insert_with(|| crate::apps::load_rgba(std::path::Path::new(path), SIZE).map(Into::into))
            .clone();
        let Some(pixels) = pixels else { return };
        let level = u32::from(self.color.0.max(self.color.1).max(self.color.2)) * 100
            / u32::from(self.base.0.max(self.base.1).max(self.base.2)).max(1);
        let (left, top) = ((cx - SIZE as f32 / 2.0) as i32, (cy - SIZE as f32 / 2.0) as i32);
        let width = self.width as i32;
        let target = self.pixmap.pixels_mut();
        for (index, rgba) in pixels.chunks_exact(4).enumerate() {
            let (x, y) = (left + (index as u32 % SIZE) as i32, top + (index as u32 / SIZE) as i32);
            if x < 0 || y < 0 || x >= width || y >= i32::from(self.height) || rgba[3] == 0 {
                continue;
            }
            let shade = |c: u8| (u32::from(c) * u32::from(rgba[3]) * level / (255 * 100)) as u8;
            // Over what is already there: a tinted key keeps its tint at the edges.
            let below = target[(y * width + x) as usize];
            let keep = 255 - u32::from(rgba[3]);
            let over = |new: u8, old: u8| (u32::from(new) + u32::from(old) * keep / 255).min(255) as u8;
            if let Some(pixel) = tiny_skia::PremultipliedColorU8::from_rgba(
                over(shade(rgba[0]), below.red()),
                over(shade(rgba[1]), below.green()),
                over(shade(rgba[2]), below.blue()),
                255,
            ) {
                target[(y * width + x) as usize] = pixel;
            }
        }
    }

    /// Grayscale cover tinted with the key color.
    fn draw_art(&mut self, left: usize, top: usize, gray: &[u8]) {
        let width = self.width as usize;
        let color = self.color;
        let pixels = self.pixmap.pixels_mut();
        for (index, value) in gray.iter().enumerate().take(ART * ART) {
            let (x, y) = (left + index % ART, top + index / ART);
            if x >= width || y >= self.height as usize {
                continue;
            }
            let shade = |channel: u8| (u32::from(channel) * u32::from(*value) / 255) as u8;
            if let Some(pixel) = tiny_skia::PremultipliedColorU8::from_rgba(
                shade(color.0),
                shade(color.1),
                shade(color.2),
                255,
            ) {
                pixels[y * width + x] = pixel;
            }
        }
    }

    /// `text` cut to `max` px with a trailing `...`.
    /// `text` cut with "..." so it measures at most `max` at `weight`.
    fn fit_ascii(&mut self, text: &str, size: f32, max: f32, weight: f32) -> String {
        let chars: Vec<char> = text.chars().collect();
        if self.text_width_at(text, size, weight) <= max {
            return text.to_owned();
        }
        let cut = |count: usize| format!("{}...", chars[..count].iter().collect::<String>().trim_end());
        // The widest prefix that fits: widths only grow with the prefix.
        let (mut low, mut high) = (0, chars.len());
        while low < high {
            let middle = (low + high).div_ceil(2);
            if self.text_width_at(&cut(middle), size, weight) <= max {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        if self.text_width_at(&cut(low), size, weight) <= max { cut(low) } else { String::new() }
    }

    /// Level feedback for a swipe on the dark bar: icon and meter beside the
    /// fingers. `fingers` is the x range from the leftmost to the rightmost
    /// finger. `right` is how much of it shows on their right side (0 to 1),
    /// the rest shows on their left, so a side change can crossfade.
    pub fn level(
        &mut self,
        kind: LevelKind,
        percent: Option<u32>,
        muted: bool,
        fingers: (f32, f32),
        right: f32,
        fade: u32,
    ) {
        // From the touch point of the finger nearest to the feedback, so any
        // number of fingers keeps the same clearance.
        const DISTANCE: f32 = 110.0;
        let total = LEVEL_ICON_SLOT + LEVEL_SPACING + LEVEL_METER;
        let width = f32::from(self.width);
        let (first, last) = fingers;
        let full = self.color;
        self.pixmap.fill(Color::BLACK);
        let right = right.clamp(0.0, 1.0);
        for (left, share) in [
            (first - DISTANCE - total, 1.0 - right),
            (last + DISTANCE, right),
        ] {
            let opacity = (fade.min(100) as f32 * share).round() as u32;
            if opacity == 0 {
                continue;
            }
            let left = left.clamp(KEY_GAP_F, width - KEY_GAP_F - total).round();
            self.color = full.scale(opacity);
            self.level_at(left, kind, percent, muted);
        }
        self.flush();
        self.color = full;
    }

    fn level_at(&mut self, left: f32, kind: LevelKind, percent: Option<u32>, muted: bool) {
        const ICON_SIZE: f32 = 1.6;
        const METER_HEIGHT: f32 = 10.0;
        let cy = f32::from(self.height) / 2.0;
        let icon = match (kind, muted) {
            (LevelKind::Volume, true) => Icon::Mute,
            (LevelKind::Volume, false) => Icon::Volume(true),
            (LevelKind::Brightness, _) => Icon::Brightness(true),
        };
        self.icon_sized(left + LEVEL_ICON_SLOT / 2.0, cy, icon, BLACK, ICON_SIZE);

        let meter_left = left + LEVEL_ICON_SLOT + LEVEL_SPACING;
        let (top, bottom) = (cy - METER_HEIGHT / 2.0, cy + METER_HEIGHT / 2.0);
        let radius = METER_HEIGHT / 2.0;
        let track = rounded_rect(meter_left, top, meter_left + LEVEL_METER, bottom, radius);
        self.fill(&track, self.color.scale(22));
        let fraction = percent.map_or(0.0, |percent| percent.min(100) as f32 / 100.0);
        if fraction > 0.0 && !muted {
            let right = meter_left + (LEVEL_METER * fraction).max(METER_HEIGHT);
            self.fill(
                &rounded_rect(meter_left, top, right, bottom, radius),
                self.color,
            );
        }
    }

    /// "**Artist** – Title" on the dark bar with borderless previous/next
    /// keys at `keys`, the fixed slots at both ends of the bar. `fade`
    /// scales everything (percent) for the fade in and out; `pressed`
    /// highlights next (`true`) or previous (`false`). `esc` is the esc
    /// key with its own opacity, drawn while it is shown on the dark bar.
    pub fn track(
        &mut self,
        artist: &str,
        title: &str,
        fade: u32,
        pressed: Option<bool>,
        keys: TrackLayout,
        esc: Option<(Button, u32)>,
    ) {
        let space_left = f32::from(keys.prev.1) + TRACK_GAP;
        let space_right = f32::from(keys.next.0) - TRACK_GAP;
        let (artist, title) = self.fit_track(artist, title, space_right - space_left);
        let separator = if artist.is_empty() { "" } else { " – " };
        let parts = [
            (artist.as_str(), STRONG_WEIGHT),
            (separator, FONT_WEIGHT),
            (title.as_str(), FONT_WEIGHT),
        ];
        let text_width: f32 = parts
            .iter()
            .map(|(text, weight)| self.text_width_at(text, TRACK_FONT_SIZE, *weight))
            .sum();

        let full = self.color;
        self.pixmap.fill(Color::BLACK);
        if let Some((esc, opacity)) = esc {
            self.color = full.scale(opacity.min(100));
            self.button(&esc, false);
        }
        self.color = full.scale(fade.min(100));
        let top = KEY_INSET;
        let bottom = f32::from(self.height) - KEY_INSET;
        let cy = f32::from(self.height) / 2.0;
        for ((left, right), icon, forward) in [
            (keys.prev, Icon::Previous, false),
            (keys.next, Icon::Next, true),
        ] {
            let (left, right) = (f32::from(left), f32::from(right));
            let background = if pressed == Some(forward) {
                let fill = self.color.scale(20);
                self.fill(&rounded_rect(left, top, right, bottom, KEY_RADIUS), fill);
                fill
            } else {
                BLACK
            };
            self.icon((left + right) / 2.0, cy, icon, background);
        }
        let baseline = (cy + self.cap_height(TRACK_FONT_SIZE) / 2.0).round();
        let mut path = PathBuilder::new();
        let mut x = ((space_left + space_right - text_width) / 2.0).round();
        for (text, weight) in parts {
            x = self.append_text(&mut path, x, baseline, text, TRACK_FONT_SIZE, weight);
        }
        if let Some(path) = path.finish() {
            self.fill(&path, self.color);
        }
        self.color = full;
        self.flush();
    }

    /// Shortens the title, then the artist, with an ellipsis until the line
    /// fits.
    fn fit_track(&mut self, artist: &str, title: &str, max: f32) -> (String, String) {
        let mut artist: Vec<char> = artist.chars().collect();
        let mut title: Vec<char> = title.chars().collect();
        let (mut artist_cut, mut title_cut) = (false, false);
        loop {
            let shown = |chars: &[char], cut: bool| {
                let mut text: String = chars.iter().collect();
                if cut {
                    text = format!("{}…", text.trim_end());
                }
                text
            };
            let (a, t) = (shown(&artist, artist_cut), shown(&title, title_cut));
            let separator = if a.is_empty() {
                0.0
            } else {
                self.text_width_at(" – ", TRACK_FONT_SIZE, FONT_WEIGHT)
            };
            let width = self.text_width_at(&a, TRACK_FONT_SIZE, STRONG_WEIGHT)
                + separator
                + self.text_width_at(&t, TRACK_FONT_SIZE, FONT_WEIGHT);
            if width <= max || (artist.len() <= 1 && title.len() <= 1) {
                return (a, t);
            }
            if title.len() > 12 || artist.len() <= 12 && !title.is_empty() {
                title.pop();
                title_cut = true;
            } else {
                artist.pop();
                artist_cut = true;
            }
        }
    }

    /// One key on an otherwise black bar, with optional pressed styling and
    /// opacity for the activity escape key.
    pub fn single_key(&mut self, layout: &Layout, index: usize, active: bool, fade: u32) {
        let full = self.color;
        self.color = full.scale(fade.min(100));
        self.pixmap.fill(Color::BLACK);
        if let Some(button) = layout.buttons.get(index) {
            self.button(button, active);
        }
        self.flush();
        self.color = full;
    }

    fn button(&mut self, button: &Button, active: bool) {
        let top = KEY_INSET;
        let bottom = f32::from(self.height) - KEY_INSET;
        let cy = f32::from(self.height) / 2.0;
        let (left, right) = (f32::from(button.left), f32::from(button.right));
        let active = active || button.on;
        // Dialog keys: accent and destructive ones get their own color in full
        // color; in monochrome they share the key color and only the title dims.
        let tone = match button.glyph {
            Glyph::Tone(_, tone) => Some(tone),
            _ => None,
        };
        let hue = match tone {
            Some(DialogTone::Accent) if self.full_color => at_level(Rgb(0x4d, 0xa3, 0xff), self.color, self.base),
            Some(DialogTone::Danger) if self.full_color => at_level(Rgb(0xff, 0x5a, 0x67), self.color, self.base),
            Some(DialogTone::Title) => self.color.scale(65),
            _ => self.color,
        };
        // The selected app of the switcher gets the accent outline.
        let chosen_app = matches!(button.glyph, Glyph::App(_)) && button.on;
        let hue = if chosen_app && self.full_color {
            at_level(Rgb(0x4d, 0xa3, 0xff), self.color, self.base)
        } else {
            hue
        };
        let loud = chosen_app || matches!(tone, Some(DialogTone::Accent | DialogTone::Danger));
        let background = if active || loud {
            let fill = if loud { hue.scale(24) } else { self.color.scale(20) };
            self.fill(&rounded_rect(left, top, right, bottom, KEY_RADIUS), fill);
            fill
        } else {
            BLACK
        };
        let half = KEY_OUTLINE / 2.0;
        if tone != Some(DialogTone::Title) {
            self.stroke(
                &rounded_rect(
                    left + half,
                    top + half,
                    right - half,
                    bottom - half,
                    KEY_RADIUS,
                ),
                if loud { hue.scale(70) } else { self.color.scale(if active { 34 } else { 22 }) },
                KEY_OUTLINE,
            );
        }
        let cx = (left + right) / 2.0;
        match button.glyph {
            Glyph::Text(text) => {
                let text = self.fit_ascii(text, KEY_FONT_SIZE, right - left - 12.0, FONT_WEIGHT);
                self.center_text(cx, cy, &text, KEY_FONT_SIZE, self.color);
            }
            Glyph::Tone(text, _) => {
                let text = self.fit_ascii(text, KEY_FONT_SIZE, right - left - 12.0, FONT_WEIGHT);
                self.center_text(cx, cy, &text, KEY_FONT_SIZE, hue);
            }
            Glyph::Icon(icon) => self.icon(cx, cy, icon, background),
            Glyph::Lucide(name) => {
                if let Some(shapes) = lucide::shapes(name) {
                    let saved = self.color;
                    if self.full_color {
                        self.color = full_tint(name, saved, self.base);
                    }
                    self.shapes_sized(cx, cy, shapes, background, 1.0);
                    self.color = saved;
                }
            }
            Glyph::App(path) => self.draw_app_icon(cx, cy, path),
            Glyph::Widget(_) | Glyph::Blank => {}
        }
    }


    /// Converts the premultiplied, always opaque RGBA pixmap into XRGB8888.
    fn flush(&mut self) {
        for (target, pixel) in self.pixels.iter_mut().zip(self.pixmap.pixels()) {
            *target = (u32::from(pixel.red()) << 16)
                | (u32::from(pixel.green()) << 8)
                | u32::from(pixel.blue());
        }
    }

    fn fill(&mut self, path: &Path, color: Rgb) {
        self.pixmap.fill_path(
            path,
            &color.paint(),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    fn stroke(&mut self, path: &Path, color: Rgb, width: f32) {
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(path, &color.paint(), &stroke, Transform::identity(), None);
    }

    fn icon(&mut self, cx: f32, cy: f32, icon: Icon, background: Rgb) {
        self.icon_sized(cx, cy, icon, background, 1.0);
    }

    /// `size` scales the key-sized icon, including its stroke weight.
    fn icon_sized(&mut self, cx: f32, cy: f32, icon: Icon, background: Rgb, size: f32) {
        self.shapes_sized(cx, cy, icon_shapes(icon), background, size);
    }

    fn shapes_sized(&mut self, cx: f32, cy: f32, shapes: Vec<Shape>, background: Rgb, size: f32) {
        let scale = ICON_SCALE * size;
        let stroke = ICON_STROKE * size;
        let mut bounds: Option<Rect> = None;
        for shape in &shapes {
            let pad = shape.pad() * size;
            let Some(path) = shape
                .path()
                .clone()
                .transform(Transform::from_scale(scale, scale))
            else {
                continue;
            };
            let path_bounds = path.bounds();
            let rect = Rect::from_ltrb(
                path_bounds.left() - pad,
                path_bounds.top() - pad,
                path_bounds.right() + pad,
                path_bounds.bottom() + pad,
            );
            bounds = match (bounds, rect) {
                (Some(old), Some(new)) => Rect::from_ltrb(
                    old.left().min(new.left()),
                    old.top().min(new.top()),
                    old.right().max(new.right()),
                    old.bottom().max(new.bottom()),
                ),
                (old, new) => old.or(new),
            };
        }
        let Some(bounds) = bounds else { return };
        let dx = (cx - (bounds.left() + bounds.right()) / 2.0).round();
        let dy = (cy - (bounds.top() + bounds.bottom()) / 2.0).round();
        let transform = Transform::from_row(scale, 0.0, 0.0, scale, dx, dy);
        for shape in shapes {
            match shape {
                Shape::Stroke(path) => {
                    if let Some(path) = path.transform(transform) {
                        self.stroke(&path, self.color, stroke);
                    }
                }
                Shape::Solid(path) => {
                    if let Some(path) = path.transform(transform) {
                        // A thin round-joined stroke softens the corners the
                        // same way the keycap glyphs are rounded.
                        self.fill(&path, self.color);
                        self.stroke(&path, self.color, 1.4 * size);
                    }
                }
                Shape::Slash(path) => {
                    if let Some(path) = path.transform(transform) {
                        self.stroke(&path, background, stroke * 2.6);
                        self.stroke(&path, self.color, stroke);
                    }
                }
            }
        }
    }

    fn text_width(&self, text: &str, size: f32) -> f32 {
        let scale = size / self.font.units_per_em().unwrap_or(1000.0);
        let mut width = 0.0;
        let mut previous = None;
        for character in text.chars() {
            let id = self.font.glyph_id(character);
            if let Some(previous) = previous {
                width += self.font.kern_unscaled(previous, id) * scale;
            }
            width += self.font.h_advance_unscaled(id) * scale;
            previous = Some(id);
        }
        width
    }

    fn cap_height(&self, size: f32) -> f32 {
        let scale = size / self.font.units_per_em().unwrap_or(1000.0);
        self.font
            .outline(self.font.glyph_id('H'))
            .map_or(size * 0.7, |outline| outline.bounds.height().abs() * scale)
    }

    fn text_width_at(&mut self, text: &str, size: f32, weight: f32) -> f32 {
        self.font.set_variation(b"wght", weight);
        let width = self.text_width(text, size);
        self.font.set_variation(b"wght", FONT_WEIGHT);
        width
    }

    fn center_text(&mut self, cx: f32, cy: f32, text: &str, size: f32, color: Rgb) {
        let x = (cx - self.text_width(text, size) / 2.0).round();
        let baseline = (cy + self.cap_height(size) / 2.0).round();
        let mut path = PathBuilder::new();
        self.append_text(&mut path, x, baseline, text, size, FONT_WEIGHT);
        if let Some(path) = path.finish() {
            self.fill(&path, color);
        }
    }

    /// Appends the glyph outlines of `text` at `weight` and returns the pen
    /// position after the last glyph.
    fn append_text(
        &mut self,
        path: &mut PathBuilder,
        mut x: f32,
        baseline: f32,
        text: &str,
        size: f32,
        weight: f32,
    ) -> f32 {
        self.font.set_variation(b"wght", weight);
        let scale = size / self.font.units_per_em().unwrap_or(1000.0);
        let mut previous = None;
        for character in text.chars() {
            let id = self.font.glyph_id(character);
            if let Some(previous) = previous {
                x += self.font.kern_unscaled(previous, id) * scale;
            }
            if let Some(outline) = self.font.outline(id) {
                let point = |p: ab_glyph::Point| (x + p.x * scale, baseline - p.y * scale);
                let mut last = None;
                for curve in outline.curves {
                    let start = match curve {
                        OutlineCurve::Line(a, _)
                        | OutlineCurve::Quad(a, _, _)
                        | OutlineCurve::Cubic(a, _, _, _) => a,
                    };
                    if last != Some(start) {
                        let (sx, sy) = point(start);
                        path.move_to(sx, sy);
                    }
                    last = Some(match curve {
                        OutlineCurve::Line(_, b) => {
                            let (bx, by) = point(b);
                            path.line_to(bx, by);
                            b
                        }
                        OutlineCurve::Quad(_, c, b) => {
                            let ((cx, cy), (bx, by)) = (point(c), point(b));
                            path.quad_to(cx, cy, bx, by);
                            b
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let ((ax, ay), (cx, cy), (bx, by)) = (point(c1), point(c2), point(b));
                            path.cubic_to(ax, ay, cx, cy, bx, by);
                            b
                        }
                    });
                }
            }
            x += self.font.h_advance_unscaled(id) * scale;
            previous = Some(id);
        }
        self.font.set_variation(b"wght", FONT_WEIGHT);
        x
    }
}

fn load_font() -> Result<FontVec> {
    for path in FONT_PATHS {
        let Ok(data) = fs::read(path) else { continue };
        let mut font = FontVec::try_from_vec(data).with_context(|| format!("parse font {path}"))?;
        font.set_variation(b"wght", FONT_WEIGHT);
        return Ok(font);
    }
    Err(anyhow!(
        "no Touch Bar font found; install adwaita-sans-fonts ({})",
        FONT_PATHS.join(", ")
    ))
}

#[derive(Clone)]
pub(crate) enum Shape {
    Stroke(Path),
    Solid(Path),
    /// A crossing stroke that cuts a gap into the strokes below it.
    Slash(Path),
}

impl Shape {
    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Stroke(path) | Self::Solid(path) | Self::Slash(path) => path,
        }
    }

    fn pad(&self) -> f32 {
        match self {
            Self::Stroke(_) | Self::Slash(_) => ICON_STROKE / 2.0,
            Self::Solid(_) => 0.7,
        }
    }
}

fn icon_shapes(icon: Icon) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let build = |build: &dyn Fn(&mut PathBuilder)| {
        let mut path = PathBuilder::new();
        build(&mut path);
        path.finish()
    };
    match icon {
        Icon::Brightness(up) => {
            let (core, inner, outer) = if up {
                (4.6, 8.2, 11.0)
            } else {
                (3.6, 7.0, 8.0)
            };
            if let Some(path) = build(&|path| {
                path.push_circle(0.0, 0.0, core);
                rays(path, 0.0, 0.0, inner, outer, 0.0, 360.0, 8);
            }) {
                shapes.push(Shape::Stroke(path));
            }
        }
        Icon::Keyboard(up) => {
            let (inner, outer) = if up { (7.5, 10.5) } else { (7.0, 8.0) };
            if let Some(path) = build(&|path| {
                arc(path, 0.0, 3.0, 4.2, 180.0, 360.0);
                path.close();
                rays(path, 0.0, 3.0, inner, outer, 180.0, 360.0, 5);
                path.move_to(-6.0, 8.0);
                path.line_to(6.0, 8.0);
            }) {
                shapes.push(Shape::Stroke(path));
            }
        }
        Icon::Microphone => {
            if let Some(path) = build(&|path| {
                path.push_path(&rounded_rect(-3.6, -11.0, 3.6, 2.5, 3.6));
                arc(path, 0.0, -2.0, 7.4, 0.0, 180.0);
                path.move_to(0.0, 5.4);
                path.line_to(0.0, 9.5);
            }) {
                shapes.push(Shape::Stroke(path));
            }
            if let Some(path) = build(&|path| {
                path.move_to(-8.5, -10.0);
                path.line_to(8.5, 9.0);
            }) {
                shapes.push(Shape::Slash(path));
            }
        }
        Icon::Previous | Icon::Next => {
            let side = if matches!(icon, Icon::Next) {
                1.0
            } else {
                -1.0
            };
            if let Some(path) = build(&|path| {
                for offset in [-9.0f32, 0.5] {
                    path.move_to(side * offset, -6.8);
                    path.line_to(side * (offset + 9.5), 0.0);
                    path.line_to(side * offset, 6.8);
                    path.close();
                }
                // The bar at the tips marks a track skip (|◀◀, ▶▶|).
                let (near, far) = (side * 11.2, side * 13.8);
                path.push_path(&rounded_rect(near.min(far), -6.8, near.max(far), 6.8, 0.6));
            }) {
                shapes.push(Shape::Solid(path));
            }
        }
        Icon::PlayPause => {
            if let Some(path) = build(&|path| {
                path.move_to(-11.0, -6.8);
                path.line_to(-1.5, 0.0);
                path.line_to(-11.0, 6.8);
                path.close();
                path.push_path(&rounded_rect(2.5, -6.8, 5.5, 6.8, 0.6));
                path.push_path(&rounded_rect(8.0, -6.8, 11.0, 6.8, 0.6));
            }) {
                shapes.push(Shape::Solid(path));
            }
        }
        Icon::Mute | Icon::Volume(_) => {
            if let Some(path) = build(&|path| {
                path.move_to(-10.0, -3.6);
                path.line_to(-6.0, -3.6);
                path.line_to(-0.8, -8.4);
                path.line_to(-0.8, 8.4);
                path.line_to(-6.0, 3.6);
                path.line_to(-10.0, 3.6);
                path.close();
            }) {
                shapes.push(Shape::Solid(path));
            }
            let waves: &[f32] = match icon {
                Icon::Volume(true) => &[4.6, 8.4, 12.2],
                Icon::Volume(false) => &[4.6],
                _ => &[],
            };
            if let Some(path) = build(&|path| {
                for radius in waves {
                    let sweep = 46.0 - radius * 1.2;
                    arc(path, 0.8, 0.0, *radius, -sweep, sweep);
                }
            }) {
                shapes.push(Shape::Stroke(path));
            }
            if matches!(icon, Icon::Mute)
                && let Some(path) = build(&|path| {
                    path.move_to(-11.0, -9.5);
                    path.line_to(6.0, 9.5);
                })
            {
                shapes.push(Shape::Slash(path));
            }
        }
    }
    shapes
}

/// Appends an open arc. Angles are in degrees, clockwise from +x.
fn arc(path: &mut PathBuilder, cx: f32, cy: f32, radius: f32, from: f32, to: f32) {
    let steps = ((to - from).abs() / 6.0).ceil().max(2.0) as usize;
    for step in 0..=steps {
        let angle = (from + (to - from) * step as f32 / steps as f32).to_radians();
        let (x, y) = (cx + radius * angle.cos(), cy + radius * angle.sin());
        if step == 0 {
            path.move_to(x, y);
        } else {
            path.line_to(x, y);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rays(
    path: &mut PathBuilder,
    cx: f32,
    cy: f32,
    inner: f32,
    outer: f32,
    from: f32,
    to: f32,
    count: usize,
) {
    let full = (to - from - 360.0).abs() < f32::EPSILON;
    let divisions = if full { count } else { count - 1 };
    for index in 0..count {
        let angle = (from + (to - from) * index as f32 / divisions as f32).to_radians();
        let (sin, cos) = angle.sin_cos();
        path.move_to(cx + inner * cos, cy + inner * sin);
        path.line_to(cx + outer * cos, cy + outer * sin);
    }
}

fn rounded_rect(left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> Path {
    let radius = radius.min((right - left) / 2.0).min((bottom - top) / 2.0);
    let mut path = PathBuilder::new();
    // Cubic quarter circles with the usual 0.5523 control distance.
    let k = radius * (1.0 - 0.552_284_8);
    path.move_to(left + radius, top);
    path.line_to(right - radius, top);
    path.cubic_to(right - k, top, right, top + k, right, top + radius);
    path.line_to(right, bottom - radius);
    path.cubic_to(right, bottom - k, right - k, bottom, right - radius, bottom);
    path.line_to(left + radius, bottom);
    path.cubic_to(left + k, bottom, left, bottom - k, left, bottom - radius);
    path.line_to(left, top + radius);
    path.cubic_to(left, top + k, left + k, top, left + radius, top);
    path.close();
    path.finish()
        .expect("rounded rectangle has a non-empty outline")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_have_no_overlapping_buttons() {
        for mode in [Mode::Media, Mode::Function, Mode::Special] {
            let layout = Layout::new(mode, 2170, false);
            for pair in layout.buttons.windows(2) {
                assert!(pair[0].right < pair[1].left);
            }
        }
    }

    fn sample_layouts(ctx: &LayoutCtx) -> Vec<Layout> {
        let config = crate::layout::defaults();
        let mut zones: Vec<&[ItemKind]> = vec![&config.default[..]];
        zones.extend(config.profiles.iter().map(|p| &p.items[..]));
        zones
            .into_iter()
            .map(|zone| Layout::from_items(zone, &config.strip, 2008, true, ctx))
            .collect()
    }

    #[test]
    fn panel_hit_maps_x_to_slider_and_level() {
        let zone = (10, 1000);
        let (left, right) = panel_slice(zone, 3, 1);
        let (bar_left, bar_right) = panel_bar(left, right);
        assert_eq!(panel_hit(zone, 3, f64::from(bar_left)), Some((1, 0)));
        assert_eq!(panel_hit(zone, 3, f64::from(bar_right)), Some((1, 100)));
        let mid = f64::from((bar_left + bar_right) / 2.0);
        assert_eq!(panel_hit(zone, 3, mid), Some((1, 50)));
        assert_eq!(panel_hit(zone, 3, -5.0), None);
        // Slices never overlap.
        assert!(panel_slice(zone, 3, 0).1 < panel_slice(zone, 3, 1).0);
    }

    #[test]
    fn items_layout_has_no_overlap_at_2008() {
        for player_active in [false, true] {
            let ctx = LayoutCtx {
                player_active,
                ..LayoutCtx::default()
            };
            for layout in sample_layouts(&ctx) {
                assert!(!layout.buttons.is_empty());
                for pair in layout.buttons.windows(2) {
                    assert!(pair[0].right < pair[1].left, "{} {}", pair[0].right, pair[1].left);
                }
                assert!(layout.buttons.last().unwrap().right <= 2008 - KEY_GAP);
            }
        }
    }

    #[test]
    fn slots_pin_items_to_grid_columns() {
        let items = [ItemKind::Screenshot, ItemKind::Screenshot, ItemKind::Screenshot];
        let slots = [
            Slot::default(),
            Slot { col: Some(4), span: Some(4) },
            // Overlaps the previous item, so it is pushed after it.
            Slot { col: Some(5), span: None },
        ];
        let layout = Layout::from_placed(&items, &slots, &[], &[], 2008, true, &LayoutCtx::default());
        let pitch = f32::from(2008 - 2 * KEY_GAP + KEY_GAP) / 24.0;
        let at = |index: usize| f32::from(layout.buttons[index].left);
        let width = |index: usize| f32::from(layout.buttons[index].right - layout.buttons[index].left);
        assert!((at(1) - at(0) - 4.0 * pitch).abs() <= 2.0, "{} {}", at(1), at(0));
        assert!((width(0) - (2.0 * pitch - f32::from(KEY_GAP))).abs() <= 2.0);
        assert!((width(1) - (4.0 * pitch - f32::from(KEY_GAP))).abs() <= 2.0);
        assert!((at(2) - at(1) - 4.0 * pitch).abs() <= 2.0);
        assert!(layout.buttons.windows(2).all(|pair| pair[0].right < pair[1].left));
    }

    #[test]
    fn strip_is_right_aligned() {
        let config = crate::layout::defaults();
        let layout = Layout::from_items(&config.default, &config.strip, 2008, true, &LayoutCtx::default());
        assert_eq!(layout.buttons.last().unwrap().right, 2008 - KEY_GAP);
        assert!(matches!(
            layout.buttons.last().unwrap().glyph,
            Glyph::Widget(WidgetKind::Battery)
        ));
    }

    #[test]
    fn now_playing_collapses_without_player() {
        let config = crate::layout::defaults();
        let build = |player_active| {
            let ctx = LayoutCtx {
                player_active,
                ..LayoutCtx::default()
            };
            Layout::from_items(&config.default, &config.strip, 2008, true, &ctx)
        };
        let (off, on) = (build(false), build(true));
        // Now playing plus the three playback keys.
        assert_eq!(on.buttons.len(), off.buttons.len() + 4);

    }

    #[test]
    fn esc_and_row_edges_stay_put_across_modes() {
        let media = Layout::new(Mode::Media, 2170, false);
        let function = Layout::new(Mode::Function, 2170, false);
        let edges = |layout: &Layout| {
            let first = layout.buttons[0];
            let last = layout.buttons[layout.buttons.len() - 1];
            (first.left, first.right, layout.buttons[1].left, last.right)
        };
        assert_eq!(edges(&media), edges(&function));
        // The special row keeps esc and the right edge, and the media width.
        let special = Layout::new(Mode::Special, 2170, false);
        let (esc, last) = (
            special.buttons[0],
            special.buttons[special.buttons.len() - 1],
        );
        let media_last = media.buttons[media.buttons.len() - 1];
        assert_eq!(
            (esc.left, esc.right),
            (media.buttons[0].left, media.buttons[0].right)
        );
        assert_eq!(last.right, media_last.right);
        assert_eq!(last.right - last.left, media_last.right - media_last.left);
    }

    #[test]
    fn personal_keys_fill_only_the_free_space() {
        let labels = ["~", "|", "a", "b", "c", "d", "e"];
        let special = Layout::with_custom(Mode::Special, 2170, false, &labels);
        let custom: Vec<_> = special
            .buttons
            .iter()
            .filter(|button| matches!(button.action, Action::Custom(_)))
            .collect();
        assert_eq!(custom.len(), 4, "four keys fit between esc and print");
        assert_eq!(custom[0].action, Action::Custom(0));
        assert!(custom[0].left > special.buttons[0].right + GROUP_GAP - 1);
        for pair in special.buttons.windows(2) {
            assert!(pair[0].right < pair[1].left);
        }
    }

    #[test]
    fn touch_id_draws_only_near_the_sensor_and_label() {
        let mut canvas = Canvas::new(2170, 60, 0x00dce6ff).unwrap();
        canvas.touch_id(&TouchIdLook::default(), "waiting", 0.5, None, false);
        assert!(canvas.pixels.iter().any(|pixel| *pixel != 0));
        assert_eq!(canvas.pixels[0], 0);
    }

    #[test]
    fn every_touch_id_look_draws_every_state_near_the_sensor() {
        let mut canvas = Canvas::new(2008, 60, 0x00dce6ff).unwrap();
        for style in ['A', 'B', 'C', 'D', 'E', 'F'] {
            for state in super::TOUCH_ID_STATES {
                for motion in [0, 1, 2] {
                    let look = TouchIdLook { style, motion, ..TouchIdLook::default() };
                    canvas.touch_id(&look, state, 0.4, None, false);
                    let lit = |range: std::ops::Range<usize>| {
                        (0..60).any(|y| range.clone().any(|x| canvas.pixels[y * 2008 + x] != 0))
                    };
                    assert!(lit(1000..2008), "{style} {state}: nothing near the sensor");
                    // The left side stays dark except for full-bar floods and the thread.
                    if !matches!(style, 'D' | 'F') {
                        assert!(!lit(0..600), "{style} {state}: drew far from the sensor");
                    }
                }
            }
        }
    }

    #[test]
    fn custom_touch_id_text_replaces_the_language_text() {
        let mut canvas = Canvas::new(2008, 60, 0x00dce6ff).unwrap();
        let plain = TouchIdLook { style: 'E', ..TouchIdLook::default() };
        canvas.touch_id(&plain, "waiting", 0.0, None, false);
        let default_lit = canvas.pixels.iter().filter(|p| **p != 0).count();
        let mut short = plain.clone();
        short.texts[0] = "Hi".into();
        canvas.touch_id(&short, "waiting", 0.0, None, false);
        assert!(canvas.pixels.iter().filter(|p| **p != 0).count() < default_lit);
    }

    /// Renders both layers and the Touch ID prompt for design review:
    /// `KAIT2EN_TOUCHBAR_PREVIEW=/tmp/tb cargo test preview -- --ignored`
    #[test]
    #[ignore]
    fn preview() {
        let dir = std::env::var("KAIT2EN_TOUCHBAR_PREVIEW").unwrap_or_else(|_| ".".into());
        let mut canvas = Canvas::new(2170, 60, 0x00dce6ff).unwrap();
        let save = |name: &str, canvas: &Canvas| {
            let mut ppm = format!("P6 {} {} 255\n", canvas.width, canvas.height).into_bytes();
            for pixel in &canvas.pixels {
                ppm.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
            }
            fs::write(format!("{dir}/{name}.ppm"), ppm).unwrap();
        };
        canvas.keys(&Layout::new(Mode::Media, 2170, false), None);
        save("media", &canvas);
        let mut strip = crate::layout::defaults().strip;
        strip.splice(0..0, [ItemKind::Cpu, ItemKind::Memory, ItemKind::Temperature, ItemKind::Fan, ItemKind::Network]);
        let data = WidgetData {
            clock: "19:07".into(),
            battery: Some((66, false)),
            cpu: Some(12),
            memory: Some(45),
            temp_c: Some(58),
            fan_rpm: Some(2807),
            net: Some((1_258_291, 81_920)),
            ..WidgetData::default()
        };
        let layout = Layout::from_items(&crate::layout::defaults().default, &strip, 2170, true, &LayoutCtx::default());
        canvas.keys_with(&layout, None, Some(&data));
        save("plus-widgets", &canvas);
        let ctx = LayoutCtx {
            player_active: true,
            ..LayoutCtx::default()
        };
        let layout = Layout::from_items(&crate::layout::defaults().default, &crate::layout::defaults().strip, 2170, true, &ctx);
        let data = WidgetData {
            clock: "19:07".into(),
            battery: Some((66, false)),
            now_playing: Some(NowPlayingView {
                art: Some((0..52 * 52).map(|i| ((i % 52) * 5) as u8).collect()),
                line: "Slipknot - Duality".into(),
                fraction: 0.4,
                bars: Some([4, 12, 7, 15, 9]),
            }),
            ..WidgetData::default()
        };
        canvas.keys_with(&layout, None, Some(&data));
        save("plus-nowplaying", &canvas);
        let config = crate::layout::defaults();
        for (name, zone) in [
            ("plus-default", &config.default[..]),
            ("plus-firefox", &config.profiles[0].items[..]),
            ("plus-files", &config.profiles[2].items[..]),
        ] {
            let ctx = LayoutCtx {
                player_active: true,
                ..LayoutCtx::default()
            };
            canvas.keys(&Layout::from_items(zone, &config.strip, 2170, true, &ctx), None);
            save(name, &canvas);
        }
        canvas.keys(&Layout::new(Mode::Function, 2170, false), None);
        save("function", &canvas);
        canvas.keys(
            &Layout::with_custom(Mode::Special, 2170, false, &["~", "|", "term"]),
            None,
        );
        save("special", &canvas);
        canvas.touch_id(&TouchIdLook::default(), "waiting", 0.0, None, false);
        save("touchid", &canvas);
        canvas.touch_id(&TouchIdLook::default(), "waiting", 1.0, None, false);
        save("touchid-nudged", &canvas);
        let media = Layout::new(Mode::Media, 2170, false);
        let keys = TrackLayout {
            prev: (media.buttons[1].left, media.buttons[1].right),
            next: (2170 - media.buttons[1].right, 2170 - media.buttons[1].left),
        };
        canvas.track("Slipknot", "Duality", 100, None, keys, None);
        save("track", &canvas);
        canvas.track(
            "Slipknot",
            "Duality",
            100,
            None,
            keys,
            Some((media.buttons[0], 100)),
        );
        save("track-esc", &canvas);
        canvas.track(
            "Heino",
            "Blau blüht der Enzian (Live aus der Festhalle mit Orchester und sehr langem Titel, der gekürzt werden muss, weil er sonst nicht passt)",
            60,
            Some(true),
            keys,
            None,
        );
        save("track-long", &canvas);
        canvas.touch_id(&TouchIdLook::default(), "retry", 0.0, None, false);
        save("touchid-retry", &canvas);
        canvas.level(LevelKind::Volume, Some(45), false, (560.0, 640.0), 1.0, 100);
        save("volume", &canvas);
        canvas.level(
            LevelKind::Brightness,
            Some(80),
            false,
            (1620.0, 1780.0),
            0.0,
            100,
        );
        save("brightness", &canvas);
        canvas.level(
            LevelKind::Volume,
            Some(45),
            false,
            (1000.0, 1100.0),
            0.3,
            100,
        );
        save("volume-crossfade", &canvas);
    }
}
