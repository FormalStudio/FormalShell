//! A command plugin of kind `overlay` (PluginOverlay.qml): a summoned modal
//! card (`surfaces::modal`) budding off the top line to the output's centre
//! over its scrim, holding the keyboard while open. It shows the plugin's
//! rows exactly as a panel plugin's card does (`surfaces::panel::plugin`);
//! Escape or a click outside closes it, Up and Down walk the rows, Enter,
//! Space or a click activates one. The plugin runs while the card is open,
//! unless its manifest keeps it loaded.

use std::time::Instant;

use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Layer, LayerSurface};
use vello_cpu::kurbo::Rect;

use super::App;
use crate::services::plugins;
use crate::store::Topic;
use crate::surfaces::bar::cell::Kit;
use crate::surfaces::modal::{Modal, Part};
use crate::surfaces::panel::plugin;
use crate::ui::{self, Size, Ui, w};

const NAMESPACE: &str = "formalshell:plugin-overlay";

/// Drawer.qml's default `deformAmount`, which PluginOverlay.qml leaves alone.
const DEFORM_AMOUNT: f64 = 0.15;

pub struct Window {
    pub(super) modal: Modal,
    ui: Ui,
    key: &'static str,
    id: String,
    cursor: Option<String>,
    /// The keyboard put the cursor there, so its ring draws.
    ring: bool,
    pressed: Option<String>,
    dirty: bool,
}

