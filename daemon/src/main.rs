// SPDX-License-Identifier: GPL-3.0-or-later

mod backlight;
mod art;
mod calendar;
mod config;
mod display;
mod events;
mod gesture;
mod haptic;
mod i18n;
mod kbdlight;
mod keyboard;
mod keys;
mod layout;
mod levels;
mod apps;
mod lucide;
mod atspi;
mod mpris;
mod notify;
mod policy;
mod pomodoro;
mod renderer;
mod screensaver;
mod shell;
mod toggles;
mod touchid;
mod upower;
mod usb;
mod weather;
mod widgets;

use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Read,
    os::{
        fd::{AsFd, AsRawFd, OwnedFd},
        unix::{fs::OpenOptionsExt, net::UnixStream},
    },
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use input::{
    Device as InputDevice, DeviceCapability, Libinput, LibinputInterface, SendEventsMode,
    event::{
        DeviceEvent, Event, EventTrait,
        keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait},
        touch::{TouchEvent, TouchEventPosition, TouchEventSlot},
    },
};
use input_linux::Key;
use libc::{O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY, pollfd};

use backlight::Backlight;
use config::{Config, state_path};
use display::Display;
use gesture::{Swipe, SwipeKind};
use haptic::Haptic;
use keyboard::VirtualKeyboard;
use keys::{CustomAction, CustomKey};
use layout::{ItemKind, KeySpec, LayoutConfig, Slot};
use levels::{Level, Reader as LevelReader};
use mpris::{Mpris, Track};
use policy::{Mode, PersistentState, TimeoutLearner};
use renderer::{Action, Canvas, Layout, LayoutCtx, LevelKind, TrackLayout};
use touchid::TouchIdState;
use widgets::WidgetData;

/// How long the esc key stays lit after it was used on the dark bar, so even
/// a quick tap is visible.
const ESC_LINGER_MS: u64 = 150;
/// Swipe feedback stays a little after the fingers lift to show the result.
const LEVEL_LINGER_MS: u64 = 700;
const LEVEL_FADE_MS: u64 = 350;
const LEVEL_FRAME_MS: u64 = 33;
const ACTIVITY_FADE_MS: u64 = 700;
const ACTIVITY_FRAME_MS: u64 = 33;
/// The desktop applies a volume or brightness key asynchronously, so the
/// value is read back repeatedly for a short while after each step.
const LEVEL_POLL_MS: u64 = 50;
const LEVEL_POLL_WINDOW_MS: u64 = 600;
/// The level feedback crossfades to the other side of the fingers once
/// they pass the middle of the bar by this much.
const LEVEL_SIDE_HYSTERESIS_PX: f64 = 60.0;
const LEVEL_SIDE_FADE_MS: u64 = 700;
/// Switching rows move far enough to read as a swipe across the bar.
const LAYER_SLIDE_PX: f32 = 190.0;
/// Switching rows take 195 ms.
const LAYER_FADE_MS: u64 = 195;
const LAYER_FRAME_MS: u64 = 20;
/// Track overlay timeline: fade in, hold, fade out.
const TRACK_FADE_MS: u64 = 700;
const TRACK_HOLD_MS: u64 = 5_000;
/// About 30 fps while the overlay fades.
const TRACK_FRAME_MS: u64 = 33;
/// Terminals paste with Ctrl+Shift+V.
const TERMINALS: [&str; 2] = ["org.gnome.Ptyxis.desktop", "com.mitchellh.ghostty.desktop"];
/// With the screen dark, a touch lights the bar for this long without touches.
const SCREEN_OFF_AWAKE_MS: u64 = 120_000;
/// Notification card: slide in, hold, slide out; a newer one swaps in.
const NOTICE_ENTER_MS: u64 = 380;
const NOTICE_HOLD_MS: u64 = 4_500;
const NOTICE_EXIT_MS: u64 = 260;
const NOTICE_SWAP_MS: u64 = 240;
/// While the card holds, the bar under it needs a frame this often.
const NOTICE_HOLD_FRAME_MS: u64 = 100;
/// A notice kept for later is dropped after this long, and rechecked this often.
const PENDING_CARD_MS: u64 = 30_000;
const PENDING_CARD_POLL_MS: u64 = 500;
/// Holding previous/next this long from touch-down starts seeking.
/// The Touch ID arrow swings towards the sensor once per period, at about
/// 30 fps.
const PROMPT_PERIOD_MS: u64 = 1_800;
const PROMPT_FRAME_MS: u64 = 33;
const MEDIA_HOLD_MS: u64 = 450;
const SEEK_INTERVAL_MS: u64 = 250;
/// Seek steps grow after this long, so long distances stay quick.
const SEEK_FAST_AFTER_MS: u64 = 1_500;
const SEEK_STEP: Duration = Duration::from_secs(5);
const SEEK_FAST_STEP: Duration = Duration::from_secs(15);

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> std::result::Result<OwnedFd, i32> {
        let mode = flags & O_ACCMODE;
        OpenOptions::new()
            .custom_flags(flags)
            .read(mode == O_RDONLY || mode == O_RDWR)
            .write(mode == O_WRONLY || mode == O_RDWR)
            .open(path)
            .map(Into::into)
            .map_err(|error| error.raw_os_error().unwrap_or(libc::EIO))
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(File::from(fd));
    }
}

#[derive(Clone, Copy, Debug)]
enum Contact {
    Wake,
    Button(usize),
    TouchIdEsc(usize),
    Cancelled,
    /// First finger, waiting whether a second one turns this into a swipe.
    Pending,
    Swipe,
    /// Esc held on the dark bar. The bar stays dark.
    DarkEsc,
    /// Previous/next held: a tap on release, seeking once held long enough.
    MediaHold(usize),
    /// A finger on the now playing widget: a tap pauses, a drag seeks.
    Scrub(usize),
    /// A finger on slider `n` of the open controls panel.
    Panel(usize),
    /// Volume or brightness key held: a tap on release, a slider once held
    /// for `hold_slider_ms`.
    SliderHold(usize),
    /// Pomodoro key: a tap pauses or starts, a long hold resets. Holds the
    /// touch time.
    PomoHold(usize, u64),
    /// A key of the track overlay on the dark bar; `true` is next.
    TrackKey(bool),
}

/// Crossfade from the previous row to the current one.
struct LayerFade {
    /// The frame that was on screen when the switch started.
    from: Vec<u32>,
    start_ms: u64,
    next_ms: u64,
    /// `1` when the new row comes in from the right, `-1` from the left.
    direction: i32,
    /// Everything left of this column (esc) stays in place.
    clip_left: usize,
    /// A swipe switched the row: a firm click confirms it once the new row
    /// has settled.
    confirm: bool,
}

/// "Artist – Title" shown on the dark bar when a new track starts.
struct TrackToast {
    track: Track,
    /// Origin of the fade in / hold / fade out timeline.
    start_ms: u64,
    next_ms: Option<u64>,
    layout: TrackLayout,
    pressed: Option<bool>,
    /// A notification or timer message: no skip keys.
    notice: bool,
    /// The card of a notification, and the one it replaced with when.
    card: Option<renderer::NoticeCard>,
    replaced: Option<(renderer::NoticeCard, u64)>,
}

struct BacklightFade {
    start_ms: u64,
    next_ms: u64,
    from: u32,
    to: u32,
}

struct EscapeFade {
    start_ms: u64,
    next_ms: u64,
    from: f32,
    to: f32,
}

/// Previous/next under a finger. Released early it skips the track, held
/// past `MEDIA_HOLD_MS` it seeks in steps until released.
/// The open controls panel: one slider per configured control.
struct Panel {
    levels: [Option<u32>; 3],
    active: Option<usize>,
    until_ms: u64,
    last_set_ms: u64,
}

struct SliderHold {
    slot: u32,
    kind: LevelKind,
    due_ms: u64,
}

/// Held keys that open a slider instead of repeating.
const TRANSIENT_IDLE_MS: u64 = 10_000;

fn close_key() -> ItemKind {
    ItemKind::Transient(layout::TransientKey {
        glyph: layout::TransientGlyph::Icon("x"),
        action: layout::TransientAction::Close,
    })
}

/// The apps of Super+Tab, mirrored on the bar while the keys are held.
struct Switcher {
    apps: shell::AppList,
    selected: usize,
}

/// The app icons around the selected one, then its name and windows.
fn switcher_row(switcher: &Switcher) -> Vec<ItemKind> {
    let count = switcher.apps.len();
    let first = switcher.selected.saturating_sub(SWITCHER_APPS / 2).min(count.saturating_sub(SWITCHER_APPS));
    let key = |glyph, action| ItemKind::Transient(layout::TransientKey { glyph, action });
    let mut items = Vec::new();
    for (index, (id, name, _)) in switcher.apps.iter().enumerate().skip(first).take(SWITCHER_APPS) {
        let chosen = index == switcher.selected;
        let glyph = match apps::icon_path(id) {
            Some(path) => layout::TransientGlyph::App(layout::intern(&path.to_string_lossy()), chosen),
            None => {
                let initial: String = fold_ascii(name).chars().take(1).collect::<String>().to_uppercase();
                let tone = if chosen { layout::DialogTone::Accent } else { layout::DialogTone::Neutral };
                layout::TransientGlyph::Tone(layout::intern(&initial), tone)
            }
        };
        items.push(key(glyph, layout::TransientAction::SwitchApp(index)));
    }
    let (_, name, windows) = &switcher.apps[switcher.selected];
    items.push(key(
        layout::TransientGlyph::Tone(layout::intern(&ascii_label(name)), layout::DialogTone::Title),
        layout::TransientAction::Label,
    ));
    for (id, title) in windows.iter().take(SWITCHER_WINDOWS) {
        items.push(key(
            layout::TransientGlyph::Text(layout::intern(&ascii_label(title))),
            layout::TransientAction::SwitchWindow(*id),
        ));
    }
    items
}

/// Widest the app icons go before the list slides around the selection.
const SWITCHER_APPS: usize = 7;
const SWITCHER_WINDOWS: usize = 2;

/// A dialog keeps its row until the app closes it.
const DIALOG_HOLD_MS: u64 = 3_600_000;

/// The title and one key per button of a dialog.
fn dialog_row(title: &str, labels: &[&str]) -> Vec<ItemKind> {
    let key = |text: &str, tone, action| {
        ItemKind::Transient(layout::TransientKey {
            glyph: layout::TransientGlyph::Tone(layout::intern(&ascii_label(text)), tone),
            action,
        })
    };
    let mut items = Vec::new();
    if !title.is_empty() {
        items.push(key(title, layout::DialogTone::Title, layout::TransientAction::Label));
    }
    for (index, label) in labels.iter().enumerate() {
        let tone = dialog_tone(label, index + 1 == labels.len());
        items.push(key(label, tone, layout::TransientAction::Dialog(index)));
    }
    items
}

/// Color role of a dialog button by its wording: the dialog's own style is
/// not exposed over AT-SPI. Destructive words are red, dismissals neutral, and
/// the last other button is the accent.
fn dialog_tone(label: &str, last: bool) -> layout::DialogTone {
    use layout::DialogTone::{Accent, Danger, Neutral};
    let text = ascii_label(label).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.starts_with(word));
    if has(&[
        "close", "delete", "quit", "discard", "remove", "erase", "terminate", "exit", "log out", "sign out",
        "shut", "restart", "reboot", "cerrar", "eliminar", "borrar", "salir", "descartar", "quitar",
        "terminar", "apagar", "reiniciar",
    ]) {
        Danger
    } else if text == "no" || has(&["cancel", "not now", "later", "dismiss", "mas tarde", "ahora no", "back", "volver"]) {
        Neutral
    } else if last {
        Accent
    } else {
        Neutral
    }
}

/// `text` as plain ASCII on one line: accents folded, control characters
/// turned into spaces, anything else non-ASCII dropped. Text that has only
/// such characters becomes `-`, never an empty label.
fn fold_ascii(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            'á' | 'à' | 'ä' | 'â' => out.push('a'),
            'é' | 'è' | 'ë' | 'ê' => out.push('e'),
            'í' | 'ì' | 'ï' | 'î' => out.push('i'),
            'ó' | 'ò' | 'ö' | 'ô' => out.push('o'),
            'ú' | 'ù' | 'ü' | 'û' => out.push('u'),
            'ñ' => out.push('n'),
            'ç' => out.push('c'),
            'Á' | 'À' | 'Ä' | 'Â' => out.push('A'),
            'É' | 'È' | 'Ë' | 'Ê' => out.push('E'),
            'Í' | 'Ì' | 'Ï' | 'Î' => out.push('I'),
            'Ó' | 'Ò' | 'Ö' | 'Ô' => out.push('O'),
            'Ú' | 'Ù' | 'Ü' | 'Û' => out.push('U'),
            'Ñ' => out.push('N'),
            'Ç' => out.push('C'),
            'ß' => out.push_str("ss"),
            c if c.is_control() => out.push(' '),
            c if c.is_ascii() => out.push(c),
            _ => {}
        }
    }
    let out = out.trim();
    if out.is_empty() && !text.trim().is_empty() { "-".to_owned() } else { out.to_owned() }
}

