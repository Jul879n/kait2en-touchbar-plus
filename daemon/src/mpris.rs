// SPDX-License-Identifier: GPL-3.0-or-later

//! Media players over MPRIS: seeking in the active player (desktops treat
//! the rewind and fast forward keys inconsistently) and track changes for
//! the dark bar.

use std::{
    collections::HashMap,
    io::Write,
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, MessageIterator, connection::Builder};
use zbus::zvariant::{OwnedValue, Value};
use zbus::{MatchRule, message::Type};

use crate::events::{Bus, Ext};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// A hung player must not stall the Touch Bar.
const CALL_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct Mpris {
    connection: Option<Connection>,
}

impl Mpris {
    /// Picks the player a seek should go to: a playing one first, then a
    /// paused one, then any.
    pub fn active_player(&mut self) -> Option<String> {
        let connection = self.connection().ok()?;
        let names: Vec<String> = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "ListNames",
                &(),
            )
            .ok()?
            .body()
            .deserialize()
            .ok()?;
        names
            .into_iter()
            .filter(|name| name.starts_with(PREFIX))
            .map(|name| {
                let rank = match playback_status(connection, &name).as_deref() {
                    Some("Playing") => 0,
                    Some("Paused") => 1,
                    _ => 2,
                };
                (rank, name)
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, name)| name)
    }

    pub fn seek(&mut self, player: &str, offset: Duration, forward: bool) -> Result<()> {
        let micros = offset.as_micros() as i64;
        let micros = if forward { micros } else { -micros };
        self.connection()?
            .call_method(Some(player), PATH, Some(PLAYER), "Seek", &(micros,))?;
        Ok(())
    }

    /// Playback position in microseconds.
    pub fn position(&mut self, player: &str) -> Option<i64> {
        let value: OwnedValue = self
            .connection()
            .ok()?
            .call_method(
                Some(player),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(PLAYER, "Position"),
            )
            .ok()?
            .body()
            .deserialize()
            .ok()?;
        i64::try_from(value).ok()
    }

    pub fn set_position(&mut self, player: &str, track_id: &str, micros: i64) -> Result<()> {
        let track = zbus::zvariant::ObjectPath::try_from(track_id)?;
        self.connection()?.call_method(
            Some(player),
            PATH,
            Some(PLAYER),
            "SetPosition",
            &(track, micros),
        )?;
        Ok(())
    }

    pub fn play_pause(&mut self, player: &str) -> Result<()> {
        self.connection()?
            .call_method(Some(player), PATH, Some(PLAYER), "PlayPause", &())?;
        Ok(())
    }

    pub fn set_shuffle(&mut self, player: &str, on: bool) -> Result<()> {
        self.set_property(player, "Shuffle", Value::from(on))
    }

    /// Cycles repeat: off, playlist, track.
    pub fn set_repeat(&mut self, player: &str, repeat: u8) -> Result<()> {
        let name = ["None", "Playlist", "Track"][usize::from(repeat % 3)];
        self.set_property(player, "LoopStatus", Value::from(name))
    }

    fn set_property(&mut self, player: &str, name: &str, value: Value<'_>) -> Result<()> {
        self.connection()?.call_method(
            Some(player),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &(PLAYER, name, value),
        )?;
        Ok(())
    }

    fn connection(&mut self) -> Result<&Connection> {
        if self.connection.is_none() {
            self.connection = Some(Builder::session()?.method_timeout(CALL_TIMEOUT).build()?);
        }
        Ok(self
            .connection
            .as_ref()
            .expect("connection was just created"))
    }
}

fn player_property(connection: &Connection, name: &str, property: &str) -> Option<OwnedValue> {
    connection
        .call_method(
            Some(name),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(PLAYER, property),
        )
        .ok()?
        .body()
        .deserialize()
        .ok()
}

fn playback_status(connection: &Connection, name: &str) -> Option<String> {
    String::try_from(player_property(connection, name, "PlaybackStatus")?).ok()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Track {
    pub artist: String,
    pub title: String,
    pub length_us: Option<i64>,
    pub art_url: Option<String>,
    pub track_id: Option<String>,
}

/// The player the Touch Bar shows: the playing one, else the last paused.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    /// Bus name, for seeking and play/pause.
    pub player: String,
    pub playing: bool,
    pub track: Track,
    pub shuffle: bool,
    /// 0 off, 1 playlist, 2 track.
    pub repeat: u8,
}

fn loop_code(status: &str) -> u8 {
    match status {
        "Playlist" => 1,
        "Track" => 2,
        _ => 0,
    }
}

