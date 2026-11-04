// SPDX-License-Identifier: GPL-3.0-or-later

//! Hilda's eyes: a purely cosmetic item that blinks, glances around, shows an emotion now and then,
//! follows the cursor while it moves and falls asleep after a while without activity.
//! Port of `HildaOjos.swift` (jul879n/hilda-ojos); the frames in `hilda/` are the ones it composes.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use image::imageops::FilterType;
use zbus::blocking::Connection;

use crate::events::{Bus, Ext};

/// Height of the eyes on the bar, in pixels; the frames in `hilda/` are already this tall.
pub const HEIGHT: u32 = 60;
/// From this item width on, the wide frames (5:1) replace the compact ones (64:30).
const WIDE_FROM: u32 = 290;
/// Seconds without keyboard, mouse or touches before Hilda falls asleep.
const SLEEP_AFTER: f64 = 30.0;
const TICK: Duration = Duration::from_millis(33);
const RETRY: Duration = Duration::from_secs(2);

const SHELL: &str = "org.kait2en.TouchbarPlus.Shell";
const SHELL_PATH: &str = "/org/kait2en/TouchbarPlus/Shell";
const IDLE: &str = "org.gnome.Mutter.IdleMonitor";
const IDLE_PATH: &str = "/org/gnome/Mutter/IdleMonitor/Core";

const NAMES: [&str; 15] = [
    "reposo", "parpadeo-0", "parpadeo-1", "parpadeo-2", "parpadeo-3",
    "mira-izq", "mira-der", "mira-arr", "mira-aba",
    "mira-izq-medio", "mira-der-medio", "mira-arr-medio", "mira-aba-medio",
    "feliz", "dormida",
];

macro_rules! frames {
    ($($name:literal),* $(,)?) => {
        [$(
            ($name, include_bytes!(concat!("../hilda/", $name, ".png")) as &[u8],
             include_bytes!(concat!("../hilda/g-", $name, ".png")) as &[u8])
        ),*]
    };
}

static FRAMES: [(&str, &[u8], &[u8]); 15] = frames![
    "reposo", "parpadeo-0", "parpadeo-1", "parpadeo-2", "parpadeo-3",
    "mira-izq", "mira-der", "mira-arr", "mira-aba",
    "mira-izq-medio", "mira-der-medio", "mira-arr-medio", "mira-aba-medio",
    "feliz", "dormida",
];

/// Whether the layout has the item (the thread idles otherwise).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Width in pixels the bar gives the item.
static WIDTH: AtomicU32 = AtomicU32::new(0);
/// A tap on the item: Hilda smiles.
static TOUCHED: AtomicBool = AtomicBool::new(false);

/// A frame already scaled to the bar, as straight RGBA.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

fn decode(png: &[u8]) -> Option<Sprite> {
    let image = image::load_from_memory(png).ok()?;
    let (width, height) = (image.width(), image.height());
    // Frames are made at `HEIGHT`; anything else is scaled so the drawing code can trust it.
    let image = if height == HEIGHT {
        image
    } else {
        image.resize_exact((width * HEIGHT / height).max(1), HEIGHT, FilterType::Triangle)
    };
    let (width, height) = (image.width(), image.height());
    Some(Sprite { width, height, rgba: image.to_rgba8().into_raw().into() })
}

/// Tells the item whether the layout has it and how wide it is.
pub fn set_layout(width: Option<u32>) {
    ACTIVE.store(width.is_some(), Ordering::Relaxed);
    WIDTH.store(width.unwrap_or(0), Ordering::Relaxed);
}

pub fn touch() {
    TOUCHED.store(true, Ordering::Relaxed);
}

/// Small xorshift generator: the eyes only need a bit of variety, not quality.
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9, |d| d.as_nanos() as u64);
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, min: f64, max: f64) -> Duration {
        Duration::from_secs_f64(min + (self.next() % 10_000) as f64 / 10_000.0 * (max - min))
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() % items.len() as u64) as usize]
    }
}

