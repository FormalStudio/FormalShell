//! The headset connect card (new in the Rust shell, after macOS's AirPods
//! popup): when a Bluetooth audio device goes from disconnected to
//! connected, a card buds off the bar's line in the joined-shape motion
//! with the device's icon, its name over "Connected" and one battery ring
//! per part that reports a level. It never fires for a device already
//! connected when the shell started or one that reconnects inside
//! `RECONNECT`, holds `HOLD` while the pointer is off it, dismisses on the
//! pointer leaving it or on Escape, opens the earbuds or Bluetooth panel on
//! a click, and stays down under fullscreen and do-not-disturb.
//!
//! The card never takes the keyboard: it is unasked for, and focus would
//! take typing from the window under it. Escape is a non-consuming
//! Hyprland bind, added while the card is up and removed with it, so the
//! focused app still gets the key.
//!
//! While the card is up it holds the earbuds service, which is what makes a
//! backend report its parts; the hold ends with the card, and so does the
//! bind.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use fs_chrome::types::Edge;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Layer;

use super::{App, Owner};
use crate::motion::{Animated, SPATIAL};
use crate::services::devices::earbuds;
use crate::services::compositor;
use crate::surfaces::headset;
use crate::surfaces::popup::Popup;

const HOLD: Duration = Duration::from_secs(4);
const RECONNECT: Duration = Duration::from_secs(3);
/// The Lua global holding the Escape bind.
const ESCAPE: &str = "formalshell_headset_escape";

struct Known {
    connected: bool,
    lost_at: Option<Instant>,
}

pub(super) struct Shown {
    popup: Popup,
    address: String,
    /// The hold's end; none while the pointer is on the card.
    hold_until: Option<Instant>,
    entered: bool,
    pressed: bool,
    /// The earbuds service and the Escape bind are held on this card's
    /// account.
    held: bool,
    /// The card's width and height, on their way to the face's.
    size: (Animated, Animated),
}

/// A card's size runs along its line then away from it, so on a
/// sidebar's vertical line its height is the along.
fn oriented(vertical: bool, (w, h): (f64, f64)) -> (f64, f64) {
    if vertical { (h, w) } else { (w, h) }
}

/// What the Escape bind runs.
fn dismiss_argv() -> Vec<String> {
    [crate::services::menu::ipc_client(), "call".into(), "headset".into(), "dismiss".into()].into()
}

#[derive(Default)]
pub struct Headset {
    pub(super) card: Option<Shown>,
    known: HashMap<String, Known>,
    /// The first Bluetooth reading has been seen: what it holds connected
    /// was connected at startup.
    primed: bool,
}

impl Headset {
    pub fn wake(&self) -> Option<Instant> {
        self.card.as_ref().and_then(|c| c.hold_until)
    }

    /// The gaps this card opens in the lines, for the bar to take.
    pub fn joins(&self) -> Vec<(Edge, f64, f64, f64)> {
        self.card.iter().flat_map(|c| c.popup.card.joins.iter().map(|j| (j.edge, j.x, j.width, j.reach))).collect()
    }
}

impl Shown {
    fn hold(&mut self) {
        if !std::mem::replace(&mut self.held, true) {
            earbuds::acquire();
            compositor::escape_bind(ESCAPE, Some(&dismiss_argv()));
        }
    }

    fn release(&mut self) {
        if std::mem::take(&mut self.held) {
            earbuds::release();
            compositor::escape_bind(ESCAPE, None);
        }
    }
}

impl Drop for Shown {
    fn drop(&mut self) {
        self.release();
    }
}