/// Reports a track once it is playing and differs from the last reported
/// one. Players repeat their metadata often (cover art, position updates),
/// so duplicates are dropped here.
pub fn watch_tracks(bus: Bus) -> Result<(Receiver<Track>, UnixStream)> {
    let gone: Gone = Arc::default();
    watch_owners(bus.clone(), gone.clone())?;
    let (sender, receiver) = mpsc::channel();
    let (mut wake_writer, wake_reader) = UnixStream::pair()?;
    thread::Builder::new()
        .name("mpris-tracks".to_owned())
        .spawn(move || {
            let mut reported = false;
            loop {
                if let Err(error) = listen_tracks(&sender, &mut wake_writer, &bus, &gone) {
                    if !reported {
                        eprintln!("kait2en-touchbar-plus: track listener: {error:#}");
                        reported = true;
                    }
                    thread::sleep(Duration::from_secs(30));
                }
            }
        })?;
    Ok((receiver, wake_reader))
}

#[derive(Default)]
struct PlayerState {
    /// Playing, Paused or Stopped.
    status: Option<String>,
    track: Option<Track>,
    shuffle: bool,
    repeat: u8,
}

/// Bus names that vanished, for the track listener to forget.
type Gone = Arc<Mutex<Vec<String>>>;

/// Reports players that leave the bus: a paused player that closes sends no
/// property change.
fn watch_owners(bus: Bus, gone: Gone) -> Result<()> {
    thread::Builder::new().name("mpris-owners".to_owned()).spawn(move || {
        loop {
            if let Err(error) = listen_owners(&bus, &gone) {
                eprintln!("kait2en-touchbar-plus: player owners: {error:#}");
            }
            thread::sleep(Duration::from_secs(30));
        }
    })?;
    Ok(())
}