/// Pointer relative to the center of the primary monitor, and the monitor's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    pub dx: i32,
    pub dy: i32,
    pub width: u32,
    pub height: u32,
}

/// Direction and strength of the look toward the pointer: rest near the center, the half frame at a medium
/// distance, the full glance far away.
fn toward(pointer: Pointer) -> &'static str {
    if pointer.width == 0 || pointer.height == 0 {
        return "reposo";
    }
    let dx = pointer.dx as f64 / f64::from(pointer.width);
    let dy = pointer.dy as f64 / f64::from(pointer.height);
    let distance = dx.abs().max(dy.abs());
    if distance < 0.05 {
        return "reposo";
    }
    let medium = distance < 0.2;
    // Screen y grows downward.
    match (dx.abs() >= dy.abs(), dx < 0.0, dy < 0.0, medium) {
        (true, true, _, false) => "mira-izq",
        (true, true, _, true) => "mira-izq-medio",
        (true, false, _, false) => "mira-der",
        (true, false, _, true) => "mira-der-medio",
        (false, _, true, false) => "mira-arr",
        (false, _, true, true) => "mira-arr-medio",
        (false, _, false, false) => "mira-aba",
        (false, _, false, true) => "mira-aba-medio",
    }
}

/// What Hilda does and when; time and the outside world come in as arguments.
struct Eyes {
    rng: Rng,
    blink_at: Instant,
    blink_start: Option<Instant>,
    gaze: &'static str,
    gaze_until: Instant,
    last_pointer: Option<Pointer>,
    pointer_moved_at: Option<Instant>,
    emotion: Option<&'static str>,
    emotion_until: Instant,
    emotion_at: Instant,
    previous: &'static str,
    half_until: Instant,
    half: Option<String>,
    current: &'static str,
}

impl Eyes {
    fn new(now: Instant) -> Self {
        let mut rng = Rng::new();
        let emotion_at = now + rng.range(30.0, 90.0);
        Eyes {
            rng,
            blink_at: now,
            blink_start: None,
            gaze: "reposo",
            gaze_until: now,
            last_pointer: None,
            pointer_moved_at: None,
            emotion: None,
            emotion_until: now,
            emotion_at,
            previous: "reposo",
            half_until: now,
            half: None,
            current: "",
        }
    }

