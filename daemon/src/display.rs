// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2023 WhatAmISupposedToPutHere
//
// The DRM setup is derived from tiny-dfr's MIT-licensed display backend:
// https://github.com/AsahiLinux/tiny-dfr

use std::{
    fs::{File, OpenOptions},
    os::fd::{AsFd, BorrowedFd},
    path::Path,
};

use anyhow::{Context, Result, anyhow};
use drm::{
    ClientCapability, Device as DrmDevice,
    buffer::DrmFourcc,
    control::{
        AtomicCommitFlags, ClipRect, Device as ControlDevice, Mode, ResourceHandle, atomic,
        connector,
        dumbbuffer::{DumbBuffer, DumbMapping},
        framebuffer, property,
    },
};

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl ControlDevice for Card {}
impl DrmDevice for Card {}

impl Card {
    fn open(path: &Path) -> Result<Self> {
        Ok(Self(OpenOptions::new().read(true).write(true).open(path)?))
    }
}

pub struct Display {
    card: Card,
    mode: Mode,
    buffer: DumbBuffer,
    framebuffer: framebuffer::Handle,
    shift: (i16, i16),
}

/// Offsets (max. 1 px) walked to spread OLED wear; hit-testing is unaffected.
pub const SHIFT_CYCLE: [(i16, i16); 9] = [
    (0, 0),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// Index into the logical frame that lands on `(x, y)` once the picture is
/// moved by `(dx, dy)`; `None` is outside the frame, so black.
pub fn shift_source(x: usize, y: usize, dx: i16, dy: i16, width: usize, height: usize) -> Option<usize> {
    let sx = x as isize - dx as isize;
    let sy = y as isize - dy as isize;
    ((0..width as isize).contains(&sx) && (0..height as isize).contains(&sy))
        .then(|| sy as usize * width + sx as usize)
}

impl Drop for Display {
    fn drop(&mut self) {
        let _ = self.card.destroy_framebuffer(self.framebuffer);
        let _ = self.card.destroy_dumb_buffer(self.buffer);
    }
}

fn property_id<T: ResourceHandle>(card: &Card, handle: T, name: &str) -> Result<property::Handle> {
    let props = card.get_properties(handle)?;
    for id in props.as_props_and_values().0 {
        let info = card.get_property(*id)?;
        if info.name().to_str()? == name {
            return Ok(*id);
        }
    }
    Err(anyhow!("DRM property {name} not found"))
}

fn try_open(path: &Path) -> Result<Display> {
    let card = Card::open(path)?;
    card.set_client_capability(ClientCapability::UniversalPlanes, true)?;
    card.set_client_capability(ClientCapability::Atomic, true)?;
    card.acquire_master_lock()?;

    let resources = card.resource_handles()?;
    let connector = resources
        .connectors()
        .iter()
        .filter_map(|handle| card.get_connector(*handle, true).ok())
        .find(|info| info.state() == connector::State::Connected)
        .ok_or_else(|| anyhow!("no connected connector"))?;
    let mode = *connector
        .modes()
        .first()
        .ok_or_else(|| anyhow!("connector has no mode"))?;
    let (physical_width, physical_height) = mode.size();
    if physical_height / physical_width < 30 {
        return Err(anyhow!("connector is not Touch-Bar-shaped"));
    }

    let crtc = resources
        .crtcs()
        .first()
        .and_then(|handle| card.get_crtc(*handle).ok())
        .ok_or_else(|| anyhow!("no CRTC"))?;
    let plane = *card
        .plane_handles()?
        .first()
        .ok_or_else(|| anyhow!("no plane"))?;
    let buffer = card.create_dumb_buffer((64, physical_height.into()), DrmFourcc::Xrgb8888, 32)?;
    let framebuffer = card.add_framebuffer(&buffer, 24, 32)?;

    let mut request = atomic::AtomicModeReq::new();
    request.add_property(
        connector.handle(),
        property_id(&card, connector.handle(), "CRTC_ID")?,
        property::Value::CRTC(Some(crtc.handle())),
    );
    let mode_blob = card.create_property_blob(&mode)?;
    request.add_property(
        crtc.handle(),
        property_id(&card, crtc.handle(), "MODE_ID")?,
        mode_blob,
    );
    request.add_property(
        crtc.handle(),
        property_id(&card, crtc.handle(), "ACTIVE")?,
        property::Value::Boolean(true),
    );
    request.add_property(
        plane,
        property_id(&card, plane, "FB_ID")?,
        property::Value::Framebuffer(Some(framebuffer)),
    );
    request.add_property(
        plane,
        property_id(&card, plane, "CRTC_ID")?,
        property::Value::CRTC(Some(crtc.handle())),
    );
    for (name, value) in [
        ("SRC_X", 0),
        ("SRC_Y", 0),
        ("SRC_W", (physical_width as u64) << 16),
        ("SRC_H", (physical_height as u64) << 16),
    ] {
        request.add_property(
            plane,
            property_id(&card, plane, name)?,
            property::Value::UnsignedRange(value),
        );
    }
    for (name, value) in [("CRTC_X", 0), ("CRTC_Y", 0)] {
        request.add_property(
            plane,
            property_id(&card, plane, name)?,
            property::Value::SignedRange(value),
        );
    }
    for (name, value) in [
        ("CRTC_W", physical_width as u64),
        ("CRTC_H", physical_height as u64),
    ] {
        request.add_property(
            plane,
            property_id(&card, plane, name)?,
            property::Value::UnsignedRange(value),
        );
    }
    card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, request)?;
    Ok(Display {
        card,
        mode,
        buffer,
        framebuffer,
        shift: (0, 0),
    })
}

