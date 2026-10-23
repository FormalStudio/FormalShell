//! The radio overlay: the atlas's window and the two scrim
//! surfaces under it (the launcher's arrangement, on the Top layer), its
//! keys and pointer, and how it opens: `panel open|toggle radio`, the
//! launcher's Radio row and the media panel's Radio buttons.

use std::sync::Arc;
use std::time::Instant;

use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::App;
use crate::services::radio::{self, Cmd, Reply};
use crate::store::Diff;
use crate::surface::{PixelSurface, Surface};
use crate::surfaces::atlas::earth;
use crate::surfaces::atlas::globe::{self, Out};
use crate::surfaces::atlas::view::Shown;
use crate::surfaces::atlas::{Atlas, Key};
use crate::surfaces::modal::Part;
use crate::surfaces::tooltip;
use crate::ui::What;

const NAMESPACE: &str = "formalshell:radio";

pub struct Window {
    pub shown: Shown,
    pressed: Option<String>,
    /// The volume track under a held button.
    sliding: bool,
}

#[derive(Default)]
pub struct State {
    pub model: Atlas,
    pub win: Option<Window>,
    /// The radio state the atlas last saw, for what moved.
    seen: Option<radio::State>,
}

impl App {
    pub fn atlas_open(&self) -> bool {
        self.atlas.model.open
    }

    pub fn atlas_toggle(&mut self) {
        if self.atlas.model.open { self.atlas_close() } else { self.atlas_show() }
    }

    /// RadioOverlay.open(): whatever panel is current gives way.
    pub fn atlas_show(&mut self) {
        if self.atlas.model.open {
            return;
        }
        self.close_panels();
        if self.launcher.open {
            self.menu_close();
        }
        let now = Instant::now();
        if self.atlas.model.want_countries()
            && let Some(rt) = &self.runtime
        {
            rt.service(|ctx| {
                let job = ctx.pool().run(globe::load);
                let publisher = ctx.publisher().clone();
                ctx.spawn(async move {
                    if let Some(c) = job.await {
                        publisher.publish(Diff::Atlas(Reply::Countries(Arc::new(c))));
                    }
                });
            });
        }
        if self.atlas.model.want_earth()
            && let Some(rt) = &self.runtime
        {
            rt.service(|ctx| {
                let (pool, publisher) = (ctx.pool().clone(), ctx.publisher().clone());
                let spawn: earth::Spawn = Box::new(move |job| pool.submit(job));
                let tiles = publisher.clone();
                let wake: earth::Wake = Box::new(move || tiles.publish(Diff::Atlas(Reply::Tiles)));
                let job = ctx.pool().run(move || earth::load(spawn, wake));
                let publisher = ctx.publisher().clone();
                ctx.spawn(async move {
                    if let Some(earth) = job.await {
                        publisher.publish(Diff::Atlas(Reply::Earth(earth.map(Arc::new))));
                    }
                });
            });
        }
        self.atlas.win = None;
        let modal = self.new_modal(["radio", "radio-scrim-band", "radio-scrim"], NAMESPACE, Layer::Top, Shown::DEFORM_AMOUNT);
        let shown = Shown::new(modal, self.output_size(), self.motion_scale);
        self.atlas.win = Some(Window { shown, pressed: None, sliding: false });
        let r = self.store.media.radio.clone();
        self.atlas.model.opened(&r, now);
        self.atlas.seen = Some(r);
        self.log("radio mapped");
    }

    pub fn atlas_close(&mut self) {
        if !self.atlas.model.open {
            return;
        }
        self.atlas.model.closed();
        self.tips.hide(None, Instant::now());
        if let Some(w) = &mut self.atlas.win {
            w.shown.modal.close(Instant::now());
        }
        self.log("radio closing");
        self.sync_join();
    }

