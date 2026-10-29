// SPDX-License-Identifier: GPL-3.0-or-later

//! Icon files of installed applications, found through their `.desktop`
//! entry, to draw them in full color on the bar.

use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

const SIZES: [&str; 6] = ["64x64", "48x48", "96x96", "128x128", "256x256", "32x32"];

/// PNG of the icon for a desktop id such as `org.mozilla.firefox.desktop`.
pub fn icon_path(desktop_id: &str) -> Option<PathBuf> {
    icon_file(&icon_name(desktop_id)?)
}

/// The icon the sender of a notification means, from its `desktop-entry`
/// hint, its `app_icon` or, failing both, an app of that name.
pub fn notice_icon(app: &str, icon: &str) -> Option<PathBuf> {
    // Notifications come from any process: results (also misses) are kept so
    // the desktop entries are not scanned again for every message.
    static CACHE: Mutex<Option<HashMap<(String, String), Option<PathBuf>>>> = Mutex::new(None);
    let key = (app.to_owned(), icon.to_owned());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(found) = cache.get(&key) {
        return found.clone();
    }
    let found = find_notice_icon(app, icon);
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(key, found.clone());
    found
}

fn find_notice_icon(app: &str, icon: &str) -> Option<PathBuf> {
    let entry = |id: &str| {
        if !is_desktop_id(id) && !is_desktop_id(&format!("{id}.desktop")) {
            return None;
        }
        let id = if id.ends_with(".desktop") { id.to_owned() } else { format!("{id}.desktop") };
        icon_path(&id)
    };
    if !icon.is_empty() {
        if let Some(path) = entry(icon).or_else(|| untrusted_icon_file(icon)) {
            return Some(path);
        }
    }
    if app.is_empty() {
        return None;
    }
    let wanted = app.to_lowercase();
    desktop_dirs().iter().find_map(|dir| {
        fs::read_dir(dir).ok()?.flatten().find_map(|file| {
            let id = file.file_name().to_string_lossy().into_owned();
            let body = fs::read_to_string(file.path()).ok()?;
            let named = body
                .lines()
                .take_while(|line| !line.starts_with("[Desktop Action"))
                .find_map(|line| line.strip_prefix("Name="))
                .is_some_and(|name| name.trim().to_lowercase() == wanted);
            let by_id = id.trim_end_matches(".desktop").to_lowercase().rsplit('.').next() == Some(wanted.as_str());
            (named || by_id).then(|| icon_path(&id)).flatten()
        })
    })
}

/// Plain id of a `.desktop` file: no path parts, only safe characters.
pub fn is_desktop_id(id: &str) -> bool {
    id.strip_suffix(".desktop")
        .is_some_and(|stem| !stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
}

/// An icon named by a notification: a theme name, or a PNG/SVG of at most
/// 1 MiB inside an icon directory.
fn untrusted_icon_file(name: &str) -> Option<PathBuf> {
    if !name.starts_with('/') {
        let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
        return (plain && !name.contains("..")).then(|| icon_file(name)).flatten();
    }
    let path = fs::canonicalize(name).ok()?;
    let allowed = theme_dirs()
        .into_iter()
        .chain([PathBuf::from("/usr/share/pixmaps"), PathBuf::from("/usr/local/share/icons")])
        .filter_map(|dir| fs::canonicalize(dir).ok())
        .any(|dir| path.starts_with(dir));
    let kind = path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext == "png" || ext == "svg");
    let small = fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() <= 1 << 20);
    (allowed && kind && small).then_some(path)
}

fn icon_file(name: &str) -> Option<PathBuf> {
    let name = name.to_owned();
    if name.starts_with('/') {
        return Path::new(&name).exists().then(|| PathBuf::from(name));
    }
    theme_dirs()
        .into_iter()
        .flat_map(|base| {
            SIZES
                .iter()
                .map(|size| base.join(format!("hicolor/{size}/apps/{name}.png")))
                .collect::<Vec<_>>()
        })
        .chain([PathBuf::from(format!("/usr/share/pixmaps/{name}.png"))])
        .chain(theme_dirs().into_iter().map(|base| {
            base.join(format!("hicolor/scalable/apps/{name}.svg"))
        }))
        .find(|path| path.exists())
}

fn desktop_dirs() -> Vec<PathBuf> {
    vec![
        home().join(".local/share/applications"),
        home().join(".local/share/flatpak/exports/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/usr/share/applications"),
    ]
}

fn icon_name(desktop_id: &str) -> Option<String> {
    desktop_dirs().iter().find_map(|dir| {
        let body = fs::read_to_string(dir.join(desktop_id)).ok()?;
        let mut in_entry = false;
        for line in body.lines() {
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
            } else if in_entry {
                if let Some(value) = line.strip_prefix("Icon=") {
                    return Some(value.trim().to_owned());
                }
            }
        }
        None
    })
}

fn theme_dirs() -> Vec<PathBuf> {
    vec![
        home().join(".local/share/icons"),
        home().join(".local/share/flatpak/exports/share/icons"),
        PathBuf::from("/var/lib/flatpak/exports/share/icons"),
        PathBuf::from("/usr/share/icons"),
    ]
}

fn home() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// `size`x`size` RGBA pixels of the PNG at `path`.
pub fn load_rgba(path: &Path, size: u32) -> Option<Vec<u8>> {
    let image = if path.extension().is_some_and(|ext| ext == "svg") {
        // No SVG renderer is linked in: ImageMagick rasterizes it once.
        let output = std::process::Command::new("magick")
            .args(["-background", "none", "-density", "384"])
            .arg(path)
            .args(["-resize", &format!("{size}x{size}"), "png:-"])
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        image::load_from_memory(&output.stdout).ok()?
    } else {
        // Refuse huge images before decoding them.
        let (width, height) = image::image_dimensions(path).ok()?;
        if width > 1024 || height > 1024 {
            return None;
        }
        image::open(path).ok()?
    };
    // Keep the proportions and center the icon in the square.
    let scaled = image.resize(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut canvas = image::RgbaImage::new(size, size);
    image::imageops::overlay(
        &mut canvas,
        &scaled,
        i64::from((size - scaled.width()) / 2),
        i64::from((size - scaled.height()) / 2),
    );
    Some(canvas.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_file_rejects_paths_outside_icon_dirs() {
        assert_eq!(untrusted_icon_file("/etc/passwd"), None);
        assert_eq!(untrusted_icon_file("../x"), None);
        assert_eq!(untrusted_icon_file("a/b"), None);
        assert!(is_desktop_id("com.mitchellh.ghostty.desktop"));
        assert!(!is_desktop_id("/etc/x.desktop"));
    }
}
