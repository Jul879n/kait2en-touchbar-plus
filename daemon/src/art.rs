// SPDX-License-Identifier: GPL-3.0-or-later

//! Album covers for the now playing widget, decoded off the render loop.

use std::{fs, process::Command, thread};

use image::imageops::FilterType;

use crate::events::{Bus, Ext};

pub const SIZE: u32 = 52;

pub fn load(url: String, bus: Bus) {
    thread::spawn(move || match fetch(&url) {
        Some(gray) => bus.send(Ext::Art(url, gray)),
        None => eprintln!("kait2en-touchbar-plus: no cover from {url}"),
    });
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    let bytes = if let Some(path) = url.strip_prefix("file://") {
        fs::read(percent_decode(path)).ok()?
    } else if url.starts_with("http://") || url.starts_with("https://") {
        // The service filesystem is read-only, so the cover goes through a pipe.
        let output = Command::new("curl")
            .args(["-sfL", "--max-time", "10", url])
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)?
    } else {
        return None;
    };
    let image = image::load_from_memory(&bytes).ok()?;
    Some(
        image
            .resize_to_fill(SIZE, SIZE, FilterType::Triangle)
            .to_luma8()
            .into_raw(),
    )
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(code) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(code);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_handles_spaces_and_plain_text() {
        assert_eq!(percent_decode("/a%20b/c.png"), "/a b/c.png");
        assert_eq!(percent_decode("/100%"), "/100%");
    }
}
