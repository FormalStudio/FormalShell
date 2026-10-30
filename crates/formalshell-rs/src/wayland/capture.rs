//! Window thumbnails over
//! ext-image-copy-capture-v1, each one a session on an
//! ext-image-capture-source-v1 made from the window's
//! ext-foreign-toplevel-list handle. Hyprland's toplevel-mapping protocol
//! names the window a handle belongs to; mango reports each client's list
//! identifier instead, matched against the handle's own. Either way a
//! thumbnail is asked for by the same opaque id every other surface carries.
//!
//! Only three owners ever hold one (the switcher and the Spaces preview, the
//! capture exception in the 2026-10-06 spec, and the launcher's preview
//! pane, launcher spec §7), and only while their card is up: dropping an
//! owner's set destroys every frame, session and source it held. The pane
//! holds one window's single frame, and only while it shows that window. A frame lands in a wl_shm buffer, is copied out and fitted to the
//! size the owner draws on the blocking pool, and comes back as a `Bitmap`.

use std::collections::HashMap;

use calloop::channel::{self, Event as ChannelEvent, Sender};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::foreign_toplevel_list::{ForeignToplevelList, ForeignToplevelListHandler};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_shm;
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle, WEnum};
use smithay_client_toolkit::reexports::protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1;
use smithay_client_toolkit::reexports::protocols::ext::image_capture_source::v1::client::ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1;
use smithay_client_toolkit::reexports::protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::ExtImageCaptureSourceV1;
use smithay_client_toolkit::reexports::protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_frame_v1::{
    self, ExtImageCopyCaptureFrameV1, FailureReason,
};
use smithay_client_toolkit::reexports::protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_manager_v1::{
    ExtImageCopyCaptureManagerV1, Options,
};
use smithay_client_toolkit::reexports::protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_session_v1::{
    self, ExtImageCopyCaptureSessionV1,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};

use super::App;
use crate::scene::Bitmap;
use crate::surface::Ignore;

pub mod mapping {
    #![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
    #![allow(non_upper_case_globals, non_snake_case, unused_imports)]
    #![allow(missing_docs, clippy::all)]

    use smithay_client_toolkit::reexports::client as wayland_client;
    use smithay_client_toolkit::reexports::client::backend as wayland_backend;
    use smithay_client_toolkit::reexports::client::protocol::*;
    use smithay_client_toolkit::reexports::protocols::ext::foreign_toplevel_list::v1::client::*;
    use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::*;

    pub mod __interfaces {
        use smithay_client_toolkit::reexports::client::backend as wayland_backend;
        use smithay_client_toolkit::reexports::client::protocol::__interfaces::*;
        use smithay_client_toolkit::reexports::protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
        use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/hyprland-toplevel-mapping-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/hyprland-toplevel-mapping-v1.xml");
}

use mapping::hyprland_toplevel_mapping_manager_v1::HyprlandToplevelMappingManagerV1;
use mapping::hyprland_toplevel_window_mapping_handle_v1::{self as window_mapping, HyprlandToplevelWindowMappingHandleV1};

/// Who holds a thumbnail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    Switcher,
    Preview,
    Launcher,
}

struct Thumb {
    owner: Owner,
    id: String,
    key: u64,
    /// The device pixels the owner draws it at, on a surface at `scale`.
    size: (u32, u32),
    scale: f64,
    live: bool,
    source: Option<ExtImageCaptureSourceV1>,
    session: Option<ExtImageCopyCaptureSessionV1>,
    /// The session's constraints as last announced: size and shm formats.
    pending: ((u32, u32), Vec<wl_shm::Format>),
    constraints: Option<((u32, u32), wl_shm::Format)>,
    buffer: Option<Buffer>,
    frame: Option<ExtImageCopyCaptureFrameV1>,
    want: bool,
    image: Option<Bitmap>,
}

