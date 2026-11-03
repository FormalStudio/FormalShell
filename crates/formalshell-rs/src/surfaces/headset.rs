//! The headset connect card's content (new in the Rust shell, spec
//! `2026-10-06-rust-rewrite.md` under R7), a compact pill after macOS's
//! AirPods one: the device's product picture (the earbuds panel's `duo`
//! set, its icon for a device with none) beside its name over one small
//! ring and value per part the backend reports, or "Connected" before any
//! does. The card is a plain `card` role box; nothing here invents a
//! colour or a curve.
//!
//! The picture's slot is the same size whether a picture or an icon fills
//! it, so a backend reporting after the card is up widens it and never
//! makes it taller.

use fs_devices::earbuds::{BatteryId, Device, DeviceKind};
use fs_theme::theme::Theme;
use vello_cpu::kurbo::Rect;

use crate::scene::IRect;
use crate::services::devices::headsets::Headset;
use crate::store::Store;
use crate::surfaces::bar::cell::Kit;
use crate::ui::el::{Ring, Size};
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
    // nothingctl takes up to ~40s to name a headphone it has just seen
    // connect, far past the card's hold, so the set it named last time
    // stands in until it does.
    let art = claimed
        .map(|d| d.art.clone())
        .filter(|a| !a.is_empty())
        .or_else(|| remembered(store, address))
        .unwrap_or_default();
    Face { icon, art, name, parts, panel: if claimed.is_some() { "earbuds" } else { "bluetooth" } }
}

/// The picture set the earbuds backend last named for `address`.
fn remembered(store: &Store, address: &str) -> Option<String> {
    store.state.data.earbuds_art.get(address.to_ascii_uppercase())?.as_str().map(str::to_owned)
}

/// The `address -> set` pairs the earbuds backend names now that
/// state.json does not hold yet: a device's own address, or for one with
/// none (the AirPods daemon's) the connected Bluetooth headset of its name.
pub fn learned(store: &Store) -> Vec<(String, String)> {
    let headsets = store.devices.bluetooth.headsets.as_deref().unwrap_or_default();
    store
        .devices
        .earbuds
        .devices()
        .iter()
        .filter(|d| !d.art.is_empty())
        .filter_map(|d| {
            let address = if d.address.is_empty() {
                headsets.iter().find(|h| h.connected && !h.name.is_empty() && h.name.eq_ignore_ascii_case(&d.name))?.address.clone()
            } else {
                d.address.clone()
            };
            let address = address.to_ascii_uppercase();
            (remembered(store, &address).as_deref() != Some(d.art.as_str())).then(|| (address, d.art.clone()))
        })
        .collect()
}

/// The picture's square slot, which sets the pill's height.
fn slot(theme: &Theme) -> f64 {
    theme.space.control_height + theme.space.sm
}

/// The pill's padding: `sm` round the picture, `lg` past the words.
fn pad(theme: &Theme) -> (f64, f64) {
    (theme.space.sm, theme.space.lg)
}

fn part(p: &Part, theme: &Theme) -> El {
    let s = &theme.space;
    let value = w::text(format!("{}%", p.percent.round() as i64))
        .size(Type::Caption)
        .mono()
        .ink(if p.percent <= LOW { Ink::Destructive } else { Ink::Fg });
    let mut row = vec![w::indicator(Ring::Progress((p.percent / 100.0).clamp(0.0, 1.0)))];
    if !p.label.is_empty() {
        row.push(w::text(p.label).size(Type::Caption).ink(Ink::Muted));
    }
    row.push(value);
    w::row(s.xs, row).mid()
}

/// The pill's content: the picture, and the name over the parts.
fn content(f: &Face, theme: &Theme, words: Size) -> El {
    let s = &theme.space;
    let size = slot(theme);
    let lead = match crate::services::devices::earbuds::picture(&f.art, "duo", size) {
        Some(picture) => w::picture(Some(picture), size),
        None => w::column(0.0, vec![w::icon(f.icon).size(Type::Heading).ink(Ink::Fg).centred()]).width(Size::Px(size)),
    };
    let status = if f.parts.is_empty() {
        w::text("Connected").size(Type::Caption).ink(Ink::Muted)
    } else {
        w::row(s.md, f.parts.iter().map(|p| part(p, theme)).collect())
    };
    // Eliding fills the row, so a measure for the content's own width
    // reads the name whole.
    let name = w::label(f.name.clone());
    let name = if words == Size::Fill { name.elide() } else { name };
    let words = w::column(0.0, vec![name, status]).width(words);
    w::row(s.md, vec![lead, words]).mid()
}

/// The pill's (width, height) for `f`: its content's own width, capped at
/// `popupWidthNarrow` with the name eliding past it.
pub fn size(f: &Face, theme: &Theme, kit: &mut Kit) -> (f64, f64) {
    let (inset, end) = pad(theme);
    let cap = theme.space.popup_width_narrow;
    let natural = ui::measure(&content(f, theme, Size::Hug), cap, theme, kit).0;
    ((natural + inset + end).min(cap).ceil(), slot(theme) + inset * 2.0)
}

/// The most room the pill can grow to, which its surface is made for.
pub fn largest(theme: &Theme) -> (f64, f64) {
    (theme.space.popup_width_narrow, slot(theme) + pad(theme).0 * 2.0)
}

/// The content's element and the rect it is laid in, inside the card's
/// `frame` and centred down it.
pub fn layout(f: &Face, frame: IRect, theme: &Theme, kit: &mut Kit) -> (El, Rect) {
    let (inset, end) = pad(theme);
    let (x0, x1) = (frame.x as f64 + inset, (frame.x + frame.w) as f64 - end);
    let el = content(f, theme, Size::Fill);
    let h = ui::measure(&el, x1 - x0, theme, kit).1;
    let top = frame.y as f64 + (frame.h as f64 - h) / 2.0;
    (el, Rect::new(x0, top, x1, top + h))
}