    /// What landed in the store's atlas inbox, and the radio moving under
    /// an open atlas.
    pub fn atlas_changed(&mut self, topic: crate::store::Topic) {
        use crate::store::Topic;
        let now = Instant::now();
        match topic {
            Topic::Atlas => {
                let r = self.store.media.radio.clone();
                for reply in std::mem::take(&mut self.store.atlas) {
                    self.atlas.model.reply(&r, reply, now);
                }
            }
            Topic::Media => {
                let r = &self.store.media.radio;
                if self.atlas.seen.as_ref() != Some(r) {
                    if let Some(before) = self.atlas.seen.replace(r.clone()) {
                        let r = r.clone();
                        self.atlas.model.radio_changed(&before, &r);
                    }
                }
            }
            Topic::Theme => self.atlas.model.dirty = true,
            _ => {}
        }
    }

    pub fn atlas_deadline(&self) -> Option<Instant> {
        self.atlas.model.open.then(|| self.atlas.model.deadline()).flatten()
    }

    pub(super) fn present_atlas(&mut self, now: Instant) {
        if self.atlas.win.as_ref().is_some_and(|w| w.shown.modal.finished(now)) {
            self.atlas.win = None;
            self.atlas.model.globe.drop_earth();
            self.sync_join();
            self.log("radio unmapped");
            return;
        }
        let Some(w) = &mut self.atlas.win else { return };
        let r = self.store.media.radio.clone();
        let m = &mut self.atlas.model;
        m.tick(&r, now);
        m.globe.tick(now);
        m.sync_geo(&r);
        let qh = self.qh.clone();
        let theme = &self.store.theme.theme;
        let moving = m.globe.animating();
        let animating = w.shown.animating(now) || moving;
        if m.dirty || animating || !w.shown.modal.surface.mapped {
            w.shown.modal.sync_region(&self.compositor);
            let _raster = crate::scene::raster_at(w.shown.modal.surface.scale());
            w.shown.layout(m, &r, theme, &mut self.bar.kit, now);
            m.dirty = false;
        }
        let animating = w.shown.animating(now) || m.globe.animating();
        w.shown.modal.present(animating, now, &qh);
        if w.shown.modal.animating(now) {
            self.sync_join();
        }
    }

    pub(super) fn atlas_part(&self, surface: &WlSurface) -> Option<Part> {
        self.atlas.win.as_ref()?.shown.modal.part(surface)
    }

    pub(super) fn atlas_frame(&mut self, part: Part, now: Instant) {
        let Some(w) = &mut self.atlas.win else { return };
        if w.shown.modal.frame(part, now) {
            self.atlas.model.dirty = true;
            self.sync_join();
        }
    }

    pub(super) fn atlas_configure(&mut self, part: Part, width: i32, height: i32) {
        let Some(w) = &mut self.atlas.win else { return };
        w.shown.modal.configure(part, width, height);
        if part == Part::Card {
            self.atlas.model.dirty = true;
        }
    }

    pub(super) fn atlas_closed(&mut self) {
        self.atlas.win = None;
        self.atlas.model.closed();
        self.atlas.model.globe.drop_earth();
        self.sync_join();
    }

    pub(super) fn atlas_joins(&self) -> Vec<(fs_chrome::types::Edge, f64, f64, f64)> {
        self.atlas.win.as_ref().map_or_else(Vec::new, |w| w.shown.modal.joins().collect())
    }

    /// One key while the atlas holds the keyboard.
    pub(super) fn atlas_key(&mut self, event: &KeyEvent) -> bool {
        if !self.atlas.model.open || self.atlas.win.as_ref().is_none_or(|w| !w.shown.modal.open) {
            return false;
        }
        if self.atlas.model.editing && !matches!(event.keysym, Keysym::Escape | Keysym::Up | Keysym::Down | Keysym::Return | Keysym::KP_Enter) {
            let r = self.store.media.radio.clone();
            self.atlas.model.edit(&r, super::field_key(event.keysym), self.mods, event.utf8.as_deref(), Instant::now());
            return true;
        }
        let key = match event.keysym {
            Keysym::Escape => Key::Escape,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::space => Key::Space,
            Keysym::BackSpace => Key::Backspace,
            _ => match event.utf8.as_deref().filter(|t| !t.is_empty() && t.chars().all(|c| c as u32 >= 0x20 && c != '\u{7f}')) {
                Some(t) => Key::Text(t.to_owned()),
                None => return true,
            },
        };
        let r = self.store.media.radio.clone();
        let mut close = false;
        self.atlas.model.key(&r, key, &mut close);
        if close {
            self.atlas_close();
        }
        true
    }

