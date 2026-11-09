// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! The power profile picker, Pond's `battery/ProfileSelector.qml`: a pill
//! shaped like a battery (body and nub one outline, `card::battery`) on
//! the output's right edge, centred, shown while the Settings tiles are
//! open under `layout: sidebar`, as Pond shows it beside its settings
//! window. It holds power saver, balanced and performance stacked, the
//! active one on the `panel.chip` on circle and the rest bare glyphs; a
//! click sets that profile through power-profiles-daemon. The laptop
//! battery's charge tints the outline from its foot up, reaching the nub
//! only when full. With no daemon the three glyphs are dim and take no
//! click; performance alone is dim where the daemon does not offer it.

use std::time::Instant;

use fs_types::Edge;
use fs_upower::Profile;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Layer;

use super::{App, Owner};
use crate::scene::IRect;
use crate::services::devices;
use crate::surfaces::card::{Card, CardSpec, Sizing};
use crate::surfaces::popup::Popup;
use crate::ui::{El, Ink, Size, Type, w};

const PROFILES: [(Profile, &str, &str); 3] =
    [(Profile::PowerSaver, "leaf", "Power Saver"), (Profile::Balanced, "gauge", "Balanced"), (Profile::Performance, "zap", "Performance")];

/// How strongly the charge tints the outline.
const LEVEL_ALPHA: f32 = 0.12;

/// The charge's tint: `primary`, `warning` under a fifth on battery and
/// `destructive` past the low warning.
fn level_colour(b: &crate::services::devices::power::Battery) -> &'static str {
    use fs_system::power::model::Thresholds;
    use fs_upower::DeviceState;
    let charging = matches!(b.state, DeviceState::Charging | DeviceState::FullyCharged | DeviceState::PendingCharge);
    match b.level {
        _ if charging => "primary",
        l if Thresholds::LAPTOP.low(l).is_some() => "destructive",
        l if l.as_percent() < 20.0 => "warning",
        _ => "primary",
    }
}

pub(super) struct Pill {
    popup: Popup,
    pressed: Option<String>,
}

/// Which profiles take a click, and the active one; none without the daemon.
fn offered(store: &crate::store::Store) -> Option<(Vec<bool>, Profile)> {
    let p = store.devices.profiles.as_ref()?;
    Some((PROFILES.iter().map(|(x, ..)| p.available.contains(x)).collect(), p.active))
}

/// The three buttons, stacked, each a round chip as wide as the pill.
fn body(store: &crate::store::Store, side: f64, gap: f64) -> El {
    let offer = offered(store);
    let buttons = PROFILES
        .iter()
        .enumerate()
        .map(|(i, (profile, glyph, name))| {
            let enabled = offer.as_ref().is_some_and(|(on, _)| on[i]);
            let active = offer.as_ref().is_some_and(|(_, a)| a == profile);
            let ink = if active { Ink::Primary } else if enabled { Ink::Muted } else { Ink::Dim };
            let chip = w::cell(w::row(0.0, vec![w::spacer(), w::icon(*glyph).size(Type::Body).ink(ink), w::spacer()]).fill().height(side))
                .cell_state(|c| {
                    c.glyph = active;
                    c.open = active;
                    c.ghost = !active;
                    c.round = true;
                    c.inset = Some((0.0, 0.0));
                })
                .width(Size::Px(side))
                .height(side)
                .tip(if offer.is_some() { (*name).to_owned() } else { format!("{name}: power-profiles-daemon is not running") });
            if enabled { chip.interactive().on(format!("profile:{i}")) } else { chip }
        })
        .collect();
    w::column(gap, buttons)
}

impl App {
    fn profiles_wanted(&self) -> bool {
        self.bar.sidebar.is_some() && self.launcher_panel() == Some("settings") && !self.chrome().bar_covered()
    }

    /// The pill's size: a chip's side plus the padding, three chips tall.
    fn profiles_size(&self) -> (f64, f64, f64, f64) {
        let s = &self.store.theme.theme.space;
        let (side, gap) = (s.control_height + s.sm, s.xs);
        let pad = s.panel_padding;
        (side, gap, side + pad * 2.0, side * 3.0 + gap * 2.0 + pad * 2.0)
    }

    fn profiles_show(&mut self, now: Instant) {
        if let Some(p) = &mut self.profiles {
            if !p.popup.is_open() {
                p.popup.set_open(now, true);
            }
            return;
        }
        let (_, _, width, height) = self.profiles_size();
        // A popover always: the right edge carries no line to bud off.
        let theme = crate::surfaces::card::popover_theme(&self.store.theme.theme);
        let (_, oh) = self.output_size();
        let s = &theme.space;
        let spec = CardSpec { nub: Some((s.xl, s.sm, s.xxs)), ..CardSpec::new(Edge::Right, 0.0, Sizing::Card { size: (height, width), anchor: oh / 2.0, length: oh as i32 }) };
        let card = Card::new(&theme, spec, self.motion_scale, self.cast);
        let surface = self.popout_surface(&card, Layer::Overlay);
        let mut popup = Popup::new(surface, card);
        popup.set_open(now, true);
        self.profiles = Some(Pill { popup, pressed: None });
        self.log("profiles pill mapped");
    }

    pub(super) fn profiles_frame(&mut self, now: Instant) {
        if let Some(p) = &mut self.profiles {
            p.popup.frame(now);
        }
    }

    pub(super) fn profiles_configure(&mut self) {
        if let Some(p) = &mut self.profiles {
            p.popup.configure();
        }
    }

    pub(super) fn profiles_owns(&self, surface: &wl_surface::WlSurface) -> bool {
        self.profiles.as_ref().is_some_and(|p| p.popup.surface.layer.wl_surface() == surface)
    }

