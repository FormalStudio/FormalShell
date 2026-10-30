//! Resolver for `bar.layout`/`bar.modules` and the strip's geometry. Takes the
//! raw settings.json `bar` object in, returns the resolved regions and
//! warnings out.
//!
//! Each resolved entry is a builtin widget, a user module (looked up by id
//! through the layout name's `custom:` prefix, distinct from the builtin names
//! so a module id can never collide with a widget) or a drop-in plugin (the
//! `plugin:` prefix). Absent or malformed input is never fatal: an unknown
//! widget name, a `custom:` reference with no matching module, or a module
//! with an unrecognised `type` is dropped with one warning string. A region
//! missing from `bar.layout` falls back to [`default_layout`] for that region
//! alone; a region present but empty (`[]`) stays empty.
//!
//! "chevron" is placed like any other builtin and its position is its entire
//! configuration: every entry on its governed side of the same region is
//! annotated `collapsible`. Which side that is depends on the region
//! ([`governs_before`]), so the group always opens away from the region's
//! anchored edge. Two rules keep it from being a control that does nothing:
//! only the first chevron in a region survives, and a chevron with nothing on
//! its governed side is dropped, each with its own warning.
//!
//! A bar plugin whose id appears in no region is appended to the region its
//! manifest asks for, id-sorted, so dropping a plugin directory in is enough
//! to see its cell; an explicit placement always wins, and a plugin named
//! anywhere is never appended twice.

use std::collections::HashMap;

use serde_json::Value;

use fs_js as js;
use crate::plugins::{PLUGIN_PREFIX, Plugin};
use crate::types::{Edge, Insets, Region};

/// The builtin widgets a user can name in `bar.layout`. All but a handful are
/// opt-in and absent from [`default_layout`]: a no-config bar carries only
/// the launcher, workspaces, active window, clock, now playing, battery,
/// audio, network, bluetooth, weather, tray, bell and indicators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Launcher,
    Workspaces,
    ActiveWindow,
    Clock,
    NowPlaying,
    Battery,
    Audio,
    Network,
    Bluetooth,
    Weather,
    Tray,
    Github,
    Usage,
    Tailscale,
    Visualizer,
    Bell,
    Indicators,
    Microphone,
    KeyboardLayout,
    SystemUpdate,
    Chevron,
    Earbuds,
    Dualsense,
    Display,
    Monitor,
    Iphone,
}

impl Builtin {
    pub const ALL: [(Builtin, &'static str); 26] = [
        (Builtin::Launcher, "launcher"),
        (Builtin::Workspaces, "workspaces"),
        (Builtin::ActiveWindow, "activeWindow"),
        (Builtin::Clock, "clock"),
        (Builtin::NowPlaying, "nowPlaying"),
        (Builtin::Battery, "battery"),
        (Builtin::Audio, "audio"),
        (Builtin::Network, "network"),
        (Builtin::Bluetooth, "bluetooth"),
        (Builtin::Weather, "weather"),
        (Builtin::Tray, "tray"),
        (Builtin::Github, "github"),
        (Builtin::Usage, "usage"),
        (Builtin::Tailscale, "tailscale"),
        (Builtin::Visualizer, "visualizer"),
        (Builtin::Bell, "bell"),
        (Builtin::Indicators, "indicators"),
        (Builtin::Microphone, "microphone"),
        (Builtin::KeyboardLayout, "keyboardLayout"),
        (Builtin::SystemUpdate, "systemUpdate"),
        (Builtin::Chevron, "chevron"),
        (Builtin::Earbuds, "earbuds"),
        (Builtin::Dualsense, "dualsense"),
        (Builtin::Display, "display"),
        (Builtin::Monitor, "monitor"),
        (Builtin::Iphone, "iphone"),
    ];

    pub fn parse(name: &str) -> Option<Builtin> {
        Self::ALL.iter().find(|(_, n)| *n == name).map(|(b, _)| *b)
    }

    /// The name a user writes in `bar.layout`.
    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(b, _)| *b == self)
            .map(|(_, n)| *n)
            .expect("every builtin is listed")
    }
}

pub const MODULE_TYPES: [&str; 1] = ["command"];

pub const CUSTOM_PREFIX: &str = "custom:";

/// Today's exact arrangement. "launcher" leads the left region because the
/// menu's only pointer-reachable summon path has to exist without a
/// settings.json edit; "bell" is always visible, so it earns a default slot.
pub fn default_layout(region: Region) -> &'static [Builtin] {
    use Builtin::*;
    match region {
        Region::Left => &[Launcher, Workspaces, ActiveWindow],
        Region::Center => &[Clock, NowPlaying],
        Region::Right => &[
            Battery, Audio, Network, Bluetooth, Weather, Tray, Bell, Indicators,
        ],
    }
}

/// What one resolved entry is.
#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    Builtin(Builtin),
    /// `module` is the matching `bar.modules` entry, verbatim.
    Module {
        id: String,
        module: Value,
    },
    Plugin {
        id: String,
        plugin: Plugin,
    },
}

/// One placed cell. `region` is carried on the entry so a consumer holding
/// one entry can name the region whose collapse state it answers to.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: EntryKind,
    pub region: Region,
    pub collapsible: bool,
}

impl Entry {
    /// How the entry is written in `bar.layout`, the only name a user ever
    /// typed for it and so the only one a status line can honestly report.
    pub fn name(&self) -> String {
        match &self.kind {
            EntryKind::Builtin(b) => b.name().into(),
            EntryKind::Module { id, .. } => format!("{CUSTOM_PREFIX}{id}"),
            EntryKind::Plugin { id, .. } => format!("{PLUGIN_PREFIX}{id}"),
        }
    }