    /// The clipboard's text into the search field, when it holds the keyboard.
    pub(super) fn atlas_paste(&mut self, text: &str) -> bool {
        if !self.atlas.model.open {
            return false;
        }
        let r = self.store.media.radio.clone();
        self.atlas.model.paste(&r, text, Instant::now())
    }

    /// The pointer over the atlas's window. True when it took the event.
    pub(super) fn atlas_pointer(&mut self, e: &PointerEvent) -> bool {
        let Some(Part::Card) = self.atlas_part(&e.surface) else {
            return self.atlas_part(&e.surface).is_some();
        };
        let now = Instant::now();
        let r = self.store.media.radio.clone();
        let (x, y) = e.position;
        let Some(w) = &mut self.atlas.win else { return true };
        let m = &mut self.atlas.model;
        m.dirty = true;
        let on_globe = !m.help && m.globe.contains(x, y);
        let mut shape = None;
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                if m.globe.grabbing() || on_globe {
                    let time = if let PointerEventKind::Motion { time } = e.kind { Some(time) } else { None };
                    if m.globe.motion(x, y, time) == Out::Interaction {
                        m.keyboard_selection = false;
                    }
                    m.keyboard_selection = false;
                    shape = Some(if m.globe.grabbing() {
                        Shape::Grabbing
                    } else if m.globe.over_station() {
                        Shape::Pointer
                    } else {
                        Shape::Grab
                    });
                } else {
                    m.globe.leave();
                    shape = Some(Shape::Default);
                }
                if on_globe {
                    self.tips.hide(None, now);
                }
                if w.sliding
                    && let Some(h) = w.shown.hit(x, y).filter(|h| h.on.as_deref() == Some("volume")).or_else(|| w.shown.hit(x, y))
                {
                    let f = ((x - h.rect.x as f64) / h.rect.w.max(1) as f64).clamp(0.0, 1.0);
                    if h.on.as_deref() == Some("volume") {
                        radio::send(Cmd::SetVolume(f * 100.0));
                    }
                }
                let hit = w.shown.hit(x, y);
                w.shown.set_hover(hit.as_ref().map(|h| h.path.clone()));
                if !on_globe {
                    let ask = hit.as_ref().and_then(|h| {
                        h.tip.clone().map(|text| tooltip::Ask { owner: format!("radio:{}:{}", h.rect.x, h.rect.y), text, rect: h.rect, bar: None })
                    });
                    match ask {
                        Some(ask) => self.tips.show(ask, now),
                        None => self.tips.hide(None, now),
                    }
                }
                if let Some(i) = hit.and_then(|h| h.on).and_then(|on| on.strip_prefix("row:").and_then(|i| i.parse::<usize>().ok())) {
                    if m.selected_index != Some(i) || m.keyboard_selection {
                        m.set_selection(&r, Some(i), false);
                    }
                }
            }
            PointerEventKind::Leave { .. } => {
                self.tips.hide(None, now);
                m.globe.leave();
                w.shown.set_hover(None);
            }
            PointerEventKind::Press { button, .. } => {
                if button != 0x110 {
                    return true;
                }
                if on_globe {
                    m.globe.press(x, y);
                    m.keyboard_selection = false;
                    m.editing = false;
                    w.pressed = Some("globe".into());
                    shape = Some(Shape::Grabbing);
                } else {
                    let hit = w.shown.hit(x, y);
                    w.pressed = hit.as_ref().and_then(|h| h.on.clone()).or_else(|| (!w.shown.modal.on_card(x, y)).then(|| "outside".into()));
                    if let Some(h) = hit.filter(|h| h.on.as_deref() == Some("volume")) {
                        w.sliding = true;
                        let f = ((x - h.rect.x as f64) / h.rect.w.max(1) as f64).clamp(0.0, 1.0);
                        radio::send(Cmd::SetVolume(f * 100.0));
                    }
                }
            }
            PointerEventKind::Release { button, time, .. } => {
                if button != 0x110 {
                    return true;
                }
                w.sliding = false;
                let pressed = w.pressed.take();
                if pressed.as_deref() == Some("globe") {
                    match m.globe.release(x, y, time, now) {
                        Out::Station(s) => m.activate_map_station(&r, &s),
                        Out::Country(code, name) => m.browse_country(&r, &code, &name),
                        _ => {}
                    }
                    return true;
                }
                let hit = w.shown.hit(x, y);
                let on = hit.as_ref().and_then(|h| h.on.clone()).or_else(|| (!w.shown.modal.on_card(x, y)).then(|| "outside".into()));
                if pressed.is_none() || pressed != on {
                    return true;
                }
                let what = hit.as_ref().and_then(|h| crate::ui::Ui::event(h, x)).map(|e| e.what);
                self.atlas_action(on.as_deref().unwrap_or(""), what);
            }
            PointerEventKind::Axis { vertical, .. } => {
                let notches = if vertical.discrete != 0 { vertical.discrete as f64 } else { vertical.absolute / 15.0 };
                if notches == 0.0 {
                    return true;
                }
                if on_globe {
                    m.globe.wheel(-notches * 120.0);
                    m.keyboard_selection = false;
                } else if let Some(w) = &self.atlas.win
                    && w.shown.hit(x, y).and_then(|h| h.on).is_some_and(|on| on.starts_with("row:") || on.starts_with("fav:"))
                {
                    let max = (m.display(&r).len() as f64 * m.row_h - m.list_h).max(0.0);
                    m.scroll = (m.scroll + vertical.absolute).clamp(0.0, max);
                }
            }
        }
        if let Some(shape) = shape
            && let (Some(device), Some((serial, _))) = (&self.cursor_device, self.cursor)
        {
            device.set_shape(serial, shape);
        }
        true
    }

    fn atlas_action(&mut self, on: &str, what: Option<What>) {
        let r = self.store.media.radio.clone();
        let m = &mut self.atlas.model;
        m.dirty = true;
        if on != "search" {
            m.editing = false;
        }
        if let Some(i) = on.strip_prefix("row:").and_then(|i| i.parse::<usize>().ok()) {
            m.set_selection(&r, Some(i), false);
            m.play_selected(&r);
            return;
        }
        if let Some(i) = on.strip_prefix("fav:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(s) = m.display(&r).get(i).cloned() {
                radio::send(Cmd::ToggleFavorite(s));
            }
            return;
        }
        if let Some(i) = on.strip_prefix("output:").and_then(|i| i.parse::<usize>().ok()) {
            let id = if i == 0 { String::new() } else { r.outputs.get(i - 1).map(|o| o.id.clone()).unwrap_or_default() };
            m.select_output(id);
            return;
        }
        match (on, what) {
            ("outside" | "close", _) => self.atlas_close(),
            ("search", _) => m.editing = true,
            ("shuffle", _) => m.tune_random(&r),
            ("help", _) => m.toggle_controls(),
            ("tab", Some(What::Pick(i))) => m.show_tab(&r, i),
            ("transport", Some(What::Pick(i))) => match i {
                0 => radio::send(Cmd::Previous),
                1 => m.play_pause(&r),
                2 => radio::send(Cmd::Next),
                _ => radio::send(Cmd::Stop),
            },
            ("outputs", _) => m.toggle_outputs(&r),
            ("mute", _) => radio::send(Cmd::ToggleMute),
            ("playing-fav", _) => {
                if let Some(s) = r.station.clone() {
                    radio::send(Cmd::ToggleFavorite(s));
                }
            }
            _ => {}
        }
    }
}