impl App {
    /// The `panel` name of the open overlay.
    pub fn overlay_open(&self) -> Option<&'static str> {
        self.plugin_overlay.as_ref().filter(|w| w.modal.open).map(|w| w.key)
    }

    pub fn overlay_toggle(&mut self, key: &'static str) {
        if self.overlay_open() == Some(key) {
            self.overlay_close();
        } else {
            self.overlay_show(key);
        }
    }

    /// Whatever panel is current gives way.
    pub fn overlay_show(&mut self, key: &'static str) {
        if self.overlay_open() == Some(key) {
            return;
        }
        self.overlay_close();
        self.close_panels();
        self.atlas_close();
        if self.launcher.open {
            self.menu_close();
        }
        let id = key.strip_prefix(plugin::PREFIX).unwrap_or(key).to_owned();
        let modal = self.new_modal(["plugin-overlay", "plugin-overlay-scrim-band", "plugin-overlay-scrim"], NAMESPACE, Layer::Top, DEFORM_AMOUNT);
        let mut ui = Ui::new(Some(modal.card.top_node()));
        ui.halo_owned = false;
        plugins::shown(&id, true);
        self.plugin_overlay = Some(Window { modal, ui, key, id, cursor: None, ring: false, pressed: None, dirty: true });
        self.log(&format!("panel {key} mapped"));
    }

    pub fn overlay_close(&mut self) {
        let Some(w) = self.plugin_overlay.as_mut().filter(|w| w.modal.open) else { return };
        w.modal.close(Instant::now());
        w.dirty = true;
        plugins::shown(&w.id, false);
        self.sync_join();
    }

    pub fn overlay_changed(&mut self, topic: Topic) {
        if matches!(topic, Topic::Plugins | Topic::PluginOutput | Topic::Theme) {
            if let Some(w) = &mut self.plugin_overlay {
                w.dirty = true;
            }
        }
    }

    fn overlay_part(&self, surface: &WlSurface) -> Option<Part> {
        self.plugin_overlay.as_ref()?.modal.part(surface)
    }

    pub(super) fn overlay_owns(&self, layer: &LayerSurface) -> bool {
        self.overlay_part(layer.wl_surface()).is_some()
    }

    pub(super) fn overlay_configure(&mut self, layer: &LayerSurface, width: i32, height: i32) {
        let Some(part) = self.overlay_part(layer.wl_surface()) else { return };
        let Some(w) = &mut self.plugin_overlay else { return };
        w.modal.configure(part, width, height);
        w.dirty = true;
    }

    pub(super) fn overlay_frame(&mut self, surface: &WlSurface) -> bool {
        let Some(part) = self.overlay_part(surface) else { return false };
        let Some(w) = &mut self.plugin_overlay else { return false };
        if w.modal.frame(part, Instant::now()) {
            w.dirty = true;
            self.sync_join();
        }
        true
    }

    pub(super) fn overlay_joins(&self) -> Vec<(fs_chrome::types::Edge, f64, f64, f64)> {
        self.plugin_overlay.as_ref().map_or_else(Vec::new, |w| w.modal.joins().collect())
    }

    fn overlay_activate(&mut self, stop: &str) {
        let Some(w) = &self.plugin_overlay else { return };
        plugin::activate(&self.store, &w.id, stop);
    }

    /// One key while the overlay holds the keyboard.
    pub(super) fn overlay_key(&mut self, event: &KeyEvent) -> bool {
        let Some(w) = self.plugin_overlay.as_mut().filter(|w| w.modal.open) else { return false };
        let stops = plugin::stops(self.store.plugins.runs.get(&w.id));
        let at = w.cursor.as_ref().and_then(|c| stops.iter().position(|s| s == c));
        let step = |delta: isize| match at {
            Some(i) => stops.get(i.saturating_add_signed(delta).min(stops.len().saturating_sub(1))).cloned(),
            None => stops.first().cloned(),
        };
        match event.keysym {
            Keysym::Escape => {
                self.overlay_close();
                return true;
            }
            Keysym::Down | Keysym::Tab => (w.cursor, w.ring) = (step(1), true),
            Keysym::Up => (w.cursor, w.ring) = (step(-1), true),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                if let Some(stop) = w.cursor.clone() {
                    self.overlay_activate(&stop);
                }
            }
            _ => {}
        }
        if let Some(w) = &mut self.plugin_overlay {
            w.dirty = true;
        }
        true
    }

    /// The pointer over the overlay's window. True when it took the event.
    pub(super) fn overlay_pointer(&mut self, e: &PointerEvent) -> bool {
        let Some(part) = self.overlay_part(&e.surface) else { return false };
        if part != Part::Card {
            return true;
        }
        let Some(w) = &mut self.plugin_overlay else { return true };
        let (x, y) = e.position;
        let hit = |w: &Window| w.ui.hit(x, y).and_then(|h| h.on.clone()).or_else(|| (!w.modal.on_card(x, y)).then(|| "outside".to_owned()));
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let path = w.ui.hit(x, y).map(|h| h.path.clone());
                if w.ui.hover != path {
                    w.ui.hover = path;
                    w.dirty = true;
                }
            }
            PointerEventKind::Leave { .. } => {
                if w.ui.hover.take().is_some() {
                    w.dirty = true;
                }
            }
            PointerEventKind::Press { button: 0x110, .. } => w.pressed = hit(w),
            PointerEventKind::Release { button: 0x110, .. } => {
                let pressed = w.pressed.take();
                let on = hit(w);
                if pressed.is_none() || pressed != on {
                    return true;
                }
                match on.as_deref() {
                    Some("outside") => self.overlay_close(),
                    Some(stop) => {
                        w.cursor = Some(stop.to_owned());
                        w.ring = false;
                        w.dirty = true;
                        self.overlay_activate(stop);
                    }
                    None => {}
                }
            }
            _ => {}
        }
        true
    }

    pub(super) fn present_overlay(&mut self, now: Instant) {
        if self.plugin_overlay.as_ref().is_some_and(|w| w.modal.finished(now)) {
            self.plugin_overlay = None;
            self.sync_join();
            self.log("panel plugin overlay unmapped");
            return;
        }
        let qh = self.qh.clone();
        let Some(w) = &mut self.plugin_overlay else { return };
        let animating = w.modal.animating(now);
        if (w.dirty || animating || !w.modal.surface.mapped) && w.modal.surface.configured {
            w.dirty = false;
            w.modal.sync_region(&self.compositor);
            let _raster = crate::scene::raster_at(w.modal.surface.scale());
            let run = self.store.plugins.runs.get(&w.id);
            let plugin = plugin::find(&self.store, w.key);
            let drawn = Self::lay_overlay(w, plugin, run, &self.store.theme.theme, &mut self.bar.kit, now);
            w.dirty = drawn;
        }
        let animating = w.modal.animating(now);
        w.modal.present(animating, now, &qh);
        if animating {
            self.sync_join();
        }
    }

    /// Lays the card out; true while something in it is still moving.
    fn lay_overlay(
        w: &mut Window,
        plugin: Option<&fs_chrome::plugins::Plugin>,
        run: Option<&plugins::Run>,
        theme: &fs_theme::theme::Theme,
        kit: &mut Kit,
        now: Instant,
    ) -> bool {
        let s = &theme.space;
        let (out_w, out_h) = (w.modal.card.scene.size.w, w.modal.card.scene.size.h);
        let inner = plugin::width(s, plugin);
        let name = plugin.map_or_else(|| w.id.clone(), |p| p.name.clone());
        let body = w::column(s.section_gap, vec![w::section_label(s, &name, None, false), plugin::body(s, run)]).width(Size::Px(inner));
        let (_, body_h) = ui::measure(&body, inner, theme, kit);
        let card_w = inner + s.panel_padding * 2.0;
        let card_h = body_h + s.panel_padding * 2.0;
        let x0 = ((out_w as f64 - card_w) / 2.0).round();
        let y0 = ((out_h as f64 - card_h) / 2.0).round();
        w.modal.place(Rect::new(x0, y0, x0 + card_w, y0 + card_h), now);
        // The content rides the card's own travel and fades in on its pose.
        let (frame, alpha) = w.modal.card.content;
        let clip = w.modal.card.content_clip();
        let top = w.modal.card.top_node();
        let scene = &mut w.modal.card.scene;
        let (x, y) = (frame.x as f64 + s.panel_padding, frame.y as f64 + s.panel_padding);
        w.ui.anchor = Some(top);
        (w.ui.cursor, w.ui.ring) = (w.cursor.clone(), w.ring);
        w.ui.draw(&body, Rect::new(x, y, x + inner, y + body_h), Some(clip), alpha, theme, kit, scene, now).animating
    }
}
