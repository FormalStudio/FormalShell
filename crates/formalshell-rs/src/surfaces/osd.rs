//! One icon, a track and the percentage in a pill the
//! drawer buds off the bottom line. The pill is exactly `popupWidthNarrow`
//! wide whatever it shows, and the readout column is measured off "100%"
//! rather than the live value, so volume ticking 3% to 97% or a long media
//! title swapping in never reflows it.

use fs_theme::theme::Theme;

use crate::surfaces::bar::cell::Kit;
use crate::ui::{self, El, Ink, Type, w};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Volume,
    Brightness,
    Media,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Volume => "volume",
            Kind::Brightness => "brightness",
            Kind::Media => "media",
        }
    }
}

/// Which icon the pill draws (Osd/icon.js). A muted sink keeps its pre-mute
/// percentage on the readout but draws `volume-x`: the icon answers "will I
/// hear this" and the number answers "where is the slider".
pub fn icon_name(kind: Kind, volume: f64, muted: bool) -> &'static str {
    match kind {
        Kind::Brightness => "sun",
        Kind::Media => "music",
        Kind::Volume if muted || !(volume > 0.0) => "volume-x",
        Kind::Volume if volume < 0.5 => "volume-1",
        Kind::Volume => "volume-2",
    }
}

/// What the pill reads off the store.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub kind: Kind,
    pub media: String,
    /// Volume 0 to 1, muted, and the backlight's percent.
    pub volume: f64,
    pub muted: bool,
    pub brightness: f64,
}

impl Reading {
    /// Muted keeps showing the pre-mute number but drops the fill to 0: the
    /// track is the "how much will I actually hear" signal.
    fn fraction(&self) -> f64 {
        match self.kind {
            Kind::Brightness => (self.brightness / 100.0).clamp(0.0, 1.0),
            _ if self.muted => 0.0,
            _ => self.volume.clamp(0.0, 1.0),
        }
    }

    fn percent(&self) -> i64 {
        match self.kind {
            Kind::Brightness => self.brightness.round() as i64,
            _ => (self.volume * 100.0).round() as i64,
        }
    }
}

pub fn content(r: &Reading, theme: &Theme, kit: &mut Kit, inner_width: f64) -> El {
    let s = &theme.space;
    let icon = w::icon(icon_name(r.kind, r.volume, r.muted)).size(Type::Title).ink(Ink::Fg);
    if r.kind == Kind::Media {
        return w::row(s.icon_gap, vec![icon, w::label(r.media.clone()).size(Type::Body).elide()]).fill();
    }
    let text = format!("{}%", r.percent());
    let column = |t: &str| w::value(t.to_owned()).ink(Ink::Fg).mono();
    let widest = ui::measure(&column("100%"), inner_width, theme, kit).0;
    let own = ui::measure(&column(&text), inner_width, theme, kit).0;
    let readout = column(&text).pad_start((widest - own).max(0.0));
    w::row(s.icon_gap, vec![icon, w::track(r.fraction()).fill(), readout]).fill()
}
