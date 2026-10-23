//! The headset connect card's content (new in the Rust shell, spec
//! `2026-10-06-rust-rewrite.md` under R7): the device's product picture
//! (its icon for a device with none), its name over "Connected", and one battery ring per part the backend reports, drawn in
//! the shell's own chrome. The card is a plain `card` role box the drawer
//! buds off the bar's line; nothing here invents a size, a colour or a curve.
//!
//! The ring block's height is reserved whether or not any ring is drawn, so
//! a backend that reports a second after the card is up does not move it.

use fs_devices::earbuds::{BatteryId, Device, DeviceKind};
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::{Affine, Arc, Circle, Rect, Shape};

use crate::scene::{Paint, Scene};
use crate::services::devices::headsets::Headset;
use crate::store::Store;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::TextStyle;
use crate::ui::{self, El, Ink, Type, w};

/// At or under this a ring takes the destructive ink (`battery.warnPercent`'s default).
const LOW: f64 = fs_system::power::model::DEFAULT_WARN_PCT;

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub label: &'static str,
    /// 0 to 100.
    pub percent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub icon: &'static str,
    /// The earbuds backend's picture set (`Device::art`), "" for none.
    pub art: String,
    pub name: String,
    pub parts: Vec<Part>,
    /// The panel a click opens.
    pub panel: &'static str,
}

fn icon_of(bluez: &str, kind: Option<DeviceKind>) -> &'static str {
    match (bluez, kind) {
        (_, Some(DeviceKind::Headphone)) | ("audio-headphones", _) => "headphones",
        ("audio-speakers", _) => "speaker",
        _ => "headset",
    }
}

/// What the card shows for the device at `address`, off the store: the
/// earbuds backend's parts when one claims it, BlueZ's Battery1 otherwise,
/// and no ring at all when nothing reports a level.
pub fn face(store: &Store, address: &str) -> Face {
    let seen: Option<&Headset> = store
        .devices
        .bluetooth
        .headsets
        .as_ref()
        .and_then(|list| list.iter().find(|h| h.address.eq_ignore_ascii_case(address)));
    // The AirPods daemon's status file carries no address, so a connected
    // device of its own with no address is matched by name.
    let claimed: Option<&Device> = store.devices.earbuds.devices().iter().find(|d| {
        d.address.eq_ignore_ascii_case(address)
            || (d.address.is_empty() && d.connected && seen.is_some_and(|h| !h.name.is_empty() && h.name.eq_ignore_ascii_case(&d.name)))
    });
    let icon = icon_of(seen.map_or("", |h| h.icon.as_str()), claimed.map(|d| d.kind));
    let named = claimed.map(|d| d.name.as_str()).filter(|n| !n.is_empty()).or_else(|| seen.map(|h| h.name.as_str()).filter(|n| !n.is_empty()));
    let name = named.unwrap_or(address).to_owned();
    let mut parts: Vec<Part> = Vec::new();
    if let Some(d) = claimed {
        for id in [BatteryId::Left, BatteryId::Right, BatteryId::Case, BatteryId::Single] {
            if let Some(b) = d.batteries.iter().find(|b| b.id == id).filter(|b| b.level >= 0.0) {
                let label = match id {
                    BatteryId::Left => "Left",
                    BatteryId::Right => "Right",
                    BatteryId::Case => "Case",
                    BatteryId::Single => "",
                };
                parts.push(Part { label, percent: b.level.clamp(0.0, 100.0) });
            }
        }
    }
    if parts.is_empty()
        && let Some(level) = seen.and_then(|h| h.battery)
    {
        parts.push(Part { label: "", percent: (level * 100.0).round() });
    }
    let art = claimed.map(|d| d.art.clone()).unwrap_or_default();
    Face { icon, art, name, parts, panel: if claimed.is_some() { "earbuds" } else { "bluetooth" } }
}

/// The left half: the icon and the name over its status.
fn words(f: &Face, theme: &Theme) -> El {
    let s = &theme.space;
    let size = s.control_height * 2.0;
    let icon = match crate::services::devices::earbuds::picture(&f.art, "duo", size) {
        Some(picture) => w::picture(Some(picture), size),
        None => w::icon(f.icon).size(Type::Heading).ink(Ink::Fg),
    };
    let text = w::column(s.xxs, vec![w::label(f.name.clone()).elide(), w::text("Connected").size(Type::BodySmall).ink(Ink::Dim)]).fill();
    w::row(s.icon_gap, vec![icon, text]).fill()
}

fn style(theme: &Theme, kit: &Kit, mono: bool) -> TextStyle {
    TextStyle { family: if mono { kit.look.mono } else { kit.look.sans }, size: theme.font_size.caption as f32, weight: if mono { 500.0 } else { 400.0 }, tracking: 0.0 }
}

