// SPDX-License-Identifier: GPL-3.0-or-later

//! The Touch ID prompt: six looks (A to F) for four states, with colors,
//! texts and motion the settings app can change.

use std::f32::consts::TAU;

use super::*;

/// Order of the states in `colors` and `texts`.
pub const STATES: [&str; 4] = ["waiting", "retry", "failed", "matched"];

#[derive(Clone, Debug)]
pub struct TouchIdLook {
    /// `A` to `F`.
    pub style: char,
    /// Per state: `None` follows the key color.
    pub colors: [Option<u32>; 4],
    /// Per state: empty uses the text of the language.
    pub texts: [String; 4],
    /// 0 still, 1 subtle, 2 full.
    pub motion: u8,
}

impl Default for TouchIdLook {
    fn default() -> Self {
        Self {
            style: 'F',
            colors: [None, Some(0xff9f0a), Some(0xff453a), Some(0x34c759)],
            texts: Default::default(),
            motion: 2,
        }
    }
}

/// What every look draws, resolved from the state and the look.
struct Prompt<'a> {
    state: usize,
    color: Rgb,
    parts: Vec<(&'a str, f32)>,
    icon: &'static str,
    /// 0..1 loop position; fixed when the look is still.
    phase: f32,
    /// Strength of the motion: 1 full, 0.5 subtle.
    amp: f32,
    /// False when the look is still: no typing, no sweep.
    moving: bool,
}

impl Prompt<'_> {
    fn matched(&self) -> bool {
        self.state == 3
    }
    fn failed(&self) -> bool {
        self.state == 2
    }
    /// 0..1..0 breathing of the loop.
    fn pulse(&self, offset: f32) -> f32 {
        (1.0 - ((self.phase + offset) * TAU).cos()) / 2.0
    }
}

impl Canvas {
    /// Authentication prompt for `state` (`waiting`, `retry`, `failed` or
    /// `matched`). `phase` (0..1) loops the animation.
    pub fn touch_id(&mut self, look: &TouchIdLook, state: &str, phase: f32, esc: Option<Button>, active: bool) {
        use crate::i18n::{Msg, t};
        self.pixmap.fill(Color::BLACK);
        let index = STATES.iter().position(|name| *name == state).unwrap_or(0);
        let custom = look.texts[index].trim();
        let default: Vec<(&str, f32)> = match index {
            3 => vec![(t(Msg::Unlocked), STRONG_WEIGHT)],
            1 => vec![(t(Msg::TryAgain), STRONG_WEIGHT)],
            2 => vec![(t(Msg::UsePassword), STRONG_WEIGHT)],
            _ => vec![(t(Msg::Unlock), STRONG_WEIGHT), (t(Msg::WithTouchId), FONT_WEIGHT)],
        };
        let parts = if custom.is_empty() { default } else { vec![(custom, STRONG_WEIGHT)] };
        let color = look.colors[index].map_or(self.color, Rgb::from_u32);
        let icon = match index {
            3 => "check",
            2 => "lock",
            _ => "fingerprint",
        };
        let moving = look.motion > 0;
        let prompt = Prompt {
            state: index,
            color,
            parts,
            icon,
            phase: if moving { phase.rem_euclid(1.0) } else { 0.25 },
            amp: if look.motion == 1 { 0.5 } else { 1.0 },
            moving,
        };
        if let Some(esc) = esc {
            // Under the effects, like the sensor halo.
            self.button(&esc, active);
        }
        match look.style {
            'A' => self.prompt_pulse(&prompt),
            'B' => self.prompt_key(&prompt),
            'C' => self.prompt_scanner(&prompt),
            'D' => self.prompt_thread(&prompt),
            'E' => self.prompt_chain(&prompt),
            _ => self.prompt_glow(&prompt),
        }
        self.flush();
    }