impl Thumb {
    fn drop_capture(&mut self) {
        if let Some(f) = self.frame.take() {
            f.destroy();
        }
        if let Some(s) = self.session.take() {
            s.destroy();
        }
        if let Some(s) = self.source.take() {
            s.destroy();
        }
        self.buffer = None;
        self.constraints = None;
    }
}

impl Drop for Thumb {
    fn drop(&mut self) {
        self.drop_capture();
    }
}

/// A fitted frame back from the pool.
struct Landed {
    key: u64,
    image: Bitmap,
}

pub struct Capture {
    toplevels: ForeignToplevelList,
    mapping: Option<HyprlandToplevelMappingManagerV1>,
    sources: Option<ExtForeignToplevelImageCaptureSourceManagerV1>,
    copy: Option<ExtImageCopyCaptureManagerV1>,
    /// Each live handle and the window id the mapping named for it.
    handles: Vec<(ExtForeignToplevelHandleV1, Option<String>)>,
    thumbs: Vec<Thumb>,
    pool: Option<SlotPool>,
    landed: Option<Sender<Landed>>,
    next_key: u64,
}

impl Capture {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Self {
        Self {
            toplevels: ForeignToplevelList::new(globals, qh),
            mapping: globals.bind(qh, 1..=1, Ignore).ok(),
            sources: globals.bind(qh, 1..=1, Ignore).ok(),
            copy: globals.bind(qh, 1..=1, Ignore).ok(),
            handles: Vec::new(),
            thumbs: Vec::new(),
            pool: None,
            landed: None,
            next_key: 0,
        }
    }

    /// `toplevel` is the window's list identifier, "" when its backend
    /// reports none.
    fn handle_for(&self, id: &str, toplevel: &str) -> Option<&ExtForeignToplevelHandleV1> {
        let by_identifier = |h: &ExtForeignToplevelHandleV1| {
            !toplevel.is_empty() && self.toplevels.info(h).is_some_and(|info| info.identifier == toplevel)
        };
        self.handles.iter().find(|(h, w)| w.as_deref() == Some(id) || by_identifier(h)).map(|(h, _)| h)
    }

    fn thumb_mut(&mut self, key: u64) -> Option<&mut Thumb> {
        self.thumbs.iter_mut().find(|t| t.key == key)
    }
}

/// User data on a session and its frames: which thumbnail they serve.
pub struct ThumbKey(u64);

/// User data on a mapping request: the handle it asked about.
pub struct MapAsk(ExtForeignToplevelHandleV1);

impl App {
    /// The thumbnails `owner` draws: one per `(window id, logical size)` on
    /// a surface at `scale`, every other one it held dropped with its
    /// capture. `live` keeps frames coming; otherwise each takes one frame
    /// and waits for [`App::capture_refresh`].
    pub fn capture_set(&mut self, owner: Owner, wants: &[(String, (u32, u32))], live: bool, scale: f64) {
        self.thumbnails.thumbs.retain(|t| t.owner != owner || wants.iter().any(|(id, _)| *id == t.id));
        for (id, size) in wants {
            let size = (crate::scene::pixels_at(f64::from(size.0), scale), crate::scene::pixels_at(f64::from(size.1), scale));
            if let Some(t) = self.thumbnails.thumbs.iter_mut().find(|t| t.owner == owner && t.id == *id) {
                t.size = size;
                t.scale = scale;
                t.live = live;
                continue;
            }
            let key = self.thumbnails.next_key;
            self.thumbnails.next_key += 1;
            self.thumbnails.thumbs.push(Thumb {
                owner,
                id: id.clone(),
                key,
                size,
                scale,
                live,
                source: None,
                session: None,
                pending: ((0, 0), Vec::new()),
                constraints: None,
                buffer: None,
                frame: None,
                want: true,
                image: None,
            });
            self.capture_start(key);
        }
    }

    /// Drops everything `owner` holds.
    pub fn capture_clear(&mut self, owner: Owner) {
        self.capture_set(owner, &[], false, 1.0);
    }

