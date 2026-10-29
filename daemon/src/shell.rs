// SPDX-License-Identifier: GPL-3.0-or-later

//! Link to the touchbar-plus GNOME Shell extension: focus, notifications and
//! input suggestions arrive as signals, windows and text go out as calls.

use std::{thread, time::Duration};

use anyhow::{Context, Result};
use zbus::blocking::{Connection, MessageIterator, Proxy, connection::Builder};
use zbus::{MatchRule, message::Type};

use crate::events::{Bus, Ext};

const NAME: &str = "org.kait2en.TouchbarPlus.Shell";
const PATH: &str = "/org/kait2en/TouchbarPlus/Shell";
const CALL_TIMEOUT: Duration = Duration::from_millis(200);

pub type WindowList = Vec<(u32, String, String)>;
/// Running apps, most recent first: desktop id, name and windows (id, title).
pub type AppList = Vec<(String, String, Vec<(u32, String)>)>;

pub fn watch(bus: Bus) -> Result<()> {
    thread::Builder::new()
        .name("shell-signals".to_owned())
        .spawn(move || {
            let mut reported = false;
            loop {
                if let Err(error) = listen(&bus) {
                    if !reported {
                        eprintln!("kait2en-touchbar-plus: shell extension link: {error:#}");
                        reported = true;
                    }
                }
                thread::sleep(Duration::from_secs(5));
            }
        })?;
    Ok(())
}

fn listen(bus: &Bus) -> Result<()> {
    let connection = Connection::session().context("connect session bus")?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(NAME)?
        .path(PATH)?
        .interface(NAME)?
        .build();
    let messages = MessageIterator::for_match_rule(rule, &connection, None)?;
    // The focus from before this daemon started: signals only report changes.
    if let Ok(reply) = connection.call_method(Some(NAME), PATH, Some(NAME), "GetFocus", &()) {
        if let Ok((app,)) = reply.body().deserialize::<(String,)>() {
            bus.send(Ext::Focus(app));
        }
    }
    for message in messages {
        let message = message?;
        let header = message.header();
        let body = message.body();
        match header.member().map(|member| member.as_str()) {
            Some("FocusChanged") => {
                let Ok((app,)) = body.deserialize::<(String,)>() else { continue };
                bus.send(Ext::Focus(app));
            }
            Some("Notification") => {
                let Ok((app, title, text)) = body.deserialize::<(String, String, String)>() else {
                    eprintln!("kait2en-touchbar-plus: unexpected Notification signal");
                    continue;
                };
                let icon = crate::apps::notice_icon(&app, "").map(|path| path.to_string_lossy().into_owned());
                bus.send(Ext::Notification { app, title, text, icon, replaces: false });
            }
            Some("Suggestions") => {
                let Ok((items,)) = body.deserialize::<(Vec<String>,)>() else { continue };
                bus.send(Ext::Suggestions(items));
            }
            Some("SuggestionsHidden") => bus.send(Ext::SuggestionsHidden),
            _ => {}
        }
    }
    Err(anyhow::anyhow!("session bus iterator ended"))
}

fn proxy() -> Result<Proxy<'static>> {
    let connection = Builder::session()?.method_timeout(CALL_TIMEOUT).build()?;
    Ok(Proxy::new(&connection, NAME, PATH, NAME)?)
}

/// Windows of the shell, asked on a worker so a missing extension never
/// stalls the bar. `None` reports that the extension did not answer.
pub fn list_windows(bus: Bus) {
    thread::spawn(move || {
        let windows = proxy()
            .and_then(|proxy| Ok(proxy.call::<_, _, (WindowList,)>("ListWindows", &())?))
            .map(|(windows,)| windows)
            .ok();
        bus.send(Ext::Windows(windows));
    });
}

/// Running apps in the order of the shell's app switcher.
pub fn list_apps(bus: Bus) {
    thread::spawn(move || {
        let apps = proxy()
            .and_then(|proxy| Ok(proxy.call::<_, _, (AppList,)>("ListApps", &())?))
            .map(|(apps,)| apps)
            .ok();
        bus.send(Ext::Apps(apps));
    });
}

pub fn activate_window(id: u32) {
    if let Err(error) = proxy().and_then(|p| Ok(p.call::<_, _, ()>("ActivateWindow", &(id,))?)) {
        eprintln!("kait2en-touchbar-plus: activate window: {error:#}");
    }
}

pub fn select_suggestion(index: u32) {
    if let Err(error) = proxy().and_then(|p| Ok(p.call::<_, _, ()>("SelectSuggestion", &(index,))?)) {
        eprintln!("kait2en-touchbar-plus: select suggestion: {error:#}");
    }
}

/// Puts `text` on the clipboard through the shell.
pub fn insert_text(text: &str) -> bool {
    match proxy().and_then(|p| Ok(p.call::<_, _, ()>("InsertText", &(text,))?)) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("kait2en-touchbar-plus: insert text: {error:#}");
            false
        }
    }
}
