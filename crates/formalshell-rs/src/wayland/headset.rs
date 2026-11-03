//! The headset connect card (new in the Rust shell, after macOS's AirPods
//! popup): when a Bluetooth audio device goes from disconnected to
//! connected, a card buds off the top edge's middle (a top bar's line, else
//! the frame ring or the bare edge) in the joined-shape motion
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
use crate::services::devices::earbuds;
use crate::services::compositor;
use crate::surfaces::card::{Card, popover_theme};
use crate::surfaces::headset;
use crate::surfaces::popup::Popup;

const HOLD: Duration = Duration::from_secs(4);
const RECONNECT: Duration = Duration::from_secs(3);
/// How long the earbuds service is held to learn a device's picture set:
/// the backend's own quick-retry window (`services/devices/earbuds.rs`
/// `QUICK_WINDOW`), since nothingctl can take ~40s to name a headphone that
/// has just connected, far past the card's hold.
const LEARN: Duration = Duration::from_secs(90);
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
}

/// The earbuds service held past the card for a device whose picture set
/// is not known yet, let go once the backend names it, the device leaves,
/// or `LEARN` runs out.
struct Learning {
    address: String,
    until: Instant,
}

impl Learning {
    fn new(address: &str, until: Instant) -> Self {
        earbuds::acquire();
        Self { address: address.to_owned(), until }
    }
}

impl Drop for Learning {
    fn drop(&mut self) {
        earbuds::release();
    }
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
    learning: Option<Learning>,
}

impl Headset {
    pub fn wake(&self) -> Option<Instant> {
        [self.card.as_ref().and_then(|c| c.hold_until), self.learning.as_ref().map(|l| l.until)].into_iter().flatten().min()
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
        let learned = headset::learned(&self.store);
        if !learned.is_empty() {
            let mut faces = self.store.state.data.earbuds_faces.clone();
            if let Some(map) = faces.as_object_mut() {
                map.extend(learned);
            }
            crate::services::state::set(vec![crate::services::state::Field::EarbudsFaces(faces)]);
        }
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
        let done = self.headset.learning.as_ref().is_some_and(|l| {
            !headset::face(&self.store, &l.address).art.is_empty() || self.headset.known.get(&l.address).is_none_or(|k| !k.connected)
        });
        if done {
            self.headset.learning = None;
        }
        if let Some(address) = show {
            self.headset_show(&address, now);
        }
    }

    fn headset_down(&self) -> bool {
        self.store.state.data.dnd || self.chrome_hidden()
    }

    fn headset_show(&mut self, address: &str, now: Instant) {
        let unknown = headset::face(&self.store, address).art.is_empty();
        if unknown && self.headset.learning.as_ref().is_none_or(|l| l.address != address) {
            self.headset.learning = Some(Learning::new(address, now + LEARN));
        }
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
        // A popover under the top edge's middle whatever the layout: under
        // a top bar's line, else the frame ring's band or the bare edge.
        let theme = popover_theme(&self.store.theme.theme);
        let length = self.output_size().0;
        let ft = if self.bar.framed() { self.bar.frame_thickness() } else { 0.0 };
        let line_at = if self.bar.edge() == Edge::Top { self.bar.thickness() as f64 } else { ft };
        let size = headset::size(&headset::face(&self.store, address), &theme, &mut self.bar.kit);
        let mut card = Card::new(&theme, Edge::Top, length as i32, line_at, length / 2.0, size, self.motion_scale, self.cast);
        card.capsule();
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
        };
        shown.hold();
        self.headset.card = Some(shown);
        self.log("headset card mapped");
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
        if self.headset.learning.as_ref().is_some_and(|l| l.until <= now) {
            self.headset.learning = None;
        }
        let down = self.headset_down();
        let Some(c) = &mut self.headset.card else { return };
        if c.popup.is_open() && (down || c.hold_until.is_some_and(|t| t <= now)) {
            self.headset_dismiss(now);
        }
        let Some(address) = self.headset.card.as_ref().map(|c| c.address.clone()) else { return };
        let face = headset::face(&self.store, &address);
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
        let (el, rect) = headset::layout(&face, c.popup.card.content.0, theme, &mut self.bar.kit);
        let drawn = c.popup.draw_in(&el, rect, theme, &mut self.bar.kit, now);
        {
            let frame = c.popup.card.content.0;
            let (mut painter, alpha) = c.popup.painter();
            headset::paint_ring(&mut painter, alpha, &face, frame, theme, &mut self.bar.kit);
            painter.finish();
        }
        let animating = c.popup.animating(now) || drawn.animating;
        let qh = self.qh.clone();
        c.popup.present(animating, &qh);
    }

    pub(super) fn headset_owns(&self, surface: &wl_surface::WlSurface) -> bool {
        self.headset.card.as_ref().is_some_and(|c| c.popup.surface.layer.wl_surface() == surface)
    }
}

impl App {
    /// The connect card for `debug dump`: open, its picture set and name, its
    /// resting rect and corner radius, and the device whose picture set
    /// the earbuds service is still held to learn.
    pub fn headset_status(&self) -> serde_json::Value {
        let learning = self.headset.learning.as_ref().map(|l| l.address.clone());
        match &self.headset.card {
            Some(c) => {
                // The surface hangs from the output's top left, so its pixels are the output's.
                let r = c.popup.card.rest_rect();
                let face = headset::face(&self.store, &c.address);
                serde_json::json!({"open": c.popup.is_open(), "art": face.art, "name": face.name, "rest": {"x": r.x, "y": r.y, "w": r.w, "h": r.h}, "radius": c.popup.card.radius(), "learning": learning})
            }
            None => serde_json::json!({"learning": learning}),
        }
    }
}