    /// One more frame for a thumbnail that is not live.
    pub fn capture_refresh(&mut self, owner: Owner, id: &str) {
        let Some(key) = self.thumbnails.thumbs.iter().find(|t| t.owner == owner && t.id == id).map(|t| t.key) else { return };
        if let Some(t) = self.thumbnails.thumb_mut(key) {
            t.want = true;
        }
        self.capture_frame(key);
    }

    pub fn capture_image(&self, owner: Owner, id: &str) -> Option<&Bitmap> {
        self.thumbnails.thumbs.iter().find(|t| t.owner == owner && t.id == id).and_then(|t| t.image.as_ref())
    }

    /// How many of `owner`'s thumbnails hold a capture source.
    pub fn capture_sourced(&self, owner: Owner) -> usize {
        self.capture_sessions(owner).len()
    }

    /// The windows `owner` holds a capture session on.
    pub fn capture_sessions(&self, owner: Owner) -> Vec<&str> {
        self.thumbnails.thumbs.iter().filter(|t| t.owner == owner && t.session.is_some()).map(|t| t.id.as_str()).collect()
    }

    /// A source and a session for a thumbnail whose window has a handle
    /// mapped; one that has none yet waits for its mapping to land.
    fn capture_start(&mut self, key: u64) {
        let qh = self.qh.clone();
        let Some(id) = self.thumbnails.thumbs.iter().find(|t| t.key == key).filter(|t| t.session.is_none()).map(|t| t.id.clone()) else { return };
        let toplevel = self.store.compositor.window(&id).map(|w| w.toplevel.clone()).unwrap_or_default();
        let c = &mut self.thumbnails;
        let (Some(sources), Some(copy)) = (c.sources.clone(), c.copy.clone()) else { return };
        let Some(handle) = c.handle_for(&id, &toplevel).cloned() else { return };
        let source = sources.create_source(&handle, &qh, Ignore);
        let session = copy.create_session(&source, Options::empty(), &qh, ThumbKey(key));
        if let Some(t) = c.thumb_mut(key) {
            t.source = Some(source);
            t.session = Some(session);
        }
    }

    /// A frame into the thumbnail's buffer, once the session has said what
    /// buffer it takes and while one is wanted and none is in flight.
    fn capture_frame(&mut self, key: u64) {
        let qh = self.qh.clone();
        if self.thumbnails.pool.is_none() {
            self.thumbnails.pool = SlotPool::new(4096, &self.shm).ok();
        }
        let c = &mut self.thumbnails;
        let Some(pool) = c.pool.as_mut() else { return };
        let Some(t) = c.thumbs.iter_mut().find(|t| t.key == key) else { return };
        let (Some(session), Some(((w, h), format))) = (t.session.clone(), t.constraints) else { return };
        if !t.want || t.frame.is_some() {
            return;
        }
        if t.buffer.is_none() {
            t.buffer = pool.create_buffer(w as i32, h as i32, w as i32 * 4, format).ok().map(|(b, _)| b);
        }
        let Some(buffer) = &t.buffer else { return };
        let frame = session.create_frame(&qh, ThumbKey(key));
        frame.attach_buffer(buffer.wl_buffer());
        frame.damage_buffer(0, 0, w as i32, h as i32);
        frame.capture();
        t.frame = Some(frame);
    }