impl App {
    /// The Bluetooth audio devices moved: a disconnected one that is now
    /// connected earns a card.
    pub fn headset_devices(&mut self) {
        let Some(list) = self.store.devices.bluetooth.headsets.clone() else { return };
        let now = Instant::now();
        let first = !std::mem::replace(&mut self.headset.primed, true);
        let mut show = None;
        let mut seen = HashSet::new();
        for h in &list {
            seen.insert(h.address.clone());
            let k = self.headset.known.entry(h.address.clone()).or_insert(Known { connected: false, lost_at: None });
            if h.connected && !k.connected && !first && !k.lost_at.is_some_and(|t| now.duration_since(t) < RECONNECT) {
                show = Some(h.address.clone());
            }
            if !h.connected && k.connected {
                k.lost_at = Some(now);
            }
            k.connected = h.connected;
        }
        for (address, k) in &mut self.headset.known {
            if !seen.contains(address) && k.connected {
                k.connected = false;
                k.lost_at = Some(now);
            }
        }
        let gone = self.headset.card.as_ref().is_some_and(|c| self.headset.known.get(&c.address).is_none_or(|k| !k.connected));
        if gone {
            self.headset_dismiss(now);
        }
        if let Some(address) = show {
            self.headset_show(&address, now);
        }
    }

    fn headset_down(&self) -> bool {
        self.store.state.data.dnd || self.chrome_hidden()
    }

    fn headset_show(&mut self, address: &str, now: Instant) {
        if self.headset_down() {
            return;
        }
        if let Some(c) = &mut self.headset.card {
            c.address = address.to_owned();
            c.hold_until = (!c.entered).then_some(now + HOLD);
            if !c.popup.is_open() {
                c.popup.set_open(now, true);
            }
            c.hold();
            return;
        }
        let theme = &self.store.theme.theme;
        let face = headset::face(&self.store, address);
        let (width, height) = headset::size(&face, theme, &mut self.bar.kit);
        let room = headset::largest(&face, theme, &mut self.bar.kit);
        let pad = theme.space.screen_padding;
        let anchor = self.bar.length() as f64 / 2.0;
        let vertical = self.bar.edge().is_vertical();
        let mut card = self.new_card(anchor, oriented(vertical, room));
        card.resize(now, anchor, self.bar.length(), pad, oriented(vertical, (width, height)));
        card.ends = self.panel_place(None, false).ends;
        let surface = self.popout_surface(&card, Layer::Top);
        let mut popup = Popup::new(surface, card);
        popup.set_open(now, true);
        let mut shown = Shown {
            popup,
            address: address.to_owned(),
            hold_until: Some(now + HOLD),
            entered: false,
            pressed: false,
            held: false,
            size: (Animated::new(width, SPATIAL), Animated::new(height, SPATIAL)),
        };
        shown.hold();
        self.headset.card = Some(shown);
        self.log("headset card mapped");
    }

    /// The card on its way to the size its face asks for: the earbuds
    /// backend's picture and rings land after it is up.
    fn headset_resize(&mut self, face: &headset::Face, now: Instant) {
        let vertical = self.bar.edge().is_vertical();
        let theme = &self.store.theme.theme;
        let (pad, length, anchor) = (theme.space.screen_padding, self.bar.length(), self.bar.length() as f64 / 2.0);
        let (w, h) = headset::size(face, theme, &mut self.bar.kit);
        let ms = crate::motion::Kind::Spatial.ms(theme) * self.motion_scale;
        let Some(c) = &mut self.headset.card else { return };
        let moved = w != c.size.0.target() || h != c.size.1.target();
        if moved {
            c.size.0.set(now, w, ms);
            c.size.1.set(now, h, ms);
        }
        if moved || c.size.0.running(now) || c.size.1.running(now) {
            let size = (c.size.0.value(now), c.size.1.value(now));
            c.popup.card.resize(now, anchor, length, pad, oriented(vertical, size));
        }
    }

    fn headset_dismiss(&mut self, now: Instant) {
        if let Some(c) = &mut self.headset.card {
            c.hold_until = None;
            c.release();
            if c.popup.is_open() {
                c.popup.set_open(now, false);
            }
        }
    }

    pub(super) fn headset_frame(&mut self, now: Instant) {
        if let Some(c) = &mut self.headset.card {
            c.popup.frame(now);
        }
        self.sync_join();
    }

