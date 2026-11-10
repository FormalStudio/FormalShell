//! The bottom-centre volume, brightness and media pill, one
//! `popupWidthNarrow` card out of the bottom edge's line, no keyboard focus
//! and no pointer input at all, gone `HIDE` after the last trigger. The
//! layer surface exists only while the card is drawn, and is a band along
//! the bottom rather than the whole output: it has to reach the line the
//! pill buds off and cover its silhouette, which is the pill, the room it
//! rests off the line and a fillet either side of it, and nothing above
//! that. The pill's own height again on top is room for the travel's
//! overshoot and the deform's stretch.
//!
//! The volume kind shows itself whenever the default sink's volume or mute
//! changes, ours or external (`wpctl`, a hardware key). Brightness and
//! media only ever show over IPC: `display brightnessStep` sets the focused
//! output's device and shows the value it asked for, and `osd brightness`
//! shows the backlight after something else set it.

use std::time::{Duration, Instant};

use fs_audio::Visual;
use fs_types::Edge;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};
use vello_cpu::kurbo::Rect;

use super::App;
use crate::services::display;
use crate::surface::{Backend, Surface};
use crate::surfaces::card::{Card, CardSpec, Sizing};
use crate::surfaces::osd::{self, Kind, Reading};
use crate::surfaces::popup::Popup;

const HIDE: Duration = Duration::from_millis(1600);
/// Harder than a panel: the pill is small, it travels its whole height,
/// and 0.25 is what caelestia gives an OSD.
const DEFORM: f64 = 0.25;
/// How long a step's own value stays the base for the next one. The store
/// hears of a set one channel hop later, and a held key's repeat can land
/// inside that hop.
const STEP_HOLD: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct Osd {
    pub(super) pill: Option<Popup>,
    kind: Option<Kind>,
    media: String,
    hide_at: Option<Instant>,
    /// The sink's last reading, for the change that fires the pill.
    audio_seen: Option<(Visual, bool)>,
    /// The last brightness step: the device, the percent it asked for and
    /// when. The pill shows it until `osd brightness` asks for the backlight.
    step: Option<(String, i64, Instant)>,
}

impl App {
    pub fn osd_show(&mut self, kind: Kind, text: &str) {
        let now = Instant::now();
        self.osd.kind = Some(kind);
        if kind == Kind::Media {
            self.osd.media = text.to_owned();
        }
        self.osd.hide_at = Some(now + HIDE);
        let covered = self.chrome().bar_covered();
        match &mut self.osd.pill {
            Some(p) => {
                if !p.is_open() {
                    p.set_open(now, true);
                }
            }
            // A layer mapped over a covered output releases the pointer
            // lock a game holds there (`present_toasts` has the why).
            None if covered => {}
            None => self.osd_create(),
        }
    }

    /// Steps the brightness of the device behind `output` by `delta` percent
    /// and shows the value asked for. The device's own writer coalesces a
    /// burst, so the newest value lands last.
    pub fn brightness_step(&mut self, output: &str, delta: i64) -> Result<(), String> {
        if output.is_empty() {
            return Err("no focused output".into());
        }
        let id = display::brightness_id(output).to_owned();
        let Some(device) = self.store.display.devices().into_iter().find(|d| d.id == id) else {
            return Err(format!("no brightness control on {output}"));
        };
        let now = Instant::now();
        let base = match &self.osd.step {
            Some((last, pct, at)) if *last == id && now < *at + STEP_HOLD => *pct,
            _ => device.percent,
        };
        let pct = display::clamp_percent(&id, base + delta);
        self.osd.step = Some((id.clone(), pct, now));
        if let Some(rt) = &self.runtime {
            let max = device.max;
            rt.service(move |ctx| display::set_percent(ctx, &id, pct as f64, max));
        }
        self.osd_show(Kind::Brightness, "");
        Ok(())
    }

    /// `osd brightness`: the backlight's own reading from here on.
    pub fn osd_brightness_read(&mut self) {
        self.osd.step = None;
    }

    fn osd_brightness(&self) -> f64 {
        match &self.osd.step {
            Some((_, pct, _)) => *pct as f64,
            None => self.store.display.backlight.as_ref().map_or(0.0, |b| b.percent as f64),
        }
    }

    pub fn osd_close(&mut self) {
        self.osd.hide_at = None;
        self.osd.kind = None;
        if let Some(p) = &mut self.osd.pill {
            p.set_open(Instant::now(), false);
        }
    }