    /// The pixels a ready frame left in the buffer, fitted on the pool.
    fn capture_ready(&mut self, key: u64) {
        let c = &mut self.thumbnails;
        let Some(pool) = c.pool.as_mut() else { return };
        let Some(t) = c.thumbs.iter_mut().find(|t| t.key == key) else { return };
        if let Some(f) = t.frame.take() {
            f.destroy();
        }
        t.want = t.live;
        let (Some(buffer), Some(((w, h), format))) = (&t.buffer, t.constraints) else { return };
        let Some(canvas) = buffer.canvas(pool) else { return };
        let pixels = canvas[..(w * h * 4) as usize].to_vec();
        let opaque = format == wl_shm::Format::Xrgb8888;
        let (size, scale) = (t.size, t.scale);
        if c.landed.is_none() {
            let (tx, rx) = channel::channel::<Landed>();
            if let Some(handle) = &self.handle {
                let _ = handle.insert_source(rx, |event, _, app: &mut App| {
                    if let ChannelEvent::Msg(l) = event {
                        app.capture_landed(l);
                    }
                });
            }
            self.thumbnails.landed = Some(tx);
        }
        let Some(tx) = self.thumbnails.landed.clone() else { return };
        let job = move || {
            let image = fit(&pixels, (w, h), opaque, size).at(scale);
            let _ = tx.send(Landed { key, image });
        };
        match &self.runtime {
            Some(rt) => rt.pool().submit(job),
            None => job(),
        }
        if self.thumbnails.thumbs.iter().any(|t| t.key == key && t.live) {
            self.capture_frame(key);
        }
    }

    fn capture_landed(&mut self, l: Landed) {
        let Some(t) = self.thumbnails.thumb_mut(l.key) else { return };
        t.image = Some(l.image);
        let owner = t.owner;
        self.thumbs_changed(owner);
    }
}

/// `bgra` (wl_shm's ARGB8888 or XRGB8888 in memory, premultiplied) averaged
/// down to `to` by area, or sampled up to it.
fn fit(bgra: &[u8], from: (u32, u32), opaque: bool, to: (u32, u32)) -> Bitmap {
    let (sw, sh) = (from.0 as usize, from.1 as usize);
    let (tw, th) = (to.0.min(u16::MAX as u32) as usize, to.1.min(u16::MAX as u32) as usize);
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        let y0 = ty * sh / th;
        let y1 = ((ty + 1) * sh / th).max(y0 + 1).min(sh);
        for tx in 0..tw {
            let x0 = tx * sw / tw;
            let x1 = ((tx + 1) * sw / tw).max(x0 + 1).min(sw);
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                let row = &bgra[(y * sw + x0) * 4..(y * sw + x1) * 4];
                for px in row.chunks_exact(4) {
                    sum[0] += px[2] as u32;
                    sum[1] += px[1] as u32;
                    sum[2] += px[0] as u32;
                    sum[3] += if opaque { 255 } else { px[3] as u32 };
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = &mut out[(ty * tw + tx) * 4..][..4];
            for i in 0..4 {
                o[i] = ((sum[i] + n / 2) / n) as u8;
            }
            if opaque {
                o[3] = 255;
            }
        }
    }
    Bitmap::from_premultiplied(tw as u16, th as u16, out)
}

impl Dispatch2<ExtImageCopyCaptureSessionV1, App> for ThumbKey {
    fn event(&self, app: &mut App, _: &ExtImageCopyCaptureSessionV1, event: ext_image_copy_capture_session_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        let key = self.0;
        let Some(t) = app.thumbnails.thumb_mut(key) else { return };
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height } => t.pending.0 = (width, height),
            ext_image_copy_capture_session_v1::Event::ShmFormat { format: WEnum::Value(f) } => t.pending.1.push(f),
            ext_image_copy_capture_session_v1::Event::Done => {
                let formats = std::mem::take(&mut t.pending.1);
                let pick = [wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888].into_iter().find(|f| formats.contains(f));
                let next = pick.map(|f| (t.pending.0, f));
                if next != t.constraints {
                    t.buffer = None;
                }
                t.constraints = next;
                app.capture_frame(key);
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                t.drop_capture();
                let owner = t.owner;
                app.thumbs_changed(owner);
            }
            _ => {}
        }
    }
}