    /// An output point over the pill, in its surface's pixels. The
    /// launcher's card surface covers the whole output to catch a click
    /// outside it and maps after the pill, so it is the one the
    /// compositor hands presses over the pill to.
    pub(super) fn profiles_under(&self, at: (f64, f64)) -> Option<(f64, f64)> {
        let p = self.profiles.as_ref().filter(|p| p.popup.is_open())?;
        let local = (at.0 - (self.output_size().0 - p.popup.card.scene.size.w as f64), at.1);
        let r = p.popup.card.rest_rect();
        (local.0 >= r.x as f64 && local.0 < r.right() as f64 && local.1 >= r.y as f64 && local.1 < r.bottom() as f64).then_some(local)
    }

    /// A release that landed anywhere but the pill ends its press.
    pub(super) fn profiles_release_elsewhere(&mut self, kind: &PointerEventKind) {
        if let (PointerEventKind::Release { .. }, Some(p)) = (kind, &mut self.profiles) {
            p.pressed = None;
        }
    }

    /// A press and release on one profile's chip sets it.
    pub(super) fn profiles_pointer(&mut self, owner: Option<Owner>, kind: &PointerEventKind, at: (f64, f64)) {
        if owner != Some(Owner::Profiles) {
            return;
        }
        let Some(p) = &mut self.profiles else { return };
        match kind {
            PointerEventKind::Press { .. } => p.pressed = p.popup.hit(at.0, at.1),
            PointerEventKind::Release { .. } => {
                let here = p.popup.hit(at.0, at.1);
                let Some(on) = p.pressed.take().filter(|on| Some(on) == here.as_ref()) else { return };
                let Some((profile, ..)) = on.strip_prefix("profile:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| PROFILES.get(i)) else { return };
                let profile = *profile;
                if let Some(rt) = &self.runtime {
                    rt.service(move |ctx| devices::run(ctx, devices::Op::Profile(profile)));
                }
            }
            PointerEventKind::Leave { .. } => p.pressed = None,
            _ => {}
        }
    }

    /// `debug dump`'s `profilesPill`: the card's rest rect and each
    /// chip's, in the output's pixels, the daemon's active profile and the
    /// battery level the body fills to; null while it is not up.
    pub fn profiles_status(&self) -> serde_json::Value {
        let Some(p) = self.profiles.as_ref().filter(|p| p.popup.is_open()) else { return serde_json::Value::Null };
        // The card's own surface hangs off the right edge, its scene's width wide.
        let ox = self.output_size().0 - p.popup.card.scene.size.w as f64;
        let rect = |r: IRect| serde_json::json!({"x": ox + r.x as f64, "y": r.y, "w": r.w, "h": r.h});
        let chips: Vec<_> = p.popup.controls().into_iter().map(|(on, r)| serde_json::json!({"on": on, "rect": rect(r)})).collect();
        serde_json::json!({
            "rect": rect(p.popup.card.rest_rect()),
            "chips": chips,
            "active": self.store.devices.profiles.as_ref().map(|p| p.active.to_wire()),
            "battery": self.store.devices.power.battery.as_ref().map(|b| b.level.as_percent()),
        })
    }

    pub(super) fn profiles_present(&mut self, now: Instant) {
        let want = self.profiles_wanted();
        if want {
            self.profiles_show(now);
        } else if let Some(p) = self.profiles.as_mut().filter(|p| p.popup.is_open()) {
            p.popup.set_open(now, false);
            p.pressed = None;
        }
        let (side, gap, ..) = self.profiles_size();
        let Some(p) = &mut self.profiles else { return };
        if p.popup.finished(now) {
            self.profiles = None;
            self.log("profiles pill unmapped");
            return;
        }
        let open = p.popup.is_open();
        p.popup.sync_region(&self.compositor, open);
        let theme = &self.store.theme.theme;
        let inner = p.popup.inner(theme);
        {
            let (frame, alpha) = p.popup.card.popover_frame;
            let (radius, bw, nub) = (p.popup.card.radius(), p.popup.card.border_width(), p.popup.card.nub().unwrap_or_default());
            let (mut painter, _) = p.popup.painter();
            let charge = self.store.devices.power.battery.as_ref().map(|b| (b.level, level_colour(b)));
            if let Some((level, colour)) = charge {
                // The charge, the outline's own inside tinted from the foot
                // up: the nub only when full.
                let level = level.as_fraction();
                let nub_h = nub.1.ceil() as i32;
                let top = if level >= 1.0 {
                    frame.y - nub_h
                } else {
                    frame.bottom() - bw.round() as i32 - ((frame.h as f64 - bw * 2.0) * level).round() as i32
                };
                let card = vello_cpu::kurbo::Rect::new(frame.x as f64, frame.y as f64, frame.right() as f64, frame.bottom() as f64);
                let tint = theme.colors.get(colour);
                let fill = (crate::surfaces::card::battery(card, radius, nub, bw), tint.with_alpha(tint.a * LEVEL_ALPHA * alpha));
                let bounds = IRect::new(frame.x, frame.y - nub_h, frame.w, frame.h + nub_h);
                painter.shape_in(bounds, crate::scene::Paint::Shape { fill: Some(fill), strokes: Vec::new() }, IRect::new(frame.x, top, frame.w, frame.bottom() - top));
            }
            painter.finish();
        }
        let el = body(&self.store, side, gap);
        let drawn = p.popup.draw_in(&el, inner, theme, &mut self.kit, now);
        let animating = p.popup.animating(now) || drawn.animating;
        let qh = self.qh.clone();
        p.popup.present(animating, &qh);
    }
}
