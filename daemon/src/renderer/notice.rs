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
        if let (Some(old), Some(_)) = (old, motion.swap) {
            // The old text rises and fades in the same place as the new one.
            let old_fit = self.notice_fit(old, zone_right);
            self.notice_text_at(&old_fit, left + BOX + 24.0, alpha * (1.0 - swap), -14.0 * swap);
        }
        self.notice_frame_outline(left, zone_right, alpha);
        self.notice_body(card, &fit, left, zone_right, alpha, motion, swap);
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

    fn notice_body(&mut self, card: &NoticeCard, fit: &NoticeFit, left: f32, zone_right: f32, alpha: f32, motion: NoticeMotion, swap: f32) {
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
        self.notice_text_at(fit, left + BOX + 24.0, alpha * swap, 14.0 * (1.0 - swap));
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
        let title = self.fit_ascii(&card.title, TITLE_SIZE, max, STRONG_WEIGHT);
        let body = self.fit_ascii(&card.body, BODY_SIZE, max, FONT_WEIGHT);
        let fit = NoticeFit { title, body };
        if self.notice_fits.len() >= 8 {
            self.notice_fits.clear();
        }
        self.notice_fits.insert(key, fit.clone());
        fit
    }

    fn notice_text_at(&mut self, fit: &NoticeFit, left: f32, alpha: f32, drop: f32) {
        let full = self.color;
        let height = f32::from(self.height);
        let two = !fit.body.is_empty();
        let (title_y, body_y) = if two { (height / 2.0 - 11.0, height / 2.0 + 14.0) } else { (height / 2.0, 0.0) };
        self.color = full.scale((alpha.clamp(0.0, 1.0) * 100.0) as u32);
        let mut path = PathBuilder::new();
        let baseline = (title_y + drop + self.cap_height(TITLE_SIZE) / 2.0).round();
        self.append_text(&mut path, left, baseline, &fit.title, TITLE_SIZE, STRONG_WEIGHT);
        if let Some(path) = path.finish() {
            self.fill(&path, self.color);
        }
        if two {
            self.color = full.scale((alpha.clamp(0.0, 1.0) * 62.0) as u32);
            let mut path = PathBuilder::new();
            let baseline = (body_y + drop + self.cap_height(BODY_SIZE) / 2.0).round();
            self.append_text(&mut path, left, baseline, &fit.body, BODY_SIZE, FONT_WEIGHT);
            if let Some(path) = path.finish() {
                self.fill(&path, self.color);
            }
        }
        self.color = full;
    }
}
