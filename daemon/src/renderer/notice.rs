// SPDX-License-Identifier: GPL-3.0-or-later

//! The notification card: the app's logo, a title and a description that
//! slide in from the right, hold while a thin bar runs down, and slide out.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum NoticeIcon {
    /// Icon file of an app, drawn in its own colors.
    App(&'static str),
    Lucide(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoticeCard {
    pub title: String,
    pub body: String,
    pub icon: NoticeIcon,
    /// Name of the app that sent it, so a touch can take the user there.
    pub app: String,
}

/// Where the card is in its timeline, each value 0..1.
#[derive(Clone, Copy, Debug)]
pub struct NoticeMotion {
    /// 0 off the bar to the right, 1 in place.
    pub enter: f32,
    /// 0 in place, 1 gone to the right.
    pub leave: f32,
    /// Time left of the hold: 1 full, 0 spent.
    pub remain: f32,
    /// A newer notice replaces this one: 0 just started, 1 done.
    pub swap: Option<f32>,
    /// Ms desde que terminó la entrada, para desplazar el texto largo.
    pub scroll_ms: u64,
}

/// Quieto este tiempo antes de desplazar el texto largo.
pub const SCROLL_PAUSE_MS: u64 = 1000;
/// Quieto este tiempo al final del desplazamiento antes de salir.
pub const SCROLL_END_MS: u64 = 1200;
const SCROLL_PX_PER_S: f32 = 50.0;

/// Cuánto se corrió un texto que se pasa `over` px, tras `ms`.
pub fn scroll_offset(over: f32, ms: u64) -> f32 {
    if over <= 0.0 || ms < SCROLL_PAUSE_MS {
        return 0.0;
    }
    over.min((ms - SCROLL_PAUSE_MS) as f32 * SCROLL_PX_PER_S / 1000.0)
}

const BOX: f32 = 50.0;
const TITLE_SIZE: f32 = 21.0;
const BODY_SIZE: f32 = 17.0;
const SLIDE: f32 = 60.0;
/// Space left at the edges: the card spans the app zone, never the strip.
const MARGIN: f32 = 8.0;

fn ease_out_back(t: f32) -> f32 {
    let (c1, c3) = (1.2_f32, 2.2_f32);
    let u = t - 1.0;
    1.0 + c3 * u * u * u + c1 * u * u
}

fn ease_in(t: f32) -> f32 {
    t * t * t
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// The text of a card fitted to the bar, and its width.
#[derive(Clone)]
pub struct NoticeFit {
    title: String,
    body: String,
    title_over: f32,
    body_over: f32,
}

impl Canvas {
    /// Draws `card` for `motion`; `old` is the card being replaced, if any.
    /// `zone_right` is where the app zone ends; the strip beyond it stays
    /// as drawn when `keep_strip` is set, and black otherwise.
    pub fn notice_card(
        &mut self,
        card: &NoticeCard,
        old: Option<&NoticeCard>,
        motion: NoticeMotion,
        zone_right: f32,
        keep_strip: bool,
    ) {
        let full = self.color;
        if keep_strip {
            if let Some(rect) = Rect::from_ltrb(0.0, 0.0, zone_right, f32::from(self.height)) {
                self.pixmap.fill_rect(rect, &BLACK.paint(), Transform::identity(), None);
            }
        } else {
            self.pixmap.fill(Color::BLACK);
        }
        let leave = motion.leave.clamp(0.0, 1.0);
        let slide = (1.0 - ease_out_back(motion.enter.clamp(0.0, 1.0))) * SLIDE + ease_in(leave) * SLIDE;
        let alpha = (motion.enter.clamp(0.0, 1.0) * (1.0 - leave)).clamp(0.0, 1.0);
        let swap = motion.swap.map_or(1.0, |t| ease_out(t.clamp(0.0, 1.0)));
        let fit = self.notice_fit(card, zone_right);
        let left = (MARGIN + slide).max(0.0);
        let clip = (left + BOX + 24.0, zone_right - MARGIN - 12.0);
        if let (Some(old), Some(_)) = (old, motion.swap) {
            // The old text rises and fades in the same place as the new one.
            let old_fit = self.notice_fit(old, zone_right);
            self.notice_text_at(&old_fit, left + BOX + 24.0, alpha * (1.0 - swap), -14.0 * swap, clip, 0);
        }
        self.notice_frame_outline(left, zone_right, alpha);
        self.notice_body(card, &fit, left, zone_right, alpha, motion, swap, clip);
        self.color = full;
        self.flush();
    }

    /// The card is one wide key, so it reads as the thing to touch.
    fn notice_frame_outline(&mut self, left: f32, zone_right: f32, alpha: f32) {
        let full = self.color;
        let height = f32::from(self.height);
        let right = zone_right - MARGIN;
        self.color = full.scale((alpha.clamp(0.0, 1.0) * 100.0) as u32);
        let shade = self.color.scale(22);
        self.stroke(&rounded_rect(left, 2.0, right, height - 2.0, KEY_RADIUS), shade, 1.5);
        self.color = full;
    }

    fn notice_body(&mut self, card: &NoticeCard, fit: &NoticeFit, left: f32, zone_right: f32, alpha: f32, motion: NoticeMotion, swap: f32, clip: (f32, f32)) {
        let full = self.color;
        let height = f32::from(self.height);
        let top = (height - BOX) / 2.0;
        // The box glows while the card lands or its text changes, then settles.
        let settle = if motion.swap.is_some() { swap } else { ease_out(motion.enter.clamp(0.0, 1.0)) };
        let glow = 26.0 + 29.0 * (1.0 - settle);
        self.color = full.scale((alpha * 100.0) as u32);
        let shade = self.color.scale(glow as u32);
        self.stroke(
            &rounded_rect(left + 4.75, top + 0.75, left + 4.0 + BOX - 0.75, top + BOX - 0.75, KEY_RADIUS),
            shade,
            1.5,
        );
        let (cx, cy) = (left + 4.0 + BOX / 2.0, height / 2.0);
        match &card.icon {
            NoticeIcon::App(path) => self.draw_app_icon(cx, cy, path),
            NoticeIcon::Lucide(name) => {
                if let Some(shapes) = lucide::shapes(name) {
                    self.shapes_sized(cx, cy, shapes, BLACK, 1.5);
                }
            }
        }
        self.color = full;
        self.notice_text_at(fit, left + BOX + 24.0, alpha * swap, 14.0 * (1.0 - swap), clip, motion.scroll_ms);
        // The time bar under the card.
        let remaining = ((zone_right - MARGIN - left) * motion.remain.clamp(0.0, 1.0)).max(0.0);
        if remaining >= 1.0 && alpha > 0.0 {
            let tint = full.scale((alpha * 40.0) as u32);
            self.fill(&rounded_rect(left, height - 3.0, left + remaining, height - 1.0, 1.0), tint);
        }
        self.color = full;
    }

    /// The card's text fitted beside the box. Computed once per card: the
    /// fitting measures glyphs and the card draws at 30 fps.
    fn notice_fit(&mut self, card: &NoticeCard, zone_right: f32) -> NoticeFit {
        // The fit depends on the room, so the zone is part of the key.
        let key = (card.title.clone(), format!("{}\0{}", card.body, zone_right as u32));
        if let Some(found) = self.notice_fits.get(&key) {
            return found.clone();
        }
        let max = (zone_right - 2.0 * MARGIN - BOX - 24.0 - 24.0 - SLIDE).max(60.0);
        let title = self.drawable(&card.title);
        let body = self.drawable(&card.body);
        let title_over = (self.text_width_at(&title, TITLE_SIZE, STRONG_WEIGHT) - max).max(0.0);
        let body_over = (self.text_width_at(&body, BODY_SIZE, FONT_WEIGHT) - max).max(0.0);
        let fit = NoticeFit { title, body, title_over, body_over };
        if self.notice_fits.len() >= 8 {
            self.notice_fits.clear();
        }
        self.notice_fits.insert(key, fit.clone());
        fit
    }

    /// `text` sin los caracteres que la fuente no dibuja (emoji, CJK).
    fn drawable(&self, text: &str) -> String {
        let kept: String = text.chars().filter(|&c| c == ' ' || self.font.glyph_id(c).0 != 0).collect();
        let kept = kept.trim().to_owned();
        if kept.is_empty() && !text.trim().is_empty() { "-".to_owned() } else { kept }
    }

    /// Cuánto se pasa la línea más larga de `card` del ancho disponible.
    pub fn notice_overflow(&mut self, card: &NoticeCard, zone_right: f32) -> f32 {
        let fit = self.notice_fit(card, zone_right);
        fit.title_over.max(fit.body_over)
    }

    fn notice_text_at(&mut self, fit: &NoticeFit, left: f32, alpha: f32, drop: f32, clip: (f32, f32), scroll_ms: u64) {
        let full = self.color;
        let height = f32::from(self.height);
        let two = !fit.body.is_empty();
        let (title_y, body_y) = if two { (height / 2.0 - 11.0, height / 2.0 + 14.0) } else { (height / 2.0, 0.0) };
        let mask = Rect::from_ltrb(clip.0, 0.0, clip.1.max(clip.0 + 1.0), height).and_then(|rect| {
            let mut mask = tiny_skia::Mask::new(u32::from(self.width), u32::from(self.height))?;
            mask.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, true, Transform::identity());
            Some(mask)
        });
        self.color = full.scale((alpha.clamp(0.0, 1.0) * 100.0) as u32);
        let mut path = PathBuilder::new();
        let baseline = (title_y + drop + self.cap_height(TITLE_SIZE) / 2.0).round();
        let x = left - scroll_offset(fit.title_over, scroll_ms);
        self.append_text(&mut path, x, baseline, &fit.title, TITLE_SIZE, STRONG_WEIGHT);
        if let Some(path) = path.finish() {
            self.pixmap.fill_path(&path, &self.color.paint(), FillRule::Winding, Transform::identity(), mask.as_ref());
        }
        if two {
            self.color = full.scale((alpha.clamp(0.0, 1.0) * 62.0) as u32);
            let mut path = PathBuilder::new();
            let baseline = (body_y + drop + self.cap_height(BODY_SIZE) / 2.0).round();
            let x = left - scroll_offset(fit.body_over, scroll_ms);
            self.append_text(&mut path, x, baseline, &fit.body, BODY_SIZE, FONT_WEIGHT);
            if let Some(path) = path.finish() {
                self.pixmap.fill_path(&path, &self.color.paint(), FillRule::Winding, Transform::identity(), mask.as_ref());
            }
        }
        self.color = full;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_offset_waits_then_moves_then_stops() {
        assert_eq!(scroll_offset(0.0, 5000), 0.0);
        assert_eq!(scroll_offset(100.0, 900), 0.0);
        assert_eq!(scroll_offset(100.0, 2000), 50.0);
        assert_eq!(scroll_offset(100.0, 9000), 100.0);
    }
}