impl Display {
    pub fn open() -> Result<Self> {
        let path = crate::usb::touchbar_drm_card()
            .ok_or_else(|| anyhow!("Touch Bar DRM device is not attached"))?;
        try_open(&path).with_context(|| format!("open Touch Bar DRM device {}", path.display()))
    }

    /// Logical dimensions after applying the panel's 90-degree orientation.
    pub fn dimensions(&self) -> (u16, u16) {
        let (height, width) = self.mode.size();
        (width, height)
    }

    fn map(&mut self) -> Result<DumbMapping<'_>> {
        Ok(self.card.map_dumb_buffer(&mut self.buffer)?)
    }

    pub fn set_shift(&mut self, dx: i16, dy: i16) {
        self.shift = (dx, dy);
    }

    pub fn present(&mut self, logical: &[u32], width: u16, height: u16) -> Result<()> {
        anyhow::ensure!(
            logical.len() == width as usize * height as usize,
            "bad frame size"
        );
        let info = self.card.get_framebuffer(self.framebuffer)?;
        let stride = info.pitch() as usize;
        let (dx, dy) = self.shift;
        let mut mapping = self.map()?;
        let target = mapping.as_mut();
        target.fill(0);

        // t2bdrm exposes the panel rotated. This is the same transform as
        // tiny-dfr's cairo translate(height, 0) + rotate(90deg).
        for y in 0..height as usize {
            for x in 0..width as usize {
                let Some(source) =
                    shift_source(x, y, dx, dy, width as usize, height as usize)
                else {
                    continue;
                };
                let physical_x = height as usize - 1 - y;
                let physical_y = x;
                let offset = physical_y * stride + physical_x * 4;
                target[offset..offset + 4]
                    .copy_from_slice(&logical[source].to_le_bytes());
            }
        }
        drop(mapping);
        self.card
            .dirty_framebuffer(self.framebuffer, &[ClipRect::new(0, 0, height, width)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_source_edges() {
        assert_eq!(shift_source(5, 5, 0, 0, 10, 10), Some(55));
        assert_eq!(shift_source(0, 0, 1, 0, 10, 10), None);
        assert_eq!(shift_source(0, 3, -1, 0, 10, 10), Some(31));
        assert_eq!(shift_source(9, 9, -1, 0, 10, 10), None);
        assert_eq!(shift_source(4, 0, 0, 1, 10, 10), None);
        assert_eq!(shift_source(4, 9, 0, -1, 10, 10), None);
        assert_eq!(shift_source(1, 1, 1, 1, 10, 10), Some(0));
    }
}
