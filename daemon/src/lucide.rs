// SPDX-License-Identifier: GPL-3.0-or-later

//! Bundled Lucide icons (ISC). Each SVG is parsed into the same stroke
//! shapes the built-in key icons use, on the same 24 px design grid.

use svgtypes::{PathParser, SimplePathSegment, SimplifyingPathParser};
use tiny_skia::PathBuilder;

use crate::renderer::Shape;

pub static ICONS: &[(&str, &str)] = &[
    ("app-window", include_str!("../icons/lucide/app-window.svg")),
    ("arrow-down-up", include_str!("../icons/lucide/arrow-down-up.svg")),
    ("arrow-left", include_str!("../icons/lucide/arrow-left.svg")),
    ("arrow-right", include_str!("../icons/lucide/arrow-right.svg")),
    ("arrow-up", include_str!("../icons/lucide/arrow-up.svg")),
    ("battery-charging", include_str!("../icons/lucide/battery-charging.svg")),
    ("battery-full", include_str!("../icons/lucide/battery-full.svg")),
    ("battery-low", include_str!("../icons/lucide/battery-low.svg")),
    ("battery-medium", include_str!("../icons/lucide/battery-medium.svg")),
    ("bell", include_str!("../icons/lucide/bell.svg")),
    ("bell-off", include_str!("../icons/lucide/bell-off.svg")),
    ("move-horizontal", include_str!("../icons/lucide/move-horizontal.svg")),
    ("bluetooth", include_str!("../icons/lucide/bluetooth.svg")),
    ("bluetooth-off", include_str!("../icons/lucide/bluetooth-off.svg")),
    ("calendar", include_str!("../icons/lucide/calendar.svg")),
    ("camera", include_str!("../icons/lucide/camera.svg")),
    ("check", include_str!("../icons/lucide/check.svg")),
    ("chevron-right", include_str!("../icons/lucide/chevron-right.svg")),
    ("clipboard-paste", include_str!("../icons/lucide/clipboard-paste.svg")),
    ("cloud", include_str!("../icons/lucide/cloud.svg")),
    ("cloud-drizzle", include_str!("../icons/lucide/cloud-drizzle.svg")),
    ("cloud-fog", include_str!("../icons/lucide/cloud-fog.svg")),
    ("cloud-lightning", include_str!("../icons/lucide/cloud-lightning.svg")),
    ("cloud-rain", include_str!("../icons/lucide/cloud-rain.svg")),
    ("cloud-snow", include_str!("../icons/lucide/cloud-snow.svg")),
    ("cloud-sun", include_str!("../icons/lucide/cloud-sun.svg")),
    ("coffee", include_str!("../icons/lucide/coffee.svg")),
    ("copy", include_str!("../icons/lucide/copy.svg")),
    ("cpu", include_str!("../icons/lucide/cpu.svg")),
    ("eraser", include_str!("../icons/lucide/eraser.svg")),
    ("fan", include_str!("../icons/lucide/fan.svg")),
    ("fingerprint", include_str!("../icons/lucide/fingerprint.svg")),
    ("flame", include_str!("../icons/lucide/flame.svg")),
    ("folder-plus", include_str!("../icons/lucide/folder-plus.svg")),
    ("heart", include_str!("../icons/lucide/heart.svg")),
    ("keyboard", include_str!("../icons/lucide/keyboard.svg")),
    ("laugh", include_str!("../icons/lucide/laugh.svg")),
    ("lock-open", include_str!("../icons/lucide/lock-open.svg")),
    ("lock", include_str!("../icons/lucide/lock.svg")),
    ("memory-stick", include_str!("../icons/lucide/memory-stick.svg")),
    ("moon", include_str!("../icons/lucide/moon.svg")),
    ("party-popper", include_str!("../icons/lucide/party-popper.svg")),
    ("pause", include_str!("../icons/lucide/pause.svg")),
    ("play", include_str!("../icons/lucide/play.svg")),
    ("plus", include_str!("../icons/lucide/plus.svg")),
    ("rotate-cw", include_str!("../icons/lucide/rotate-cw.svg")),
    ("search", include_str!("../icons/lucide/search.svg")),
    ("smile", include_str!("../icons/lucide/smile.svg")),
    ("repeat", include_str!("../icons/lucide/repeat.svg")),
    ("repeat-1", include_str!("../icons/lucide/repeat-1.svg")),
    ("shuffle", include_str!("../icons/lucide/shuffle.svg")),
    ("sliders-horizontal", include_str!("../icons/lucide/sliders-horizontal.svg")),
    ("sun", include_str!("../icons/lucide/sun.svg")),
    ("sun-moon", include_str!("../icons/lucide/sun-moon.svg")),
    ("thermometer", include_str!("../icons/lucide/thermometer.svg")),
    ("thumbs-up", include_str!("../icons/lucide/thumbs-up.svg")),
    ("timer", include_str!("../icons/lucide/timer.svg")),
    ("volume-2", include_str!("../icons/lucide/volume-2.svg")),
    ("wifi", include_str!("../icons/lucide/wifi.svg")),
    ("wifi-off", include_str!("../icons/lucide/wifi-off.svg")),
    ("x", include_str!("../icons/lucide/x.svg")),
];

pub fn names() -> impl Iterator<Item = &'static str> {
    ICONS.iter().map(|(name, _)| *name)
}

#[cfg(test)]
pub fn exists(name: &str) -> bool {
    ICONS.iter().any(|(icon, _)| *icon == name)
}

/// Strokes of `name`, centered on the origin of the 24 px grid. Parsed once.
pub fn shapes(name: &str) -> Option<Vec<Shape>> {
    use std::{collections::HashMap, sync::Mutex};
    static CACHE: Mutex<Option<HashMap<String, Option<Vec<Shape>>>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(HashMap::new)
        .entry(name.to_owned())
        .or_insert_with(|| parse_shapes(name))
        .clone()
}

