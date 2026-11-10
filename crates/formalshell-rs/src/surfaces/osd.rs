//! One icon, a track and the percentage in a pill the
//! drawer buds off the bottom line. The pill is exactly `popupWidthNarrow`
//! wide whatever it shows, and the readout column is measured off "100%"
//! rather than the live value, so volume ticking 3% to 97% or a long media
//! title swapping in never reflows it.

use fs_audio::Visual;
use fs_theme::icons;
use fs_theme::theme::Theme;

use crate::paint::Kit;
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

/// What the pill reads off the store, in its own kind's scale.
#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    /// The default sink.
    Volume { level: Visual, muted: bool },
    /// The backlight's percent.
    Brightness { percent: f64 },
    Media(String),
}

impl Reading {
    /// Which icon the pill draws. A muted sink keeps its
    /// pre-mute percentage on the readout but draws `volume-x`: the icon
    /// answers "will I hear this" and the number answers "where is the
    /// slider".
    fn icon(&self) -> &'static str {
        match self {
            Reading::Brightness { .. } => "sun",
            Reading::Media(_) => "music",
            Reading::Volume { muted: true, .. } => "volume-x",
            Reading::Volume { level, .. } => icons::volume(level.track()),
        }
    }
}

pub fn content(r: &Reading, theme: &Theme, kit: &mut Kit, inner_width: f64) -> El {
    let s = &theme.space;
    let icon = w::icon(r.icon()).size(Type::Title).ink(Ink::Fg);
    // Muted keeps showing the pre-mute number but drops the fill to 0: the
    // track is the "how much will I actually hear" signal.
    let (fraction, percent) = match r {
        Reading::Media(text) => return w::row(s.icon_gap, vec![icon, w::label(text.clone()).size(Type::Body).elide()]).fill(),
        Reading::Volume { level, muted } => {
            (if *muted { 0.0 } else { level.track().as_fraction() }, (level.as_f64() * 100.0).round() as i64)
        }
        Reading::Brightness { percent } => ((percent / 100.0).clamp(0.0, 1.0), percent.round() as i64),
    };
    let text = format!("{percent}%");
    let column = |t: &str| w::value(t.to_owned()).ink(Ink::Fg).mono();
    let widest = ui::measure(&column("100%"), inner_width, theme, kit).0;
    let own = ui::measure(&column(&text), inner_width, theme, kit).0;
    let readout = column(&text).pad_start((widest - own).max(0.0));
    w::row(s.icon_gap, vec![icon, w::track(fraction).fill(), readout]).fill()
}
