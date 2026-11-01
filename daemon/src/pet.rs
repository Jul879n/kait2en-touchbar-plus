// SPDX-License-Identifier: GPL-3.0-or-later

//! Trazko pet (`cl.trazko.Mascota`): it publishes its current frame and balloon
//! as signals; a tap on the item calls `Tocar`, which opens its chat.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use image::imageops::FilterType;
use zbus::blocking::{Connection, MessageIterator, Proxy, connection::Builder};
use zbus::{MatchRule, message::Type};

use crate::events::{Bus, Ext};

const NAME: &str = "cl.trazko.Mascota";
const PATH: &str = "/cl/trazko/Mascota";
const CALL_TIMEOUT: Duration = Duration::from_millis(200);
/// Height of the pet on the bar, in pixels.
pub const HEIGHT: u32 = 60;

/// A frame of the pet already scaled to the bar, as straight RGBA.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// Scales a PNG to `HEIGHT` keeping its proportions.
pub fn scale(png: &[u8]) -> Option<Sprite> {
    let image = image::load_from_memory(png).ok()?;
    let (src_w, src_h) = (image.width(), image.height());
    if src_w == 0 || src_h == 0 || src_w > 1024 || src_h > 1024 {
        return None;
    }
    let width = ((src_w * HEIGHT + src_h / 2) / src_h).max(1);
    let rgba = image.resize_exact(width, HEIGHT, FilterType::Triangle).to_rgba8();
    Some(Sprite { width, height: HEIGHT, rgba: rgba.into_raw().into() })
}

/// Scaled frames by name: the pet repeats a few dozen poses.
fn sprite(name: &str, png: &[u8]) -> Option<Sprite> {
    static CACHE: Mutex<Option<HashMap<String, Sprite>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(found) = cache.get(name) {
        return Some(found.clone());
    }
    let found = scale(png)?;
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert(name.to_owned(), found.clone());
    Some(found)
}

/// Follows the pet: its signals, and its appearing or disappearing on the bus.
/// Always on, since the app can add the item later; without the pet it costs
/// two idle match rules.
pub fn watch(bus: Bus) -> Result<()> {
    for (name, run) in [("pet-signals", listen as fn(&Bus) -> Result<()>), ("pet-owner", owners)] {
        let bus = bus.clone();
        thread::Builder::new().name(name.to_owned()).spawn(move || loop {
            if let Err(error) = run(&bus) {
                eprintln!("kait2en-touchbar-plus: pet link: {error:#}");
            }
            thread::sleep(Duration::from_secs(5));
        })?;
    }
    Ok(())
}

fn listen(bus: &Bus) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    let rule = MatchRule::builder().msg_type(Type::Signal).sender(NAME)?.path(PATH)?.interface(NAME)?.build();
    let messages = MessageIterator::for_match_rule(rule, &connection, None)?;
    send_state(&connection, bus);
    for message in messages {
        let message = message?;
        let header = message.header();
        let body = message.body();
        match header.member().map(|member| member.as_str()) {
            Some("Cuadro") => {
                let Ok((name, png)) = body.deserialize::<(String, Vec<u8>)>() else { continue };
                if let Some(sprite) = sprite(&name, &png) {
                    bus.send(Ext::PetFrame(sprite));
                }
            }
            Some("Globo") => {
                let Ok((text,)) = body.deserialize::<(String,)>() else { continue };
                bus.send(Ext::PetText(text));
            }
            _ => {}
        }
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}

fn owners(bus: &Bus) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns(NAME)?
        .build();
    for message in MessageIterator::for_match_rule(rule, &connection, None)? {
        let Ok((name, _, new)) = message?.body().deserialize::<(String, String, String)>() else {
            continue;
        };
        if name != NAME {
            continue;
        }
        if new.is_empty() {
            bus.send(Ext::PetGone);
        } else {
            send_state(&connection, bus);
        }
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}

/// The pet as it is now, for when it starts after the daemon or the daemon after it.
/// Without the pet the call fails, which is normal and not reported.
fn send_state(connection: &Connection, bus: &Bus) {
    let state = connection
        .call_method(Some(NAME), PATH, Some(NAME), "Actual", &())
        .ok()
        .and_then(|reply| reply.body().deserialize::<(String, Vec<u8>, String)>().ok())
        .and_then(|(name, png, text)| Some((sprite(&name, &png)?, text)));
    match state {
        Some((sprite, text)) => {
            bus.send(Ext::PetFrame(sprite));
            bus.send(Ext::PetText(text));
        }
        None => bus.send(Ext::PetGone),
    }
}

/// Asks the pet to open its chat.
pub fn touch() {
    let result = Builder::session()
        .and_then(|builder| builder.method_timeout(CALL_TIMEOUT).build())
        .and_then(|connection| Proxy::new(&connection, NAME, PATH, NAME))
        .and_then(|proxy| proxy.call::<_, _, ()>("Tocar", &()));
    if let Err(error) = result {
        eprintln!("kait2en-touchbar-plus: pet touch: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_fits_height() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(51, 64).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let sprite = scale(bytes.get_ref()).unwrap();
        assert_eq!((sprite.width, sprite.height), (48, 60));
        assert_eq!(sprite.rgba.len(), 48 * 60 * 4);
    }
}
