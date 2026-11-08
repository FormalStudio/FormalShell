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

use fs_devices::earbuds::{BatteryId, Device, DeviceKind};
use fs_system::power::model::Thresholds;
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use fs_types::{BtAddress, Percent};
use vello_cpu::kurbo::{Affine, Rect};

use crate::scene::{IRect, Paint, Scene};
use crate::services::devices::headsets::Headset;
use crate::store::Store;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::TextStyle;
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
pub fn face(store: &Store, address: BtAddress) -> Face {
    let seen: Option<&Headset> = store
        .devices
        .bluetooth
        .headsets
        .as_ref()
        .and_then(|list| list.iter().find(|h| h.address == address));
    // The AirPods daemon's status file carries no address, so a connected
    // device of its own with no address is matched by name.
    let claimed: Option<&Device> = store.devices.earbuds.devices().iter().find(|d| {
        d.address == Some(address)
            || (d.address.is_none() && d.connected && seen.is_some_and(|h| !h.name.is_empty() && h.name.eq_ignore_ascii_case(&d.name)))
    });
    let icon = icon_of(seen.map_or("", |h| h.icon.as_str()), claimed.map(|d| d.kind));
    // BlueZ holds the name the device advertises, which some cut short
    // ("CMF Head" for a CMF Headphone Pro), and the backend's own comes up
    // to ~40s after connecting, so the one it gave last stands in until then.
    let name = claimed
        .map(|d| d.name.as_str())
        .filter(|n| !n.is_empty())
        .or_else(|| remembered(store, address, "name"))
        .or_else(|| seen.map(|h| h.name.as_str()).filter(|n| !n.is_empty()))
        .map_or_else(|| address.to_string(), str::to_owned);
    let mut parts: Vec<Part> = Vec::new();
    if let Some(d) = claimed {
        for side in [BatteryId::Left, BatteryId::Right, BatteryId::Case, BatteryId::Single] {
            if let Some(b) = d.batteries.iter().find(|b| b.id == side) {
                parts.push(Part { side, level: b.level });
            }
        }
    }
    if parts.is_empty()
        && let Some(level) = seen.and_then(|h| h.battery)
    {
        parts.push(Part { side: BatteryId::Single, level: level.to_percent().round() });
    }
    // nothingctl takes up to ~40s to name a headphone it has just seen
    // connect, far past the card's hold, so the set it named last time
    // stands in until it does.
    let art = claimed
        .map(|d| d.art.clone())
        .filter(|a| !a.is_empty())
        .or_else(|| remembered(store, address, "art").map(str::to_owned))
        .unwrap_or_default();
    Face { icon, art, name, parts, panel: if claimed.is_some() { "earbuds" } else { "bluetooth" } }
}

/// What the earbuds backend last called `address` (`key` "name") or the
/// picture set it named (`key` "art").
fn remembered<'a>(store: &'a Store, address: BtAddress, key: &str) -> Option<&'a str> {
    store.state.data.earbuds_faces.get(&address.to_string())?.get(key)?.as_str().filter(|s| !s.is_empty())
}

/// The `address -> {art, name}` entries the earbuds backend reports now
/// that state.json does not hold yet: a device's own address, or for one
/// with none (the AirPods daemon's) the connected Bluetooth headset of its
/// name. A set it has not named yet keeps the one remembered.
pub fn learned(store: &Store) -> Vec<(String, serde_json::Value)> {
    let headsets = store.devices.bluetooth.headsets.as_deref().unwrap_or_default();
    store
        .devices
        .earbuds
        .devices()
        .iter()
        .filter(|d| !d.art.is_empty() || !d.name.is_empty())
        .filter_map(|d| {
            let address = match d.address {
                Some(address) => address,
                None => headsets.iter().find(|h| h.connected && !h.name.is_empty() && h.name.eq_ignore_ascii_case(&d.name))?.address,
            };
            let art = if d.art.is_empty() { remembered(store, address, "art").unwrap_or_default() } else { d.art.as_str() };
            let entry = serde_json::json!({"art": art, "name": d.name});
            let key = address.to_string();
            (store.state.data.earbuds_faces.get(&key) != Some(&entry)).then_some((key, entry))
        })
        .collect()
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

/// The capsule's (width, height): the picture's and the ring's slots, the
/// gaps beside the words and the wider of its two lines, by a slot and its
/// padding.
pub fn size(f: &Face, theme: &Theme, kit: &mut Kit) -> (f64, f64) {
    let (p, n) = (pad(theme), slot(theme));
    let mut wide = |el: El| ui::measure(&el, f64::MAX, theme, kit).0;
    let words = wide(w::label(f.name.clone())).max(wide(w::text("Connected")));
    let width = (p + n + theme.space.md) * 2.0 + words;
    (width.clamp(n * 4.0 + p * 2.0, theme.space.popup_width_narrow).ceil(), n + p * 2.0)
}

/// The picture and the words; the ring's slot is held empty for
/// `paint_ring`, so the words sit in the capsule's middle.
fn content(f: &Face, theme: &Theme) -> El {
    let s = &theme.space;
    let size = slot(theme);
    let lead = match crate::services::devices::earbuds::picture(&f.art, "duo", size) {
        Some(picture) => w::picture(Some(picture), size),
        None => w::column(0.0, vec![w::icon(f.icon).size(Type::Heading).ink(Ink::Fg).centred()]).width(Size::Px(size)),
    };
    // Eliding fills a row; held to its own width it still elides and centres.
    let line = |el: El| el.elide().width(Size::AtMost(1.0)).centred();
    let words = w::column(0.0, vec![line(w::label(f.name.clone())), line(w::text("Connected").ink(Ink::Muted))]);
    w::row(s.md, vec![lead, words.fill(), w::space(size)]).mid()
}

/// The content's element and the rect it is laid in, inside the card's
/// `frame` and centred down it.
pub fn layout(f: &Face, frame: IRect, theme: &Theme, kit: &mut Kit) -> (El, Rect) {
    let p = pad(theme);
    let (x0, x1) = (frame.x as f64 + p, (frame.x + frame.w) as f64 - p);
    let el = content(f, theme);
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
    let groove = theme.box_style("track.groove", None).fill;
    let low = Thresholds::HEADSET.low(level).is_some();
    let fill = if low { theme.colors.get("destructive") } else { theme.box_style("track.fill", None).fill };
    let ring = Ring::Progress(level.as_fraction());
    let strokes = ui::ring_strokes((cx, cy), d, theme.space.xs, ring, 0.0, with_alpha(groove, alpha), with_alpha(fill, alpha));
    let bounds = Scene::cover(Rect::new(cx - d / 2.0, cy - d / 2.0, cx + d / 2.0, cy + d / 2.0), Affine::IDENTITY, 2.0);
    p.shape(bounds, Paint::Shape { fill: None, strokes });
    let style = TextStyle { family: kit.look.mono, size: theme.font_size.body_small as f32, weight: 500.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
    let text = kit.shape(&level.rounded().to_string(), style);
    let at = ((cx - text.width as f64 / 2.0).round() as i32, (cy - text.line_height() as f64 / 2.0).round() as i32);
    p.text(&text, at, with_alpha(theme.colors.get("mutedForeground"), alpha), &[]);
}
