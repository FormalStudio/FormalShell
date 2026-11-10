//! The headset connect card's content (new in the Rust shell, spec
//! `2026-10-06-rust-rewrite.md` under R7), macOS's AirPods capsule: the
//! device's product picture (the earbuds panel's `duo` set, its icon for a
//! device with none) at one end, its name over "Connected" centred between,
//! and one battery ring with its level inside at the other end. The ring
//! is the buds' lower level where the backend reports a pair, as macOS
//! shows one.
//!
//! The capsule is as wide as the name it opens with needs, as macOS's is,
//! between four slots and `popupWidthNarrow`, and keeps that width while it
//! is up, so a backend reporting later never moves it; a longer name elides.

use fs_chrome::bar::panels::PanelId;
use fs_devices::earbuds::{BatteryId, DeviceKind};
use fs_system::power::model::Thresholds;
use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use fs_theme::style::Role;
use fs_theme::theme::Theme;
use fs_theme::tokens::WEIGHTS;
use fs_types::Percent;
use vello_cpu::kurbo::{Affine, Rect};

use crate::scene::{IRect, Paint, Scene};
use crate::services::devices::headsets::Identity;
use crate::paint::{Kit, Painter};
use crate::ui::el::{Ring, Size};
use crate::ui::{self, El, Ink, Type, w};

/// One battery the earbuds backend reports, or BlueZ's rounded to a whole
/// percent.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub side: BatteryId,
    pub level: Percent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub icon: &'static str,
    /// The earbuds backend's picture set (`Device::art`), "" for none.
    pub art: String,
    pub name: String,
    pub parts: Vec<Part>,
    /// The panel a click opens.
    pub panel: PanelId,
}

fn icon_of(bluez: &str, kind: Option<DeviceKind>) -> &'static str {
    match (bluez, kind) {
        (_, Some(DeviceKind::Headphone)) | ("audio-headphones", _) => "headphones",
        ("audio-speakers", _) => "speaker",
        _ => "headset",
    }
}

/// What the card shows for a device: the earbuds backend's parts when one
/// claims it, BlueZ's Battery1 otherwise, and no ring at all when nothing
/// reports a level.
pub fn face(id: &Identity) -> Face {
    let icon = icon_of(id.seen.map_or("", |h| h.icon.as_str()), id.claimed.map(|d| d.kind));
    let mut parts: Vec<Part> = Vec::new();
    if let Some(d) = id.claimed {
        for side in [BatteryId::Left, BatteryId::Right, BatteryId::Case, BatteryId::Single] {
            if let Some(b) = d.batteries.iter().find(|b| b.id == side) {
                parts.push(Part { side, level: b.level });
            }
        }
    }
    if parts.is_empty()
        && let Some(level) = id.seen.and_then(|h| h.battery)
    {
        parts.push(Part { side: BatteryId::Single, level: level.to_percent().round() });
    }
    Face { icon, art: id.art.clone(), name: id.name.clone(), parts, panel: if id.claimed.is_some() { PanelId::Earbuds } else { PanelId::Bluetooth } }
}

/// The picture's and the ring's square slot, which sets the capsule's height.
fn slot(theme: &Theme) -> f64 {
    theme.space.control_height + theme.space.sm
}

/// The capsule's padding round both slots.
fn pad(theme: &Theme) -> f64 {
    theme.space.sm
}

/// The one level the ring shows: a headphone's, the buds' lower one, or
/// the case's when that is all the backend has.
fn level(f: &Face) -> Option<Percent> {
    let of = |side: BatteryId| f.parts.iter().find(|p| p.side == side).map(|p| p.level);
    of(BatteryId::Single)
        .or_else(|| match (of(BatteryId::Left), of(BatteryId::Right)) {
            (Some(l), Some(r)) => Some(if r < l { r } else { l }),
            (l, r) => l.or(r),
        })
        .or_else(|| of(BatteryId::Case))
}