/// The ring's diameter, which is a control's height.
fn diameter(theme: &Theme) -> f64 {
    theme.space.control_height
}

fn stroke(theme: &Theme) -> f64 {
    theme.space.xs
}

/// One column per part: wide enough for its ring and its label.
fn column_width(part: &Part, theme: &Theme, kit: &mut Kit) -> f64 {
    let label = kit.shape(part.label, style(theme, kit, false)).width as f64;
    diameter(theme).max(label)
}

fn rings_width(parts: &[Part], theme: &Theme, kit: &mut Kit) -> f64 {
    if parts.is_empty() {
        return 0.0;
    }
    let widths: f64 = parts.iter().map(|p| column_width(p, theme, kit)).sum();
    widths + theme.space.md * (parts.len() - 1) as f64
}

/// The card's own height: padding round the taller of the words and a ring
/// over its label.
pub fn height(f: &Face, width: f64, theme: &Theme, kit: &mut Kit) -> f64 {
    let s = &theme.space;
    let inner = width - s.panel_padding * 2.0;
    let text = ui::measure(&words(f, theme), inner, theme, kit).1;
    let label = kit.shape("Case", style(theme, kit, false)).line_height() as f64;
    let rings = diameter(theme) + s.xxs + label;
    text.max(rings) + s.panel_padding * 2.0
}

/// The words' element and the rect it is laid in, left of the rings.
pub fn left(f: &Face, inner: Rect, theme: &Theme, kit: &mut Kit) -> (El, Rect) {
    let gap = if f.parts.is_empty() { 0.0 } else { theme.space.lg };
    let rings = rings_width(&f.parts, theme, kit);
    let el = words(f, theme);
    let width = (inner.width() - rings - gap).max(0.0);
    let h = ui::measure(&el, width, theme, kit).1;
    let top = inner.y0 + (inner.height() - h) / 2.0;
    (el, Rect::new(inner.x0, top, inner.x0 + width, top + h))
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    c.with_alpha(c.a * a)
}

/// The rings, right-aligned in `inner`, each a groove under an arc from
/// twelve o'clock with its percentage inside and its part under it.
pub fn paint_rings(p: &mut Painter, alpha: f32, f: &Face, inner: Rect, theme: &Theme, kit: &mut Kit) {
    let s = &theme.space;
    let d = diameter(theme);
    let width = stroke(theme);
    let groove = theme.box_style("track.groove", None).fill;
    let fill = theme.box_style("track.fill", None).fill;
    let ink = theme.colors.get("foreground");
    let muted = theme.colors.get("mutedForeground");
    let low = theme.colors.get("destructive");
    let label_h = kit.shape("Case", style(theme, kit, false)).line_height() as f64;
    let block = d + s.xxs + label_h;
    let top = inner.y0 + (inner.height() - block) / 2.0;
    let mut x = inner.x1 - rings_width(&f.parts, theme, kit);
    for part in &f.parts {
        let cw = column_width(part, theme, kit);
        let (cx, cy) = (x + cw / 2.0, top + d / 2.0);
        let r = (d - width) / 2.0;
        let frac = (part.percent / 100.0).clamp(0.0, 1.0);
        let mut strokes = vec![(Circle::new((cx, cy), r).to_path(0.1), with_alpha(groove, alpha), width)];
        if frac > 0.0 {
            let colour = if part.percent <= LOW { low } else { fill };
            let path = if frac >= 0.999 {
                Circle::new((cx, cy), r).to_path(0.1)
            } else {
                Arc::new((cx, cy), (r, r), -std::f64::consts::FRAC_PI_2, std::f64::consts::TAU * frac, 0.0).to_path(0.1)
            };
            strokes.push((path, with_alpha(colour, alpha), width));
        }
        let bounds = Scene::cover(Rect::new(cx - d / 2.0, cy - d / 2.0, cx + d / 2.0, cy + d / 2.0), Affine::IDENTITY, 2.0);
        p.shape(bounds, Paint::Shape { fill: None, strokes });
        let pct = kit.shape(&format!("{}", part.percent.round() as i64), style(theme, kit, true));
        let at = ((cx - pct.width as f64 / 2.0).round() as i32, (cy - pct.line_height() as f64 / 2.0).round() as i32);
        p.text(&pct, at, with_alpha(ink, alpha), &[]);
        if !part.label.is_empty() {
            let label = kit.shape(part.label, style(theme, kit, false));
            let at = ((cx - label.width as f64 / 2.0).round() as i32, (top + d + s.xxs).round() as i32);
            p.text(&label, at, with_alpha(muted, alpha), &[]);
        }
        x += cw + s.md;
    }
}
