// SPDX-License-Identifier: GPL-3.0-or-later

//! Texts the bar draws itself, in Spanish or English.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lang {
    Es = 0,
    En = 1,
}

#[derive(Clone, Copy)]
pub enum Msg {
    NoEvents,
    Break,
    Work,
    CopyFailed,
    Unlock,
    WithTouchId,
    TryAgain,
    UsePassword,
    Unlocked,
}

static CURRENT: AtomicU8 = AtomicU8::new(Lang::Es as u8);

/// `auto` follows the locale (`en*` is English, anything else Spanish).
pub fn resolve(setting: &str, locale: &str) -> Lang {
    match setting {
        "en" => Lang::En,
        "es" => Lang::Es,
        _ if locale.starts_with("en") => Lang::En,
        _ => Lang::Es,
    }
}

pub fn set(setting: &str) {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    CURRENT.store(resolve(setting, &locale) as u8, Ordering::Relaxed);
}

pub fn t(msg: Msg) -> &'static str {
    let english = CURRENT.load(Ordering::Relaxed) == Lang::En as u8;
    match (msg, english) {
        (Msg::NoEvents, false) => "sin eventos",
        (Msg::NoEvents, true) => "no events",
        (Msg::Break, false) => "Descanso",
        (Msg::Break, true) => "Break",
        (Msg::Work, false) => "A trabajar",
        (Msg::Work, true) => "Back to work",
        (Msg::CopyFailed, false) => "No se pudo copiar",
        (Msg::CopyFailed, true) => "Could not copy",
        (Msg::Unlock, false) => "Desbloquear",
        (Msg::Unlock, true) => "Unlock",
        (Msg::WithTouchId, false) => " con Touch ID",
        (Msg::WithTouchId, true) => " with Touch ID",
        (Msg::TryAgain, false) => "Intentar de nuevo",
        (Msg::TryAgain, true) => "Try Again",
        (Msg::UsePassword, false) => "Usar contrase\u{f1}a",
        (Msg::UsePassword, true) => "Use Password",
        (Msg::Unlocked, false) => "Desbloqueado",
        (Msg::Unlocked, true) => "Unlocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_setting_and_locale() {
        assert_eq!(resolve("auto", "en_US.UTF-8"), Lang::En);
        assert_eq!(resolve("auto", "es_CL.UTF-8"), Lang::Es);
        assert_eq!(resolve("auto", ""), Lang::Es);
        assert_eq!(resolve("en", "es_CL.UTF-8"), Lang::En);
        assert_eq!(resolve("es", "en_US.UTF-8"), Lang::Es);
    }
}