    /// Fills `x0..x1` with a horizontal gradient from transparent to `color`
    /// at `alpha` (0..1), or the reverse.
    fn gradient(&mut self, x0: f32, x1: f32, y0: f32, y1: f32, color: Rgb, alpha: f32, reverse: bool) {
        let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let clear = Color::from_rgba8(color.0, color.1, color.2, 0);
        let solid = Color::from_rgba8(color.0, color.1, color.2, a);
        let (first, last) = if reverse { (solid, clear) } else { (clear, solid) };
        let (lo, hi) = (x0.min(x1), x0.max(x1));
        if hi - lo < 1.0 {
            return;
        }
        if let (Some(shader), Some(rect)) = (
            LinearGradient::new(
                Point::from_xy(x0, 0.0),
                Point::from_xy(x1, 0.0),
                vec![GradientStop::new(0.0, first), GradientStop::new(1.0, last)],
                SpreadMode::Pad,
                Transform::identity(),
            ),
            Rect::from_ltrb(lo.max(0.0), y0, hi.min(f32::from(self.width)), y1),
        ) {
            let mut paint = Paint::default();
            paint.shader = shader;
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    /// Icon and text ending `right` px from the left edge. `shown` limits the
    /// characters drawn (typing). Returns the left edge and the icon center.
    fn prompt_line(&mut self, p: &Prompt<'_>, right: f32, shown: Option<usize>, icon_size: f32) -> (f32, f32) {
        let size = PROMPT_FONT_SIZE;
        let cy = f32::from(self.height) / 2.0;
        let full: f32 = p.parts.iter().map(|(text, weight)| self.text_width_at(text, size, *weight)).sum();
        let left = (right - full - PROMPT_ICON_GAP - PROMPT_ICON_SLOT).round();
        let center = left + PROMPT_ICON_SLOT / 2.0;
        if let Some(shapes) = lucide::shapes(p.icon) {
            let saved = self.color;
            self.color = p.color;
            self.shapes_sized(center, cy, shapes, BLACK, icon_size);
            self.color = saved;
        }
        let baseline = (cy + self.cap_height(size) / 2.0).round();
        let mut text = PathBuilder::new();
        let mut x = left + PROMPT_ICON_SLOT + PROMPT_ICON_GAP;
        let mut left_chars = shown.unwrap_or(usize::MAX);
        for (part, weight) in &p.parts {
            let cut: String = part.chars().take(left_chars).collect();
            left_chars = left_chars.saturating_sub(part.chars().count());
            x = self.append_text(&mut text, x, baseline, &cut, size, *weight);
        }
        if let Some(path) = text.finish() {
            self.fill(&path, p.color);
        }
        (left, center)
    }

    /// F: a halo of the state color glows from the sensor, with waves.
    fn prompt_glow(&mut self, p: &Prompt<'_>) {
        let (width, height) = (f32::from(self.width), f32::from(self.height));
        let reach = if p.matched() { 1500.0 } else { 760.0 };
        let breathe = if p.matched() { 1.0 } else { 1.0 - p.amp * 0.55 * (1.0 - p.pulse(0.0)) };
        let peak = p.color.scale((32.0 * breathe) as u32);
        let from = (width - reach).max(0.0);
        let stops = vec![
            GradientStop::new(0.0, Color::BLACK),
            GradientStop::new(1.0, Color::from_rgba8(peak.0, peak.1, peak.2, 255)),
        ];
        if let (Some(shader), Some(rect)) = (
            LinearGradient::new(
                Point::from_xy(from, 0.0),
                Point::from_xy(width, 0.0),
                stops,
                SpreadMode::Pad,
                Transform::identity(),
            ),
            Rect::from_ltrb(from, 0.0, width, height),
        ) {
            let mut paint = Paint::default();
            paint.shader = shader;
            self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
        if !p.matched() {
            for wave in 0..2 {
                let t = (p.phase + wave as f32 * 0.5).rem_euclid(1.0);
                let mut circle = PathBuilder::new();
                circle.push_circle(width, height / 2.0, 12.0 + t * 30.0);
                if let Some(path) = circle.finish() {
                    self.stroke(&path, p.color.scale(((1.0 - t) * 85.0 * p.amp) as u32), 2.5);
                }
            }
        }
        self.prompt_line(p, width - PROMPT_RIGHT, None, 1.5);
    }

    /// A: rings spread from the fingerprint, like calling the finger.
    fn prompt_pulse(&mut self, p: &Prompt<'_>) {
        let (width, cy) = (f32::from(self.width), f32::from(self.height) / 2.0);
        let (_, center) = self.prompt_line(p, width - PROMPT_RIGHT, None, 1.0);
        let (rings, period) = if p.failed() || p.matched() { (2, 0.5) } else { (3, 1.0 / 3.0) };
        for ring in 0..rings {
            let t = (p.phase + ring as f32 * period).rem_euclid(1.0);
            let mut circle = PathBuilder::new();
            circle.push_circle(center, cy, 15.0 + t * 15.0);
            if let Some(path) = circle.finish() {
                self.stroke(&path, p.color.scale(((1.0 - t) * 80.0 * p.amp) as u32), 2.0);
            }
        }
    }

    /// B: a key like the others, with a glint crossing it while it waits.
    fn prompt_key(&mut self, p: &Prompt<'_>) {
        let (width, height) = (f32::from(self.width), f32::from(self.height));
        let size = PROMPT_FONT_SIZE;
        let text: f32 = p.parts.iter().map(|(t, w)| self.text_width_at(t, size, *w)).sum();
        let right = width - PROMPT_RIGHT;
        let left = (right - text - PROMPT_ICON_GAP - PROMPT_ICON_SLOT - 40.0).round();
        let (top, bottom) = (KEY_INSET, height - KEY_INSET);
        let shape = rounded_rect(left, top, right, bottom, KEY_RADIUS);
        if p.matched() {
            self.fill(&shape, p.color);
        } else {
            self.fill(&shape, p.color.scale(10));
            let travel = right - left + 120.0;
            let at = left - 90.0 + travel * p.phase;
            self.gradient(at, at + 45.0, top, bottom, p.color, 0.28 * p.amp, false);
            self.gradient(at + 45.0, at + 90.0, top, bottom, p.color, 0.28 * p.amp, true);
            // The glint spills past the key: cover it with the black frame.
            self.fill(&rect_path(left - 140.0, 0.0, left, height), BLACK);
            self.fill(&rect_path(right, 0.0, right + 140.0, height), BLACK);
        }
        let outline = if p.matched() { p.color } else { p.color.scale(if p.failed() { 90 } else { 40 }) };
        let half = KEY_OUTLINE / 2.0;
        self.stroke(
            &rounded_rect(left + half, top + half, right - half, bottom - half, KEY_RADIUS),
            outline,
            KEY_OUTLINE,
        );
        // Dark ink on the filled key.
        let mut ink = Prompt { state: p.state, color: p.color, parts: p.parts.clone(), icon: p.icon, phase: p.phase, amp: p.amp, moving: p.moving };
        if p.matched() {
            // Dark ink on a light fill, light ink on a dark one.
            let Rgb(r, g, b) = p.color;
            let luminance = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
            ink.color = if luminance > 140.0 { Rgb(4, 33, 13) } else { Rgb(235, 245, 238) };
        }
        self.prompt_line(&ink, right - 20.0, None, 1.2);
    }

    /// C: a line of light scans the fingerprint and the text types itself.
    fn prompt_scanner(&mut self, p: &Prompt<'_>) {
        let (width, height) = (f32::from(self.width), f32::from(self.height));
        let total: usize = p.parts.iter().map(|(text, _)| text.chars().count()).sum();
        let typing = !p.matched() && !p.failed() && p.moving;
        let shown = typing.then(|| ((p.phase * 1.6).min(1.0) * total as f32).ceil() as usize);
        // Box the line out to the full text so the icon does not move while typing.
        let (_, center) = self.prompt_line(p, width - PROMPT_RIGHT, shown, 1.2);
        if !p.matched() && !p.failed() {
            let beam = 1.0 - (2.0 * p.phase - 1.0).abs();
            let y = height / 2.0 - 17.0 + beam * 34.0;
            self.fill(&rect_path(center - 18.0, y - 1.5, center + 18.0, y + 1.5), p.color);
            self.fill(&rect_path(center - 18.0, y - 6.0, center + 18.0, y - 1.5), p.color.scale(18));
            self.fill(&rect_path(center - 18.0, y + 1.5, center + 18.0, y + 6.0), p.color.scale(18));
        }
    }

    /// D: a thread of light runs along the bottom edge toward the sensor.
    fn prompt_thread(&mut self, p: &Prompt<'_>) {
        let (width, height) = (f32::from(self.width), f32::from(self.height));
        if p.matched() {
            self.gradient(0.0, width, 0.0, height, p.color, 0.38, false);
        } else {
            self.fill(&rect_path(0.0, height - 4.0, width, height), p.color.scale(10));
            if p.failed() {
                self.fill(&rect_path(0.0, height - 4.0, width, height), p.color.scale(90));
            } else {
                let at = -340.0 + (width + 340.0) * p.phase;
                self.gradient(at, at + 340.0, height - 4.0, height, p.color, p.amp.min(1.0), false);
            }
        }
        self.prompt_line(p, width - PROMPT_RIGHT, None, 1.3);
    }

    /// E: six dots light up in a chain toward the sensor.
    fn prompt_chain(&mut self, p: &Prompt<'_>) {
        let (width, cy) = (f32::from(self.width), f32::from(self.height) / 2.0);
        let dots_right = width - PROMPT_RIGHT + 20.0;
        let dots = 6;
        let pitch = 24.0;
        let first = dots_right - pitch * dots as f32;
        self.prompt_line(p, first - 24.0, None, 1.3);
        for dot in 0..dots {
            let level = if p.failed() || p.matched() {
                1.0
            } else {
                let lit = ((p.phase * 1.5 - dot as f32 * 0.12).rem_euclid(1.0) * TAU / 2.0).sin();
                0.18 + 0.82 * lit.powi(2) * p.amp
            };
            let mut circle = PathBuilder::new();
            circle.push_circle(first + dot as f32 * pitch + pitch / 2.0, cy, 5.0);
            if let Some(path) = circle.finish() {
                self.fill(&path, p.color.scale((level * 100.0) as u32));
            }
        }
    }
}

fn rect_path(left: f32, top: f32, right: f32, bottom: f32) -> Path {
    rounded_rect(left, top, right.max(left + 1.0), bottom.max(top + 1.0), 0.0)
}