    pub(super) fn headset_configure(&mut self) {
        if let Some(c) = &mut self.headset.card {
            c.popup.configure();
        }
    }

    /// The pointer on the card: entering holds it, leaving lets it go, a
    /// press and release on it opens the device's panel.
    pub(super) fn headset_pointer(&mut self, owner: Option<Owner>, kind: &PointerEventKind) {
        if owner != Some(Owner::Headset) {
            return;
        }
        let now = Instant::now();
        let Some(c) = &mut self.headset.card else { return };
        match kind {
            PointerEventKind::Enter { .. } => {
                c.entered = true;
                c.hold_until = None;
            }
            PointerEventKind::Leave { .. } => {
                c.pressed = false;
                if c.entered {
                    self.headset_dismiss(now);
                }
            }
            PointerEventKind::Press { .. } => c.pressed = true,
            PointerEventKind::Release { .. } => {
                if std::mem::take(&mut c.pressed) && c.popup.is_open() {
                    let panel = headset::face(&self.store, &c.address).panel;
                    let anchor = self.bar.length() as f64 / 2.0;
                    self.headset_dismiss(now);
                    self.set_panel(panel, true, Some(anchor));
                }
            }
            _ => {}
        }
    }

    /// `headset dismiss`, what the Escape bind runs: true when a card was up.
    pub fn headset_escape(&mut self) -> bool {
        let open = self.headset.card.as_ref().is_some_and(|c| c.popup.is_open());
        self.headset_dismiss(Instant::now());
        open
    }

    pub(super) fn headset_present(&mut self, now: Instant) {
        let down = self.headset_down();
        let Some(c) = &mut self.headset.card else { return };
        if c.popup.is_open() && (down || c.hold_until.is_some_and(|t| t <= now)) {
            self.headset_dismiss(now);
        }
        let Some(address) = self.headset.card.as_ref().map(|c| c.address.clone()) else { return };
        let face = headset::face(&self.store, &address);
        self.headset_resize(&face, now);
        let Some(c) = &mut self.headset.card else { return };
        if c.popup.finished(now) {
            self.headset.card = None;
            self.sync_join();
            self.log("headset card unmapped");
            return;
        }
        let open = c.popup.is_open();
        c.popup.sync_region(&self.compositor, open);
        let theme = &self.store.theme.theme;
        let inner = c.popup.inner(theme);
        let (el, rect) = headset::left(&face, inner, theme, &mut self.bar.kit);
        let drawn = c.popup.draw_in(&el, rect, theme, &mut self.bar.kit, now);
        {
            let (mut painter, alpha) = c.popup.painter();
            headset::paint_rings(&mut painter, alpha, &face, inner, theme, &mut self.bar.kit);
            painter.finish();
        }
        let animating = c.popup.animating(now) || drawn.animating || c.size.0.running(now) || c.size.1.running(now);
        let qh = self.qh.clone();
        c.popup.present(animating, &qh);
    }

    pub(super) fn headset_owns(&self, surface: &wl_surface::WlSurface) -> bool {
        self.headset.card.as_ref().is_some_and(|c| c.popup.surface.layer.wl_surface() == surface)
    }
}

impl App {
    /// The connect card for `debug dump`: open and its resting rect.
    pub fn headset_status(&self) -> serde_json::Value {
        match &self.headset.card {
            Some(c) => {
                // In output pixels: a right or bottom surface starts its scene's size in from the far side.
                let (ow, oh) = self.output_size();
                let size = c.popup.card.scene.size;
                let (ox, oy) = match c.popup.card.edge {
                    Edge::Right => (ow - size.w as f64, 0.0),
                    Edge::Bottom => (0.0, oh - size.h as f64),
                    _ => (0.0, 0.0),
                };
                let r = c.popup.card.rest_rect();
                serde_json::json!({"open": c.popup.is_open(), "rest": {"x": ox + r.x as f64, "y": oy + r.y as f64, "w": r.w, "h": r.h}})
            }
            None => serde_json::Value::Null,
        }
    }
}