impl Dispatch2<ExtImageCopyCaptureFrameV1, App> for ThumbKey {
    fn event(&self, app: &mut App, frame: &ExtImageCopyCaptureFrameV1, event: ext_image_copy_capture_frame_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        let key = self.0;
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => app.capture_ready(key),
            ext_image_copy_capture_frame_v1::Event::Failed { reason } => {
                frame.destroy();
                let Some(t) = app.thumbnails.thumb_mut(key) else { return };
                t.frame = None;
                match reason {
                    // The session sends its new constraints and a `done`,
                    // which asks again.
                    WEnum::Value(FailureReason::BufferConstraints) => t.buffer = None,
                    WEnum::Value(FailureReason::Stopped) => {
                        t.drop_capture();
                        let owner = t.owner;
                        app.thumbs_changed(owner);
                    }
                    _ => t.want = false,
                }
            }
            _ => {}
        }
    }
}

impl Dispatch2<HyprlandToplevelWindowMappingHandleV1, App> for MapAsk {
    fn event(&self, app: &mut App, proxy: &HyprlandToplevelWindowMappingHandleV1, event: window_mapping::Event, _: &Connection, _: &QueueHandle<App>) {
        match event {
            window_mapping::Event::WindowAddress { address_hi, address } => {
                let id = format!("{:x}", ((address_hi as u64) << 32) | address as u64);
                if let Some(slot) = app.thumbnails.handles.iter_mut().find(|(h, _)| *h == self.0) {
                    slot.1 = Some(id.clone());
                }
                let waiting: Vec<u64> = app.thumbnails.thumbs.iter().filter(|t| t.id == id && t.session.is_none()).map(|t| t.key).collect();
                for key in waiting {
                    app.capture_start(key);
                }
            }
            window_mapping::Event::Failed => {}
        }
        proxy.destroy();
    }
}

impl ForeignToplevelListHandler for App {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelList {
        &mut self.thumbnails.toplevels
    }

    fn new_toplevel(&mut self, _: &Connection, qh: &QueueHandle<Self>, handle: ExtForeignToplevelHandleV1) {
        if let Some(m) = &self.thumbnails.mapping {
            m.get_window_for_toplevel(&handle, qh, MapAsk(handle.clone()));
        }
        self.thumbnails.handles.push((handle, None));
    }

    /// A handle's identifier lands with its first `done`, which is what a
    /// thumbnail matched by identifier was waiting for.
    fn update_toplevel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: ExtForeignToplevelHandleV1) {
        if self.thumbnails.mapping.is_some() {
            return;
        }
        let waiting: Vec<u64> = self.thumbnails.thumbs.iter().filter(|t| t.session.is_none()).map(|t| t.key).collect();
        for key in waiting {
            self.capture_start(key);
        }
    }

    fn toplevel_closed(&mut self, _: &Connection, _: &QueueHandle<Self>, handle: ExtForeignToplevelHandleV1) {
        self.thumbnails.handles.retain(|(h, _)| *h != handle);
    }
}

macro_rules! ignore {
    ($($iface:ty),*) => {$(
        impl Dispatch2<$iface, App> for Ignore {
            fn event(&self, _: &mut App, _: &$iface, _: <$iface as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
        }
    )*};
}

ignore!(
    HyprlandToplevelMappingManagerV1,
    ExtForeignToplevelImageCaptureSourceManagerV1,
    ExtImageCaptureSourceV1,
    ExtImageCopyCaptureManagerV1
);

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn fit_averages_a_block_down_to_one_pixel() {
        // Two pixels, BGRA: pure blue and pure red, opaque.
        let bgra = [255, 0, 0, 255, 0, 0, 255, 255];
        let b = fit(&bgra, (2, 1), true, (1, 1));
        let px = b.pixmap.data()[0];
        assert_eq!((px.r, px.g, px.b, px.a), (128, 0, 128, 255));
    }

    #[test]
    fn fit_samples_up() {
        let bgra = [10, 20, 30, 0];
        let b = fit(&bgra, (1, 1), true, (3, 2));
        assert_eq!(b.pixmap.width(), 3);
        assert!(b.pixmap.data().iter().all(|p| (p.r, p.g, p.b, p.a) == (30, 20, 10, 255)));
    }
}