fn listen_owners(bus: &Bus, gone: &Gone) -> Result<()> {
    let connection = Builder::session()?.method_timeout(CALL_TIMEOUT).build().context("connect session bus")?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns("org.mpris.MediaPlayer2")?
        .build();
    for message in MessageIterator::for_match_rule(rule, &connection, None)? {
        let Ok((_, old, new)) = message?.body().deserialize::<(String, String, String)>() else {
            continue;
        };
        if new.is_empty() && !old.is_empty() {
            gone.lock().unwrap_or_else(|e| e.into_inner()).push(old.clone());
            bus.send(Ext::PlayerGone(old));
        }
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}

fn listen_tracks(sender: &Sender<Track>, wake: &mut UnixStream, bus: &Bus, gone: &Gone) -> Result<()> {
    let connection = Builder::session()?
        .method_timeout(CALL_TIMEOUT)
        .build()
        .context("connect session bus")?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(PATH)?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .arg(0, PLAYER)?
        .build();
    let iterator = MessageIterator::for_match_rule(rule, &connection, None)?;
    let mut players: HashMap<String, PlayerState> = HashMap::new();
    let mut last: Option<Track> = None;
    let mut shown: Option<PlayerInfo> = None;
    for message in iterator {
        let message = message?;
        for name in std::mem::take(&mut *gone.lock().unwrap_or_else(|e| e.into_inner())) {
            players.remove(&name);
            if shown.as_ref().is_some_and(|info| info.player == name) {
                shown = None;
            }
        }
        let Some(name) = message.header().sender().map(ToString::to_string) else {
            continue;
        };
        let Ok((_, changed, _)) = message
            .body()
            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
        else {
            continue;
        };
        let known = players.contains_key(&name);
        let state = players.entry(name.clone()).or_default();
        if let Some(status) = changed.get("PlaybackStatus") {
            if let Value::Str(status) = &**status {
                state.status = Some(status.to_string());
            }
        } else if !known {
            state.status = playback_status(&connection, &name);
        }
        if let Some(metadata) = changed.get("Metadata") {
            state.track = parse_track(metadata);
        } else if !known {
            state.track = metadata_of(&connection, &name);
        }
        if let Some(Value::Bool(shuffle)) = changed.get("Shuffle").map(|value| &**value) {
            state.shuffle = *shuffle;
        } else if !known {
            state.shuffle = player_property(&connection, &name, "Shuffle")
                .and_then(|value| bool::try_from(value).ok())
                .unwrap_or(false);
        }
        if let Some(Value::Str(status)) = changed.get("LoopStatus").map(|value| &**value) {
            state.repeat = loop_code(status.as_str());
        } else if !known {
            state.repeat = player_property(&connection, &name, "LoopStatus")
                .and_then(|value| String::try_from(value).ok())
                .map_or(0, |status| loop_code(&status));
        }
        let playing = state.status.as_deref() == Some("Playing");
        let candidate = state
            .track
            .clone()
            .filter(|_| playing || state.status.as_deref() == Some("Paused"))
            .map(|track| PlayerInfo {
                player: name.clone(),
                playing,
                track,
                shuffle: state.shuffle,
                repeat: state.repeat,
            });
        let owned = shown.as_ref().is_some_and(|info| info.player == name);
        let next = match candidate {
            Some(info) if info.playing || owned || shown.is_none() => Some(Some(info)),
            None if owned => Some(None),
            _ => None,
        };
        if let Some(next) = next
            && shown != next
        {
            shown = next.clone();
            bus.send(Ext::Player(next));
        }
        let Some(track) = state.track.clone().filter(|_| playing) else {
            continue;
        };
        if last
            .as_ref()
            .is_some_and(|last| last.artist == track.artist && last.title == track.title)
        {
            continue;
        }
        last = Some(track.clone());
        if sender.send(track).is_err() {
            return Ok(());
        }
        let _ = wake.write_all(&[1]);
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}

fn parse_track(metadata: &OwnedValue) -> Option<Track> {
    let Value::Dict(dict) = unwrap_variant(metadata) else {
        return None;
    };
    let mut artist = String::new();
    let mut title = String::new();
    let (mut length_us, mut art_url, mut track_id) = (None, None, None);
    for (key, value) in dict.iter() {
        let Value::Str(key) = unwrap_variant(key) else {
            continue;
        };
        match (key.as_str(), unwrap_variant(value)) {
            ("xesam:title", Value::Str(value)) => title = value.to_string(),
            ("xesam:artist", Value::Array(values)) => {
                artist = values
                    .iter()
                    .filter_map(|value| match unwrap_variant(value) {
                        Value::Str(name) => Some(name.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            // Some players send a single string instead of the list.
            ("xesam:artist", Value::Str(value)) => artist = value.to_string(),
            ("mpris:length", Value::I64(value)) => length_us = Some(*value),
            ("mpris:length", Value::U64(value)) => length_us = i64::try_from(*value).ok(),
            ("mpris:artUrl", Value::Str(value)) if !value.is_empty() => {
                art_url = Some(value.to_string());
            }
            ("mpris:trackid", Value::ObjectPath(value)) => track_id = Some(value.to_string()),
            ("mpris:trackid", Value::Str(value)) => track_id = Some(value.to_string()),
            _ => {}
        }
    }
    let (artist, title) = (artist.trim().to_owned(), title.trim().to_owned());
    (!title.is_empty()).then_some(Track {
        artist,
        title,
        length_us,
        art_url,
        track_id,
    })
}

fn metadata_of(connection: &Connection, name: &str) -> Option<Track> {
    let value: OwnedValue = connection
        .call_method(
            Some(name),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(PLAYER, "Metadata"),
        )
        .ok()?
        .body()
        .deserialize()
        .ok()?;
    parse_track(&value)
}

/// Share of the track played, 0 to 1.
pub fn progress_fraction(position_us: i64, length_us: Option<i64>) -> f32 {
    match length_us {
        Some(length) if length > 0 => (position_us as f64 / length as f64).clamp(0.0, 1.0) as f32,
        _ => 0.0,
    }
}

fn unwrap_variant<'a>(value: &'a Value<'a>) -> &'a Value<'a> {
    match value {
        Value::Value(inner) => unwrap_variant(inner),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(entries: Vec<(&str, Value<'static>)>) -> OwnedValue {
        let map: HashMap<String, Value<'static>> = entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect();
        OwnedValue::try_from(Value::from(map)).unwrap()
    }

    #[test]
    fn artist_list_and_title_are_read() {
        let value = metadata(vec![
            ("xesam:title", Value::from("Duality")),
            (
                "xesam:artist",
                Value::from(vec!["Slipknot", "Corey Taylor"]),
            ),
            ("mpris:length", Value::from(252_000_000i64)),
        ]);
        assert_eq!(
            parse_track(&value),
            Some(Track {
                artist: "Slipknot, Corey Taylor".to_owned(),
                title: "Duality".to_owned(),
                length_us: Some(252_000_000),
                art_url: None,
                track_id: None,
            })
        );
    }

    #[test]
    fn parse_track_reads_length_art_and_trackid() {
        let value = metadata(vec![
            ("xesam:title", Value::from("Duality")),
            ("mpris:length", Value::from(252_000_000i64)),
            ("mpris:artUrl", Value::from("file:///cover.png")),
            (
                "mpris:trackid",
                Value::from(zbus::zvariant::ObjectPath::try_from("/org/track/1").unwrap()),
            ),
        ]);
        let track = parse_track(&value).unwrap();
        assert_eq!(track.length_us, Some(252_000_000));
        assert_eq!(track.art_url.as_deref(), Some("file:///cover.png"));
        assert_eq!(track.track_id.as_deref(), Some("/org/track/1"));
    }

    #[test]
    fn progress_fraction_clamps() {
        assert_eq!(progress_fraction(50, Some(100)), 0.5);
        assert_eq!(progress_fraction(500, Some(100)), 1.0);
        assert_eq!(progress_fraction(-5, Some(100)), 0.0);
        assert_eq!(progress_fraction(5, None), 0.0);
        assert_eq!(progress_fraction(5, Some(0)), 0.0);
    }

    #[test]
    fn missing_title_is_no_track() {
        let value = metadata(vec![("xesam:artist", Value::from("Heino"))]);
        assert_eq!(parse_track(&value), None);
    }
}
