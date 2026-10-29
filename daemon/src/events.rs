// SPDX-License-Identifier: GPL-3.0-or-later

//! One channel and one wake fd shared by every background watcher, so the
//! main poll loop needs a single extra descriptor however many sources exist.

use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
};

use anyhow::Result;

#[derive(Debug)]
pub enum Ext {
    /// org.gnome.ScreenSaver ActiveChanged.
    ScreenLocked(bool),
    /// Mutter PowerSaveMode: true while the monitors are on.
    ScreenPower(bool),
    /// Percent and whether the battery charges.
    Battery(u8, bool),
    Weather(crate::widgets::Weather),
    Toggles([bool; 5]),
    /// Focused app id from the shell extension; empty without a window.
    Focus(String),
    Notification {
        app: String,
        title: String,
        text: String,
        /// Icon file already resolved by the thread that heard it.
        icon: Option<String>,
        /// A newer version of a notice already shown (progress, track).
        replaces: bool,
    },
    Suggestions(Vec<String>),
    SuggestionsHidden,
    /// Open windows, or none when the extension did not answer.
    Windows(Option<crate::shell::WindowList>),
    /// Running apps for the app switcher, or none when the extension did not answer.
    Apps(Option<crate::shell::AppList>),
    /// The player shown in the media row, or none.
    Player(Option<crate::mpris::PlayerInfo>),
    /// The bus name of a player (its unique name) disappeared.
    PlayerGone(String),
    /// Cover for the URL, as 52x52 grayscale.
    Art(String, Vec<u8>),
    Events(Vec<crate::calendar::Event>),
    /// The dialog open in the focused window, or none.
    Dialog(Option<crate::atspi::Dialog>),
}

#[derive(Clone)]
pub struct Bus {
    sender: Sender<Ext>,
    wake: Arc<UnixStream>,
}

impl Bus {
    pub fn send(&self, event: Ext) {
        if self.sender.send(event).is_ok() {
            let _ = (&*self.wake).write(&[1]);
        }
    }
}

pub fn channel() -> Result<(Bus, Receiver<Ext>, UnixStream)> {
    let (sender, receiver) = mpsc::channel();
    let (writer, reader) = UnixStream::pair()?;
    writer.set_nonblocking(true)?;
    reader.set_nonblocking(true)?;
    Ok((
        Bus {
            sender,
            wake: Arc::new(writer),
        },
        receiver,
        reader,
    ))
}