/// Notification bodies may carry `<b>`-style markup and entities. A `<` that
/// does not open a tag stays as text.
fn strip_markup(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let starts_tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/');
        match (after.find('>'), after.find('<')) {
            (Some(close), next) if starts_tag && next.is_none_or(|next| close < next) => rest = &after[close + 1..],
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    // `&amp;` last, so `&amp;lt;` stays `&lt;`.
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// A label of at most 12 ASCII characters for a key.
fn ascii_label(text: &str) -> String {
    let label: String = fold_ascii(text).chars().take(12).collect();
    let label = label.trim().to_owned();
    if label.is_empty() { "-".into() } else { label }
}

/// A row that temporarily replaces the app zone.
struct Transient {
    items: Vec<ItemKind>,
    until_ms: u64,
    /// The window list and page, while the windows row is open.
    windows: Option<(shell::WindowList, usize)>,
}

fn slider_kind(action: Action) -> Option<LevelKind> {
    match action {
        Action::Key(Key::VolumeDown | Key::VolumeUp | Key::Mute) => Some(LevelKind::Volume),
        Action::Key(Key::BrightnessDown | Key::BrightnessUp) => Some(LevelKind::Brightness),
        _ => None,
    }
}

struct Scrub {
    start_x: f64,
    x: f64,
}

struct MediaHold {
    forward: bool,
    next_ms: u64,
    seeking_since: Option<u64>,
    player: Option<String>,
}

/// A first touch held back for `two_finger_window_ms`. On the dark bar it
/// becomes a wake, on the lit bar a key press, unless a second finger joins.
#[derive(Clone, Copy, Debug)]
struct PendingTouch {
    slot: u32,
    deadline_ms: u64,
    dark: bool,
    button: Option<usize>,
}

struct Runtime {
    config: Config,
    state: PersistentState,
    state_path: std::path::PathBuf,
    learner: TimeoutLearner,
    started: Instant,
    visible: bool,
    touch_id: TouchIdState,
    deadline_ms: Option<u64>,
    fn_down_ms: Option<u64>,
    fn_toggled: bool,
    contacts: HashMap<u32, Contact>,
    positions: HashMap<u32, f64>,
    pending: Option<PendingTouch>,
    swipe: Option<Swipe>,
    /// The dark bar currently lights only the esc key.
    overlay_shown: bool,
    overlay_hide_ms: Option<u64>,
    /// Level feedback while a dark-bar swipe runs.
    level_kind: Option<LevelKind>,
    level: Level,
    /// Leftmost and rightmost finger the feedback was last placed against.
    level_fingers: (f64, f64),
    /// How much of the feedback shows right of the fingers, moving towards
    /// `level_right_target` for the crossfade.
    level_right: f32,
    level_right_target: bool,
    level_right_ms: u64,
    level_poll_ms: Option<u64>,
    level_poll_until_ms: u64,
    /// The level feedback changed and is drawn once the pending input is
    /// processed, so a burst of touch events costs one frame.
    level_dirty: bool,
    level_fade_start_ms: Option<u64>,
    level_frame_ms: Option<u64>,
    /// This swipe has sent a key, so a level at 0 or 100 was reached by it.
    level_stepped: bool,
    /// The stop the level sits at was already felt as a firm click.
    level_stop_felt: bool,
    /// OLED care: dim after idle, and move the picture one pixel now and then.
    dim_deadline_ms: Option<u64>,
    dimmed: bool,
    shift_index: usize,
    shift_next_ms: Option<u64>,
    activity_deadline_ms: Option<u64>,
    activity_dimmed: bool,
    activity_restore_brightness: Option<u32>,
    activity_max_brightness: Option<u32>,
    activity_esc_shown: bool,
    activity_current_brightness: Option<u32>,
    backlight_fade: Option<BacklightFade>,
    escape_opacity: f32,
    escape_fade: Option<EscapeFade>,
    /// Last values seen per kind, shown until a fresh read arrives.
    level_cache: [Level; 2],
    level_reader: LevelReader,
    level_wake: UnixStream,
    key_level: u32,
    media_hold: Option<MediaHold>,
    mpris: Mpris,
    toast: Option<TrackToast>,
    /// The screen is locked, or the monitors are off.
    locked: bool,
    monitors_on: bool,
    layer_fade: Option<LayerFade>,
    /// The blended frame while a crossfade runs.
    blended: Vec<u32>,
    touches_armed: bool,
    active_button: Option<usize>,
    layout: Layout,
    canvas: Canvas,
    display: Display,
    backlight: Backlight,
    keyboard: VirtualKeyboard,
    haptic: Haptic,
    physical_escape: bool,
    /// Personal keys from keys.toml, shown in the special row.
    custom_keys: Vec<CustomKey>,
    custom_labels: Vec<&'static str>,
    /// Configured media row (layout.toml) and the app that has the focus.
    plus: LayoutConfig,
    focus_app: String,
    /// Items of the visible media row: app zone, then control strip.
    items: Vec<ItemKind>,
    layout_ctx: LayoutCtx,
    widgets: WidgetData,
    events: Vec<calendar::Event>,
    bus: Option<events::Bus>,
    panel: Option<Panel>,
    panel_zone: (u16, u16),
    recent_notices: std::collections::VecDeque<(String, String, u64)>,
    /// A notice kept while Touch ID or a slider has the bar, and when it expires.
    pending_card: Option<(renderer::NoticeCard, u64)>,
    pending_card_ms: Option<u64>,
    transient: Option<Transient>,
    /// Dialog of the focused window shown as the bar's row, and the switch
    /// that lets its poller run while the bar is lit.
    switcher: Option<Switcher>,
    /// Super or Alt keys held, Shift held, Tabs waiting for the app list,
    /// and until when the Tabs we inject ourselves are not counted.
    switch_mods: u8,
    shift_held: u8,
    switch_steps: i32,
    switch_wanted: bool,
        dialog: Option<atspi::Dialog>,
    dialog_poll: Arc<AtomicBool>,
    suggest: Option<Vec<ItemKind>>,
    pomodoro: pomodoro::Pomodoro,
    pomo_next_ms: Option<u64>,
    windows_missing_until_ms: Option<u64>,
    slider_hold: Option<SliderHold>,
    /// The level overlay covers the lit bar.
    slider_shown: bool,
    player: Option<mpris::PlayerInfo>,
    /// Position at `position_at_ms`; interpolated while playing.
    position_us: i64,
    position_at_ms: u64,
    position_read_ms: u64,
    art: Option<(String, Vec<u8>)>,
    scrub: Option<Scrub>,
    np_next_ms: Option<u64>,
    viz: [u8; 5],
    viz_seed: u32,
    toggles_next_ms: Option<u64>,
    sampler: widgets::Sampler,
    widgets_next_ms: Option<u64>,
    clock_next_ms: Option<u64>,
    animation_start_ms: u64,
    next_animation_ms: Option<u64>,
}

impl Runtime {
    fn new(config: Config, state: PersistentState) -> Result<Self> {
        let mut display = Display::open()?;
        let (width, height) = display.dimensions();
        let mut canvas = Canvas::new(width, height, canvas_color(&config))?;
        canvas.full_color = config.icon_color == "full";
        display.present(&canvas.pixels, width, height)?;
        let mut backlight = Backlight::open()?;
        backlight.set(0)?;
        let physical_escape = has_physical_escape();
        let custom_keys = keys::load();
        let custom_labels: Vec<&'static str> = custom_keys.iter().map(|key| key.label).collect();
        let plus = layout::load();
        let (work_min, break_min) = (plus.work_min, plus.break_min);
        let configured = plus
            .default
            .iter()
            .chain(&plus.strip)
            .chain(plus.profiles.iter().flat_map(|profile| &profile.items));
        let custom_codes: Vec<Key> = custom_keys
            .iter()
            .flat_map(|key| key.keys().iter().copied())
            .chain(configured.flat_map(|item| match item {
                ItemKind::Key(KeySpec {
                    action: CustomAction::Send(keys),
                    ..
                }) => keys.clone(),
                _ => Vec::new(),
            }))
            .collect();
        let layout = Layout::with_custom(state.mode, width, physical_escape, &custom_labels);
        let haptic = Haptic::open(config.haptic_feedback, config.haptic_touch_up);
        let (level_reader, level_wake) = LevelReader::spawn()?;
        level_wake.set_nonblocking(true)?;
        let initial_keyboard_brightness = config
            .activity_backlight
            .then(|| kbdlight::brightness().ok())
            .flatten();
        let activity_restore_brightness =
            initial_keyboard_brightness.filter(|brightness| *brightness > 0);
        let activity_dimmed = config.activity_backlight && initial_keyboard_brightness == Some(0);
        let activity_deadline_ms = config
            .activity_backlight
            .then_some(config.activity_timeout_ms);
        let pixel_shift_first_ms = config
            .pixel_shift
            .then_some(config.pixel_shift_interval_ms);
        Ok(Self {
            config,
            state,
            state_path: state_path(),
            learner: TimeoutLearner::new(),
            started: Instant::now(),
            visible: false,
            touch_id: TouchIdState::Idle,
            deadline_ms: None,
            fn_down_ms: None,
            fn_toggled: false,
            contacts: HashMap::new(),
            positions: HashMap::new(),
            pending: None,
            swipe: None,
            overlay_shown: false,
            overlay_hide_ms: None,
            level_kind: None,
            level: Level::default(),
            level_fingers: (0.0, 0.0),
            level_right: 1.0,
            level_right_target: true,
            level_right_ms: 0,
            level_poll_ms: None,
            level_poll_until_ms: 0,
            level_dirty: false,
            level_fade_start_ms: None,
            level_frame_ms: None,
            level_stepped: false,
            level_stop_felt: false,
            activity_deadline_ms,
            activity_dimmed,
            activity_restore_brightness,
            activity_max_brightness: None,
            activity_esc_shown: false,
            activity_current_brightness: initial_keyboard_brightness,
            backlight_fade: None,
            escape_opacity: 0.0,
            escape_fade: None,
            dim_deadline_ms: None,
            dimmed: false,
            shift_index: 0,
            shift_next_ms: pixel_shift_first_ms,
            level_cache: [Level::default(); 2],
            level_reader,
            level_wake,
            key_level: 100,
            media_hold: None,
            mpris: Mpris::default(),
            toast: None,
            locked: false,
            monitors_on: true,
            layer_fade: None,
            blended: Vec::new(),
            touches_armed: true,
            active_button: None,
            layout,
            canvas,
            display,
            backlight,
            keyboard: VirtualKeyboard::open(&custom_codes)?,
            haptic,
            physical_escape,
            custom_keys,
            custom_labels,
            plus,
            focus_app: String::new(),
            items: Vec::new(),
            layout_ctx: LayoutCtx::default(),
            widgets: WidgetData {
                next_event: Some(i18n::t(i18n::Msg::NoEvents).into()),
                pomodoro: Some(("timer", format!("{:02}:00", work_min))),
                ..WidgetData::default()
            },
            events: Vec::new(),
            bus: None,
            panel: None,
            panel_zone: (0, 0),
            recent_notices: std::collections::VecDeque::new(),
            pending_card: None,
            pending_card_ms: None,
            transient: None,
            switcher: None,
            switch_mods: 0,
            shift_held: 0,
            switch_steps: 0,
            switch_wanted: false,
                        dialog: None,
            dialog_poll: Arc::new(AtomicBool::new(false)),
            suggest: None,
            pomodoro: pomodoro::Pomodoro::new(work_min, break_min),
            pomo_next_ms: None,
            windows_missing_until_ms: None,
            slider_hold: None,
            slider_shown: false,
            player: None,
            position_us: 0,
            position_at_ms: 0,
            position_read_ms: 0,
            art: None,
            scrub: None,
            np_next_ms: None,
            viz: [4; 5],
            viz_seed: 0x2545_f491,
            toggles_next_ms: None,
            sampler: widgets::Sampler::default(),
            widgets_next_ms: None,
            clock_next_ms: Some(0),
            animation_start_ms: 0,
            next_animation_ms: None,
        })
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn save(&self) {
        if let Err(error) = self.state.save(&self.state_path) {
            eprintln!("kait2en-touchbar-plus: could not save state: {error:#}");
        }
    }

    /// The media row for the focused app.
    fn media_layout(&mut self) -> Layout {
        let zone: Vec<ItemKind> = if self.panel.is_some() {
            vec![ItemKind::Controls]
        } else if let Some(transient) = &self.transient {
            transient.items.clone()
        } else if let Some(suggestions) = &self.suggest {
            suggestions.clone()
        } else {
            self.plus.zone_for(&self.focus_app).0.to_vec()
        };
        // The panel keeps the Controls button where it sits in the normal row.
        let slots: Vec<Slot> = if self.panel.is_some() {
            let (items, slots) = self.plus.zone_for(&self.focus_app);
            items
                .iter()
                .position(|item| *item == ItemKind::Controls)
                .and_then(|at| slots.get(at).copied())
                .into_iter()
                .collect()
        } else if self.transient.is_some() || self.suggest.is_some() {
            Vec::new()
        } else {
            self.plus.zone_for(&self.focus_app).1.to_vec()
        };
        self.items = zone.iter().chain(&self.plus.strip).cloned().collect();
        let layout = Layout::from_placed(
            &zone,
            &slots,
            &self.plus.strip,
            &self.plus.strip_slots,
            self.canvas.width,
            self.physical_escape,
            &self.layout_ctx,
        );
        self.widgets.panel = self.panel.as_ref().map(|panel| {
            let button = layout
                .buttons
                .iter()
                .find(|button| matches!(button.action, Action::Item(_)));
            let left = button.map_or(layout.zone.0, |button| button.right + 40);
            let width = self.plus.controls.width_px;
            let right = if width == 0 {
                layout.zone.1
            } else {
                left.saturating_add(width).min(layout.zone.1)
            };
            self.panel_zone = (left, right.max(left.saturating_add(120)).min(layout.zone.1.max(left)));
            widgets::PanelView {
                zone: self.panel_zone,
                rows: self
                    .plus
                    .controls
                    .items
                    .iter()
                    .map(|control| widgets::PanelRow {
                        icon: control.icon(),
                        percent: panel.levels[control.index()],
                    })
                    .collect(),
                active: panel.active,
            }
        });
        layout
    }

    /// Slides the controls panel in (`1`) or out (`-1`), keeping the Controls
    /// button in place. Only when nothing else is animating or in the way.
    fn start_panel_fade(&mut self, direction: i32, now: u64) {
        if !self.visible || self.state.mode != Mode::Media || self.touch_id != TouchIdState::Idle {
            return;
        }
        let from = match self.layer_fade {
            Some(_) => self.blended.clone(),
            None => self.canvas.pixels.clone(),
        };
        let clip_left = self
            .layout
            .buttons
            .iter()
            .find(|button| {
                matches!(button.action, Action::Item(index)
                    if matches!(self.items.get(index), Some(ItemKind::Controls)))
            })
            .map_or(0, |button| usize::from(button.right) + 1);
        self.layer_fade = Some(LayerFade {
            from,
            start_ms: now,
            next_ms: now,
            direction,
            clip_left,
            confirm: false,
        });
    }

    fn toggle_panel(&mut self, now: u64) {
        if self.panel.take().is_some() {
            self.start_panel_fade(-1, now);
            self.restore_dialog_row(now);
            return;
        }
        self.start_panel_fade(1, now);
        self.transient = None;
        self.suggest = None;
        let mut levels = [None; 3];
        for control in &self.plus.controls.items {
            levels[control.index()] = levels::read_control(*control);
        }
        self.panel = Some(Panel {
            levels,
            active: None,
            until_ms: now + self.plus.controls.timeout_ms,
            last_set_ms: 0,
        });
    }

    /// Slider and level under a touch, while the panel is open.
    fn panel_touch(&self, x: f64, y: f64) -> Option<(usize, u32)> {
        self.panel.as_ref()?;
        if y < 5.0 || y > f64::from(self.canvas.height.saturating_sub(5)) {
            return None;
        }
        renderer::panel_hit(self.panel_zone, self.plus.controls.items.len(), x)
    }

    fn panel_set(&mut self, slider: usize, level: u32, now: u64, force: bool) -> Result<()> {
        let Some(control) = self.plus.controls.items.get(slider).copied() else {
            return Ok(());
        };
        let Some(panel) = self.panel.as_mut() else {
            return Ok(());
        };
        panel.active = Some(slider);
        panel.until_ms = now + self.plus.controls.timeout_ms;
        let changed = panel.levels[control.index()] != Some(level);
        panel.levels[control.index()] = Some(level);
        if changed && (force || now.saturating_sub(panel.last_set_ms) >= 40) {
            panel.last_set_ms = now;
            levels::set_control(control, level);
            self.haptic.click();
        }
        self.refresh_widgets()
    }

    fn panel_tick(&mut self, now: u64) -> Result<()> {
        if self
            .panel
            .as_ref()
            .is_some_and(|panel| panel.active.is_none() && now >= panel.until_ms)
        {
            self.panel = None;
            self.start_panel_fade(-1, now);
            if self.layer_fade.is_some() {
                return self.present_keys();
            }
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn present_keys(&mut self) -> Result<()> {
        self.layout = if self.state.mode == Mode::Media {
            self.media_layout()
        } else {
            Layout::with_custom(
                self.state.mode,
                self.canvas.width,
                self.physical_escape,
                &self.custom_labels,
            )
        };
        if self.state.mode == Mode::Media {
            self.widgets.clock = widgets::format_clock(&self.plus.clock_format);
            self.canvas
                .keys_with(&self.layout, self.active_button, Some(&self.widgets));
        } else {
            self.canvas.keys(&self.layout, self.active_button);
        }
        if self.layer_fade.is_some() {
            return self.present_layer_fade(self.now_ms());
        }
        self.display
            .present(&self.canvas.pixels, self.canvas.width, self.canvas.height)
    }

    /// Shows the crossfade between the previous row and the freshly drawn
    /// current one for `now`, and ends it once the current row is complete.
    fn present_layer_fade(&mut self, now: u64) -> Result<()> {
        let Some(fade) = self.layer_fade.as_mut() else {
            return Ok(());
        };
        let elapsed = now.saturating_sub(fade.start_ms);
        if elapsed >= LAYER_FADE_MS {
            if fade.confirm {
                self.haptic.firm();
            }
            self.layer_fade = None;
            return self.display.present(
                &self.canvas.pixels,
                self.canvas.width,
                self.canvas.height,
            );
        }
        fade.next_ms = now + LAYER_FRAME_MS;
        let progress = elapsed as f32 / LAYER_FADE_MS as f32;
        self.blended.resize(self.canvas.pixels.len(), 0);
        compose_slide(
            &fade.from,
            &self.canvas.pixels,
            &mut self.blended,
            usize::from(self.canvas.width),
            fade.clip_left,
            fade.direction,
            progress,
        );
        self.display
            .present(&self.blended, self.canvas.width, self.canvas.height)
    }

    /// Sleeping deadline for the bar; `always_on` keeps it lit until locked.
    fn arm_deadline(&mut self, now: u64) {
        self.deadline_ms = if self.screen_off() {
            Some(now + SCREEN_OFF_AWAKE_MS)
        } else {
            (!self.config.always_on).then(|| now + self.state.learned(self.state.mode).timeout_ms)
        };
    }

    /// The screen is dark: locked or with the monitors off.
    fn screen_off(&self) -> bool {
        self.locked || !self.monitors_on
    }

    fn screen_locked(&mut self, locked: bool) -> Result<()> {
        let was_off = self.screen_off();
        self.locked = locked;
        self.screen_changed(was_off)
    }

    fn screen_power(&mut self, on: bool) -> Result<()> {
        let was_off = self.screen_off();
        self.monitors_on = on;
        self.screen_changed(was_off)
    }

    fn screen_changed(&mut self, was_off: bool) -> Result<()> {
        let off = self.screen_off();
        if off != was_off {
            eprintln!("kait2en-touchbar-plus: screen {}", if off { "off" } else { "on" });
        }
        match screen_transition(was_off, off, self.config.screen_lock_dark, self.config.always_on) {
            Some(false) => self.go_dark(false),
            Some(true) => self.wake(false),
            None => Ok(()),
        }
    }

    fn wake(&mut self, quarantine_touch: bool) -> Result<()> {
        if self.touch_id != TouchIdState::Idle {
            return Ok(());
        }
        let now = self.now_ms();
        self.refresh_key_level();
        if !self.visible {
            self.learner.wake(now, &self.config);
        }
        self.visible = true;
        self.dialog_poll.store(true, Ordering::Relaxed);
        self.overlay_shown = false;
        self.activity_esc_shown = false;
        self.escape_fade = None;
        self.escape_opacity = 0.0;
        self.overlay_hide_ms = None;
        self.level_kind = None;
        self.level_poll_ms = None;
        self.toast = None;
        self.layer_fade = None;
        if quarantine_touch {
            self.touches_armed = false;
        }
        self.arm_deadline(now);
        self.present_keys()?;
        self.dimmed = false;
        self.widgets_next_ms = Some(now);
        self.np_next_ms = Some(now);
        self.toggles_next_ms = Some(now + 10_000);
        self.backlight.set(self.config.active_brightness)?;
        self.touch_dim(now);
        Ok(())
    }

    fn go_dark(&mut self, learned_off: bool) -> Result<()> {
        self.go_dark_inner(learned_off, false)
    }

    fn go_dark_to_activity_esc(&mut self, learned_off: bool) -> Result<()> {
        self.go_dark_inner(learned_off, true)
    }

    fn go_dark_inner(&mut self, learned_off: bool, preserve_activity_esc: bool) -> Result<()> {
        let now = self.now_ms();
        let escape = (preserve_activity_esc
            && self.config.activity_backlight
            && self.touch_id == TouchIdState::Idle
            && self
                .activity_deadline_ms
                .is_some_and(|deadline| now < deadline))
        .then(|| {
            self.layout
                .buttons
                .iter()
                .position(|button| button.action == Action::Key(Key::Esc))
        })
        .flatten();
        self.keyboard.release()?;
        self.active_button = None;
        self.clear_touches();
        self.touches_armed = true;
        self.deadline_ms = None;
        self.dim_deadline_ms = None;
        self.dimmed = false;
        if self.visible && learned_off {
            self.learner
                .auto_off(self.now_ms(), self.state.mode, &self.config);
        }
        self.visible = false;
        self.dialog_poll.store(false, Ordering::Relaxed);
        self.overlay_shown = escape.is_some();
        self.activity_esc_shown = escape.is_some();
        self.escape_fade = None;
        self.escape_opacity = if escape.is_some() { 1.0 } else { 0.0 };
        self.overlay_hide_ms = None;
        self.level_kind = None;
        self.level_poll_ms = None;
        self.toast = None;
        self.layer_fade = None;
        if let Some(index) = escape {
            self.canvas.single_key(&self.layout, index, false, 100);
        } else {
            self.canvas.clear();
        }
        self.display
            .present(&self.canvas.pixels, self.canvas.width, self.canvas.height)?;
        if escape.is_none() {
            self.backlight.set(0)?;
        }
        Ok(())
    }

    fn clear_touches(&mut self) {
        self.contacts.clear();
        self.positions.clear();
        self.pending = None;
        self.swipe = None;
        self.media_hold = None;
    }

    fn touch_down(&mut self, slot: u32, x: f64, y: f64) -> Result<()> {
        let now = self.now_ms();
        self.touch_dim(now);
        if self.visible && self.toast.as_ref().is_some_and(|toast| toast.notice) {
            // A message over the keys: the touch only dismisses it.
            self.toast = None;
            self.contacts.insert(slot, Contact::Cancelled);
            return self.present_keys();
        }
        if let Some((slider, level)) = self.panel_touch(x, y) {
            self.contacts.insert(slot, Contact::Panel(slider));
            self.touches_armed = true;
            return self.panel_set(slider, level, now, true);
        }
        self.user_activity(now, false)?;
        self.resolve_pending(now)?;
        self.positions.insert(slot, x);
        if self.touch_id == TouchIdState::Idle
            && self
                .swipe
                .is_some_and(|swipe| swipe.kind != SwipeKind::Mode)
        {
            return self.join_dark_swipe(slot);
        }
        if self.touch_id != TouchIdState::Idle {
            let esc_index = self
                .layout
                .buttons
                .iter()
                .position(|button| button.action == Action::Key(Key::Esc));
            if let Some(index) = esc_index.filter(|&index| {
                self.layout.hit(x, y, self.canvas.height) == Some(index)
            }) {
                self.active_button = Some(index);
                self.contacts.insert(slot, Contact::TouchIdEsc(index));
                self.render_touch_id(now)?;
            } else {
                self.contacts.insert(slot, Contact::Cancelled);
            }
            return Ok(());
        }
        if self.swipe.is_some() {
            self.contacts.insert(slot, Contact::Cancelled);
            return Ok(());
        }
        if let Some(pending) = self.pending.take() {
            return self.start_swipe(pending, slot);
        }
        if !self.contacts.is_empty()
            || (self.visible && (!self.touches_armed || self.active_button.is_some()))
        {
            self.contacts.insert(slot, Contact::Cancelled);
            return Ok(());
        }
        if !self.visible
            && let Some(toast) = self.toast.as_mut()
        {
            let inside = |(left, right): (u16, u16)| x >= f64::from(left) && x < f64::from(right);
            let forward = if inside(toast.layout.next) {
                Some(true)
            } else if inside(toast.layout.prev) {
                Some(false)
            } else {
                None
            };
            if let Some(forward) = forward {
                toast.pressed = Some(forward);
                self.contacts.insert(slot, Contact::TrackKey(forward));
                return self.track_frame(now);
            }
            // Anything else on the bar takes over from the overlay.
            self.toast = None;
        }
        let button = self.layout.hit(x, y, self.canvas.height);
        // The esc slot never moves, so it stays usable while the bar is dark.
        let button = if self.visible {
            button
        } else {
            button.filter(|&index| self.is_esc(index))
        };
        self.pending = Some(PendingTouch {
            slot,
            deadline_ms: now + self.config.two_finger_window_ms,
            dark: !self.visible,
            button,
        });
        self.contacts.insert(slot, Contact::Pending);
        if let Some(index) = button {
            if self.visible {
                // Highlight at once. Only the key event waits for the window.
                self.active_button = button;
                self.present_keys()?;
            } else {
                self.show_dark_esc(index)?;
            }
        }
        Ok(())
    }

    /// The hold on a volume or brightness key became a slider: the finger now
    /// steps the level and the keys give way to the level overlay.
    fn slider_due(&mut self, now: u64) -> Result<()> {
        let Some(hold) = self.slider_hold.take_if(|hold| now >= hold.due_ms) else {
            return Ok(());
        };
        let x = self.positions.get(&hold.slot).copied().unwrap_or_default();
        let (kind, step) = match hold.kind {
            LevelKind::Volume => (SwipeKind::Volume, self.config.volume_swipe_step_px),
            LevelKind::Brightness => (SwipeKind::Brightness, self.config.brightness_swipe_step_px),
        };
        self.contacts.insert(hold.slot, Contact::Swipe);
        self.swipe = Some(Swipe::new(kind, x, f64::from(step)));
        self.active_button = None;
        self.slider_shown = true;
        self.begin_level(hold.kind)?;
        self.haptic.click();
        Ok(())
    }

    fn start_swipe(&mut self, pending: PendingTouch, slot: u32) -> Result<()> {
        let (kind, step) = if pending.dark {
            (SwipeKind::Volume, self.config.volume_swipe_step_px)
        } else {
            (SwipeKind::Mode, self.config.mode_swipe_px)
        };
        self.contacts.insert(pending.slot, Contact::Swipe);
        self.contacts.insert(slot, Contact::Swipe);
        self.swipe = Some(Swipe::new(kind, self.swipe_centroid(), f64::from(step)));
        if pending.dark {
            return self.begin_level(LevelKind::Volume);
        }
        if pending.button.is_some() {
            self.active_button = None;
            self.present_keys()?;
        }
        Ok(())
    }

    /// Three fingers on the dark bar always mean brightness.
    fn join_dark_swipe(&mut self, slot: u32) -> Result<()> {
        if self
            .swipe
            .is_some_and(|swipe| swipe.kind == SwipeKind::Brightness)
        {
            self.contacts.insert(slot, Contact::Cancelled);
            return Ok(());
        }
        self.contacts.insert(slot, Contact::Swipe);
        self.swipe = Some(Swipe::new(
            SwipeKind::Brightness,
            self.swipe_centroid(),
            f64::from(self.config.brightness_swipe_step_px),
        ));
        self.begin_level(LevelKind::Brightness)
    }

    fn swipe_span(&self) -> (f64, f64) {
        let xs = self
            .contacts
            .iter()
            .filter(|(_, contact)| matches!(contact, Contact::Swipe))
            .filter_map(|(slot, _)| self.positions.get(slot).copied());
        xs.fold((f64::MAX, f64::MIN), |(first, last), x| {
            (first.min(x), last.max(x))
        })
    }

    fn swipe_centroid(&self) -> f64 {
        let xs: Vec<f64> = self
            .contacts
            .iter()
            .filter(|(_, contact)| matches!(contact, Contact::Swipe))
            .filter_map(|(slot, _)| self.positions.get(slot).copied())
            .collect();
        xs.iter().sum::<f64>() / xs.len().max(1) as f64
    }

    /// Turns a first touch whose window has expired into its single-finger
    /// meaning: a wake on the dark bar, a held key on the lit bar.
    fn resolve_pending(&mut self, now: u64) -> Result<()> {
        let Some(pending) = self.pending.filter(|pending| now >= pending.deadline_ms) else {
            return Ok(());
        };
        self.pending = None;
        if pending.dark && pending.button.is_some() {
            self.contacts.insert(pending.slot, Contact::DarkEsc);
            self.keyboard.press(Key::Esc)?;
            self.haptic.key_press();
            return Ok(());
        }
        if pending.dark {
            self.contacts.insert(pending.slot, Contact::Wake);
            return self.wake(true);
        }
        match pending.button {
            Some(index) if self.media_direction(index).is_some() => {
                // Stays highlighted. The action is decided on release or hold.
                let touched = pending.deadline_ms - self.config.two_finger_window_ms;
                self.contacts
                    .insert(pending.slot, Contact::MediaHold(index));
                self.media_hold = Some(MediaHold {
                    forward: self.media_direction(index) == Some(true),
                    next_ms: touched + MEDIA_HOLD_MS,
                    seeking_since: None,
                    player: None,
                });
                self.arm_deadline(now);
                Ok(())
            }
            Some(index) if slider_kind(self.layout.buttons[index].action).is_some() => {
                let kind = slider_kind(self.layout.buttons[index].action)
                    .expect("checked by the guard");
                let touched = pending.deadline_ms - self.config.two_finger_window_ms;
                self.contacts.insert(pending.slot, Contact::SliderHold(index));
                self.slider_hold = Some(SliderHold {
                    slot: pending.slot,
                    kind,
                    due_ms: touched + self.config.hold_slider_ms,
                });
                Ok(())
            }
            Some(index)
                if matches!(
                    self.layout.buttons[index].action,
                    Action::Item(item) if self.items.get(item) == Some(&ItemKind::Pomodoro)
                ) =>
            {
                let touched = pending.deadline_ms - self.config.two_finger_window_ms;
                self.contacts
                    .insert(pending.slot, Contact::PomoHold(index, touched));
                Ok(())
            }
            Some(index) if self.layout.now_playing_index() == Some(index) => {
                let x = self.positions.get(&pending.slot).copied().unwrap_or_default();
                self.contacts.insert(pending.slot, Contact::Scrub(index));
                self.scrub = Some(Scrub { start_x: x, x });
                self.haptic.key_press();
                Ok(())
            }
            Some(index) => {
                self.contacts.insert(pending.slot, Contact::Button(index));
                self.press_button(index, now)
            }
            None => {
                self.contacts.insert(pending.slot, Contact::Cancelled);
                Ok(())
            }
        }
    }

    fn track_changed(&mut self, track: Track) -> Result<()> {
        let now = self.now_ms();
        // A notification card keeps its own timeline; the track is not shown over it.
        if self.toast.as_ref().is_some_and(|toast| toast.card.is_some()) {
            return Ok(());
        }
        if let Some(toast) = self.toast.as_mut() {
            toast.track = track;
            toast.start_ms = now.saturating_sub(TRACK_FADE_MS);
            return self.track_frame(now);
        }
        // Only on a quiet dark bar: never over keys, gestures or Touch ID.
        // The esc key from activity stays and is drawn alongside.
        if self.visible
            || self.screen_off()
            || (self.overlay_shown && !self.activity_esc_shown)
            || self.touch_id != TouchIdState::Idle
            || !self.contacts.is_empty()
        {
            return Ok(());
        }
        self.refresh_key_level();
        self.toast = Some(TrackToast {
            track,
            start_ms: now,
            next_ms: None,
            layout: self.track_keys(),
            pressed: None,
            notice: false,
            card: None,
            replaced: None,
        });
        self.track_frame(now)
    }

    /// Draws the track overlay for `now` and schedules the next frame: about
    /// 30 fps while fading, nothing while it holds or a key is pressed.
    fn track_frame(&mut self, now: u64) -> Result<()> {
        if self.toast.as_ref().is_some_and(|toast| toast.card.is_some()) {
            return self.notice_frame(now);
        }
        let Some(toast) = self.toast.as_mut() else {
            return Ok(());
        };
        let elapsed = now.saturating_sub(toast.start_ms);
        let fade_out = TRACK_FADE_MS + TRACK_HOLD_MS;
        let (opacity, next) = if toast.pressed.is_some() {
            (1.0, None)
        } else if elapsed < TRACK_FADE_MS {
            (
                elapsed as f32 / TRACK_FADE_MS as f32,
                Some(now + TRACK_FRAME_MS),
            )
        } else if elapsed < fade_out {
            (1.0, Some(toast.start_ms + fade_out))
        } else if elapsed < fade_out + TRACK_FADE_MS {
            (
                1.0 - (elapsed - fade_out) as f32 / TRACK_FADE_MS as f32,
                Some(now + TRACK_FRAME_MS),
            )
        } else {
            return self.toast_done();
        };
        // Smoothstep, so the fades start and end softly.
        let eased = opacity * opacity * (3.0 - 2.0 * opacity);
        toast.next_ms = next;
        let (track, pressed, keys) = (toast.track.clone(), toast.pressed, toast.layout);
        let esc = self
            .activity_esc_shown
            .then(|| {
                self.layout
                    .buttons
                    .iter()
                    .find(|button| button.action == Action::Key(Key::Esc))
                    .copied()
            })
            .flatten()
            .map(|button| (button, (self.escape_opacity * 100.0).round() as u32));
        self.canvas.track(
            &track.artist,
            &track.title,
            (eased * 100.0).round() as u32,
            pressed,
            keys,
            esc,
        );
        self.present_overlay()
    }

    /// The overlay is over: back to the keys, the activity key or the dark bar.
    fn toast_done(&mut self) -> Result<()> {
        self.toast = None;
        if self.visible {
            return self.present_keys();
        }
        if self.activity_esc_shown {
            return self.render_activity_esc();
        }
        self.hide_overlay()
    }

    /// One frame of the notification card: 30 fps while it moves, a slower
    /// pace while it holds and its bar runs down.
    fn notice_frame(&mut self, now: u64) -> Result<()> {
        let Some(toast) = self.toast.as_mut() else {
            return Ok(());
        };
        let elapsed = now.saturating_sub(toast.start_ms);
        if elapsed >= NOTICE_ENTER_MS + NOTICE_HOLD_MS + NOTICE_EXIT_MS {
            return self.toast_done();
        }
        let enter = (elapsed as f32 / NOTICE_ENTER_MS as f32).min(1.0);
        let leave = (elapsed.saturating_sub(NOTICE_ENTER_MS + NOTICE_HOLD_MS) as f32 / NOTICE_EXIT_MS as f32).min(1.0);
        let remain = 1.0 - (elapsed.saturating_sub(NOTICE_ENTER_MS) as f32 / NOTICE_HOLD_MS as f32).min(1.0);
        let swap = toast
            .replaced
            .as_ref()
            .map(|(_, at)| now.saturating_sub(*at) as f32 / NOTICE_SWAP_MS as f32)
            .filter(|t| *t < 1.0);
        if swap.is_none() {
            toast.replaced = None;
        }
        let moving = enter < 1.0 || leave > 0.0 || swap.is_some();
        toast.next_ms = Some(now + if moving { TRACK_FRAME_MS } else { NOTICE_HOLD_FRAME_MS });
        let card = toast.card.clone().expect("a card toast has its card");
        let old = toast.replaced.as_ref().map(|(old, _)| old.clone());
        let motion = renderer::NoticeMotion { enter, leave, remain, swap };
        self.canvas.notice_card(&card, old.as_ref(), motion);
        self.present_overlay()
    }

    /// The overlay's previous key takes the first key slot after esc. Next
    /// mirrors it on the right, as if an invisible esc sat there too. The
    /// keys stay in fixed places, esc keeps working and the text centers on
    /// the bar.
    fn track_keys(&self) -> TrackLayout {
        // Taken from the media row, whose keys fill the whole bar.
        let media = Layout::new(Mode::Media, self.canvas.width, self.physical_escape);
        let first = &media.buttons[usize::from(!self.physical_escape)];
        let width = self.canvas.width;
        TrackLayout {
            prev: (first.left, first.right),
            next: (width - first.right, width - first.left),
        }
    }

    fn show_dark_esc(&mut self, index: usize) -> Result<()> {
        self.refresh_key_level();
        self.level_kind = None;
        self.activity_esc_shown = self.config.activity_backlight;
        self.escape_fade = None;
        self.escape_opacity = 1.0;
        self.level_fade_start_ms = None;
        self.level_frame_ms = None;
        self.toast = None;
        self.overlay_hide_ms = None;
        self.canvas.single_key(&self.layout, index, true, 100);
        self.present_overlay()
    }

    fn begin_level(&mut self, kind: LevelKind) -> Result<()> {
        self.refresh_key_level();
        self.toast = None;
        self.level_kind = Some(kind);
        self.activity_esc_shown = false;
        self.escape_fade = None;
        self.escape_opacity = 0.0;
        self.level_fade_start_ms = None;
        self.level_frame_ms = None;
        self.level = self.level_cache[cache_slot(kind)];
        self.level_stepped = false;
        self.level_stop_felt = false;
        self.level_reader.request(kind);
        self.level_fingers = self.swipe_span();
        self.update_level_side(self.now_ms(), true);
        self.overlay_hide_ms = None;
        self.level_dirty = true;
        Ok(())
    }

    /// Places the feedback on the side of the fingers with more room. A
    /// change crossfades unless `snap` is set.
    fn update_level_side(&mut self, now: u64, snap: bool) {
        let centre = (self.level_fingers.0 + self.level_fingers.1) / 2.0;
        let middle = f64::from(self.canvas.width) / 2.0;
        // Progress so far counts towards the old target.
        self.advance_level_side(now);
        let margin = if snap { 0.0 } else { LEVEL_SIDE_HYSTERESIS_PX };
        if centre < middle - margin {
            self.level_right_target = true;
        } else if centre >= middle + margin {
            self.level_right_target = false;
        }
        if snap {
            self.level_right = f32::from(u8::from(self.level_right_target));
        }
        if self.level_right != f32::from(u8::from(self.level_right_target)) {
            self.level_frame_ms.get_or_insert(now + LEVEL_FRAME_MS);
        }
    }

    /// Moves the side crossfade on to `now`. Returns whether it still runs.
    fn advance_level_side(&mut self, now: u64) -> bool {
        let target = f32::from(u8::from(self.level_right_target));
        let step = now.saturating_sub(self.level_right_ms) as f32 / LEVEL_SIDE_FADE_MS as f32;
        self.level_right_ms = now;
        self.level_right = if self.level_right < target {
            (self.level_right + step).min(target)
        } else {
            (self.level_right - step).max(target)
        };
        self.level_right != target
    }

    /// Draws the level feedback if anything changed since the last frame.
    fn flush_level(&mut self) -> Result<()> {
        if !std::mem::take(&mut self.level_dirty) {
            return Ok(());
        }
        let Some(kind) = self.level_kind.filter(|_| !self.visible || self.slider_shown) else {
            return Ok(());
        };
        let now = self.now_ms();
        self.advance_level_side(now);
        let right = self.level_right * self.level_right * (3.0 - 2.0 * self.level_right);
        let opacity = self.level_fade_start_ms.map_or(100, |start| {
            let elapsed = self.now_ms().saturating_sub(start).min(LEVEL_FADE_MS);
            let opacity = 1.0 - elapsed as f32 / LEVEL_FADE_MS as f32;
            (opacity * opacity * (3.0 - 2.0 * opacity) * 100.0).round() as u32
        });
        self.canvas.level(
            kind,
            self.level.percent,
            self.level.muted,
            (self.level_fingers.0 as f32, self.level_fingers.1 as f32),
            right,
            opacity,
        );
        self.present_overlay()
    }

    /// Shows whatever the canvas holds on the otherwise dark bar.
    fn present_overlay(&mut self) -> Result<()> {
        self.display
            .present(&self.canvas.pixels, self.canvas.width, self.canvas.height)?;
        if self.visible {
            return Ok(());
        }
        if !self.overlay_shown {
            self.backlight.set(self.config.active_brightness)?;
            self.overlay_shown = true;
        }
        Ok(())
    }

    fn hide_overlay(&mut self) -> Result<()> {
        self.overlay_hide_ms = None;
        self.level_kind = None;
        self.toast = None;
        self.level_poll_ms = None;
        self.level_fade_start_ms = None;
        self.level_frame_ms = None;
        self.activity_esc_shown = false;
        self.escape_fade = None;
        self.escape_opacity = 0.0;
        if std::mem::take(&mut self.slider_shown) {
            return self.present_keys();
        }
        if !self.overlay_shown || self.visible {
            return Ok(());
        }
        self.overlay_shown = false;
        self.canvas.clear();
        self.display
            .present(&self.canvas.pixels, self.canvas.width, self.canvas.height)?;
        self.backlight.set(0)
    }

    fn poll_level(&mut self, now: u64) {
        self.level_poll_ms = (now < self.level_poll_until_ms).then_some(now + LEVEL_POLL_MS);
        if let Some(kind) = self.level_kind {
            self.level_reader.request(kind);
        }
    }

    fn levels_read(&mut self) {
        let now = self.now_ms();
        let results: Vec<_> = self.level_reader.results().collect();
        for (kind, level) in results {
            self.level_cache[cache_slot(kind)] = level;
            if self.level_kind != Some(kind) || level == self.level {
                continue;
            }
            self.level = level;
            self.level_dirty = true;
            // Reaching a stop is only known once the desktop has applied
            // the key, so the firm click follows the read.
            if !at_stop(kind, level, None) {
                self.level_stop_felt = false;
            } else if self.level_stepped && !self.level_stop_felt {
                self.level_stop_felt = true;
                self.haptic.firm();
            }
            // Keep a lingering overlay alive until the value has settled.
            self.overlay_hide_ms = self
                .overlay_hide_ms
                .map(|deadline| deadline.max(now + LEVEL_POLL_MS));
        }
    }

    /// Follows the keyboard backlight. Returns whether the level changed.
    fn refresh_key_level(&mut self) -> bool {
        let level = if self.config.follow_keyboard_backlight {
            kbdlight::key_level().unwrap_or(100)
        } else {
            100
        };
        if level == self.key_level {
            return false;
        }
        self.key_level = level;
        self.canvas.set_level(level);
        true
    }

    fn keyboard_backlight_changed(&mut self) -> Result<()> {
        if let Ok(brightness) = kbdlight::brightness() {
            self.activity_current_brightness = Some(brightness);
            if self.activity_dimmed && self.backlight_fade.is_none() && brightness > 0 {
                self.activity_restore_brightness = Some(brightness);
                self.activity_dimmed = false;
                self.user_activity(self.now_ms(), true)?;
            }
        }
        let changed = self.refresh_key_level();
        if self.touch_id != TouchIdState::Idle {
            return Ok(());
        }
        if self.activity_esc_shown && changed {
            return self.render_activity_esc();
        }
        if !changed {
            return Ok(());
        }
        if self.visible {
            return self.present_keys();
        }
        self.level_dirty = true;
        Ok(())
    }

    /// Restores full brightness and rearms the idle dimming.
    fn touch_dim(&mut self, now: u64) {
        if self.dimmed {
            self.dimmed = false;
            if let Err(error) = self.backlight.set(self.config.active_brightness) {
                eprintln!("kait2en-touchbar-plus: undim Touch Bar: {error:#}");
            }
        }
        self.dim_deadline_ms = (self.visible && self.config.dim_after_ms > 0)
            .then(|| now + self.config.dim_after_ms);
    }

    fn dim_if_due(&mut self, now: u64) {
        if self.dim_deadline_ms.is_some_and(|deadline| now >= deadline) {
            self.dim_deadline_ms = None;
            if self.visible && self.touch_id == TouchIdState::Idle {
                self.dimmed = true;
                eprintln!("kait2en-touchbar-plus: dim");
                if let Err(error) = self.backlight.set(self.config.dim_brightness) {
                    eprintln!("kait2en-touchbar-plus: dim Touch Bar: {error:#}");
                }
            }
        }
    }

    fn shift_if_due(&mut self, now: u64) -> Result<()> {
        if !self.shift_next_ms.is_some_and(|deadline| now >= deadline) {
            return Ok(());
        }
        self.shift_next_ms = Some(now + self.config.pixel_shift_interval_ms);
        self.shift_index = (self.shift_index + 1) % display::SHIFT_CYCLE.len();
        let (dx, dy) = display::SHIFT_CYCLE[self.shift_index];
        self.display.set_shift(dx, dy);
        eprintln!("kait2en-touchbar-plus: shift=({dx},{dy})");
        if self.visible && self.touch_id == TouchIdState::Idle && self.layer_fade.is_none() && self.toast.is_none() {
            self.present_keys()?;
        }
        Ok(())
    }

    fn user_activity(&mut self, now: u64, show_esc: bool) -> Result<()> {
        self.touch_dim(now);
        if !self.config.activity_backlight {
            return Ok(());
        }
        self.activity_deadline_ms = Some(now + self.config.activity_timeout_ms);
        let fading_out = self
            .backlight_fade
            .as_ref()
            .is_some_and(|fade| fade.to == 0);
        if self.activity_dimmed || fading_out {
            let brightness = match self.activity_restore_brightness {
                Some(brightness) if brightness > 0 => Some(brightness),
                _ => match self
                    .activity_max_brightness
                    .map(Ok)
                    .unwrap_or_else(kbdlight::maximum_brightness)
                {
                    Ok(brightness) if brightness > 0 => {
                        self.activity_max_brightness = Some(brightness);
                        Some(brightness)
                    }
                    Ok(_) => None,
                    Err(error) => {
                        eprintln!("kait2en-touchbar-plus: read maximum keyboard brightness: {error:#}");
                        None
                    }
                },
            };
            if let Some(brightness) = brightness {
                let already_fading_in = self
                    .backlight_fade
                    .as_ref()
                    .is_some_and(|fade| fade.to == brightness && brightness > 0);
                if !already_fading_in {
                    let current = self.activity_current_brightness.unwrap_or(0);
                    self.start_backlight_fade(now, current, brightness);
                }
            }
        }
        if self.visible {
            self.arm_deadline(now);
        } else if show_esc && self.touch_id == TouchIdState::Idle {
            self.show_activity_esc()?;
        }
        Ok(())
    }

    fn show_activity_esc(&mut self) -> Result<()> {
        if self.visible || self.touch_id != TouchIdState::Idle {
            return Ok(());
        }
        if self.activity_esc_shown {
            if self.escape_fade.as_ref().is_some_and(|fade| fade.to <= 0.0) {
                self.start_escape_fade(self.now_ms(), self.escape_opacity, 1.0);
            }
            return Ok(());
        }
        let Some(index) = self
            .layout
            .buttons
            .iter()
            .position(|button| button.action == Action::Key(Key::Esc))
        else {
            return Ok(());
        };
        // A track overlay stays and draws the esc key with it.
        self.level_kind = None;
        self.overlay_hide_ms = None;
        self.level_fade_start_ms = None;
        self.level_frame_ms = None;
        self.refresh_key_level();
        self.activity_esc_shown = true;
        self.start_escape_fade(self.now_ms(), self.escape_opacity, 1.0);
        self.render_activity_esc_with_index(index)
    }

    fn activity_timeout(&mut self) {
        self.activity_deadline_ms = None;
        if self.visible && self.touch_id == TouchIdState::Idle && !self.config.always_on {
            if let Err(error) = self.go_dark(false) {
                eprintln!("kait2en-touchbar-plus: darken Touch Bar after inactivity: {error:#}");
            }
        }
        match kbdlight::brightness() {
            Ok(brightness) => {
                self.activity_current_brightness = Some(brightness);
                if brightness > 0 {
                    self.activity_restore_brightness = Some(brightness);
                    self.start_backlight_fade(self.now_ms(), brightness, 0);
                } else {
                    self.backlight_fade = None;
                    self.activity_dimmed = true;
                }
            }
            Err(error) => {
                eprintln!("kait2en-touchbar-plus: read keyboard brightness before dimming: {error:#}")
            }
        }
        if self.activity_esc_shown {
            self.start_escape_fade(self.now_ms(), self.escape_opacity, 0.0);
        }
    }

    fn start_backlight_fade(&mut self, now: u64, from: u32, to: u32) {
        if from == to {
            self.activity_current_brightness = Some(to);
            self.activity_dimmed = to == 0;
            self.backlight_fade = None;
            return;
        }
        self.backlight_fade = Some(BacklightFade {
            start_ms: now,
            next_ms: now,
            from,
            to,
        });
    }

    fn start_escape_fade(&mut self, now: u64, from: f32, to: f32) {
        if (from - to).abs() < f32::EPSILON {
            self.escape_opacity = to;
            self.escape_fade = None;
            return;
        }
        self.escape_fade = Some(EscapeFade {
            start_ms: now,
            next_ms: now,
            from,
            to,
        });
    }

    fn render_activity_esc(&mut self) -> Result<()> {
        let Some(index) = self
            .layout
            .buttons
            .iter()
            .position(|button| button.action == Action::Key(Key::Esc))
        else {
            return Ok(());
        };
        self.render_activity_esc_with_index(index)
    }

    fn render_activity_esc_with_index(&mut self, index: usize) -> Result<()> {
        if self.toast.is_some() {
            return self.track_frame(self.now_ms());
        }
        self.canvas.single_key(
            &self.layout,
            index,
            false,
            (self.escape_opacity * 100.0).round() as u32,
        );
        self.present_overlay()
    }

    fn advance_backlight_fade(&mut self, now: u64) {
        let Some(fade) = self.backlight_fade.as_mut() else {
            return;
        };
        if now < fade.next_ms {
            return;
        }
        let elapsed = now.saturating_sub(fade.start_ms).min(ACTIVITY_FADE_MS);
        let progress = elapsed as f32 / ACTIVITY_FADE_MS as f32;
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let value = (fade.from as f32 + (fade.to as f32 - fade.from as f32) * eased).round() as u32;
        let target = fade.to;
        fade.next_ms = now + ACTIVITY_FRAME_MS;
        if self.activity_current_brightness != Some(value) {
            match kbdlight::set_brightness(value) {
                Ok(()) => self.activity_current_brightness = Some(value),
                Err(error) => {
                    eprintln!("kait2en-touchbar-plus: fade keyboard backlight: {error:#}");
                    self.backlight_fade = None;
                    return;
                }
            }
        }
        if elapsed >= ACTIVITY_FADE_MS {
            self.activity_current_brightness = Some(target);
            self.activity_dimmed = target == 0;
            self.backlight_fade = None;
        }
    }

    fn advance_escape_fade(&mut self, now: u64) -> Result<()> {
        let Some(fade) = self.escape_fade.as_mut() else {
            return Ok(());
        };
        if now < fade.next_ms {
            return Ok(());
        }
        let elapsed = now.saturating_sub(fade.start_ms).min(ACTIVITY_FADE_MS);
        let progress = elapsed as f32 / ACTIVITY_FADE_MS as f32;
        let eased = progress * progress * (3.0 - 2.0 * progress);
        self.escape_opacity = fade.from + (fade.to - fade.from) * eased;
        let target = fade.to;
        fade.next_ms = now + ACTIVITY_FRAME_MS;
        self.render_activity_esc()?;
        if elapsed >= ACTIVITY_FADE_MS {
            self.escape_opacity = target;
            self.escape_fade = None;
            if target <= 0.0 {
                if self.toast.is_some() {
                    // The track overlay goes on alone.
                    self.activity_esc_shown = false;
                    self.track_frame(now)?;
                } else {
                    self.hide_overlay()?;
                }
            }
        }
        Ok(())
    }

    fn restore_activity_backlight_on_exit(&mut self) {
        if self.config.activity_backlight
            && (self.activity_dimmed
                || self
                    .backlight_fade
                    .as_ref()
                    .is_some_and(|fade| fade.to == 0))
            && let Some(brightness) = self.activity_restore_brightness
        {
            if let Err(error) = kbdlight::set_brightness(brightness) {
                eprintln!("kait2en-touchbar-plus: restore keyboard brightness on exit: {error:#}");
            }
        }
    }

    /// `Some(true)` for next, `Some(false)` for previous.
    fn media_direction(&self, index: usize) -> Option<bool> {
        match self.layout.buttons[index].action {
            Action::Key(Key::NextSong) => Some(true),
            Action::Key(Key::PreviousSong) => Some(false),
            _ => None,
        }
    }

    fn seek_step(&mut self, now: u64) -> Result<()> {
        let Some(mut hold) = self.media_hold.take() else {
            return Ok(());
        };
        let since = match hold.seeking_since {
            Some(since) => since,
            None => {
                // The hold turned into seeking: no track skip on release.
                hold.player = self.mpris.active_player();
                self.haptic.click();
                if self
                    .learner
                    .action(now, self.state.mode, &mut self.state, &self.config)
                {
                    self.save();
                }
                *hold.seeking_since.insert(now)
            }
        };
        let step = if now - since >= SEEK_FAST_AFTER_MS {
            SEEK_FAST_STEP
        } else {
            SEEK_STEP
        };
        let seeked = hold
            .player
            .as_deref()
            .is_some_and(|player| self.mpris.seek(player, step, hold.forward).is_ok());
        if !seeked {
            // No MPRIS player: leave it to whoever handles the seek keys.
            let key = if hold.forward {
                Key::FastForward
            } else {
                Key::Rewind
            };
            self.keyboard.press(key)?;
            self.keyboard.release()?;
        }
        hold.next_ms = now + SEEK_INTERVAL_MS;
        self.arm_deadline(now);
        self.media_hold = Some(hold);
        Ok(())
    }

    fn is_esc(&self, index: usize) -> bool {
        self.layout.buttons[index].action == Action::Key(Key::Esc)
    }

    /// Redraws the media row when only widget data changed.
    fn refresh_widgets(&mut self) -> Result<()> {
        if self.visible
            && self.state.mode == Mode::Media
            && self.touch_id == TouchIdState::Idle
            && self.layer_fade.is_none()
            && self.toast.is_none()
            && self.level_kind.is_none()
            && !self.overlay_shown
        {
            self.present_keys()?;
        }
        Ok(())
    }

    fn has_monitors(&self) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item,
                ItemKind::Cpu
                    | ItemKind::Memory
                    | ItemKind::Temperature
                    | ItemKind::Fan
                    | ItemKind::Network
            )
        })
    }

    fn widgets_tick(&mut self, now: u64) -> Result<()> {
        if self.clock_next_ms.is_some_and(|due| now >= due) {
            self.clock_next_ms = Some(now + widgets::ms_to_next_minute() + 20);
            let clock = widgets::format_clock(&self.plus.clock_format);
            self.update_next_event()?;
            if clock != self.widgets.clock {
                self.widgets.clock = clock;
                self.refresh_widgets()?;
            }
        }
        if self.toggles_next_ms.is_some_and(|due| now >= due) {
            if self.visible && self.has_toggles() {
                self.toggles_next_ms = Some(now + 10_000);
                if let Some(bus) = &self.bus {
                    toggles::refresh(bus);
                }
            } else {
                self.toggles_next_ms = None;
            }
        }
        if self.widgets_next_ms.is_some_and(|due| now >= due) {
            if self.visible && self.has_monitors() {
                self.widgets_next_ms = Some(now + 2_000);
                if self.sampler.sample(&mut self.widgets, now) {
                    self.refresh_widgets()?;
                }
            } else {
                self.widgets_next_ms = None;
            }
        }
        Ok(())
    }

    fn has_now_playing(&self) -> bool {
        self.items.contains(&ItemKind::NowPlaying)
    }

    /// Current position, advanced by the time since it was read.
    fn current_position_us(&self, now: u64) -> i64 {
        let elapsed = if self.player.as_ref().is_some_and(|info| info.playing) {
            now.saturating_sub(self.position_at_ms) as i64 * 1000
        } else {
            0
        };
        self.position_us + elapsed
    }

    /// Shuffle and repeat of the active player, for the buttons beside the
    /// now playing widget. Returns whether anything changed.
    fn player_command(&mut self, command: renderer::PlayerCmd, now: u64) -> Result<()> {
        let Some(player) = self.player.as_ref().map(|info| info.player.clone()) else {
            return Ok(());
        };
        // The player's own signal brings the new value back; the call runs off
        // the main loop so a hung player cannot freeze the bar.
        let (shuffle, repeat) = (!self.layout_ctx.shuffle, (self.layout_ctx.repeat + 1) % 3);
        std::thread::spawn(move || {
            let mut mpris = mpris::Mpris::default();
            let result = match command {
                renderer::PlayerCmd::Shuffle => mpris.set_shuffle(&player, shuffle),
                renderer::PlayerCmd::Repeat => mpris.set_repeat(&player, repeat),
            };
            if let Err(error) = result {
                eprintln!("kait2en-touchbar-plus: player mode: {error:#}");
            }
        });
        self.np_next_ms = Some(now);
        self.refresh_widgets()
    }

    fn read_position(&mut self, now: u64) {
        let Some(player) = self.player.as_ref().map(|info| info.player.clone()) else {
            return;
        };
        if let Some(position) = self.mpris.position(&player) {
            self.position_us = position;
            self.position_at_ms = now;
        }
        self.position_read_ms = now;
    }

    /// Rebuilds what the now playing widget shows.
    fn update_now_playing(&mut self, now: u64) {
        let Some(info) = &self.player else {
            self.widgets.now_playing = None;
            return;
        };
        let track = &info.track;
        let line = if track.artist.is_empty() {
            track.title.clone()
        } else {
            format!("{} - {}", track.artist, track.title)
        };
        let fraction = match &self.scrub {
            Some(scrub) => self.scrub_fraction(scrub.x),
            None => mpris::progress_fraction(self.current_position_us(now), track.length_us),
        };
        let art = self
            .art
            .as_ref()
            .filter(|(url, _)| Some(url) == track.art_url.as_ref())
            .map(|(_, gray)| gray.clone());
        self.widgets.now_playing = Some(widgets::NowPlayingView {
            art,
            line,
            fraction,
            bars: (self.plus.visualizer && info.playing).then_some(self.viz),
        });
    }

    fn scrub_fraction(&self, x: f64) -> f32 {
        let Some(button) = self.layout.now_playing_index().map(|index| self.layout.buttons[index])
        else {
            return 0.0;
        };
        let has_art = self
            .widgets
            .now_playing
            .as_ref()
            .is_some_and(|view| view.art.is_some());
        let (left, right) = renderer::now_playing_bar(&button, has_art);
        (((x as f32) - left) / (right - left).max(1.0)).clamp(0.0, 1.0)
    }

    fn player_changed(&mut self, info: Option<mpris::PlayerInfo>) -> Result<()> {
        let now = self.now_ms();
        let had = self.player.is_some();
        let art_url = info.as_ref().and_then(|info| info.track.art_url.clone());
        let new_art = art_url
            .as_ref()
            .filter(|url| self.art.as_ref().map(|(old, _)| old) != Some(*url))
            .cloned();
        self.player = info;
        self.layout_ctx.player_active = self.player.is_some();
        self.position_us = 0;
        self.position_at_ms = now;
        (self.layout_ctx.shuffle, self.layout_ctx.repeat) = self
            .player
            .as_ref()
            .map_or((false, 0), |info| (info.shuffle, info.repeat));
        self.read_position(now);
        if let (Some(url), Some(bus)) = (new_art, &self.bus) {
            art::load(url, bus.clone());
        }
        self.np_next_ms = Some(now);
        self.update_now_playing(now);
        if had != self.player.is_some() || self.visible {
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn art_loaded(&mut self, url: String, gray: Vec<u8>) -> Result<()> {
        self.art = Some((url, gray));
        self.update_now_playing(self.now_ms());
        self.refresh_widgets()
    }

    fn now_playing_tick(&mut self, now: u64) -> Result<()> {
        if !self.np_next_ms.is_some_and(|due| now >= due) {
            return Ok(());
        }
        let playing = self.player.as_ref().is_some_and(|info| info.playing);
        if !(self.visible && self.has_now_playing() && self.player.is_some()) {
            self.np_next_ms = None;
            return Ok(());
        }
        let animated = playing && self.plus.visualizer;
        self.np_next_ms = (playing || self.scrub.is_some())
            .then(|| now + if animated { TRACK_FRAME_MS } else { 1_000 });
        if playing && now.saturating_sub(self.position_read_ms) >= 5_000 {
            self.read_position(now);
        }
        if animated {
            for bar in &mut self.viz {
                self.viz_seed ^= self.viz_seed << 13;
                self.viz_seed ^= self.viz_seed >> 17;
                self.viz_seed ^= self.viz_seed << 5;
                let target = 3 + (self.viz_seed % 14) as u8;
                *bar = ((u16::from(*bar) * 2 + u16::from(target)) / 3) as u8;
            }
        }
        let before = self.widgets.now_playing.clone();
        self.update_now_playing(now);
        if self.widgets.now_playing != before {
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn toggles_changed(&mut self, states: [bool; 5]) -> Result<()> {
        if self.layout_ctx.toggles != states {
            self.layout_ctx.toggles = states;
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn has_toggles(&self) -> bool {
        self.items.iter().any(|item| matches!(item, ItemKind::Toggle(_)))
    }

    fn events_changed(&mut self, events: Vec<calendar::Event>) -> Result<()> {
        self.events = events;
        self.update_next_event()
    }

    fn update_next_event(&mut self) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let next = calendar::next_label(&self.events, now, |t| {
            widgets::format_time(Some(t), "%H:%M")
        })
        .unwrap_or_else(|| i18n::t(i18n::Msg::NoEvents).into());
        if self.widgets.next_event.as_deref() != Some(&next) {
            self.widgets.next_event = Some(next);
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn weather_changed(&mut self, weather: widgets::Weather) -> Result<()> {
        if self.widgets.weather.as_ref() != Some(&weather) {
            self.widgets.weather = Some(weather);
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn battery_changed(&mut self, percent: u8, charging: bool) -> Result<()> {
        if self.widgets.battery != Some((percent, charging)) {
            self.widgets.battery = Some((percent, charging));
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn item_pressed(&mut self, item: usize) -> Result<()> {
        let Some(kind) = self.items.get(item).cloned() else {
            return Ok(());
        };
        let now = self.now_ms();
        match kind {
            ItemKind::Key(spec) => match &spec.action {
                CustomAction::Send(keys) => self.keyboard.tap_combination(keys)?,
                CustomAction::Run(command) => CustomKey::run(command),
            },
            ItemKind::Toggle(toggle) => {
                if let Some(bus) = &self.bus {
                    toggles::set(toggle, !self.layout_ctx.toggles[toggle.index()], bus);
                }
            }
            ItemKind::NowPlaying => {
                if let Some(player) = self.player.as_ref().map(|info| info.player.clone()) {
                    if let Err(error) = self.mpris.play_pause(&player) {
                        eprintln!("kait2en-touchbar-plus: play/pause: {error:#}");
                    }
                }
            }
            ItemKind::Clock => CustomKey::run("gnome-clocks"),
            ItemKind::Weather => CustomKey::run("gnome-weather"),
            ItemKind::NextEvent => CustomKey::run("gnome-calendar"),
            ItemKind::Battery => CustomKey::run("gnome-control-center power"),
            ItemKind::Cpu
            | ItemKind::Memory
            | ItemKind::Temperature
            | ItemKind::Fan
            | ItemKind::Network => CustomKey::run("gnome-system-monitor"),
            ItemKind::Pomodoro => self.pomodoro_tap(now)?,
            ItemKind::Controls => self.toggle_panel(now),
            ItemKind::Emoji => self.open_emoji(now),
            ItemKind::Windows => {
                if let Some(bus) = &self.bus {
                    shell::list_windows(bus.clone());
                }
            }
            ItemKind::Transient(key) => self.transient_pressed(key.action, now)?,
            _ => {}
        }
        Ok(())
    }

    /// Widest the windows row goes before it pages.
    const WINDOWS_PER_PAGE: usize = 8;

    fn close_transient(&mut self) -> Result<()> {
        if self.transient.take().is_some() {
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn open_emoji(&mut self, now: u64) {
        let mut order: Vec<usize> = self
            .state
            .recent_emoji
            .iter()
            .copied()
            .filter(|index| *index < self.plus.favorites.len())
            .collect();
        for index in 0..self.plus.favorites.len() {
            if !order.contains(&index) {
                order.push(index);
            }
        }
        let mut items: Vec<ItemKind> = order
            .into_iter()
            .map(|index| {
                ItemKind::Transient(layout::TransientKey {
                    glyph: layout::TransientGlyph::Icon(self.plus.favorites[index].icon),
                    action: layout::TransientAction::Emoji(index),
                })
            })
            .collect();
        items.push(close_key());
        self.transient = Some(Transient {
            items,
            until_ms: now + TRANSIENT_IDLE_MS,
            windows: None,
        });
    }

    fn windows_row(&self, windows: &shell::WindowList, page: usize) -> Vec<ItemKind> {
        let per = Self::WINDOWS_PER_PAGE;
        let mut items: Vec<ItemKind> = windows
            .iter()
            .skip(page * per)
            .take(per)
            .map(|(id, _, title)| {
                let label: String = title.chars().filter(char::is_ascii).take(12).collect();
                let label = if label.trim().is_empty() { "-".into() } else { label };
                ItemKind::Transient(layout::TransientKey {
                    glyph: layout::TransientGlyph::Text(layout::intern(label.trim_end())),
                    action: layout::TransientAction::Window(*id),
                })
            })
            .collect();
        if windows.len() > per {
            items.push(ItemKind::Transient(layout::TransientKey {
                glyph: layout::TransientGlyph::Icon("chevron-right"),
                action: layout::TransientAction::NextPage,
            }));
        }
        items.push(close_key());
        items
    }

    fn windows_listed(&mut self, windows: Option<shell::WindowList>) -> Result<()> {
        let now = self.now_ms();
        match windows {
            Some(windows) if !windows.is_empty() => {
                let items = self.windows_row(&windows, 0);
                self.transient = Some(Transient {
                    items,
                    until_ms: now + TRANSIENT_IDLE_MS,
                    windows: Some((windows, 0)),
                });
            }
            Some(_) => {}
            None => {
                self.layout_ctx.windows_missing = true;
                self.windows_missing_until_ms = Some(now + 2_000);
            }
        }
        self.refresh_widgets()
    }

    fn transient_pressed(&mut self, action: layout::TransientAction, now: u64) -> Result<()> {
        use layout::TransientAction as A;
        match action {
            A::Suggest(index) => shell::select_suggestion(index),
            A::Emoji(index) => {
                if let Some(favorite) = self.plus.favorites.get(index).cloned() {
                    policy::push_recent(&mut self.state.recent_emoji, index, 12);
                    self.save();
                    self.paste(&favorite.text)?;
                }
                self.transient = None;
                self.restore_dialog_row(now);
            }
            A::Window(id) => {
                std::thread::spawn(move || shell::activate_window(id));
                self.transient = None;
                self.restore_dialog_row(now);
            }
            A::NextPage => {
                let next = self.transient.as_ref().and_then(|t| t.windows.as_ref()).map(
                    |(windows, page)| {
                        let pages = windows.len().div_ceil(Self::WINDOWS_PER_PAGE);
                        (windows.clone(), (page + 1) % pages.max(1))
                    },
                );
                if let Some((windows, page)) = next {
                    let items = self.windows_row(&windows, page);
                    self.transient = Some(Transient {
                        items,
                        until_ms: now + TRANSIENT_IDLE_MS,
                        windows: Some((windows, page)),
                    });
                }
            }
            A::Close => {
                self.transient = None;
                self.restore_dialog_row(now);
            }
            A::Dialog(index) => {
                if let Some(button) = self.dialog.as_ref().and_then(|d| d.buttons.get(index)) {
                    atspi::press(button);
                }
            }
            A::Label => {}
            A::SwitchApp(index) => self.switcher_pick(index, now)?,
            A::SwitchWindow(id) => {
                std::thread::spawn(move || shell::activate_window(id));
            }
        }
        Ok(())
    }

    /// Follows Super/Alt+Tab: the shell switches the apps, the bar shows them.
    fn switcher_key(&mut self, code: u32, pressed: bool) -> Result<()> {
        if !self.config.app_switcher {
            return Ok(());
        }
        let now = self.now_ms();
        let is = |key: Key| code == key as u32;
        // AltGr is not a switcher key. Shortcuts the compositor never receives
        // (a VM or remote desktop grabbing them) can still desync the bar.
        if is(Key::LeftMeta) || is(Key::RightMeta) || is(Key::LeftAlt) {
            self.switch_mods = if pressed {
                self.switch_mods.saturating_add(1)
            } else {
                self.switch_mods.saturating_sub(1)
            };
            if self.switch_mods == 0 {
                self.switcher_close()?;
            }
        } else if is(Key::LeftShift) || is(Key::RightShift) {
            self.shift_held = if pressed {
                self.shift_held.saturating_add(1)
            } else {
                self.shift_held.saturating_sub(1)
            };
        } else if is(Key::Esc) && pressed {
            self.switcher_close()?;
        } else if is(Key::Tab) && pressed && self.switch_mods > 0 {
            let step = if self.shift_held > 0 { -1 } else { 1 };
            if let Some(switcher) = &mut self.switcher {
                let count = switcher.apps.len() as i32;
                switcher.selected = (switcher.selected as i32 + step).rem_euclid(count.max(1)) as usize;
                self.show_switcher(now);
                return self.refresh_widgets();
            }
            self.switch_steps += step;
            if !self.switch_wanted {
                self.switch_wanted = true;
                if let Some(bus) = &self.bus {
                    shell::list_apps(bus.clone());
                }
            }
        }
        Ok(())
    }

    fn switcher_close(&mut self) -> Result<()> {
        self.switch_wanted = false;
        self.switch_steps = 0;
        if self.switcher.take().is_some() {
            self.transient = None;
            self.restore_dialog_row(self.now_ms());
            return self.refresh_widgets();
        }
        Ok(())
    }

    fn apps_listed(&mut self, apps: Option<shell::AppList>) -> Result<()> {
        let now = self.now_ms();
        if !self.switch_wanted {
            return Ok(());
        }
        // A failed list keeps the steps and asks again on the next Tab.
        let Some(apps) = apps.filter(|apps| !apps.is_empty()) else {
            return Ok(());
        };
        self.switch_wanted = false;
        let steps = std::mem::take(&mut self.switch_steps);
        if self.switch_mods == 0 {
            return Ok(());
        }
        let selected = steps.rem_euclid(apps.len() as i32) as usize;
        self.switcher = Some(Switcher { apps, selected });
        self.show_switcher(now);
        self.refresh_widgets()
    }

    fn show_switcher(&mut self, now: u64) {
        let Some(switcher) = &self.switcher else { return };
        self.transient = Some(Transient {
            items: switcher_row(switcher),
            until_ms: now + DIALOG_HOLD_MS,
            windows: None,
        });
    }

    /// A touch on an app: Tabs the shell's switcher to it (Super is still held).
    fn switcher_pick(&mut self, index: usize, now: u64) -> Result<()> {
        let Some(switcher) = &mut self.switcher else { return Ok(()) };
        let count = switcher.apps.len() as i32;
        let forward = (index as i32 - switcher.selected as i32).rem_euclid(count);
        let (taps, keys): (i32, &[Key]) = if forward * 2 <= count {
            (forward, &[Key::Tab])
        } else {
            (count - forward, &[Key::LeftShift, Key::Tab])
        };
        switcher.selected = index;
        for _ in 0..taps {
            self.keyboard.tap_combination(keys)?;
        }
        self.show_switcher(now);
        self.refresh_widgets()
    }

    /// Shows the buttons of a dialog of the focused window as the bar's row,
    /// and takes the row away when the dialog is gone.
    /// Puts the open dialog's buttons back on the bar after another row closed.
    fn restore_dialog_row(&mut self, now: u64) {
        if self.transient.is_some() {
            return;
        }
        if let Some(open) = self.dialog.as_ref().filter(|open| !open.buttons.is_empty()) {
            let labels: Vec<&str> = open.buttons.iter().map(|button| button.label.as_str()).collect();
            self.transient = Some(Transient {
                items: dialog_row(&open.title, &labels),
                until_ms: now + DIALOG_HOLD_MS,
                windows: None,
            });
        }
    }

    fn dialog_changed(&mut self, dialog: Option<atspi::Dialog>) -> Result<()> {
        let now = self.now_ms();
        match &dialog {
            Some(open) => eprintln!("kait2en-touchbar-plus: dialog with {} buttons", open.buttons.len()),
            None => eprintln!("kait2en-touchbar-plus: dialog closed"),
        }
        let showing = self.transient.as_ref().is_some_and(|transient| {
            transient.items.iter().any(|item| {
                matches!(item, ItemKind::Transient(key) if matches!(key.action, layout::TransientAction::Dialog(_)))
            })
        });
        match &dialog {
            // Emoji, windows and the app switcher keep the row; the dialog
            // comes back when they close.
            Some(open) if !open.buttons.is_empty() && (self.transient.is_none() || showing) => {
                let labels: Vec<&str> = open.buttons.iter().map(|button| button.label.as_str()).collect();
                let items = dialog_row(&open.title, &labels);
                self.transient = Some(Transient {
                    items,
                    until_ms: now + DIALOG_HOLD_MS,
                    windows: None,
                });
            }
            None if showing => self.transient = None,
            _ => {}
        }
        self.dialog = dialog;
        self.refresh_widgets()
    }

    /// Copies `text` through the shell and pastes it into the focused app.
    fn paste(&mut self, text: &str) -> Result<()> {
        if !shell::insert_text(text) {
            self.show_notice("Touch Bar", i18n::t(i18n::Msg::CopyFailed), self.now_ms())?;
            return Ok(());
        }
        let terminal = TERMINALS.contains(&self.focus_app.as_str());
        let keys: &[Key] = if terminal {
            &[Key::LeftCtrl, Key::LeftShift, Key::V]
        } else {
            &[Key::LeftCtrl, Key::V]
        };
        self.keyboard.tap_combination(keys)
    }

    fn pomodoro_tap(&mut self, now: u64) -> Result<()> {
        self.pomodoro.tap(now);
        self.pomodoro_updated(now)
    }

    fn pomodoro_updated(&mut self, now: u64) -> Result<()> {
        self.widgets.pomodoro = Some(self.pomodoro.view());
        self.pomo_next_ms = self.pomodoro.running.then_some(now + 1_000);
        self.refresh_widgets()
    }

    fn pomodoro_tick(&mut self, now: u64) -> Result<()> {
        if !self.pomo_next_ms.is_some_and(|due| now >= due) {
            return Ok(());
        }
        if let Some(phase) = self.pomodoro.advance(now) {
            let text = if phase == pomodoro::Phase::Break {
                i18n::t(i18n::Msg::Break)
            } else {
                i18n::t(i18n::Msg::Work)
            };
            self.show_notice("Pomodoro", text, now)?;
        }
        self.pomodoro_updated(now)
    }

    fn focus_changed(&mut self, app: String) -> Result<()> {
        if app == self.focus_app {
            return Ok(());
        }
        let changed = self.plus.zone_for(&app) != self.plus.zone_for(&self.focus_app);
        self.focus_app = app;
        if !changed {
            return Ok(());
        }
        let now = self.now_ms();
        let idle = self.visible
            && self.state.mode == Mode::Media
            && self.touch_id == TouchIdState::Idle
            && self.transient.is_none()
            && self.suggest.is_none()
            && self.toast.is_none()
            && self.level_kind.is_none()
            && self.contacts.is_empty();
        if idle {
            let from = match self.layer_fade {
                Some(_) => self.blended.clone(),
                None => self.canvas.pixels.clone(),
            };
            let clip_left = if self.physical_escape {
                0
            } else {
                usize::from(self.layout.buttons[0].right) + 1
            };
            self.layer_fade = Some(LayerFade {
                from,
                start_ms: now,
                next_ms: now,
                direction: 1,
                clip_left,
                confirm: false,
            });
            return self.present_keys();
        }
        self.refresh_widgets()
    }

    fn suggestions_changed(&mut self, items: Vec<String>) -> Result<()> {
        if !self.config.suggestions {
            return Ok(());
        }
        self.suggest = Some(
            items
                .iter()
                .take(5)
                .enumerate()
                .map(|(index, text)| {
                    let label: String = text.chars().take(14).collect();
                    ItemKind::Transient(layout::TransientKey {
                        glyph: layout::TransientGlyph::Text(layout::intern(&label)),
                        action: layout::TransientAction::Suggest(index as u32),
                    })
                })
                .collect(),
        );
        self.refresh_widgets()
    }

    fn suggestions_hidden(&mut self) -> Result<()> {
        if self.suggest.take().is_some() {
            self.refresh_widgets()?;
        }
        Ok(())
    }

    /// A notification or timer message, over the keys or the dark bar.
    /// A plain message (timer, copy failure): `text` as the title, the sender below.
    fn show_notice(&mut self, app: &str, text: &str, now: u64) -> Result<()> {
        let card = renderer::NoticeCard {
            title: fold_ascii(text),
            body: fold_ascii(app),
            icon: renderer::NoticeIcon::Lucide("bell"),
        };
        self.show_card(card, now, false)
    }

    /// Shows `card`, or keeps the latest one for later while Touch ID or a
    /// slider has the bar. `quiet` skips the haptic tap (an update).
    fn show_card(&mut self, card: renderer::NoticeCard, now: u64, quiet: bool) -> Result<()> {
        if self.screen_off() {
            return Ok(());
        }
        if self.touch_id != TouchIdState::Idle || self.level_kind.is_some() {
            self.pending_card = Some((card, now + PENDING_CARD_MS));
            self.pending_card_ms = Some(now + PENDING_CARD_POLL_MS);
            return Ok(());
        }
        let total = NOTICE_ENTER_MS + NOTICE_HOLD_MS;
        if let Some(toast) = self.toast.as_mut().filter(|toast| toast.card.is_some()) {
            // Already showing one: swap the text and restart the hold.
            if now.saturating_sub(toast.start_ms) < total {
                toast.replaced = toast.card.take().map(|old| (old, now));
                toast.card = Some(card);
                toast.start_ms = now.saturating_sub(NOTICE_ENTER_MS);
                if !quiet {
                    self.haptic.click();
                }
                return self.track_frame(now);
            }
        }
        self.refresh_key_level();
        if !quiet {
            self.haptic.click();
        }
        self.toast = Some(TrackToast {
            track: Track {
                artist: String::new(),
                title: card.title.clone(),
                length_us: None,
                art_url: None,
                track_id: None,
            },
            start_ms: now,
            next_ms: None,
            layout: self.track_keys(),
            pressed: None,
            notice: true,
            card: Some(card),
            replaced: None,
        });
        self.track_frame(now)
    }

    /// Shows the notice kept while Touch ID or a slider was up, once they are done.
    fn pending_card_due(&mut self, now: u64) -> Result<()> {
        if !self.pending_card_ms.is_some_and(|due| now >= due) {
            return Ok(());
        }
        self.pending_card_ms = None;
        let Some((card, expires)) = self.pending_card.take() else {
            return Ok(());
        };
        if now >= expires || self.screen_off() {
            return Ok(());
        }
        if self.touch_id != TouchIdState::Idle || self.level_kind.is_some() {
            self.pending_card = Some((card, expires));
            self.pending_card_ms = Some(now + PENDING_CARD_POLL_MS);
            return Ok(());
        }
        self.show_card(card, now, false)
    }

    fn notification(
        &mut self,
        app: String,
        title: String,
        text: String,
        icon: Option<String>,
        replaces: bool,
    ) -> Result<()> {
        // A dark screen shows nothing: GNOME keeps the notice in its list.
        if !self.config.notifications || self.screen_off() {
            return Ok(());
        }
        let now = self.now_ms();
        let (title, body) = if title.trim().is_empty() { (text, String::new()) } else { (title, text) };
        let (title, body) = (fold_ascii(&strip_markup(&title)), fold_ascii(&strip_markup(&body)));
        if title.is_empty() && body.is_empty() {
            return Ok(());
        }
        let icon = match icon {
            Some(path) => renderer::NoticeIcon::App(layout::intern(&path)),
            None => renderer::NoticeIcon::Lucide("bell"),
        };
        // The monitor and the shell extension can both report one notice.
        self.recent_notices.retain(|(_, _, at)| now.saturating_sub(*at) < 1500);
        if self.recent_notices.iter().any(|(t, b, _)| *t == title && *b == body) {
            // The copy with an icon completes a card that showed the bell.
            let showing = self.toast.as_ref().and_then(|toast| toast.card.clone());
            if let Some(shown) = showing.filter(|card| card.title == title && card.icon == renderer::NoticeIcon::Lucide("bell")) {
                if icon != renderer::NoticeIcon::Lucide("bell") {
                    if let Some(toast) = self.toast.as_mut() {
                        toast.card = Some(renderer::NoticeCard { icon, ..shown });
                    }
                }
            }
            return Ok(());
        }
        if self.recent_notices.len() >= 8 {
            self.recent_notices.pop_front();
        }
        self.recent_notices.push_back((title.clone(), body.clone(), now));
        eprintln!("kait2en-touchbar-plus: notice from {app} ({} chars)", title.len() + body.len());
        let updating = replaces && self.toast.as_ref().is_some_and(|toast| toast.card.is_some());
        self.show_card(renderer::NoticeCard { title, body, icon }, now, updating)
    }

    fn transient_tick(&mut self, now: u64) -> Result<()> {
        if self
            .transient
            .as_ref()
            .is_some_and(|transient| now >= transient.until_ms)
        {
            self.close_transient()?;
        }
        if self.windows_missing_until_ms.is_some_and(|due| now >= due) {
            self.windows_missing_until_ms = None;
            self.layout_ctx.windows_missing = false;
            self.refresh_widgets()?;
        }
        Ok(())
    }

    fn press_button(&mut self, index: usize, now: u64) -> Result<()> {
        self.touch_dim(now);
        match self.layout.buttons[index].action {
            Action::Key(key) => self.keyboard.press(key)?,
            // Personal keys act once per tap. Holding repeats nothing.
            Action::Custom(custom) => match self.custom_keys.get(custom).map(|key| &key.action) {
                Some(CustomAction::Send(keys)) => self.keyboard.tap_combination(keys)?,
                Some(CustomAction::Run(command)) => CustomKey::run(command),
                None => {}
            },
            Action::Item(item) => self.item_pressed(item)?,
            Action::Player(command) => self.player_command(command, now)?,
        }
        self.haptic.key_press();
        self.active_button = Some(index);
        if self
            .learner
            .action(now, self.state.mode, &mut self.state, &self.config)
        {
            self.save();
        }
        self.arm_deadline(now);
        self.present_keys()
    }

    fn touch_motion(&mut self, slot: u32, x: f64, y: f64) -> Result<()> {
        let now = self.now_ms();
        self.user_activity(now, false)?;
        self.resolve_pending(now)?;
        self.positions.insert(slot, x);
        match self.contacts.get(&slot).copied() {
            Some(Contact::Swipe) => self.swipe_moved(now),
            Some(Contact::Pending) => {
                let Some(pending) = self.pending else {
                    return Ok(());
                };
                if let Some(index) = pending.button
                    && self.layout.hit(x, y, self.canvas.height) != Some(index)
                {
                    if pending.dark {
                        // Slid off the dark esc slot: an ordinary wake touch.
                        self.pending = Some(PendingTouch {
                            button: None,
                            ..pending
                        });
                        return self.hide_overlay();
                    }
                    self.pending = None;
                    self.active_button = None;
                    self.contacts.insert(slot, Contact::Cancelled);
                    self.present_keys()?;
                }
                Ok(())
            }
            Some(Contact::Panel(slider)) => {
                let level = renderer::panel_level(
                    self.panel_zone,
                    self.plus.controls.items.len(),
                    slider,
                    x,
                );
                self.panel_set(slider, level, now, false)
            }
            Some(Contact::Scrub(_)) => {
                if let Some(scrub) = self.scrub.as_mut() {
                    scrub.x = x;
                }
                self.np_next_ms = Some(now);
                self.update_now_playing(now);
                self.refresh_widgets()
            }
            Some(Contact::Button(index)) => {
                if self.layout.hit(x, y, self.canvas.height) != Some(index) {
                    self.keyboard.release()?;
                    self.active_button = None;
                    self.contacts.insert(slot, Contact::Cancelled);
                    self.present_keys()?;
                }
                Ok(())
            }
            Some(Contact::TouchIdEsc(index)) => {
                if self.layout.hit(x, y, self.canvas.height) != Some(index) {
                    self.active_button = None;
                    self.contacts.insert(slot, Contact::Cancelled);
                    self.render_touch_id(now)?;
                }
                Ok(())
            }
            Some(Contact::MediaHold(index)) => {
                if self.layout.hit(x, y, self.canvas.height) != Some(index) {
                    self.media_hold = None;
                    self.active_button = None;
                    self.contacts.insert(slot, Contact::Cancelled);
                    self.present_keys()?;
                }
                Ok(())
            }
            Some(Contact::TrackKey(forward)) => {
                let Some(toast) = self.toast.as_mut() else {
                    return Ok(());
                };
                let (left, right) = if forward {
                    toast.layout.next
                } else {
                    toast.layout.prev
                };
                if x < f64::from(left) || x >= f64::from(right) {
                    toast.pressed = None;
                    toast.start_ms = now.saturating_sub(TRACK_FADE_MS);
                    self.contacts.insert(slot, Contact::Cancelled);
                    return self.track_frame(now);
                }
                Ok(())
            }
            Some(Contact::DarkEsc) => {
                if !self
                    .layout
                    .hit(x, y, self.canvas.height)
                    .is_some_and(|index| self.is_esc(index))
                {
                    self.keyboard.release()?;
                    self.contacts.insert(slot, Contact::Cancelled);
                    self.hide_overlay()?;
                    self.show_activity_esc()?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn swipe_moved(&mut self, now: u64) -> Result<()> {
        let centroid = self.swipe_centroid();
        let Some(swipe) = self.swipe.as_mut() else {
            return Ok(());
        };
        let steps = swipe.advance(centroid);
        let kind = swipe.kind;
        let span = self.swipe_span();
        if self.level_kind.is_some()
            && ((span.0 - self.level_fingers.0).abs() >= 3.0
                || (span.1 - self.level_fingers.1).abs() >= 3.0)
        {
            // The feedback follows the fingers.
            self.level_fingers = span;
            self.level_dirty = true;
            self.update_level_side(now, false);
        }
        if steps == 0 {
            return Ok(());
        }
        match kind {
            SwipeKind::Volume | SwipeKind::Brightness => {
                // A fast swipe steps more often than the poll interval, so
                // a pending poll is never pushed back.
                self.level_poll_ms.get_or_insert(now + LEVEL_POLL_MS);
                self.level_poll_until_ms = now + LEVEL_POLL_WINDOW_MS;
                let key = match (kind, steps > 0) {
                    (SwipeKind::Volume, true) => Key::VolumeUp,
                    (SwipeKind::Volume, false) => Key::VolumeDown,
                    (_, true) => Key::BrightnessUp,
                    (_, false) => Key::BrightnessDown,
                };
                for _ in 0..steps.unsigned_abs() {
                    self.keyboard.press(key)?;
                    self.keyboard.release()?;
                }
                self.level_stepped = true;
                // Pushing against a stop clicks firmly once, then stays
                // silent like the level does.
                if self
                    .level_kind
                    .is_some_and(|level_kind| at_stop(level_kind, self.level, Some(steps > 0)))
                {
                    if !std::mem::replace(&mut self.level_stop_felt, true) {
                        self.haptic.firm();
                    }
                } else {
                    self.haptic.click();
                }
                Ok(())
            }
            SwipeKind::Mode => {
                // The rows follow the fingers: swiping left brings in the
                // next row from the right. The ring wraps around.
                let mode = if steps < 0 {
                    self.state.mode.next()
                } else {
                    self.state.mode.previous()
                };
                self.switch_mode(mode, now)?;
                if let Some(fade) = self.layer_fade.as_mut() {
                    fade.confirm = true;
                }
                Ok(())
            }
        }
    }

    fn touch_up(&mut self, slot: u32) -> Result<()> {
        let now = self.now_ms();
        self.resolve_pending(now)?;
        self.positions.remove(&slot);
        match self.contacts.remove(&slot) {
            Some(Contact::Pending) => {
                // Lifted within the window: a plain tap.
                if let Some(pending) = self.pending.take() {
                    if pending.dark && pending.button.is_some() {
                        self.keyboard.press(Key::Esc)?;
                        self.keyboard.release()?;
                        self.haptic.key_press();
                        self.haptic.key_release();
                        self.overlay_hide_ms = Some(now + ESC_LINGER_MS);
                    } else if pending.dark {
                        self.wake(true)?;
                    } else if let Some(index) = pending.button {
                        self.press_button(index, now)?;
                        self.keyboard.release()?;
                        self.haptic.key_release();
                        self.active_button = None;
                        self.present_keys()?;
                    }
                }
            }
            Some(Contact::Panel(slider)) => {
                // The last level always goes out, even if throttled before.
                let x = self.positions.get(&slot).copied().unwrap_or_default();
                let level = self.panel.as_ref().and_then(|panel| {
                    panel.levels[self.plus.controls.items.get(slider)?.index()]
                });
                if let (Some(level), Some(control)) =
                    (level, self.plus.controls.items.get(slider).copied())
                {
                    levels::set_control(control, level);
                }
                let _ = x;
                if let Some(panel) = self.panel.as_mut() {
                    panel.active = None;
                    panel.until_ms = now + self.plus.controls.timeout_ms;
                }
                self.refresh_widgets()?;
            }
            Some(Contact::PomoHold(index, touched)) => {
                if now.saturating_sub(touched) >= self.config.hold_slider_ms {
                    self.pomodoro.reset();
                    self.haptic.click();
                    self.pomodoro_updated(now)?;
                } else {
                    self.press_button(index, now)?;
                    self.keyboard.release()?;
                }
                self.haptic.key_release();
                self.active_button = None;
                self.present_keys()?;
            }
            Some(Contact::SliderHold(index)) => {
                // Released before the slider opened: an ordinary key tap.
                self.slider_hold = None;
                self.press_button(index, now)?;
                self.keyboard.release()?;
                self.haptic.key_release();
                self.active_button = None;
                self.present_keys()?;
            }
            Some(Contact::Scrub(index)) => {
                if let Some(scrub) = self.scrub.take() {
                    if (scrub.x - scrub.start_x).abs() < 12.0 {
                        self.press_button(index, now)?;
                    } else {
                        let fraction = self.scrub_fraction(scrub.x);
                        let target = self.player.as_ref().and_then(|info| {
                            Some((
                                info.player.clone(),
                                info.track.track_id.clone()?,
                                (f64::from(fraction) * info.track.length_us? as f64) as i64,
                            ))
                        });
                        if let Some((player, track_id, micros)) = target {
                            if let Err(error) = self.mpris.set_position(&player, &track_id, micros) {
                                eprintln!("kait2en-touchbar-plus: seek: {error:#}");
                            }
                            self.position_us = micros;
                            self.position_at_ms = now;
                        }
                    }
                }
                self.keyboard.release()?;
                self.haptic.key_release();
                self.active_button = None;
                self.np_next_ms = Some(now);
                self.update_now_playing(now);
                self.present_keys()?;
            }
            Some(Contact::Button(_)) => {
                self.keyboard.release()?;
                self.haptic.key_release();
                self.active_button = None;
                self.present_keys()?;
            }
            Some(Contact::TouchIdEsc(_)) => {
                self.keyboard.press(Key::Esc)?;
                self.keyboard.release()?;
                self.haptic.key_press();
                self.haptic.key_release();
                self.active_button = None;
                self.render_touch_id(now)?;
            }
            Some(Contact::MediaHold(index)) => {
                let seeking = self
                    .media_hold
                    .take()
                    .is_some_and(|hold| hold.seeking_since.is_some());
                if !seeking {
                    self.press_button(index, now)?;
                    self.keyboard.release()?;
                }
                self.haptic.key_release();
                self.active_button = None;
                self.present_keys()?;
            }
            Some(Contact::DarkEsc) => {
                self.keyboard.release()?;
                self.haptic.key_release();
                if self.activity_esc_shown {
                    self.escape_opacity = 1.0;
                    self.escape_fade = None;
                    self.render_activity_esc()?;
                } else {
                    self.overlay_hide_ms = Some(now + ESC_LINGER_MS);
                }
            }
            Some(Contact::TrackKey(forward)) => {
                let key = if forward {
                    Key::NextSong
                } else {
                    Key::PreviousSong
                };
                self.keyboard.press(key)?;
                self.keyboard.release()?;
                self.haptic.key_press();
                self.haptic.key_release();
                if let Some(toast) = self.toast.as_mut() {
                    // Stay fully visible. The next track replaces the text.
                    toast.pressed = None;
                    toast.start_ms = now.saturating_sub(TRACK_FADE_MS);
                }
                self.track_frame(now)?;
            }
            Some(Contact::Swipe) => {
                let swipe_fingers = self
                    .contacts
                    .values()
                    .filter(|contact| matches!(contact, Contact::Swipe))
                    .count();
                if swipe_fingers >= 2 {
                    // Lifting to two fingers drops the swipe back to volume.
                    self.swipe = Some(Swipe::new(
                        SwipeKind::Volume,
                        self.swipe_centroid(),
                        f64::from(self.config.volume_swipe_step_px),
                    ));
                    return self.begin_level(LevelKind::Volume);
                }
                // The centroid would jump to the remaining finger, so the
                // first lift ends the swipe.
                self.swipe = None;
                if self.level_kind.is_some() {
                    self.overlay_hide_ms = Some(now + LEVEL_LINGER_MS);
                }
                for contact in self.contacts.values_mut() {
                    if matches!(contact, Contact::Swipe) {
                        *contact = Contact::Cancelled;
                    }
                }
            }
            _ => {}
        }
        if self.contacts.is_empty() {
            self.touches_armed = true;
            self.swipe = None;
        }
        Ok(())
    }

    fn fn_event(&mut self, pressed: bool) -> Result<()> {
        if self.touch_id != TouchIdState::Idle {
            return Ok(());
        }
        let now = self.now_ms();
        if pressed {
            self.fn_down_ms = Some(now);
            self.fn_toggled = false;
            self.wake(false)?;
        } else {
            self.fn_down_ms = None;
            self.fn_toggled = false;
            if self.visible {
                self.arm_deadline(now);
            }
        }
        Ok(())
    }

    fn toggle_mode_if_due(&mut self, now: u64) -> Result<()> {
        let Some(down) = self.fn_down_ms else {
            return Ok(());
        };
        if self.fn_toggled || !long_press_due(down, now, self.config.fn_long_press_ms) {
            return Ok(());
        }
        self.fn_toggled = true;
        self.switch_mode(self.state.mode.toggled(self.state.alternate), now)
    }

    fn switch_mode(&mut self, mode: Mode, now: u64) -> Result<()> {
        self.keyboard.release()?;
        self.active_button = None;
        // Fade from whatever is on screen, even from a running fade.
        let from = match self.layer_fade {
            Some(_) => self.blended.clone(),
            None => self.canvas.pixels.clone(),
        };
        // Forward around the ring comes in from the right, like a swipe to
        // the left; Fn hold picks its direction the same way.
        let direction = if mode == self.state.mode.next() {
            1
        } else {
            -1
        };
        let clip_left = if self.physical_escape {
            0
        } else {
            usize::from(self.layout.buttons[0].right) + 1
        };
        self.layer_fade = Some(LayerFade {
            from,
            start_ms: now,
            next_ms: now,
            direction,
            clip_left,
            confirm: false,
        });
        self.state.set_mode(mode);
        self.save();
        self.arm_deadline(now);
        self.present_keys()
    }

    fn touch_id_changed(&mut self, state: TouchIdState) -> Result<()> {
        self.touch_id = state;
        self.layer_fade = None;
        self.keyboard.release()?;
        self.active_button = None;
        self.clear_touches();
        self.touches_armed = false;
        self.deadline_ms = None;
        if state == TouchIdState::Idle {
            self.next_animation_ms = None;
            if self.config.always_on && !self.screen_off() {
                return self.wake(false);
            }
            return self.go_dark_to_activity_esc(false);
        }
        self.visible = true;
        let now = self.now_ms();
        // Waiting, scanning and retry share the arrow. Keep its swing smooth
        // across those transitions.
        if self.next_animation_ms.is_none() {
            self.animation_start_ms = now;
        }
        self.next_animation_ms = None;
        self.render_touch_id(now)?;
        self.backlight.set(self.config.active_brightness)
    }

    /// Draws the prompt and schedules the next frame while it shows an arrow.
    fn render_touch_id(&mut self, now: u64) -> Result<()> {
        let animated = matches!(
            self.touch_id,
            TouchIdState::Waiting | TouchIdState::Scanning | TouchIdState::Retry
        );
        let phase = if animated {
            // A still look draws one frame and waits for the next state.
            self.next_animation_ms = (self.config.touch_id_look().motion > 0).then(|| now + PROMPT_FRAME_MS);
            let period = PROMPT_PERIOD_MS * 100 / u64::from(self.config.touch_id_speed.max(1));
            let elapsed = now.saturating_sub(self.animation_start_ms) % period.max(1);
            elapsed as f32 / period.max(1) as f32
        } else {
            self.next_animation_ms = None;
            0.0
        };
        let esc = self
            .layout
            .buttons
            .iter()
            .find(|button| button.action == Action::Key(Key::Esc))
            .copied();
        let active = esc.is_some_and(|button| {
            self.active_button.is_some_and(|index| {
                self.layout
                    .buttons
                    .get(index)
                    .is_some_and(|active_button| active_button.action == button.action)
            })
        });
        let look = self.config.touch_id_look();
        self.canvas.touch_id(&look, self.touch_id.as_str(), phase, esc, active);
        self.display
            .present(&self.canvas.pixels, self.canvas.width, self.canvas.height)
    }

    fn tick(&mut self) -> Result<()> {
        let now = self.now_ms();
        self.resolve_pending(now)?;
        if self
            .layer_fade
            .as_ref()
            .is_some_and(|fade| now >= fade.next_ms)
        {
            self.present_layer_fade(now)?;
        }
        if self
            .toast
            .as_ref()
            .and_then(|toast| toast.next_ms)
            .is_some_and(|deadline| now >= deadline)
        {
            self.track_frame(now)?;
        }
        if self.level_poll_ms.is_some_and(|deadline| now >= deadline) {
            self.poll_level(now);
        }
        if self.overlay_hide_ms.is_some_and(|deadline| now >= deadline) {
            if self.level_kind.is_some() {
                self.overlay_hide_ms = None;
                self.level_fade_start_ms = Some(now);
                self.level_frame_ms = Some(now + LEVEL_FRAME_MS);
                self.level_dirty = true;
                self.flush_level()?;
            } else {
                self.hide_overlay()?;
            }
        }
        if self.level_frame_ms.is_some_and(|deadline| now >= deadline) {
            if self
                .level_fade_start_ms
                .is_some_and(|start| now.saturating_sub(start) >= LEVEL_FADE_MS)
            {
                self.hide_overlay()?;
            } else {
                self.level_dirty = true;
                self.flush_level()?;
                // Frames run while fading out or crossfading sides.
                let moving = self.level_right != f32::from(u8::from(self.level_right_target));
                self.level_frame_ms =
                    (self.level_fade_start_ms.is_some() || moving).then_some(now + LEVEL_FRAME_MS);
            }
        }
        if self
            .activity_deadline_ms
            .is_some_and(|deadline| now >= deadline)
        {
            self.activity_timeout();
        }
        self.slider_due(now)?;
        self.panel_tick(now)?;
        self.transient_tick(now)?;
        self.pomodoro_tick(now)?;
        self.widgets_tick(now)?;
        self.now_playing_tick(now)?;
        self.dim_if_due(now);
        self.shift_if_due(now)?;
        self.advance_backlight_fade(now);
        self.advance_escape_fade(now)?;
        if self
            .media_hold
            .as_ref()
            .is_some_and(|hold| now >= hold.next_ms)
        {
            self.seek_step(now)?;
        }
        self.pending_card_due(now)?;
        self.toggle_mode_if_due(now)?;
        if self.touch_id == TouchIdState::Idle
            && self.visible
            && self.deadline_ms.is_some_and(|deadline| now >= deadline)
        {
            if self.screen_off() {
                self.go_dark(false)?;
            } else {
                self.go_dark_to_activity_esc(true)?;
            }
        }
        if self.learner.settle(now, &mut self.state, &self.config) {
            self.save();
        }
        if self
            .next_animation_ms
            .is_some_and(|deadline| now >= deadline)
        {
            self.render_touch_id(now)?;
        }
        Ok(())
    }

    fn next_timeout(&self) -> i32 {
        let now = self.now_ms();
        let mut deadlines = Vec::new();
        if let Some(fade) = &self.layer_fade {
            deadlines.push(fade.next_ms);
        }
        if let Some(deadline) = self.toast.as_ref().and_then(|toast| toast.next_ms) {
            deadlines.push(deadline);
        }
        if let Some(hold) = &self.media_hold {
            deadlines.push(hold.next_ms);
        }
        if let Some(deadline) = self.deadline_ms {
            deadlines.push(deadline);
        }
        if let Some(down) = self.fn_down_ms
            && !self.fn_toggled
        {
            deadlines.push(down + self.config.fn_long_press_ms);
        }
        if let Some(deadline) = self.learner.next_settle_ms(&self.config) {
            deadlines.push(deadline);
        }
        if let Some(deadline) = self.next_animation_ms {
            deadlines.push(deadline);
        }
        if let Some(pending) = self.pending {
            deadlines.push(pending.deadline_ms);
        }
        if let Some(deadline) = self.level_poll_ms {
            deadlines.push(deadline);
        }
        if let Some(deadline) = self.overlay_hide_ms {
            deadlines.push(deadline);
        }
        if let Some(deadline) = self.level_frame_ms {
            deadlines.push(deadline);
        }
        if let Some(deadline) = self.activity_deadline_ms {
            deadlines.push(deadline);
        }
        deadlines.extend(self.dim_deadline_ms);
        deadlines.extend(self.slider_hold.as_ref().map(|hold| hold.due_ms));
        deadlines.extend(self.transient.as_ref().map(|t| t.until_ms));
        deadlines.extend(self.windows_missing_until_ms);
        deadlines.extend(self.pomo_next_ms);
        deadlines.extend(self.panel.as_ref().map(|panel| panel.until_ms));
        deadlines.extend(self.widgets_next_ms);
        deadlines.extend(self.np_next_ms);
        deadlines.extend(self.toggles_next_ms);
        deadlines.extend(self.clock_next_ms);
        deadlines.extend(self.shift_next_ms);
        deadlines.extend(self.pending_card_ms);
        if let Some(fade) = &self.backlight_fade {
            deadlines.push(fade.next_ms);
        }
        if let Some(fade) = &self.escape_fade {
            deadlines.push(fade.next_ms);
        }
        deadlines
            .into_iter()
            .min()
            .map(|deadline| deadline.saturating_sub(now).min(i32::MAX as u64) as i32)
            .unwrap_or(-1)
    }
}

/// One frame of a row switch. The old row leaves before the new row fully
/// appears, while both travel farther than a simple crossfade. Columns left
/// of `clip_left` show the new row as is.
fn compose_slide(
    from: &[u32],
    to: &[u32],
    out: &mut [u32],
    width: usize,
    clip_left: usize,
    direction: i32,
    progress: f32,
) {
    let progress = progress.clamp(0.0, 1.0);
    let shift_out = (direction as f32 * LAYER_SLIDE_PX * progress).round() as isize;
    let shift_in = (direction as f32 * LAYER_SLIDE_PX * (1.0 - progress)).round() as isize;
    let smooth = |value: f32| value * value * (3.0 - 2.0 * value);
    let old_opacity = 1.0 - smooth((progress / 0.68).clamp(0.0, 1.0));
    let new_opacity = smooth(((progress - 0.24) / 0.76).clamp(0.0, 1.0));
    let sample = |frame: &[u32], row: usize, x: isize| -> u32 {
        if x < clip_left as isize || x >= width as isize {
            0
        } else {
            frame[row + x as usize]
        }
    };
    for (row_index, out_row) in out.chunks_mut(width).enumerate() {
        let row = row_index * width;
        for (x, pixel) in out_row.iter_mut().enumerate() {
            if x < clip_left {
                *pixel = to[row + x];
                continue;
            }
            // The old row moved by -shift_out, the new one still lags by
            // shift_in, so each samples its source that far to the side.
            let old = sample(from, row, x as isize + shift_out);
            let new = sample(to, row, x as isize - shift_in);
            let channel = |shift: u32| {
                let a = ((old >> shift) & 0xff) as f32 * old_opacity;
                let b = ((new >> shift) & 0xff) as f32 * new_opacity;
                ((a + b).round().min(255.0) as u32) << shift
            };
            *pixel = channel(16) | channel(8) | channel(0);
        }
    }
}

/// Full-color icons bring their own hue; text and outlines stay neutral
/// instead of taking the (usually green) key color.
fn canvas_color(config: &Config) -> u32 {
    if config.icon_color == "full" {
        0x00dce6ff
    } else {
        config.key_color()
    }
}

/// Draws the bar and the open controls panel to PNGs, without hardware.
/// `<prefix>-<style>-<state>.png` of the Touch ID prompt for each style, with
/// the colors, texts and motion of the configuration.
fn snapshot_touch_id(prefix: &str, styles: &str) -> Result<()> {
    let config = Config::load()?;
    let mut canvas = Canvas::new(2008, 60, canvas_color(&config))?;
    canvas.full_color = config.icon_color == "full";
    for style in styles.chars() {
        let mut look = config.touch_id_look();
        look.style = style;
        for state in renderer::TOUCH_ID_STATES {
            canvas.touch_id(&look, state, 0.7, None, false);
            let mut rgb = Vec::with_capacity(canvas.pixels.len() * 3);
            for pixel in &canvas.pixels {
                rgb.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
            }
            image::save_buffer(
                format!("{prefix}-{style}-{state}.png"),
                &rgb,
                u32::from(canvas.width),
                u32::from(canvas.height),
                image::ExtendedColorType::Rgb8,
            )?;
        }
    }
    Ok(())
}

fn snapshot(path: &str) -> Result<()> {
    let config = Config::load()?;
    let plus = layout::load();
    let mut canvas = Canvas::new(2008, 60, canvas_color(&config))?;
    canvas.full_color = config.icon_color == "full";
    let ctx = LayoutCtx { player_active: true, ..LayoutCtx::default() };
    let mut data = widgets::WidgetData {
        clock: "19:45".into(),
        weather: Some(widgets::Weather { temp_c: 23, icon: "cloud-sun" }),
        now_playing: Some(widgets::NowPlayingView {
            line: "Queen - Bohemian Rhapsody".into(),
            fraction: 0.4,
            bars: Some([6, 14, 9, 16, 7]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let save = |canvas: &Canvas, path: &std::path::Path| -> Result<()> {
        let mut rgb = Vec::with_capacity(canvas.pixels.len() * 3);
        for pixel in &canvas.pixels {
            rgb.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
        }
        image::save_buffer(
            path,
            &rgb,
            u32::from(canvas.width),
            u32::from(canvas.height),
            image::ExtendedColorType::Rgb8,
        )?;
        Ok(())
    };
    let bar = Layout::from_placed(&plus.default, &plus.default_slots, &plus.strip, &plus.strip_slots, 2008, true, &ctx);
    canvas.keys_with(&bar, None, Some(&data));
    save(&canvas, std::path::Path::new(path))?;

    let zone = [ItemKind::Controls];
    let controls_slot = plus
        .default
        .iter()
        .position(|item| *item == ItemKind::Controls)
        .and_then(|at| plus.default_slots.get(at).copied())
        .unwrap_or_default();
    let layout = Layout::from_placed(&zone, &[controls_slot], &plus.strip, &plus.strip_slots, 2008, true, &ctx);
    let left = layout
        .buttons
        .iter()
        .find(|button| matches!(button.action, Action::Item(_)))
        .map_or(layout.zone.0, |button| button.right + 40);
    let right = if plus.controls.width_px == 0 {
        layout.zone.1
    } else {
        left.saturating_add(plus.controls.width_px).min(layout.zone.1)
    };
    data.panel = Some(widgets::PanelView {
        zone: (left, right.max(left.saturating_add(120)).min(layout.zone.1.max(left))),
        rows: plus
            .controls
            .items
            .iter()
            .map(|control| widgets::PanelRow { icon: control.icon(), percent: Some(if control.index() == 0 { 100 } else { 50 }) })
            .collect(),
        active: None,
    });
    canvas.keys_with(&layout, None, Some(&data));
    let panel_path = std::path::Path::new(path).with_extension("panel.png");
    save(&canvas, &panel_path)?;
    for state in ["waiting", "retry", "failed", "matched"] {
        canvas.touch_id(&config.touch_id_look(), state, 0.25, None, false);
        save(&canvas, &std::path::Path::new(path).with_extension(format!("touchid-{state}.png")))?;
    }
    canvas.clear();
    data.panel = None;
    let dialog = dialog_row("Close Tab?", &["Cancel", "Close"]);
    let dialog_layout = Layout::from_placed(&dialog, &[], &plus.strip, &plus.strip_slots, 2008, true, &ctx);
    canvas.keys_with(&dialog_layout, None, Some(&data));
    save(&canvas, &std::path::Path::new(path).with_extension("dialog.png"))?;
    let app = |id: &str, name: &str, windows: &[&str]| {
        (id.to_owned(), name.to_owned(), windows.iter().enumerate().map(|(i, t)| (i as u32, (*t).to_owned())).collect())
    };
    let switcher = Switcher {
        apps: vec![
            app("com.mitchellh.ghostty.desktop", "Ghostty", &["~/dev/kait2en-touchbar-plus", "~ claude"]),
            app("org.gnome.Nautilus.desktop", "Archivos", &["Documentos"]),
            app("org.gnome.Ptyxis.desktop", "Terminal", &["ptyxis"]),
            app("org.mozilla.firefox.desktop", "Firefox", &["Inicio"]),
        ],
        selected: 1,
    };
    let row = switcher_row(&switcher);
    let switcher_layout = Layout::from_placed(&row, &[], &plus.strip, &plus.strip_slots, 2008, true, &ctx);
    canvas.clear();
    canvas.keys_with(&switcher_layout, None, Some(&data));
    save(&canvas, &std::path::Path::new(path).with_extension("switcher.png"))?;
    let card = renderer::NoticeCard {
        title: "Build finished".into(),
        body: "cargo test: 53 passed, 0 failed".into(),
        icon: renderer::NoticeIcon::App(layout::intern(
            &apps::icon_path("com.mitchellh.ghostty.desktop").map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        )),
    };
    let next = renderer::NoticeCard { title: "New message".into(), body: "Ana: are we still on for lunch?".into(), icon: renderer::NoticeIcon::Lucide("bell") };
    for (name, enter, leave, swap) in [("enter", 0.55, 0.0, None), ("hold", 1.0, 0.0, None), ("swap", 1.0, 0.0, Some(0.5)), ("leave", 1.0, 0.5, None)] {
        let motion = renderer::NoticeMotion { enter, leave, remain: 0.6, swap };
        if swap.is_some() {
            canvas.notice_card(&next, Some(&card), motion);
        } else {
            canvas.notice_card(&card, None, motion);
        }
        save(&canvas, &std::path::Path::new(path).with_extension(format!("notice-{name}.png")))?;
    }
    println!("{path}\n{}", panel_path.display());
    Ok(())
}

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--attach") {
        return usb::ensure_display_configuration();
    }
    if std::env::args().any(|arg| arg == "--detach") {
        return usb::restore_firmware_configuration();
    }
    if std::env::args().any(|arg| arg == "--check-config") {
        return match Config::load()
            .and_then(|_| layout::check())
            .and_then(|()| keys::check()) {
            Ok(()) => {
                println!("ok");
                Ok(())
            }
            Err(error) => {
                println!("{error:#}");
                std::process::exit(1);
            }
        };
    }
    if let Some(position) = std::env::args().position(|arg| arg == "--snapshot-touchid") {
        let prefix = std::env::args()
            .nth(position + 1)
            .context("--snapshot-touchid needs an output prefix")?;
        let styles = std::env::args().nth(position + 2).unwrap_or_else(|| "ABCDEF".into());
        return snapshot_touch_id(&prefix, &styles);
    }
    if let Some(position) = std::env::args().position(|arg| arg == "--snapshot") {
        let path = std::env::args()
            .nth(position + 1)
            .context("--snapshot needs an output path")?;
        return snapshot(&path);
    }
    let config = Config::load()?;
    let state_file = state_path();
    if std::env::args().any(|arg| arg == "--reset-learning") {
        let state = PersistentState::reset(&config);
        state.save(&state_file)?;
        println!("reset {}", state_file.display());
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--status") {
        let state = PersistentState::load(&state_file, &config);
        println!(
            "mode={:?}\nmedia_timeout_ms={}\nfunction_timeout_ms={}",
            state.mode, state.media.timeout_ms, state.function.timeout_ms
        );
        return Ok(());
    }

    i18n::set(&config.language);
    let state = PersistentState::load(&state_file, &config);
    let mut runtime = Runtime::new(config, state)?;
    if runtime.config.always_on {
        runtime.wake(false)?;
    } else {
        runtime.go_dark(false)?;
    }
    let (bus, ext_rx, mut ext_wake) = events::channel()?;
    runtime.bus = Some(bus.clone());
    toggles::refresh(&bus);
    screensaver::watch(bus.clone())?;
    upower::watch(bus.clone())?;
    notify::watch(bus.clone())?;
    shell::watch(bus.clone())?;
    atspi::watch(bus.clone(), runtime.dialog_poll.clone())?;
    weather::watch(bus.clone(), runtime.plus.weather.clone())?;
    if runtime.plus.default.iter().chain(&runtime.plus.strip).chain(runtime.plus.profiles.iter().flat_map(|p| &p.items)).any(|item| *item == ItemKind::NextEvent) {
        calendar::watch(bus.clone())?;
    }

    unsafe {
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
    }

    let mut touch_input = Libinput::new_with_udev(Interface);
    let mut main_input = Libinput::new_with_udev(Interface);
    touch_input
        .udev_assign_seat("seat-touchbar")
        .map_err(|()| anyhow::anyhow!("assign Touch Bar seat"))?;
    main_input
        .udev_assign_seat("seat0")
        .map_err(|()| anyhow::anyhow!("assign main seat"))?;
    let (touch_id_rx, mut touch_id_wake) = touchid::watch()?;
    touch_id_wake.set_nonblocking(true)?;
    let mut kbd_wake = kbdlight::watch()?;
    kbd_wake.set_nonblocking(true)?;
    let tracks = {
        let (receiver, wake) = mpris::watch_tracks(bus.clone())?;
        wake.set_nonblocking(true)?;
        Some((receiver, wake))
    };
    let mut touch_device: Option<InputDevice> = None;

    while !STOP.load(Ordering::Relaxed) {
        let mut poll_fds = [
            pollfd {
                fd: touch_input.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            pollfd {
                fd: main_input.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            pollfd {
                fd: touch_id_wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            pollfd {
                fd: kbd_wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            pollfd {
                fd: runtime.level_wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            pollfd {
                // poll ignores negative descriptors.
                fd: tracks.as_ref().map_or(-1, |(_, wake)| wake.as_raw_fd()),
                events: libc::POLLIN,
                revents: 0,
            },
            pollfd {
                fd: ext_wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let result = unsafe {
            libc::poll(
                poll_fds.as_mut_ptr(),
                poll_fds.len() as _,
                runtime.next_timeout(),
            )
        };
        if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(std::io::Error::last_os_error()).context("poll input devices");
        }

        if poll_fds[2].revents & libc::POLLIN != 0 {
            let mut discard = [0u8; 64];
            while touch_id_wake.read(&mut discard).is_ok() {}
            while let Ok(state) = touch_id_rx.try_recv() {
                runtime.touch_id_changed(state)?;
            }
        }

        if poll_fds[3].revents & libc::POLLIN != 0 {
            let mut discard = [0u8; 64];
            while kbd_wake.read(&mut discard).is_ok() {}
            runtime.keyboard_backlight_changed()?;
        }

        if poll_fds[5].revents & libc::POLLIN != 0
            && let Some((receiver, wake)) = &tracks
        {
            let mut discard = [0u8; 64];
            while (&*wake).read(&mut discard).is_ok() {}
            while let Ok(track) = receiver.try_recv() {
                if runtime.config.show_track_changes {
                    runtime.track_changed(track)?;
                }
            }
        }

        if poll_fds[6].revents & libc::POLLIN != 0 {
            let mut discard = [0u8; 64];
            while ext_wake.read(&mut discard).is_ok() {}
            while let Ok(event) = ext_rx.try_recv() {
                match event {
                    events::Ext::ScreenLocked(locked) => runtime.screen_locked(locked)?,
                    events::Ext::ScreenPower(on) => runtime.screen_power(on)?,
                    events::Ext::Focus(app) => runtime.focus_changed(app)?,
                    events::Ext::Notification { app, title, text, icon, replaces } => {
                        runtime.notification(app, title, text, icon, replaces)?
                    }
                    events::Ext::Suggestions(items) => runtime.suggestions_changed(items)?,
                    events::Ext::SuggestionsHidden => runtime.suggestions_hidden()?,
                    events::Ext::Windows(windows) => runtime.windows_listed(windows)?,
                    events::Ext::Player(info) => runtime.player_changed(info)?,
                    events::Ext::PlayerGone(name) => {
                        if runtime.player.as_ref().is_some_and(|info| info.player == name) {
                            runtime.player_changed(None)?
                        }
                    }
                    events::Ext::Dialog(dialog) => runtime.dialog_changed(dialog)?,
                    events::Ext::Apps(apps) => runtime.apps_listed(apps)?,
                    events::Ext::Art(url, gray) => runtime.art_loaded(url, gray)?,
                    events::Ext::Toggles(states) => runtime.toggles_changed(states)?,
                    events::Ext::Events(events) => runtime.events_changed(events)?,
                    events::Ext::Weather(weather) => runtime.weather_changed(weather)?,
                    events::Ext::Battery(percent, charging) => {
                        runtime.battery_changed(percent, charging)?
                    }
                }
            }
        }

        if poll_fds[4].revents & libc::POLLIN != 0 {
            let mut discard = [0u8; 64];
            while (&runtime.level_wake).read(&mut discard).is_ok() {}
            runtime.levels_read();
        }

        if poll_fds[0].revents & libc::POLLIN != 0 {
            touch_input.dispatch()?;
            for event in &mut touch_input {
                match event {
                    Event::Device(DeviceEvent::Added(added))
                        if is_touch_bar(added.device().name()) =>
                    {
                        touch_device = Some(added.device());
                    }
                    Event::Touch(event)
                        if touch_device
                            .as_ref()
                            .is_none_or(|device| *device == event.device()) =>
                    {
                        match event {
                            TouchEvent::Down(down) => runtime.touch_down(
                                down.seat_slot(),
                                down.x_transformed(runtime.canvas.width as u32),
                                down.y_transformed(runtime.canvas.height as u32),
                            )?,
                            TouchEvent::Motion(motion) => runtime.touch_motion(
                                motion.seat_slot(),
                                motion.x_transformed(runtime.canvas.width as u32),
                                motion.y_transformed(runtime.canvas.height as u32),
                            )?,
                            TouchEvent::Up(up) => runtime.touch_up(up.seat_slot())?,
                            TouchEvent::Cancel(cancel) => runtime.touch_up(cancel.seat_slot())?,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }

        if poll_fds[1].revents & libc::POLLIN != 0 {
            main_input.dispatch()?;
            for event in &mut main_input {
                match event {
                    Event::Device(DeviceEvent::Added(added)) => {
                        let device = added.device();
                        if !provides_fn(&device)
                            && !(runtime.config.activity_backlight && provides_activity(&device))
                        {
                            let _ = device.config_send_events_set_mode(SendEventsMode::DISABLED);
                        }
                    }
                    Event::Keyboard(KeyboardEvent::Key(key))
                        if key.device().name() != "T2 Touch Bar" =>
                    {
                        let pressed = key.key_state() == KeyState::Pressed;
                        runtime.user_activity(runtime.now_ms(), true)?;
                        if key.key() == Key::Fn as u32 {
                            runtime.fn_event(pressed)?;
                        }
                        runtime.switcher_key(key.key(), pressed)?;
                    }
                    Event::Pointer(pointer) if pointer.device().name() != "T2 Touch Bar" => {
                        runtime.user_activity(runtime.now_ms(), true)?;
                    }
                    _ => {}
                }
            }
        }
        runtime.tick()?;
        runtime.flush_level()?;
    }
    toggles::shutdown();
    runtime.go_dark(false)?;
    runtime.restore_activity_backlight_on_exit();
    Ok(())
}

fn provides_fn(device: &InputDevice) -> bool {
    device.has_capability(DeviceCapability::Keyboard)
        && device.keyboard_has_key(Key::Fn as u32) == Ok(true)
}

fn provides_activity(device: &InputDevice) -> bool {
    device.name() != "T2 Touch Bar"
        && (device.has_capability(DeviceCapability::Keyboard)
            || device.has_capability(DeviceCapability::Pointer))
}

/// Whether `level` sits at the bottom or at 100, or only at the one `up`
/// points to. GNOME never dims the display below 1%, which is its bottom.
fn at_stop(kind: LevelKind, level: Level, up: Option<bool>) -> bool {
    let Some(percent) = level.percent else {
        return false;
    };
    let bottom = match kind {
        LevelKind::Volume => 0,
        LevelKind::Brightness => 1,
    };
    match up {
        Some(true) => percent >= 100,
        Some(false) => percent <= bottom,
        None => percent <= bottom || percent >= 100,
    }
}

fn cache_slot(kind: LevelKind) -> usize {
    match kind {
        LevelKind::Volume => 0,
        LevelKind::Brightness => 1,
    }
}

fn is_touch_bar(name: &str) -> bool {
    name.contains("Touch Bar") || name.contains("TouchBar")
}

fn long_press_due(down_ms: u64, now_ms: u64, threshold_ms: u64) -> bool {
    now_ms.saturating_sub(down_ms) >= threshold_ms
}

fn has_physical_escape() -> bool {
    let Ok(product) = std::fs::read_to_string("/sys/class/dmi/id/product_name") else {
        return false;
    };
    product
        .trim()
        .strip_prefix("MacBookPro")
        .and_then(|rest| rest.split(',').next())
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 16)
}

/// What the bar does when the screen changes: `Some(false)` goes dark,
/// `Some(true)` lights up, `None` leaves it.
fn screen_transition(was_off: bool, now_off: bool, enabled: bool, always_on: bool) -> Option<bool> {
    match (enabled && was_off != now_off, now_off) {
        (true, true) => Some(false),
        (true, false) if always_on => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn strip_markup_keeps_loose_less_than() {
        assert_eq!(super::strip_markup("a < b"), "a < b");
        assert_eq!(super::strip_markup("<b>x</b> &amp;lt;"), "x &lt;");
        assert_eq!(super::strip_markup("x<5 and y>3"), "x<5 and y>3");
    }

    #[test]
    fn fold_ascii_never_returns_empty() {
        assert_eq!(super::fold_ascii("日本"), "-");
        assert_eq!(super::fold_ascii("Árbol\tß"), "Arbol ss");
        assert_eq!(super::fold_ascii("   "), "");
    }

    #[test]
    fn screen_transition_turns_off_and_back_on() {
        use super::screen_transition as t;
        assert_eq!(t(false, true, true, false), Some(false));
        assert_eq!(t(true, false, true, true), Some(true));
        assert_eq!(t(true, false, true, false), None);
        assert_eq!(t(false, true, false, true), None);
        assert_eq!(t(true, true, true, true), None);
    }

    use super::*;

    #[test]
    fn hold_turns_volume_key_into_slider() {
        assert_eq!(
            slider_kind(Action::Key(Key::VolumeUp)),
            Some(LevelKind::Volume)
        );
        assert_eq!(slider_kind(Action::Key(Key::Mute)), Some(LevelKind::Volume));
        assert_eq!(
            slider_kind(Action::Key(Key::BrightnessDown)),
            Some(LevelKind::Brightness)
        );
        assert_eq!(slider_kind(Action::Key(Key::PlayPause)), None);
        assert_eq!(slider_kind(Action::Item(0)), None);
        // One finger then steps like the multi-finger swipe does.
        let mut swipe = Swipe::new(SwipeKind::Volume, 500.0, 40.0);
        assert_eq!(swipe.advance(581.0), 2);
    }

    #[test]
    fn touch_bar_names_are_narrow() {
        assert!(is_touch_bar("Apple Inc. Touch Bar Display Touchpad"));
        assert!(!is_touch_bar("Apple Internal Keyboard / Trackpad"));
    }

    #[test]
    fn row_switch_slides_and_keeps_esc() {
        // One row, 300 px: esc left of column 10, a lit pixel at 150.
        let width = 300;
        let mut from = vec![0; width];
        let mut to = vec![0; width];
        from[150] = 0x00ff_ffff;
        to[150] = 0x00ff_ffff;
        from[5] = 0x0011_1111;
        to[5] = 0x0022_2222;
        let mut out = vec![0; width];
        // Halfway through a forward switch, the rows have moved apart and
        // each is independently fading through black.
        compose_slide(&from, &to, &mut out, width, 10, 1, 0.5);
        assert_eq!(out[55], 0x002c_2c2c);
        assert_eq!(out[245], 0x0045_4545);
        assert_eq!(out[150], 0);
        assert_eq!(out[5], 0x0022_2222, "esc is never moved or blended");
        // At the end only the new row remains, in place.
        compose_slide(&from, &to, &mut out, width, 10, 1, 1.0);
        assert_eq!(out[150], 0x00ff_ffff);
        assert_eq!(out[100], 0);
    }

    #[test]
    fn fn_hold_switches_at_the_configured_boundary() {
        assert!(!long_press_due(1_000, 1_599, 600));
        assert!(long_press_due(1_000, 1_600, 600));
    }
}