/// The name over "Connected": what `size` measures and `content` draws.
fn lines(f: &Face) -> [El; 2] {
    [w::label(f.name.clone()), w::text("Connected").ink(Ink::Muted)]
}

/// The capsule's (width, height): the picture's and the ring's slots, the
/// gaps beside the words and the wider of its two lines, by a slot and its
/// padding.
pub fn size(f: &Face, theme: &Theme, kit: &mut Kit) -> (f64, f64) {
    let (p, n) = (pad(theme), slot(theme));
    let words = lines(f).iter().map(|el| ui::measure(el, f64::MAX, theme, kit).0).fold(0.0, f64::max);
    let width = (p + n + theme.space.md) * 2.0 + words;
    (width.clamp(n * 4.0 + p * 2.0, theme.space.popup_width_narrow).ceil(), n + p * 2.0)
}

/// The picture, made at `scale`, and the words; the ring's slot is held
/// empty for `paint_ring`, so the words sit in the capsule's middle.
fn content(f: &Face, theme: &Theme, scale: f64) -> El {
    let s = &theme.space;
    let size = slot(theme);
    let lead = match crate::services::devices::earbuds::picture(&f.art, "duo", size, scale) {
        Some(picture) => w::picture(Some(picture), size),
        None => w::column(0.0, vec![w::icon(f.icon).size(Type::Heading).ink(Ink::Fg).centred()]).width(Size::Px(size)),
    };
    // Eliding fills a row; held to its own width it still elides and centres.
    let line = |el: El| el.elide().width(Size::AtMost(1.0)).centred();
    let words = w::column(0.0, lines(f).into_iter().map(line).collect());
    w::row(s.md, vec![lead, words.fill(), w::space(size)]).mid()
}

/// The content's element and the rect it is laid in, inside the card's
/// `frame` and centred down it, its picture made at `scale`.
pub fn layout(f: &Face, frame: IRect, scale: f64, theme: &Theme, kit: &mut Kit) -> (El, Rect) {
    let p = pad(theme);
    let (x0, x1) = (frame.x as f64 + p, (frame.x + frame.w) as f64 - p);
    let el = content(f, theme, scale);
    let h = ui::measure(&el, x1 - x0, theme, kit).1;
    let top = frame.y as f64 + (frame.h as f64 - h) / 2.0;
    (el, Rect::new(x0, top, x1, top + h))
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    c.with_alpha(c.a * a)
}

/// The ring in the capsule's far slot: a groove under an arc from twelve
/// o'clock, its level inside; nothing before any level is known.
pub fn paint_ring(p: &mut Painter, alpha: f32, f: &Face, frame: IRect, theme: &Theme, kit: &mut Kit) {
    let Some(level) = level(f) else { return };
    let d = slot(theme);
    let (cx, cy) = ((frame.x + frame.w) as f64 - pad(theme) - d / 2.0, frame.y as f64 + frame.h as f64 / 2.0);
    let groove = theme.box_style(Role::TrackGroove, None).fill;
    let low = Thresholds::HEADSET.low(level).is_some();
    let fill = if low { theme.colors.get(ColorRole::Destructive) } else { theme.box_style(Role::TrackFill, None).fill };
    let ring = Ring::Progress(level.as_fraction());
    let strokes = ui::ring_strokes((cx, cy), d, theme.space.xs, ring, 0.0, with_alpha(groove, alpha), with_alpha(fill, alpha));
    let bounds = Scene::cover(Rect::new(cx - d / 2.0, cy - d / 2.0, cx + d / 2.0, cy + d / 2.0), Affine::IDENTITY, 2.0);
    p.shape(bounds, Paint::Shape { fill: None, strokes });
    let style = kit.weight(kit.look.mono, theme.font_size.body_small as f32, WEIGHTS.medium);
    let text = kit.shape(&level.rounded().to_string(), style);
    let at = ((cx - text.width as f64 / 2.0).round() as i32, (cy - text.line_height() as f64 / 2.0).round() as i32);
    p.text(&text, at, with_alpha(theme.colors.get(ColorRole::MutedForeground), alpha), &[]);
}
