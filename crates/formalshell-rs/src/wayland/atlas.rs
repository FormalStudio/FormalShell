//! The radio overlay: the atlas's window and the two scrim
//! surfaces under it (the launcher's arrangement, on the Top layer), its
//! keys and pointer, and how it opens: `panel open|toggle radio`, the
//! launcher's Radio row and the media panel's Radio buttons.

use std::sync::Arc;
use std::time::Instant;

use fs_types::Fraction;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::wlr_layer::Layer;

use super::App;
use crate::services::radio::{self, Cmd, Reply};
use crate::store::Diff;
use crate::services::earth;
use crate::surfaces::atlas::view::Shown;
use crate::surfaces::atlas::{Atlas, Cursor, Hit, Key};
use crate::surfaces::modal::{Modal, Part};
use crate::surfaces::tooltip;
use crate::ui::What;

const NAMESPACE: &str = "formalshell:radio";

pub struct Window {
    pub shown: Shown,
    pressed: Option<crate::ui::On>,
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
        self.atlas.model.open()
    }

    pub fn atlas_toggle(&mut self) {
        if self.atlas.model.open() { self.atlas_close() } else { self.atlas_show() }
    }

    /// RadioOverlay.open(): whatever panel is current gives way.
    pub fn atlas_show(&mut self) {
        if self.atlas.model.open() {
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
                let job = ctx.pool().run(crate::services::countries::load);
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
        let names = ["radio", "radio-scrim-band", "radio-scrim"];
        let modal = Modal::open(&self.modal_make(), names, NAMESPACE, Layer::Top, None, Shown::DEFORM_AMOUNT, false);
        let shown = Shown::new(modal, self.output_size());
        self.atlas.win = Some(Window { shown, pressed: None, sliding: false });
        let r = self.store.media.radio.clone();
        self.atlas.model.opened(&r, now);
        self.atlas.seen = Some(r);
        crate::trace::mapped(crate::trace::Surface::Radio, crate::trace::State::Mapped);
    }

    pub fn atlas_close(&mut self) {
        if !self.atlas.model.open() {
            return;
        }
        self.atlas.model.closed();
        self.tips.hide(None, Instant::now());
        if let Some(w) = &mut self.atlas.win {
            w.shown.modal.close(Instant::now());
        }
        crate::trace::mapped(crate::trace::Surface::Radio, crate::trace::State::Closing);
        self.sync_join();
    }

    /// What landed in the store's atlas inbox, and the radio moving under
    /// an open atlas.
    pub(super) fn atlas_changed(&mut self, topic: crate::store::Topic) {
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
            Topic::Theme => self.atlas.model.redraw(),
            _ => {}
        }
    }

    pub fn atlas_deadline(&self) -> Option<Instant> {
        self.atlas.model.open().then(|| self.atlas.model.deadline()).flatten()
    }

    pub(super) fn present_atlas(&mut self, now: Instant) {
        if self.atlas.win.as_ref().is_some_and(|w| w.shown.modal.win.finished(now)) {
            self.atlas.win = None;
            self.atlas.model.unmapped();
            self.sync_join();
            crate::trace::mapped(crate::trace::Surface::Radio, crate::trace::State::Unmapped);
            return;
        }
        let Some(w) = &mut self.atlas.win else { return };
        let r = self.store.media.radio.clone();
        let m = &mut self.atlas.model;
        m.tick(&r, now);
        let qh = self.qh.clone();
        let theme = &self.store.theme.theme;
        let animating = w.shown.animating(now) || m.animating();
        if m.dirty() || animating || !w.shown.modal.win.surface.mapped() {
            w.shown.modal.sync_region(&self.compositor);
            w.shown.layout(m, &r, theme, &mut self.kit, now);
            m.drawn();
        }
        let animating = w.shown.animating(now) || m.animating();
        w.shown.modal.present(animating, now, &qh);
        if w.shown.modal.win.animating(now) {
            self.sync_join();
        }
    }

    pub(super) fn atlas_part(&self, surface: &WlSurface) -> Option<Part> {
        self.atlas.win.as_ref()?.shown.modal.part(surface)
    }

    pub(super) fn atlas_frame(&mut self, part: Part, now: Instant) {
        let Some(w) = &mut self.atlas.win else { return };
        if w.shown.modal.frame(part, now) {
            self.atlas.model.redraw();
            self.sync_join();
        }
    }

    pub(super) fn atlas_configure(&mut self, part: Part, width: i32, height: i32) {
        let Some(w) = &mut self.atlas.win else { return };
        w.shown.modal.configure(part, width, height);
        if part == Part::Card {
            self.atlas.model.redraw();
        }
    }

    pub(super) fn atlas_closed(&mut self) {
        self.atlas.win = None;
        self.atlas.model.closed();
        self.atlas.model.unmapped();
        self.sync_join();
    }

    /// One key while the atlas holds the keyboard.
    pub(super) fn atlas_key(&mut self, event: &KeyEvent) -> bool {
        if !self.atlas.model.open() || self.atlas.win.as_ref().is_none_or(|w| !w.shown.modal.open) {
            return false;
        }
        if self.atlas.model.editing() && !matches!(event.keysym, Keysym::Escape | Keysym::Up | Keysym::Down | Keysym::Return | Keysym::KP_Enter) {
            let r = self.store.media.radio.clone();
            self.atlas.model.edit(&r, crate::input::field_key(event.keysym), self.mods, event.utf8.as_deref(), Instant::now());
            return true;
        }
        let key = match event.keysym {
            Keysym::Escape => Key::Escape,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::space => Key::Space,
            Keysym::BackSpace => Key::Backspace,
            _ => match crate::input::printable(event.utf8.as_deref()) {
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
        if !self.atlas.model.open() {
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
        m.redraw();
        let on_globe = m.on_globe(x, y);
        let mut shape = None;
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let time = if let PointerEventKind::Motion { time } = e.kind { Some(time) } else { None };
                shape = Some(match m.hover(x, y, time, on_globe) {
                    Cursor::Default => Shape::Default,
                    Cursor::Grab => Shape::Grab,
                    Cursor::Grabbing => Shape::Grabbing,
                    Cursor::Pointer => Shape::Pointer,
                });
                if on_globe {
                    self.tips.hide(None, now);
                }
                if w.sliding
                    && let Some(h) = w.shown.hit(x, y).filter(|h| h.on.as_deref() == Some("volume")).or_else(|| w.shown.hit(x, y))
                {
                    let f = ((x - h.rect.x as f64) / h.rect.w.max(1) as f64).clamp(0.0, 1.0);
                    if h.on.as_deref() == Some("volume") {
                        radio::send(Cmd::SetVolume(radio::Volume::from_track(Fraction::from_fraction(f))));
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
                if let Some(&Hit::Row(i)) = hit.as_ref().and_then(|h| h.on.as_ref()).and_then(|on| on.get::<Hit>()) {
                    m.hover_row(&r, i);
                }
            }
            PointerEventKind::Leave { .. } => {
                self.tips.hide(None, now);
                m.leave();
                w.shown.set_hover(None);
            }
            PointerEventKind::Press { button, .. } => {
                if button != crate::input::BTN_LEFT {
                    return true;
                }
                if on_globe {
                    m.press_globe(x, y);
                    w.pressed = Some("globe".into());
                    shape = Some(Shape::Grabbing);
                } else {
                    let hit = w.shown.hit(x, y);
                    w.pressed = hit.as_ref().and_then(|h| h.on.clone()).or_else(|| (!w.shown.modal.win.on_card(x, y)).then(|| "outside".into()));
                    if let Some(h) = hit.filter(|h| h.on.as_deref() == Some("volume")) {
                        w.sliding = true;
                        let f = ((x - h.rect.x as f64) / h.rect.w.max(1) as f64).clamp(0.0, 1.0);
                        radio::send(Cmd::SetVolume(radio::Volume::from_track(Fraction::from_fraction(f))));
                    }
                }
            }
            PointerEventKind::Release { button, time, .. } => {
                if button != crate::input::BTN_LEFT {
                    return true;
                }
                w.sliding = false;
                let pressed = w.pressed.take();
                if pressed.as_deref() == Some("globe") {
                    m.release_globe(&r, x, y, time, now);
                    return true;
                }
                let hit = w.shown.hit(x, y);
                let on = hit.as_ref().and_then(|h| h.on.clone()).or_else(|| (!w.shown.modal.win.on_card(x, y)).then(|| "outside".into()));
                if pressed.is_none() || pressed != on {
                    return true;
                }
                let what = hit.as_ref().and_then(|h| crate::ui::Ui::event(h, x, crate::ui::Button::Left)).map(|e| e.what);
                if let Some(on) = on {
                    self.atlas_action(&on, what);
                }
            }
            PointerEventKind::Axis { vertical, .. } => {
                let notches = crate::scroll::notches(&vertical);
                if notches == 0.0 {
                    return true;
                }
                if on_globe {
                    m.zoom_globe(notches);
                } else if let Some(w) = &self.atlas.win
                    && w.shown.hit(x, y).and_then(|h| h.on).is_some_and(|on| matches!(on.get::<Hit>(), Some(Hit::Row(_) | Hit::Fav(_))))
                {
                    m.scroll_list(&r, vertical.absolute);
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

    fn atlas_action(&mut self, on: &crate::ui::On, what: Option<What>) {
        let r = self.store.media.radio.clone();
        if self.atlas.model.act(&r, on, what) {
            self.atlas_close();
        }
    }
}