    pub fn osd_state(&self) -> serde_json::Value {
        serde_json::json!({
            "visible": self.osd.pill.is_some(),
            "kind": self.osd.kind.map_or("", Kind::name),
            "mediaText": self.osd.media,
            "brightness": fs_js::json_number(self.osd_brightness()),
        })
    }

    /// The default sink moved: ours, or something else's.
    pub(super) fn osd_audio(&mut self) {
        let a = &self.store.audio;
        let reading = a.volume.map(|v| (v, a.muted));
        let before = std::mem::replace(&mut self.osd.audio_seen, reading);
        if let (Some(b), Some(n)) = (before, reading)
            && b != n
        {
            self.osd_show(Kind::Volume, "");
        }
    }

    fn osd_reading(&self) -> Reading {
        let audio = &self.store.audio;
        match self.osd.kind.unwrap_or(Kind::Volume) {
            Kind::Volume => Reading::Volume { level: audio.volume.unwrap_or_default(), muted: audio.muted },
            Kind::Brightness => Reading::Brightness { percent: self.osd_brightness() },
            Kind::Media => Reading::Media(self.osd.media.clone()),
        }
    }

    fn osd_create(&mut self) {
        let (out_w, out_h) = self.output_size();
        if out_w <= 0.0 || out_h <= 0.0 {
            return;
        }
        let theme = &self.store.theme.theme;
        let pad = theme.space.panel_padding;
        let width = theme.space.popup_width_narrow;
        let reading = self.osd_reading();
        let el = osd::content(&reading, theme, &mut self.kit, width - pad * 2.0);
        let height = crate::ui::measure(&el, width - pad * 2.0, theme, &mut self.kit).1 + pad * 2.0;
        let chrome = self.chrome();
        let inset = chrome.inset(Edge::Bottom);
        let rest = inset + theme.space.screen_padding;
        let band = out_h.min(rest + height * 2.0);
        let u = ((out_w - width) / 2.0).round();
        let rest_rect = Rect::new(u, rest, u + width, rest + height);
        let spec = CardSpec {
            ends: chrome.ends(Edge::Bottom),
            deform: DEFORM,
            ..CardSpec::new(Edge::Bottom, inset, Sizing::Surface { size: (out_w as i32, band as i32), rest: rest_rect })
        };
        let card = Card::new(theme, spec, self.motion_scale, self.cast);
        let layer = self.overlay("formalshell:osd", Layer::Overlay, Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT, (0, band as u32), -1);
        let surface = Surface::new("osd", Backend::Gpu, layer, &self.shm, self.started, self.store.display.scales.primary());
        let mut popup = Popup::new(surface, card);
        popup.set_open(Instant::now(), true);
        self.osd.pill = Some(popup);
        crate::trace::mapped(crate::trace::Surface::Osd, crate::trace::State::Mapped);
    }

    pub(super) fn osd_frame(&mut self, now: Instant) {
        if let Some(p) = &mut self.osd.pill {
            p.win.frame(now);
        }
        self.sync_join();
    }

    pub(super) fn osd_owns(&self, surface: &wl_surface::WlSurface) -> bool {
        self.osd.pill.as_ref().is_some_and(|p| p.win.owns(surface))
    }

    pub(super) fn osd_configure(&mut self) {
        if let Some(p) = &mut self.osd.pill {
            p.win.configure();
        }
    }

    pub(super) fn osd_wake(&self) -> Option<Instant> {
        self.osd.hide_at
    }

    pub(super) fn osd_present(&mut self, now: Instant) {
        if self.osd.hide_at.is_some_and(|t| t <= now) {
            self.osd_close();
        }
        if self.osd.pill.as_ref().is_some_and(|p| p.win.finished(now)) {
            self.osd.pill = None;
            self.sync_join();
            crate::trace::mapped(crate::trace::Surface::Osd, crate::trace::State::Unmapped);
            return;
        }
        if self.osd.pill.is_none() {
            return;
        }
        let reading = self.osd_reading();
        let theme = &self.store.theme.theme;
        let qh = self.qh.clone();
        let Some(p) = &mut self.osd.pill else { return };
        let width = p.win.card.rest().width() - theme.space.panel_padding * 2.0;
        let el = osd::content(&reading, theme, &mut self.kit, width);
        let drawn = p.draw(&el, theme, &mut self.kit, now);
        let animating = p.win.animating(now) || drawn.animating;
        p.win.present(animating, &qh);
    }
}