    /// The real blink sequence 0/1/3/2/3/1/0 over 280 ms.
    fn blinking(&mut self, now: Instant) -> Option<&'static str> {
        let start = match self.blink_start {
            Some(start) => start,
            None => {
                if now < self.blink_at {
                    return None;
                }
                self.blink_start = Some(now);
                now
            }
        };
        let ms = now.duration_since(start).as_millis();
        match ms {
            0..35 | 245..280 => Some("parpadeo-0"),
            35..70 | 210..245 => Some("parpadeo-1"),
            70..105 | 175..210 => Some("parpadeo-3"),
            105..175 => Some("parpadeo-2"),
            _ => {
                self.blink_start = None;
                self.blink_at = now + self.rng.range(3.0, 6.0);
                None
            }
        }
    }

    /// The frame to show now; `None` when it did not change.
    fn next(&mut self, now: Instant, idle: f64, pointer: Option<Pointer>, smile: bool) -> Option<&'static str> {
        let mut variant;
        if idle > SLEEP_AFTER && !smile {
            variant = "dormida";
        } else {
            if smile {
                self.emotion = Some("feliz");
                self.emotion_until = now + Duration::from_secs(2);
                self.emotion_at = self.emotion_until + self.rng.range(30.0, 90.0);
            }
            if self.emotion.is_some() && now >= self.emotion_until {
                self.emotion = None;
            }
            if self.emotion.is_none() && now >= self.emotion_at {
                self.emotion = Some("feliz");
                self.emotion_until = now + Duration::from_secs(2);
                self.emotion_at = self.emotion_until + self.rng.range(30.0, 90.0);
            }
            if let Some(emotion) = self.emotion {
                variant = emotion;
            } else if let Some(blink) = self.blinking(now) {
                variant = blink;
            } else {
                if pointer != self.last_pointer {
                    self.last_pointer = pointer;
                    if pointer.is_some() {
                        self.pointer_moved_at = Some(now);
                    }
                }
                let following = self
                    .pointer_moved_at
                    .is_some_and(|moved| now.duration_since(moved) < Duration::from_secs(4));
                match pointer.filter(|_| following) {
                    Some(pointer) => {
                        // Follows the cursor while it moves, and 4 s after.
                        self.gaze = toward(pointer);
                        self.gaze_until = now;
                    }
                    None if now >= self.gaze_until => {
                        self.gaze = if self.rng.next() % 2 == 0 {
                            *self.rng.pick(&["mira-izq", "mira-der", "mira-arr", "mira-aba"])
                        } else {
                            "reposo"
                        };
                        self.gaze_until = now + self.rng.range(1.5, 4.0);
                    }
                    None => {}
                }
                variant = self.gaze;
            }
        }

        // Between rest and a glance (either way) it passes through the real half frame.
        if variant != self.previous {
            let glance = if self.previous == "reposo" { variant } else { self.previous };
            if (self.previous == "reposo") != (variant == "reposo") && glance.starts_with("mira-") && !glance.ends_with("-medio") {
                self.half = Some(format!("{glance}-medio"));
                self.half_until = now + Duration::from_millis(40);
            } else {
                self.half = None;
            }
            self.previous = variant;
        }
        let show = match &self.half {
            Some(half) if now < self.half_until => NAMES.iter().copied().find(|name| name == half).unwrap_or(variant),
            _ => variant,
        };
        variant = show;
        if variant == self.current {
            return None;
        }
        self.current = variant;
        Some(variant)
    }
}

/// Starts the thread that publishes the frames. It idles while the layout has no Hilda item.
pub fn watch(bus: Bus) -> Result<()> {
    let mut sprites: HashMap<&'static str, (Sprite, Sprite)> = HashMap::new();
    for (name, narrow, wide) in FRAMES {
        let (Some(narrow), Some(wide)) = (decode(narrow), decode(wide)) else {
            anyhow::bail!("hilda frame {name} does not decode");
        };
        sprites.insert(name, (narrow, wide));
    }
    thread::Builder::new().name("hilda".to_owned()).spawn(move || {
        loop {
            if let Err(error) = run(&bus, &sprites) {
                eprintln!("kait2en-touchbar-plus: hilda: {error:#}");
            }
            thread::sleep(RETRY);
        }
    })?;
    Ok(())
}