    fn is_chevron(&self) -> bool {
        self.kind == EntryKind::Builtin(Builtin::Chevron)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Regions {
    pub left: Vec<Entry>,
    pub center: Vec<Entry>,
    pub right: Vec<Entry>,
}

impl Regions {
    pub fn get(&self, region: Region) -> &[Entry] {
        match region {
            Region::Left => &self.left,
            Region::Center => &self.center,
            Region::Right => &self.right,
        }
    }

    fn get_mut(&mut self, region: Region) -> &mut Vec<Entry> {
        match region {
            Region::Left => &mut self.left,
            Region::Center => &mut self.center,
            Region::Right => &mut self.right,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub regions: Regions,
    pub warnings: Vec<String>,
}

/// `bar.position`: anything but one of the four edges reads as the default,
/// so a typo in settings.json leaves the bar where it always was.
pub fn position(raw: Option<&str>) -> Edge {
    raw.and_then(Edge::parse).unwrap_or(Edge::Top)
}

pub fn is_vertical(pos: Edge) -> bool {
    pos.is_vertical()
}

/// The spacing tokens the strip is built from, already scaled
/// (`Theme.space`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Space {
    pub bar_cell_height: f64,
    pub bar_cell_width: f64,
    pub bar_margin: f64,
    pub md: f64,
}

/// The strip's own box. The bar is one continuous band along one output edge:
/// the cell row with a `bar_margin` band either side of it, and that whole
/// thickness is also its exclusive zone. `thickness` is the strip's extent
/// across the bar (its height on a top or bottom bar, its width on a left or
/// right one), and the same for `cell_thickness` and `cell_inset`. The inset
/// from the two output edges the strip runs between is `md` rather than
/// `bar_margin`, so a cell sits further from the screen edge than from the
/// strip's own edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripGeometry {
    pub position: Edge,
    pub vertical: bool,
    pub thickness: f64,
    pub cell_thickness: f64,
    pub cell_inset: f64,
    pub edge_inset: f64,
}

pub fn strip_geometry(space: &Space, pos: Option<&str>) -> StripGeometry {
    let resolved = position(pos);
    let vertical = is_vertical(resolved);
    // A vertical strip is wider than a horizontal one is tall, because a cell
    // stacks its icon over its label there instead of running the two along
    // the strip.
    let cell_thickness = if vertical {
        space.bar_cell_width
    } else {
        space.bar_cell_height
    };
    StripGeometry {
        position: resolved,
        vertical,
        thickness: cell_thickness + space.bar_margin * 2.0,
        cell_thickness,
        cell_inset: space.bar_margin,
        edge_inset: space.md,
    }
}

/// How much of each output edge is spoken for: the bar's `thickness` on its
/// own edge, and the screen frame's on the other three (0 when off). The bar
/// covers its own edge whatever the frame is, so the frame never adds to it.
pub fn insets(pos: Option<&str>, thickness: f64, frame_thickness: Option<f64>) -> Insets {
    let resolved = position(pos);
    let frame = frame_thickness.filter(|f| *f > 0.0).unwrap_or(0.0);
    let on = |edge: Edge| if resolved == edge { thickness } else { frame };
    Insets {
        top: on(Edge::Top),
        bottom: on(Edge::Bottom),
        left: on(Edge::Left),
        right: on(Edge::Right),
    }
}

/// The unit vector from the output's centre toward the bar's edge: where a
/// surface that hangs off the bar slides in from.
pub fn edge_vector(pos: Option<&str>) -> (f64, f64) {
    match position(pos) {
        Edge::Bottom => (0.0, 1.0),
        Edge::Left => (-1.0, 0.0),
        Edge::Right => (1.0, 0.0),
        Edge::Top => (0.0, -1.0),
    }
}

/// How a free-running line of text turns on a vertical bar, in degrees. Only
/// text of arbitrary length (the window title, the now-playing track) turns,
/// a left bar reading bottom to top and a right bar top to bottom so the
/// baseline faces the desktop on both. Everything else in a cell stays upright.
pub fn label_rotation(pos: Option<&str>) -> i32 {
    match position(pos) {
        Edge::Left => -90,
        Edge::Right => 90,
        _ => 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitExtent {
    pub extent: f64,
    pub count: usize,
}

/// How much of a rail's own extents fit a room: the sum of the longest prefix
/// of `extents`, `gap` between each pair of counted neighbours, that still
/// fits `room`. An entry of 0 (a hidden or not-yet-shown cell) is skipped
/// rather than ending the prefix, and costs no gap either side; the first
/// entry that does not fit stops the run there, so the answer is always a
/// whole number of cells.
pub fn fit_extent(extents: &[f64], gap: f64, room: f64) -> FitExtent {
    if extents.is_empty() || !(room > 0.0) {
        return FitExtent {
            extent: 0.0,
            count: 0,
        };
    }
    let mut extent = 0.0;
    let mut count = 0;
    for &e in extents {
        if !(e > 0.0) {
            continue;
        }
        let next = extent + if count > 0 { gap } else { 0.0 } + e;
        if next > room {
            break;
        }
        extent = next;
        count += 1;
    }
    FitExtent { extent, count }
}

/// Each region's current extent along the strip, labels included.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rails {
    pub left: f64,
    pub center: f64,
    pub right: f64,
}

/// One free-running label. `extent` is what it draws now, `natural` what it
/// would draw uncapped, `cap` its own ceiling and `min` the least it is left
/// with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Label {
    pub region: Region,
    pub extent: f64,
    pub natural: f64,
    pub cap: f64,
    pub min: f64,
}

fn slot(region: Region) -> usize {
    match region {
        Region::Left => 0,
        Region::Center => 1,
        Region::Right => 2,
    }
}

/// What each free-running label on the strip may draw: the now-playing track
/// and the window title, the two cells whose text has no fixed length and
/// scrolls once it hits this number. `labels` are in keep order, the first
/// one keeping its text longest. Taking each label's own extent back out of
/// its rail makes the answer independent of the labels' current sizes, so
/// handing it out settles in one step instead of chasing itself.
///
/// Three passes:
/// 1. Minimums, out of the room the cells leave and never past it, so a
///    label gives way before an end region hides a whole cell. Each label is
///    first sure of its minimum or an even share of that room, whichever is
///    less, then the minimums fill up in keep order.
/// 2. One common level on top: each label draws up to the level or its own
///    want, at the highest level where every label fits the strip and the
///    centre region still sits at the middle. When the cells and minimums
///    alone already push the centre off the middle, only the strip bounds
///    the level.
/// 3. A label in an end region stops where the centre actually sits: the
///    middle, or wherever the other end region's cells push it. Past the
///    level it may take what room its region still has, up to its want, and
///    the last label up to its cap, so a title that grows between two refits
///    has somewhere to grow into. Only the last: two labels both handed the
///    same leftover overfill the strip until the next refit.
///
/// An unmeasured strip (`along` not above 0) has no answer: every budget is
/// infinite.
pub fn label_budgets(
    along: f64,
    edge_inset: f64,
    gap: f64,
    rails: Rails,
    labels: &[Label],
) -> Vec<f64> {
    let n = labels.len();
    if !(along > 0.0) {
        return vec![f64::INFINITY; n];
    }
    let pos = |v: f64| if v > 0.0 { v } else { 0.0 };
    let mut fixed =
        [rails.left, rails.center, rails.right].map(|v| if v.is_nan() { 0.0 } else { v });
    let mut wants = Vec::with_capacity(n);
    let mut caps = Vec::with_capacity(n);
    for label in labels {
        fixed[slot(label.region)] -= pos(label.extent);
        let cap = pos(label.cap);
        caps.push(cap);
        wants.push(pos(label.natural).min(cap).max(0.0));
    }
    for f in &mut fixed {
        *f = f.max(0.0);
    }

    let inner = along - edge_inset * 2.0 - gap * 2.0;
    let room = inner - fixed[0] - fixed[1] - fixed[2];

    let live = wants.iter().filter(|w| **w > 0.0).count();
    let even = if live > 0 {
        room.max(0.0) / live as f64
    } else {
        0.0
    };
    let leasts: Vec<f64> = (0..n).map(|i| wants[i].min(pos(labels[i].min))).collect();
    let mut floors: Vec<f64> = leasts.iter().map(|l| l.min(even)).collect();
    let mut spent: f64 = floors.iter().sum();
    for i in 0..n {
        let more = (leasts[i] - floors[i]).min((room - spent).max(0.0));
        floors[i] += more;
        spent += more;
    }

    let at_level =
        |level: f64| -> Vec<f64> { (0..n).map(|k| floors[k].max(wants[k].min(level))).collect() };
    let totals = |b: &[f64]| -> [f64; 3] {
        let mut t = fixed;
        for k in 0..n {
            t[slot(labels[k].region)] += b[k];
        }
        t
    };
    let fits = |b: &[f64]| b.iter().sum::<f64>() <= room;
    let centred = |b: &[f64]| {
        let t = totals(b);
        t[1] + 2.0 * t[0].max(t[2]) <= inner
    };
    let hold_centre = centred(&floors);
    let holds = |level: f64| {
        let b = at_level(level);
        fits(&b) && (!hold_centre || centred(&b))
    };

    let top = wants.iter().fold(0.0_f64, |acc, w| acc.max(w.ceil()));
    let mut lo = 0.0;
    if holds(top) {
        lo = top;
    } else if holds(0.0) {
        let mut hi = top;
        while hi - lo > 1.0 {
            let mid = ((lo + hi) / 2.0).floor();
            if holds(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    let level = at_level(lo);
    let mut out = level.clone();

    // The centre's own start and end, held to the middle only while pass 2
    // could hold it there.
    let t = totals(&level);
    let mut start = along - edge_inset - gap - t[2] - t[1];
    let mut end = edge_inset + gap + t[0] + t[1];
    if hold_centre {
        start = ((along - t[1]) / 2.0).min(start);
        end = ((along + t[1]) / 2.0).max(end);
    }
    let ends = [
        (Region::Left, start - gap - edge_inset - fixed[0]),
        (Region::Right, along - edge_inset - gap - end - fixed[2]),
    ];
    for (region, mut avail) in ends {
        let at: Vec<usize> = (0..n).filter(|i| labels[*i].region == region).collect();
        for &i in &at {
            out[i] = out[i].min(avail);
            avail -= out[i];
        }
        for &j in &at {
            let want = if j == n - 1 { caps[j] } else { wants[j] };
            let grow = (want - out[j]).min(avail);
            if grow > 0.0 {
                out[j] += grow;
                avail -= grow;
            }
        }
    }
    out.iter().map(|v| v.max(0.0)).collect()
}

fn module_by_id(modules: &[Value]) -> HashMap<&str, &Value> {
    let mut by_id = HashMap::new();
    for module in modules {
        if let Some(id) = module
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            by_id.insert(id, module);
        }
    }
    by_id
}

fn resolve_region(
    names: &[Value],
    region: Region,
    modules: &HashMap<&str, &Value>,
    plugins: &HashMap<&str, &Plugin>,
    warnings: &mut Vec<String>,
) -> Vec<Entry> {
    let place = |kind| Entry {
        kind,
        region,
        collapsible: false,
    };
    let at = region.as_str();
    let mut resolved = Vec::new();
    for name in names {
        let text = name.as_str();
        if let Some(plugin_id) = text.and_then(|s| s.strip_prefix(PLUGIN_PREFIX)) {
            match plugins.get(plugin_id) {
                None => warnings.push(format!(
                    "bar.layout.{at}: unknown bar plugin \"{plugin_id}\""
                )),
                Some(plugin) => resolved.push(place(EntryKind::Plugin {
                    id: plugin_id.into(),
                    plugin: (*plugin).clone(),
                })),
            }
        } else if let Some(id) = text.and_then(|s| s.strip_prefix(CUSTOM_PREFIX)) {
            match modules.get(id) {
                None => warnings.push(format!("bar.layout.{at}: unknown module \"{id}\"")),
                Some(module) => {
                    if module
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|t| MODULE_TYPES.contains(&t))
                    {
                        resolved.push(place(EntryKind::Module {
                            id: id.into(),
                            module: (*module).clone(),
                        }));
                    } else {
                        warnings.push(format!(
                            "bar.layout.{at}: module \"{id}\" has unknown type \"{}\"",
                            js::str_or_undefined(module.get("type"))
                        ));
                    }
                }
            }
        } else if let Some(builtin) = text.and_then(Builtin::parse) {
            resolved.push(place(EntryKind::Builtin(builtin)));
        } else {
            warnings.push(format!(
                "bar.layout.{at}: unknown widget \"{}\"",
                js::to_str(name)
            ));
        }
    }
    resolved
}

/// Which side of a region's chevron collapses behind it. A region pinned to a
/// screen edge governs inward, away from that edge: the right region is
/// anchored right, so its chevron governs the entries before it and the cells
/// between it and the edge, the chevron itself included, keep their x when
/// the group opens. `center` is anchored to nothing and reflows from both
/// ends whichever side it governs, so it stays on the left region's "after"
/// direction.
pub fn governs_before(region: Region) -> bool {
    region == Region::Right
}

/// Two passes, in this order, because the second depends on what the first
/// leaves behind: a left region's `["clock", "chevron", "chevron"]` loses its
/// trailing chevron as a duplicate, which makes the surviving one trailing in
/// turn, and a chevron that collapses nothing is exactly the dead control the
/// rule exists to prevent. Both passes warn per drop.
fn drop_dead_chevrons(
    entries: Vec<Entry>,
    region: Region,
    warnings: &mut Vec<String>,
) -> Vec<Entry> {
    let at = region.as_str();
    let mut deduped = Vec::new();
    let mut seen = false;
    for entry in entries {
        if entry.is_chevron() {
            if seen {
                warnings.push(format!("bar.layout.{at}: only one chevron per region"));
                continue;
            }
            seen = true;
        }
        deduped.push(entry);
    }
    let before = governs_before(region);
    if !deduped.is_empty() {
        let edge = if before { 0 } else { deduped.len() - 1 };
        if deduped[edge].is_chevron() {
            warnings.push(format!(
                "bar.layout.{at}: chevron has nothing {} it",
                if before { "before" } else { "after" }
            ));
            deduped.remove(edge);
        }
    }
    deduped
}

fn annotate(entries: &mut [Entry], region: Region) {
    let before = governs_before(region);
    let chevron_at = entries.iter().position(Entry::is_chevron);
    for (i, entry) in entries.iter_mut().enumerate() {
        entry.region = region;
        entry.collapsible = chevron_at.is_some_and(|c| if before { i < c } else { i > c });
    }
}

/// The names a region's chevron governs, in layout order. Empty for a region
/// with no chevron; never empty for a region that has one, since a chevron
/// with nothing on its governed side was already dropped.
pub fn collapsed_names(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.collapsible)
        .map(Entry::name)
        .collect()
}

/// The same governed entries, as copies that no longer answer to the chevron.
/// The second bar renders the group through the bar's own region delegate,
/// which collapses anything annotated `collapsible` to nothing, which inside
/// the overflow is exactly the group it is there to show. Clearing the
/// annotation on a copy leaves the strip's own entries untouched.
pub fn overflow_entries(entries: &[Entry]) -> Vec<Entry> {
    entries
        .iter()
        .filter(|e| e.collapsible)
        .map(|e| Entry {
            collapsible: false,
            ..e.clone()
        })
        .collect()
}

pub fn has_chevron(entries: &[Entry]) -> bool {
    entries.iter().any(Entry::is_chevron)
}

/// `bar` is the raw settings.json `bar` object, which may be absent or lack
/// either `layout` or `modules`. `bar_plugins` is the id-sorted bar plugins
/// from the manifest resolver; none keeps every pre-plugin caller's exact
/// previous result.
pub fn resolve(bar: Option<&Value>, bar_plugins: &[Plugin]) -> Resolved {
    let layout = bar.and_then(|b| b.get("layout")).and_then(Value::as_object);
    let modules: &[Value] = bar
        .and_then(|b| b.get("modules"))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let module_ids = module_by_id(modules);
    let plugin_ids: HashMap<&str, &Plugin> =
        bar_plugins.iter().map(|p| (p.id.as_str(), p)).collect();

    let mut regions = Regions::default();
    let mut warnings = Vec::new();

    for region in Region::ALL {
        let names: Vec<Value> = match layout
            .and_then(|l| l.get(region.as_str()))
            .and_then(Value::as_array)
        {
            Some(names) => names.clone(),
            None => default_layout(region)
                .iter()
                .map(|b| Value::String(b.name().into()))
                .collect(),
        };
        *regions.get_mut(region) =
            resolve_region(&names, region, &module_ids, &plugin_ids, &mut warnings);
    }

    // An unnamed bar plugin lands in the region its own manifest asks for, so
    // installing a plugin is a directory drop rather than a directory drop
    // plus a settings.json edit. Explicit placement above always wins.
    for plugin in bar_plugins {
        let placed = Region::ALL.iter().any(|r| {
            regions
                .get(*r)
                .iter()
                .any(|e| matches!(&e.kind, EntryKind::Plugin { id, .. } if *id == plugin.id))
        });
        if !placed {
            let region = plugin.region.unwrap_or(Region::Right);
            regions.get_mut(region).push(Entry {
                kind: EntryKind::Plugin {
                    id: plugin.id.clone(),
                    plugin: plugin.clone(),
                },
                region,
                collapsible: false,
            });
        }
    }

    // After the auto-append, never inside the per-region resolve: an unnamed
    // plugin lands past everything bar.layout listed, so a left or centre
    // chevron the user wrote last is only genuinely last once that pass has
    // run. A right region's chevron governs the other way and no append can
    // land before it.
    for region in Region::ALL {
        let entries = std::mem::take(regions.get_mut(region));
        let mut entries = drop_dead_chevrons(entries, region, &mut warnings);
        annotate(&mut entries, region);
        *regions.get_mut(region) = entries;
    }

    Resolved { regions, warnings }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::plugins::Kind;

    fn names(entries: &[Entry]) -> String {
        entries
            .iter()
            .map(|e| match &e.kind {
                EntryKind::Builtin(b) => b.name().to_string(),
                EntryKind::Module { id, .. } => format!("custom:{id}"),
                EntryKind::Plugin { id, .. } => format!("plugin:{id}"),
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn collapsible(entries: &[Entry]) -> String {
        entries
            .iter()
            .map(|e| if e.collapsible { '1' } else { '0' })
            .collect()
    }

    fn resolve_bar(bar: Value) -> Resolved {
        resolve(Some(&bar), &[])
    }

    fn defaults() -> Resolved {
        resolve(None, &[])
    }

    const DEFAULT_RIGHT: &str = "battery,audio,network,bluetooth,weather,tray,bell,indicators";

    #[test]
    fn default_fallback_when_bar_is_undefined() {
        let r = defaults();
        assert_eq!(names(&r.regions.left), "launcher,workspaces,activeWindow");
        assert_eq!(names(&r.regions.center), "clock,nowPlaying");
        assert_eq!(names(&r.regions.right), DEFAULT_RIGHT);
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn default_fallback_when_layout_key_missing() {
        let r = resolve_bar(json!({}));
        assert_eq!(names(&r.regions.left), "launcher,workspaces,activeWindow");
        assert_eq!(names(&r.regions.right), DEFAULT_RIGHT);
    }

    #[test]
    fn partial_layout_falls_back_per_region() {
        let r = resolve_bar(json!({ "layout": { "left": ["clock"] } }));
        assert_eq!(names(&r.regions.left), "clock");
        assert_eq!(names(&r.regions.center), "clock,nowPlaying");
        assert_eq!(names(&r.regions.right), DEFAULT_RIGHT);
    }

    #[test]
    fn custom_order_is_preserved() {
        let r = resolve_bar(json!({ "layout": { "left": ["activeWindow", "workspaces"] } }));
        assert_eq!(names(&r.regions.left), "activeWindow,workspaces");
    }

    #[test]
    fn empty_region_stays_empty() {
        let r = resolve_bar(json!({ "layout": { "right": [] } }));
        assert_eq!(r.regions.right.len(), 0);
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn unknown_widget_name_is_skipped_with_warning() {
        let r = resolve_bar(json!({ "layout": { "left": ["workspaces", "notreal"] } }));
        assert_eq!(names(&r.regions.left), "workspaces");
        assert_eq!(r.warnings.len(), 1);
        assert_eq!(r.warnings[0], "bar.layout.left: unknown widget \"notreal\"");
    }

    #[test]
    fn non_string_widget_name_is_skipped_with_warning() {
        let r = resolve_bar(json!({ "layout": { "left": [7, null] } }));
        assert_eq!(r.regions.left.len(), 0);
        assert_eq!(
            r.warnings,
            [
                "bar.layout.left: unknown widget \"7\"",
                "bar.layout.left: unknown widget \"null\""
            ]
        );
    }

    #[test]
    fn custom_module_resolves_to_its_definition() {
        let module = json!({ "id": "disk", "type": "command", "command": ["echo", "hi"] });
        let r = resolve_bar(
            json!({ "layout": { "right": ["custom:disk"] }, "modules": [module.clone()] }),
        );
        assert_eq!(r.regions.right.len(), 1);
        assert_eq!(
            r.regions.right[0].kind,
            EntryKind::Module {
                id: "disk".into(),
                module
            }
        );
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn unknown_module_reference_is_skipped_with_warning() {
        let r = resolve_bar(json!({ "layout": { "right": ["custom:missing"] }, "modules": [] }));
        assert_eq!(r.regions.right.len(), 0);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("missing"));
    }

    #[test]
    fn module_with_unknown_type_is_skipped_with_warning() {
        let r = resolve_bar(json!({
            "layout": { "right": ["custom:bad"] },
            "modules": [{ "id": "bad", "type": "shellscript" }]
        }));
        assert_eq!(r.regions.right.len(), 0);
        assert_eq!(
            r.warnings,
            ["bar.layout.right: module \"bad\" has unknown type \"shellscript\""]
        );
    }

    #[test]
    fn module_with_no_type_reads_undefined_in_the_warning() {
        let r = resolve_bar(
            json!({ "layout": { "right": ["custom:bad"] }, "modules": [{ "id": "bad" }] }),
        );
        assert_eq!(
            r.warnings,
            ["bar.layout.right: module \"bad\" has unknown type \"undefined\""]
        );
    }

    fn optin(name: &str) {
        let r = resolve_bar(json!({ "layout": { "right": [name] } }));
        assert_eq!(names(&r.regions.right), name);
        assert_eq!(r.warnings.len(), 0);
        let d = defaults();
        for region in Region::ALL {
            assert!(
                !names(d.regions.get(region)).contains(name),
                "{name} in {region:?}"
            );
        }
    }

    #[test]
    fn optin_builtins_are_absent_from_defaults() {
        for name in [
            "github",
            "usage",
            "tailscale",
            "visualizer",
            "microphone",
            "keyboardLayout",
            "systemUpdate",
            "earbuds",
            "display",
            "monitor",
        ] {
            optin(name);
        }
    }

    #[test]
    fn bell_is_a_default_builtin_before_indicators() {
        let r = defaults();
        let n = names(&r.regions.right);
        let right: Vec<_> = n.split(',').collect();
        let bell = right.iter().position(|n| *n == "bell").unwrap();
        assert_eq!(right[bell + 1], "indicators");
    }

    #[test]
    fn every_builtin_round_trips_its_name() {
        for (b, n) in Builtin::ALL {
            assert_eq!(Builtin::parse(n), Some(b));
            assert_eq!(b.name(), n);
        }
        assert_eq!(Builtin::parse("notreal"), None);
    }

    #[test]
    fn custom_module_id_never_collides_with_builtin_name() {
        let module = json!({ "id": "clock", "type": "command", "command": ["echo", "hi"] });
        let r = resolve_bar(
            json!({ "layout": { "center": ["clock", "custom:clock"] }, "modules": [module] }),
        );
        assert_eq!(r.regions.center.len(), 2);
        assert!(matches!(r.regions.center[0].kind, EntryKind::Builtin(_)));
        assert!(matches!(r.regions.center[1].kind, EntryKind::Module { .. }));
    }

    /// A resolved record as the manifest resolver emits one: every optional
    /// key already defaulted, keys with no meaning for the kind none.
    fn bar_plugin(id: &str, region: Region) -> Plugin {
        Plugin {
            id: id.into(),
            kind: Kind::Bar,
            entry: "E.sh".into(),
            dir: format!("/p/{id}"),
            name: id.into(),
            region: Some(region),
            keep_loaded: None,
            width: None,
            mode: None,
            subjects: None,
            entry_url: format!("file:///p/{id}/E.sh"),
        }
    }

    #[test]
    fn plugin_resolves_to_its_manifest() {
        let p = bar_plugin("diskwatch", Region::Right);
        let r = resolve(
            Some(&json!({ "layout": { "right": ["plugin:diskwatch"] } })),
            std::slice::from_ref(&p),
        );
        assert_eq!(r.regions.right.len(), 1);
        assert_eq!(
            r.regions.right[0].kind,
            EntryKind::Plugin {
                id: "diskwatch".into(),
                plugin: p
            }
        );
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn unknown_plugin_reference_is_skipped_with_warning() {
        let r = resolve(
            Some(&json!({ "layout": { "right": ["plugin:missing"] } })),
            &[],
        );
        assert_eq!(r.regions.right.len(), 0);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("missing"));
    }

    #[test]
    fn plugin_id_never_collides_with_builtin_name() {
        let p = bar_plugin("clock", Region::Center);
        let r = resolve(
            Some(&json!({ "layout": { "center": ["clock", "plugin:clock"] } })),
            &[p],
        );
        assert_eq!(r.regions.center.len(), 2);
        assert!(matches!(r.regions.center[0].kind, EntryKind::Builtin(_)));
        assert!(matches!(r.regions.center[1].kind, EntryKind::Plugin { .. }));
    }

    #[test]
    fn unnamed_bar_plugin_auto_appends_to_its_region() {
        let r = resolve(None, &[bar_plugin("diskwatch", Region::Left)]);
        assert_eq!(r.regions.left.len(), 4);
        assert!(
            matches!(&r.regions.left[3].kind, EntryKind::Plugin { id, .. } if id == "diskwatch")
        );
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn explicitly_placed_plugin_is_not_also_appended() {
        let r = resolve(
            Some(&json!({ "layout": { "right": ["plugin:diskwatch"] } })),
            &[bar_plugin("diskwatch", Region::Left)],
        );
        assert_eq!(r.regions.left.len(), 3);
        assert_eq!(r.regions.right.len(), 1);
        assert!(
            matches!(&r.regions.right[0].kind, EntryKind::Plugin { id, .. } if id == "diskwatch")
        );
    }

    #[test]
    fn governs_before_only_in_the_right_region() {
        assert!(governs_before(Region::Right));
        assert!(!governs_before(Region::Left));
        assert!(!governs_before(Region::Center));
    }

    #[test]
    fn chevron_is_an_optin_builtin_absent_from_defaults() {
        let r = resolve_bar(json!({ "layout": { "right": ["tray", "chevron"] } }));
        assert_eq!(names(&r.regions.right), "tray,chevron");
        assert_eq!(r.warnings.len(), 0);
        let d = defaults();
        for region in Region::ALL {
            assert!(!names(d.regions.get(region)).contains("chevron"));
        }
    }

    #[test]
    fn no_chevron_leaves_every_entry_uncollapsible() {
        let r = defaults();
        assert_eq!(collapsible(&r.regions.left), "000");
        assert_eq!(collapsible(&r.regions.center), "00");
        assert_eq!(collapsible(&r.regions.right), "00000000");
    }

    #[test]
    fn right_region_chevron_marks_only_what_precedes_it() {
        let r = resolve_bar(
            json!({ "layout": { "right": ["battery", "chevron", "weather", "tray"] } }),
        );
        assert_eq!(names(&r.regions.right), "battery,chevron,weather,tray");
        assert_eq!(collapsible(&r.regions.right), "1000");
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn left_region_chevron_marks_only_what_follows_it() {
        let r =
            resolve_bar(json!({ "layout": { "left": ["battery", "chevron", "weather", "tray"] } }));
        assert_eq!(names(&r.regions.left), "battery,chevron,weather,tray");
        assert_eq!(collapsible(&r.regions.left), "0011");
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn center_region_chevron_marks_what_follows_it() {
        let r = resolve_bar(
            json!({ "layout": { "center": ["clock", "chevron", "nowPlaying", "weather"] } }),
        );
        assert_eq!(collapsible(&r.regions.center), "0011");
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn every_entry_carries_its_own_region() {
        let r =
            resolve_bar(json!({ "layout": { "left": ["clock"], "center": ["chevron", "clock"] } }));
        assert_eq!(r.regions.left[0].region, Region::Left);
        assert_eq!(r.regions.center[0].region, Region::Center);
        assert_eq!(r.regions.center[1].region, Region::Center);
        assert_eq!(r.regions.right[0].region, Region::Right);
    }

    #[test]
    fn chevron_against_its_regions_anchored_edge_survives() {
        let r = resolve_bar(
            json!({ "layout": { "right": ["battery", "chevron"], "left": ["chevron", "workspaces"] } }),
        );
        assert_eq!(names(&r.regions.right), "battery,chevron");
        assert_eq!(collapsible(&r.regions.right), "10");
        assert_eq!(names(&r.regions.left), "chevron,workspaces");
        assert_eq!(collapsible(&r.regions.left), "01");
        assert_eq!(r.warnings.len(), 0);
    }

    #[test]
    fn left_region_chevron_placed_last_is_dropped_with_a_warning() {
        let r = resolve_bar(json!({ "layout": { "left": ["workspaces", "chevron"] } }));
        assert_eq!(names(&r.regions.left), "workspaces");
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("nothing after it"));
        assert!(r.warnings[0].contains("bar.layout.left"));
    }

    #[test]
    fn right_region_chevron_placed_first_is_dropped_with_a_warning() {
        let r = resolve_bar(json!({ "layout": { "right": ["chevron", "battery"] } }));
        assert_eq!(names(&r.regions.right), "battery");
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("nothing before it"));
        assert!(r.warnings[0].contains("bar.layout.right"));
    }

    #[test]
    fn lone_chevron_in_a_region_is_dropped_with_a_warning() {
        let r = resolve_bar(json!({ "layout": { "center": ["chevron"], "right": ["chevron"] } }));
        assert_eq!(r.regions.center.len(), 0);
        assert_eq!(r.regions.right.len(), 0);
        assert_eq!(r.warnings.len(), 2);
        assert!(r.warnings[0].contains("nothing after it"));
        assert!(r.warnings[1].contains("nothing before it"));
    }

    #[test]
    fn second_chevron_in_a_region_is_dropped_with_a_warning() {
        let r =
            resolve_bar(json!({ "layout": { "left": ["chevron", "battery", "chevron", "tray"] } }));
        assert_eq!(names(&r.regions.left), "chevron,battery,tray");
        assert_eq!(collapsible(&r.regions.left), "011");
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("only one chevron per region"));
    }

    // The dedupe pass runs first, so the survivor can itself end up on the
    // region's anchored edge and has to be dropped by the second pass with
    // its own warning.
    #[test]
    fn duplicate_chevron_that_leaves_a_dead_survivor_drops_both() {
        let r = resolve_bar(json!({ "layout": { "left": ["battery", "chevron", "chevron"] } }));
        assert_eq!(names(&r.regions.left), "battery");
        assert_eq!(r.warnings.len(), 2);
        assert!(r.warnings[0].contains("only one chevron per region"));
        assert!(r.warnings[1].contains("nothing after it"));
    }

    #[test]
    fn right_region_duplicate_chevron_that_leaves_a_dead_survivor_drops_both() {
        let r = resolve_bar(json!({ "layout": { "right": ["chevron", "chevron", "battery"] } }));
        assert_eq!(names(&r.regions.right), "battery");
        assert_eq!(r.warnings.len(), 2);
        assert!(r.warnings[0].contains("only one chevron per region"));
        assert!(r.warnings[1].contains("nothing before it"));
    }

    #[test]
    fn chevron_in_one_region_does_not_mark_another() {
        let r =
            resolve_bar(json!({ "layout": { "left": ["chevron", "workspaces", "activeWindow"] } }));
        assert_eq!(collapsible(&r.regions.left), "011");
        assert_eq!(collapsible(&r.regions.center), "00");
        assert_eq!(collapsible(&r.regions.right), "00000000");
    }

    // An unnamed bar plugin is appended after everything bar.layout listed,
    // so a left or center chevron written last is only genuinely last once
    // that pass is done.
    #[test]
    fn auto_appended_plugin_saves_an_otherwise_trailing_chevron() {
        let r = resolve(
            Some(&json!({ "layout": { "left": ["battery", "chevron"] } })),
            &[bar_plugin("diskwatch", Region::Left)],
        );
        assert_eq!(r.warnings.len(), 0);
        assert_eq!(r.regions.left.len(), 3);
        assert_eq!(collapsible(&r.regions.left), "001");
    }

    // The same append lands on the wrong side to save a right region's
    // chevron, which governs the other way.
    #[test]
    fn auto_appended_plugin_cannot_save_a_right_region_chevron() {
        let r = resolve(
            Some(&json!({ "layout": { "right": ["chevron", "battery"] } })),
            &[bar_plugin("diskwatch", Region::Right)],
        );
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("nothing before it"));
        assert_eq!(r.regions.right.len(), 2);
        assert_eq!(collapsible(&r.regions.right), "00");
    }

    #[test]
    fn collapsed_names_reports_layout_names_in_order() {
        let module = json!({ "id": "disk", "type": "command", "command": ["echo", "hi"] });
        let r = resolve(
            Some(&json!({
                "layout": { "right": ["custom:disk", "plugin:diskwatch", "tray", "chevron", "battery"] },
                "modules": [module]
            })),
            &[bar_plugin("diskwatch", Region::Right)],
        );
        assert_eq!(
            collapsed_names(&r.regions.right).join(","),
            "custom:disk,plugin:diskwatch,tray"
        );
        assert_eq!(collapsed_names(&r.regions.left).len(), 0);
    }

    #[test]
    fn has_chevron_answers_per_region() {
        let r = resolve_bar(json!({ "layout": { "right": ["battery", "chevron", "tray"] } }));
        assert!(has_chevron(&r.regions.right));
        assert!(!has_chevron(&r.regions.left));
        assert!(!has_chevron(&r.regions.center));
    }

    // tst_bar_chevron_overflow: what the chevron's second bar is handed.

    #[test]
    fn overflow_entries_are_the_group_with_the_gate_cleared() {
        let r = resolve_bar(
            json!({ "layout": { "right": ["bluetooth", "weather", "chevron", "battery", "audio"] } }),
        );
        let entries = overflow_entries(&r.regions.right);
        assert_eq!(entries.len(), 2);
        assert_eq!(names(&entries), "bluetooth,weather");
        for e in &entries {
            assert!(!e.collapsible);
            assert_eq!(e.region, Region::Right);
            assert!(matches!(e.kind, EntryKind::Builtin(_)));
        }
        // Copies: the strip's own entries stay annotated, which is what keeps
        // them off it.
        assert_eq!(
            collapsed_names(&r.regions.right).join(","),
            "bluetooth,weather"
        );
    }

    #[test]
    fn overflow_governed_side_follows_the_region() {
        let r = resolve_bar(
            json!({ "layout": { "left": ["launcher", "chevron", "workspaces", "activeWindow"] } }),
        );
        assert_eq!(
            names(&overflow_entries(&r.regions.left)),
            "workspaces,activeWindow"
        );
    }

    #[test]
    fn a_region_with_no_chevron_has_no_overflow() {
        let r = resolve_bar(json!({ "layout": { "right": ["battery", "audio"] } }));
        assert_eq!(overflow_entries(&r.regions.right).len(), 0);
    }

    // A module entry carries more than a name, and the copy has to keep all
    // of it.
    #[test]
    fn a_module_entry_survives_the_copy_whole() {
        let r = resolve_bar(json!({
            "layout": { "right": ["custom:cpu", "chevron", "audio"] },
            "modules": [{ "id": "cpu", "type": "command", "command": ["true"], "interval": 1000 }]
        }));
        let entries = overflow_entries(&r.regions.right);
        assert_eq!(entries.len(), 1);
        match &entries[0].kind {
            EntryKind::Module { id, module } => {
                assert_eq!(id, "cpu");
                assert_eq!(module["type"], "command");
            }
            other => panic!("not a module: {other:?}"),
        }
        assert!(!entries[0].collapsible);
    }

    // The strip's box.

    const SPACE: Space = Space {
        bar_cell_height: 28.0,
        bar_cell_width: 44.0,
        bar_margin: 6.0,
        md: 12.0,
    };

    #[test]
    fn strip_is_the_cell_row_plus_a_margin_band() {
        let g = strip_geometry(&SPACE, Some("top"));
        assert_eq!(g.thickness, 40.0);
        assert_eq!(g.cell_thickness, 28.0);
        assert_eq!(g.cell_inset, 6.0);
        assert_eq!(g.position, Edge::Top);
        assert!(!g.vertical);
    }

    #[test]
    fn strip_stands_up_on_a_side_edge() {
        let left = strip_geometry(&SPACE, Some("left"));
        assert!(left.vertical);
        assert_eq!(left.position, Edge::Left);
        assert!(strip_geometry(&SPACE, Some("right")).vertical);
        assert!(!strip_geometry(&SPACE, Some("bottom")).vertical);
    }

    // A cell stacks its icon over its label on a side edge, so the strip is
    // `bar_cell_width` wide there rather than `bar_cell_height` tall.
    #[test]
    fn a_side_strip_takes_the_wider_cell() {
        assert_eq!(strip_geometry(&SPACE, Some("left")).cell_thickness, 44.0);
        assert_eq!(strip_geometry(&SPACE, Some("left")).thickness, 56.0);
        assert_eq!(strip_geometry(&SPACE, Some("right")).thickness, 56.0);
        assert_eq!(strip_geometry(&SPACE, Some("top")).cell_thickness, 28.0);
        assert_eq!(strip_geometry(&SPACE, Some("top")).thickness, 40.0);
    }

    #[test]
    fn strip_insets_the_regions_by_md_not_by_the_margin_band() {
        let g = strip_geometry(&SPACE, Some("top"));
        assert_eq!(g.edge_inset, 12.0);
        assert!(g.edge_inset > g.cell_inset);
    }

    #[test]
    fn strip_follows_the_spacing_scale() {
        let space = Space {
            bar_cell_height: 42.0,
            bar_cell_width: 66.0,
            bar_margin: 9.0,
            md: 18.0,
        };
        let g = strip_geometry(&space, Some("top"));
        assert_eq!(g.thickness, 60.0);
        assert_eq!(g.cell_thickness, 42.0);
        assert_eq!(g.cell_inset, 9.0);
        assert_eq!(g.edge_inset, 18.0);
    }

    // bar.position: a typo, an absent key or a stale value all leave the bar
    // where it always was.
    #[test]
    fn an_unknown_position_is_the_top() {
        assert_eq!(position(None), Edge::Top);
        assert_eq!(position(Some("middle")), Edge::Top);
        assert_eq!(position(Some("bottom")), Edge::Bottom);
        assert_eq!(position(Some("left")), Edge::Left);
        assert_eq!(position(Some("right")), Edge::Right);
        let space = Space {
            bar_cell_height: 28.0,
            bar_margin: 6.0,
            md: 12.0,
            ..Space::default()
        };
        assert_eq!(strip_geometry(&space, Some("sideways")).position, Edge::Top);
        assert!(is_vertical(Edge::Left));
        assert!(!is_vertical(Edge::Top));
    }

    fn ins(top: f64, bottom: f64, left: f64, right: f64) -> Insets {
        Insets {
            top,
            bottom,
            left,
            right,
        }
    }

    #[test]
    fn insets_put_the_thickness_on_the_bars_own_edge() {
        assert_eq!(insets(Some("top"), 40.0, None), ins(40.0, 0.0, 0.0, 0.0));
        assert_eq!(insets(Some("bottom"), 40.0, None), ins(0.0, 40.0, 0.0, 0.0));
        assert_eq!(insets(Some("left"), 40.0, None), ins(0.0, 0.0, 40.0, 0.0));
        assert_eq!(insets(Some("right"), 40.0, None), ins(0.0, 0.0, 0.0, 40.0));
        assert_eq!(
            insets(Some("nowhere"), 40.0, None),
            ins(40.0, 0.0, 0.0, 0.0)
        );
    }

    // With the screen frame on, its band takes the other three edges; the
    // bar's own edge is the bar's whatever the band is.
    #[test]
    fn insets_carry_the_frame_on_the_other_three_edges() {
        assert_eq!(
            insets(Some("left"), 40.0, Some(10.0)),
            ins(10.0, 10.0, 40.0, 10.0)
        );
        assert_eq!(
            insets(Some("top"), 40.0, Some(0.0)),
            ins(40.0, 0.0, 0.0, 0.0)
        );
        assert_eq!(insets(Some("top"), 40.0, None), ins(40.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn the_edge_vector_points_at_the_bar() {
        assert_eq!(edge_vector(Some("top")), (0.0, -1.0));
        assert_eq!(edge_vector(Some("bottom")), (0.0, 1.0));
        assert_eq!(edge_vector(Some("left")), (-1.0, 0.0));
        assert_eq!(edge_vector(Some("right")), (1.0, 0.0));
    }

    #[test]
    fn only_free_text_turns_and_only_on_a_side_edge() {
        assert_eq!(label_rotation(Some("left")), -90);
        assert_eq!(label_rotation(Some("right")), 90);
        assert_eq!(label_rotation(Some("top")), 0);
        assert_eq!(label_rotation(Some("bottom")), 0);
    }

    // fit_extent: an end region's cap, snapped to a cell boundary rather than
    // a raw pixel budget.

    fn fit(extents: &[f64], gap: f64, room: f64) -> (f64, usize) {
        let f = fit_extent(extents, gap, room);
        (f.extent, f.count)
    }

    #[test]
    fn fit_extent_empty_list_is_nothing() {
        assert_eq!(fit(&[], 8.0, 100.0), (0.0, 0));
    }

    #[test]
    fn fit_extent_everything_fits() {
        assert_eq!(fit(&[10.0, 20.0, 30.0], 5.0, 100.0), (70.0, 3));
    }

    #[test]
    fn fit_extent_only_the_first_fits() {
        assert_eq!(fit(&[10.0, 20.0], 5.0, 12.0), (10.0, 1));
    }

    #[test]
    fn fit_extent_none_fit() {
        assert_eq!(fit(&[10.0, 20.0], 5.0, 5.0), (0.0, 0));
    }

    #[test]
    fn fit_extent_zero_entries_are_skipped_not_counted() {
        assert_eq!(fit(&[0.0, 10.0, 0.0, 20.0], 5.0, 100.0), (35.0, 2));
    }

    #[test]
    fn fit_extent_gap_arithmetic_is_exact_at_the_boundary() {
        // 10 + 5 + 10 == 25, so the room is fully spent and both still fit.
        assert_eq!(fit(&[10.0, 10.0], 5.0, 25.0), (25.0, 2));
        // One pixel short of the same sum drops the second entry.
        assert_eq!(fit(&[10.0, 10.0], 5.0, 24.0), (10.0, 1));
    }

    #[test]
    fn fit_extent_negative_room_is_nothing() {
        assert_eq!(fit(&[10.0, 20.0], 5.0, -5.0), (0.0, 0));
    }

    // label_budgets: the room the track and the title may draw in. A 1920
    // strip with a 12px end inset and an 8px gap, the rig's own numbers.

    const INF: f64 = f64::INFINITY;

    fn title(extent: f64, natural: f64) -> Label {
        Label {
            region: Region::Left,
            extent,
            natural,
            cap: INF,
            min: 0.0,
        }
    }

    fn rails(left: f64, center: f64, right: f64) -> Rails {
        Rails {
            left,
            center,
            right,
        }
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn label_budgets_unmeasured_strip_is_no_answer() {
        let b = label_budgets(0.0, 12.0, 8.0, rails(0.0, 0.0, 0.0), &[title(0.0, 300.0)]);
        assert_eq!(b.len(), 1);
        assert!(!b[0].is_finite());
    }

    #[test]
    fn label_budgets_end_label_stops_at_the_centred_centre() {
        // Centre 100 wide at the middle starts at 910; the left cells end at
        // 12 + 100, so the title has 910 - 8 - 112 = 790 before the gap.
        let b = label_budgets(
            1920.0,
            12.0,
            8.0,
            rails(400.0, 100.0, 200.0),
            &[title(300.0, 2000.0)],
        );
        close(b[0], 790.0);
    }

    #[test]
    fn label_budgets_short_label_keeps_room_to_grow() {
        let b = label_budgets(
            1920.0,
            12.0,
            8.0,
            rails(400.0, 100.0, 200.0),
            &[title(300.0, 300.0)],
        );
        close(b[0], 790.0);
    }

    #[test]
    fn label_budgets_do_not_depend_on_the_labels_own_extent() {
        let a = label_budgets(
            1920.0,
            12.0,
            8.0,
            rails(400.0, 100.0, 200.0),
            &[title(300.0, 2000.0)],
        );
        let b = label_budgets(
            1920.0,
            12.0,
            8.0,
            rails(890.0, 100.0, 200.0),
            &[title(790.0, 2000.0)],
        );
        close(b[0], a[0]);
    }

    #[test]
    fn label_budgets_title_yields_before_the_track() {
        // 520px left for labels once every cell and gap is paid: the track
        // keeps its whole 220 and the title scrolls in the 300 after it.
        let track = Label {
            region: Region::Center,
            extent: 220.0,
            natural: 400.0,
            cap: 220.0,
            min: 160.0,
        };
        let t = Label {
            region: Region::Left,
            extent: 500.0,
            natural: 2000.0,
            cap: INF,
            min: 88.0,
        };
        let b = label_budgets(1920.0, 12.0, 8.0, rails(600.0, 280.0, 1200.0), &[track, t]);
        close(b[0], 220.0);
        close(b[1], 300.0);
    }

    #[test]
    fn label_budgets_only_the_last_label_takes_the_leftover() {
        // Both fit with room to spare; the track keeps what it draws and the
        // title alone may grow into the rest, so the two never both spend it.
        let track = Label {
            region: Region::Center,
            extent: 100.0,
            natural: 100.0,
            cap: 220.0,
            min: 0.0,
        };
        let b = label_budgets(
            1920.0,
            12.0,
            8.0,
            rails(400.0, 200.0, 200.0),
            &[track, title(300.0, 300.0)],
        );
        close(b[0], 100.0);
        close(b[1], 740.0);
    }

    #[test]
    fn label_budgets_nothing_when_the_cells_alone_overflow() {
        let track = Label {
            region: Region::Center,
            extent: 100.0,
            natural: 400.0,
            cap: 220.0,
            min: 0.0,
        };
        let b = label_budgets(
            1000.0,
            12.0,
            8.0,
            rails(300.0, 160.0, 900.0),
            &[track, title(200.0, 2000.0)],
        );
        close(b[0], 0.0);
        close(b[1], 0.0);
    }

    #[test]
    fn label_budgets_minimums_give_way_before_a_cell_hides() {
        let track = Label {
            region: Region::Center,
            extent: 100.0,
            natural: 400.0,
            cap: 220.0,
            min: 160.0,
        };
        let t = Label {
            region: Region::Left,
            extent: 200.0,
            natural: 2000.0,
            cap: 230.0,
            min: 88.0,
        };
        let b = label_budgets(1000.0, 12.0, 8.0, rails(300.0, 160.0, 900.0), &[track, t]);
        close(b[0], 0.0);
        close(b[1], 0.0);
    }

    #[test]
    fn label_budgets_short_room_splits_evenly() {
        // 76px for labels, under either minimum: half each.
        let track = Label {
            region: Region::Center,
            extent: 0.0,
            natural: 400.0,
            cap: 220.0,
            min: 160.0,
        };
        let t = Label {
            region: Region::Left,
            extent: 0.0,
            natural: 2000.0,
            cap: 230.0,
            min: 88.0,
        };
        let b = label_budgets(1920.0, 12.0, 8.0, rails(800.0, 200.0, 804.0), &[track, t]);
        close(b[0], 38.0);
        close(b[1], 38.0);
    }

    #[test]
    fn label_budgets_track_minimum_before_the_title_grows() {
        // 200px for labels: an even 100 each, the title needs only its 88,
        // and the 12 left tops the track up; the title grows no further.
        let track = Label {
            region: Region::Center,
            extent: 0.0,
            natural: 400.0,
            cap: 220.0,
            min: 160.0,
        };
        let t = Label {
            region: Region::Left,
            extent: 0.0,
            natural: 2000.0,
            cap: 230.0,
            min: 88.0,
        };
        let b = label_budgets(1920.0, 12.0, 8.0, rails(800.0, 200.0, 680.0), &[track, t]);
        close(b[0], 112.0);
        close(b[1], 88.0);
    }

    // The owner's laptop: a 1460 top bar, launcher, workspaces and the
    // title's cell on the left (300 without its title), the clock and the
    // track's cell in the centre (193 without its title), a right region of
    // 480, and a browser tab title far past its 230 ceiling. The track draws
    // all of its 172 and the title scrolls in what the centred centre leaves
    // it, ending one gap short of the clock.
    fn owner(track_natural: f64) -> Vec<f64> {
        let track = Label {
            region: Region::Center,
            extent: 0.0,
            natural: track_natural,
            cap: 219.0,
            min: 160.0,
        };
        let t = Label {
            region: Region::Left,
            extent: 385.0,
            natural: 1400.0,
            cap: 230.0,
            min: 88.0,
        };
        label_budgets(1460.0, 12.0, 8.0, rails(685.0, 193.0, 480.0), &[track, t])
    }

    #[test]
    fn label_budgets_owner_long_title_keeps_the_track() {
        let b = owner(172.0);
        close(b[0], 172.0);
        close(b[1], 227.5);
        let centre = 193.0 + b[0];
        close((1460.0 - centre) / 2.0, 12.0 + 300.0 + b[1] + 8.0);
    }

    #[test]
    fn label_budgets_owner_two_long_labels_split_evenly() {
        let b = owner(400.0);
        close(b[0], 209.0);
        close(b[1], 209.0);
        assert!(193.0 + b[0] + 2.0 * (300.0 + b[1]).max(480.0) <= 1460.0 - 40.0);
    }

    #[test]
    fn label_budgets_title_stops_at_its_ceiling_on_an_empty_strip() {
        let t = Label {
            region: Region::Left,
            extent: 100.0,
            natural: 2000.0,
            cap: 230.0,
            min: 88.0,
        };
        let b = label_budgets(1920.0, 12.0, 8.0, rails(200.0, 100.0, 200.0), &[t]);
        close(b[0], 230.0);
    }

    #[test]
    fn label_budgets_right_region_label_is_the_mirror() {
        let right = Label {
            region: Region::Right,
            extent: 300.0,
            natural: 2000.0,
            cap: INF,
            min: 0.0,
        };
        let b = label_budgets(1920.0, 12.0, 8.0, rails(200.0, 100.0, 400.0), &[right]);
        close(b[0], 790.0);
    }

    #[test]
    fn label_budgets_centre_label_takes_its_own_cap() {
        let track = Label {
            region: Region::Center,
            extent: 50.0,
            natural: 400.0,
            cap: 220.0,
            min: 0.0,
        };
        let b = label_budgets(1920.0, 12.0, 8.0, rails(200.0, 110.0, 200.0), &[track]);
        close(b[0], 220.0);
    }
}