fn parse_shapes(name: &str) -> Option<Vec<Shape>> {
    let (_, svg) = ICONS.iter().find(|(icon, _)| *icon == name)?;
    let mut shapes = Vec::new();
    for element in elements(svg) {
        let mut builder = PathBuilder::new();
        if let Some(d) = element.attr("d") {
            if !append_path(&mut builder, d) {
                return None;
            }
        } else {
            match element.name {
                "circle" => {
                    let (cx, cy, r) = (element.num("cx"), element.num("cy"), element.num("r"));
                    builder.push_circle(cx, cy, r);
                }
                "ellipse" => {
                    let (cx, cy, rx, ry) = (
                        element.num("cx"),
                        element.num("cy"),
                        element.num("rx"),
                        element.num("ry"),
                    );
                    let rect = tiny_skia::Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry)?;
                    builder.push_oval(rect);
                }
                "rect" => {
                    let (x, y, w, h) = (
                        element.num("x"),
                        element.num("y"),
                        element.num("width"),
                        element.num("height"),
                    );
                    let rx = element.num("rx").max(element.num("ry"));
                    let path = rect_path(x, y, x + w, y + h, rx)?;
                    builder.push_path(&path);
                }
                "line" => {
                    builder.move_to(element.num("x1"), element.num("y1"));
                    builder.line_to(element.num("x2"), element.num("y2"));
                }
                "polyline" | "polygon" => {
                    let points: Vec<f32> = element
                        .attr("points")?
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|part| !part.is_empty())
                        .filter_map(|part| part.parse().ok())
                        .collect();
                    for (index, pair) in points.chunks_exact(2).enumerate() {
                        if index == 0 {
                            builder.move_to(pair[0], pair[1]);
                        } else {
                            builder.line_to(pair[0], pair[1]);
                        }
                    }
                    if element.name == "polygon" {
                        builder.close();
                    }
                }
                _ => continue,
            }
        }
        if let Some(path) = builder.finish() {
            shapes.push(Shape::Stroke(path));
        }
    }
    if shapes.is_empty() {
        return None;
    }
    // Same convention as the built-in icons: origin at the center.
    let to_origin = tiny_skia::Transform::from_translate(-12.0, -12.0);
    Some(
        shapes
            .into_iter()
            .filter_map(|shape| shape.path().clone().transform(to_origin).map(Shape::Stroke))
            .collect(),
    )
}

fn rect_path(left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> Option<tiny_skia::Path> {
    let radius = radius.min((right - left) / 2.0).min((bottom - top) / 2.0).max(0.0);
    let k = radius * (1.0 - 0.552_284_8);
    let mut path = PathBuilder::new();
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
}

fn append_path(builder: &mut PathBuilder, d: &str) -> bool {
    // Reject malformed data instead of drawing a truncated icon.
    if PathParser::from(d).any(|segment| segment.is_err()) {
        return false;
    }
    for segment in SimplifyingPathParser::from(d) {
        let Ok(segment) = segment else { return false };
        match segment {
            SimplePathSegment::MoveTo { x, y } => builder.move_to(x as f32, y as f32),
            SimplePathSegment::LineTo { x, y } => builder.line_to(x as f32, y as f32),
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                builder.quad_to(x1 as f32, y1 as f32, x as f32, y as f32)
            }
            SimplePathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => builder.cubic_to(
                x1 as f32, y1 as f32, x2 as f32, y2 as f32, x as f32, y as f32,
            ),
            SimplePathSegment::ClosePath => builder.close(),
        }
    }
    true
}

struct Element<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, &'a str)>,
}

impl Element<'_> {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    fn num(&self, key: &str) -> f32 {
        self.attr(key).and_then(|v| v.parse().ok()).unwrap_or(0.0)
    }
}

/// Drawing elements of a Lucide SVG. The files are machine formatted
/// (`<name attr="v" ... />`), so a tiny scanner is enough.
fn elements(svg: &str) -> Vec<Element<'_>> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if tag.starts_with(['!', '/', '?']) || tag.starts_with("svg") {
            continue;
        }
        let tag = tag.trim_end_matches('/').trim();
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let (name, mut attrs_text) = tag.split_at(name_end);
        let mut attrs = Vec::new();
        while let Some(eq) = attrs_text.find("=\"") {
            let key = attrs_text[..eq].trim();
            let value_start = eq + 2;
            let Some(value_len) = attrs_text[value_start..].find('"') else { break };
            attrs.push((key, &attrs_text[value_start..value_start + value_len]));
            attrs_text = &attrs_text[value_start + value_len + 1..];
        }
        out.push(Element { name, attrs });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_lucide_icon_parses() {
        for (name, _) in ICONS {
            let shapes = shapes(name).unwrap_or_else(|| panic!("{name} does not parse"));
            assert!(!shapes.is_empty(), "{name}");
        }
    }

    #[test]
    fn icons_used_by_the_code_exist() {
        for n in [
            "bell", "repeat", "repeat-1", "shuffle", "sliders-horizontal", "volume-2", "keyboard", "sun", "camera", "cloud-sun",
            "smile", "fingerprint", "lock", "lock-open", "check", "app-window", "bell-off", "sun-moon", "coffee", "wifi", "wifi-off",
            "bluetooth", "bluetooth-off",
        ] {
            assert!(exists(n), "{n}");
        }
    }

    #[test]
    fn unknown_icon_is_none() {
        assert!(shapes("nope").is_none());
        assert!(!exists("nope"));
    }
}