fn run(bus: &Bus, sprites: &HashMap<&'static str, (Sprite, Sprite)>) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    let mut eyes = Eyes::new(Instant::now());
    let mut idle = 0.0;
    let mut idle_at = Instant::now() - RETRY;
    let mut pointer_retry = Instant::now();
    let mut shown: Option<(&'static str, bool)> = None;
    loop {
        thread::sleep(TICK);
        if !ACTIVE.load(Ordering::Relaxed) {
            shown = None;
            thread::sleep(Duration::from_millis(400));
            continue;
        }
        let now = Instant::now();
        if now.duration_since(idle_at) >= Duration::from_millis(500) {
            idle_at = now;
            // Without the monitor, Hilda simply never sleeps.
            idle = connection
                .call_method(Some(IDLE), IDLE_PATH, Some(IDLE), "GetIdletime", &())
                .ok()
                .and_then(|reply| reply.body().deserialize::<u64>().ok())
                .map_or(0.0, |ms| ms as f64 / 1000.0);
        }
        let pointer = if now >= pointer_retry && idle <= SLEEP_AFTER {
            let reply = connection
                .call_method(Some(SHELL), SHELL_PATH, Some(SHELL), "GetPointer", &())
                .ok()
                .and_then(|reply| reply.body().deserialize::<(i32, i32, u32, u32)>().ok());
            if reply.is_none() {
                // The extension is not there (or too old): glance around at random until it appears.
                pointer_retry = now + RETRY;
            }
            reply.map(|(dx, dy, width, height)| Pointer { dx, dy, width, height })
        } else {
            None
        };
        let smile = TOUCHED.swap(false, Ordering::Relaxed);
        let name = eyes.next(now, idle, pointer, smile);
        let wide = WIDTH.load(Ordering::Relaxed) >= WIDE_FROM;
        let want = (name.unwrap_or(eyes.current), wide);
        if name.is_none() && shown == Some(want) {
            continue;
        }
        shown = Some(want);
        if let Some((narrow, wide_sprite)) = sprites.get(want.0).or_else(|| sprites.get("reposo")) {
            bus.send(Ext::HildaFrame(if wide { wide_sprite.clone() } else { narrow.clone() }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(dx: i32, dy: i32) -> Pointer {
        Pointer { dx, dy, width: 1000, height: 1000 }
    }

    #[test]
    fn frames_decode_at_bar_height() {
        for (name, narrow, wide) in FRAMES {
            for png in [narrow, wide] {
                let sprite = decode(png).unwrap_or_else(|| panic!("{name}"));
                assert_eq!(sprite.height, HEIGHT);
                assert_eq!(sprite.rgba.len() as u32, sprite.width * sprite.height * 4);
            }
        }
        assert!(NAMES.iter().all(|name| FRAMES.iter().any(|(frame, ..)| frame == name)));
    }

    #[test]
    fn looks_toward_the_pointer() {
        assert_eq!(toward(pointer(10, -10)), "reposo");
        assert_eq!(toward(pointer(-100, 0)), "mira-izq-medio");
        assert_eq!(toward(pointer(500, 0)), "mira-der");
        assert_eq!(toward(pointer(0, -500)), "mira-arr");
        assert_eq!(toward(pointer(0, 100)), "mira-aba-medio");
    }

    #[test]
    fn sleeps_when_idle_and_wakes() {
        let start = Instant::now();
        let mut eyes = Eyes::new(start);
        assert_eq!(eyes.next(start, 31.0, None, false), Some("dormida"));
        assert_ne!(eyes.next(start, 0.0, None, false), Some("dormida"));
    }

    #[test]
    fn follows_a_moving_pointer_through_the_half_frame() {
        let start = Instant::now();
        let mut eyes = Eyes::new(start);
        // Skip the first blink so the gaze decides.
        eyes.blink_at = start + Duration::from_secs(60);
        eyes.gaze_until = start + Duration::from_secs(60);
        eyes.next(start, 0.0, Some(pointer(0, 0)), false);
        assert_eq!(eyes.next(start, 0.0, Some(pointer(-500, 0)), false), Some("mira-izq-medio"));
        let later = start + Duration::from_millis(50);
        assert_eq!(eyes.next(later, 0.0, Some(pointer(-500, 0)), false), Some("mira-izq"));
    }

    #[test]
    fn a_tap_makes_it_smile() {
        let start = Instant::now();
        let mut eyes = Eyes::new(start);
        eyes.blink_at = start + Duration::from_secs(60);
        assert_eq!(eyes.next(start, 0.0, None, true), Some("feliz"));
    }

    #[test]
    fn blink_runs_the_whole_sequence() {
        let start = Instant::now();
        let mut eyes = Eyes::new(start);
        eyes.gaze_until = start + Duration::from_secs(60);
        let mut seen = Vec::new();
        for ms in (0..300).step_by(5) {
            if let Some(name) = eyes.next(start + Duration::from_millis(ms), 0.0, None, false) {
                seen.push(name);
            }
        }
        assert_eq!(seen.first(), Some(&"parpadeo-0"));
        assert!(seen.contains(&"parpadeo-2"));
        assert_eq!(seen.last(), Some(&"reposo"));
    }
}
