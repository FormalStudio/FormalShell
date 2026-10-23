//! Drawn from the active device alone, never branching on
//! vendor. A device with a picture set (`Device::art`) leads with its
//! product picture and gives each bud and the case a tile of its own. A `choice` control is a wrapping `ButtonGroup`, a `toggle` a
//! `Switch` row, a `range` a track, each under its control's section in the
//! order the adapter listed them. No backend source at all is the dim "No
//! daemon" row; a source with no device is "No earbuds connected"; with more
//! than one device a choice of them heads the panel.
//!
//! One cursor walks the device choice and then every control. On a group
//! Left and Right move its ring and Enter presses the button under it; on a
//! range they step by the control's `step`. Every write goes through
//! `Earbuds::plan`, the same allow-list the `earbuds set` IPC uses (M77).
//! The panel holds the service while it is open (`wayland.rs`'s sync_open).

use std::collections::HashMap;
use std::path::PathBuf;

use fs_devices::earbuds::{self as model, BatteryId, Control, ControlKind, Device, Value};

use super::{Effect, Panel, View};
use crate::services::devices::{self, earbuds};
use crate::store::{self, Topic};
use crate::ui::el::Opt;
use crate::services::icons;
use crate::ui::{El, Event, Ink, Type, Weight, What, w};

const DEVICE: &str = "__device";

#[derive(Default)]
pub struct Earbuds {
    /// Control key to its ring's place, once the ring has moved.
    rings: HashMap<String, usize>,
}

/// A choice's options and selected index, the device choice included.
fn choice<'a>(v: &'a earbuds::Earbuds, key: &str) -> Option<(Vec<(String, String, String)>, Option<usize>)> {
    if key == DEVICE {
        let active = v.active().map(|d| d.key.clone());
        let opts: Vec<_> = v.devices().iter().map(|d| (d.key.clone(), d.name.clone(), String::new())).collect();
        let index = opts.iter().position(|o| Some(&o.0) == active.as_ref());
        return Some((opts, index));
    }
    let ctl = model::control(v.active(), key).filter(|c| c.kind == ControlKind::Choice)?;
    let opts = ctl.options.as_deref().unwrap_or_default().iter().map(|o| (value_text(&o.value), o.label.clone(), o.icon.clone())).collect();
    Some((opts, model::option_index(Some(ctl))))
}

/// A number as JavaScript prints it: whole values carry no fraction.
fn num(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }
}

/// The raw string `Earbuds::plan` coerces back into the control's type.
fn value_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => num(*n),
        Value::Str(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Other => String::new(),
    }
}

fn set(fx: &Effect, key: &str, raw: String) {
    if key == DEVICE {
        fx.service(move |ctx| ctx.publish(store::Diff::Devices(devices::Diff::Earbuds(earbuds::Diff::Select(raw)))));
        return;
    }
    match fx.store.devices.earbuds.plan(key, &raw) {
        Ok(Some(op)) => earbuds::write(op),
        Ok(None) => {}
        Err(reason) => eprintln!("earbuds: {key}: {reason}"),
    }
}

impl Earbuds {
    fn ring(&self, key: &str, opts: usize, selected: Option<usize>) -> usize {
        self.rings.get(key).copied().filter(|i| *i < opts).unwrap_or(selected.unwrap_or(0))
    }

    fn group(&self, key: &str, opts: &[(String, String, String)], selected: Option<usize>, values: Option<&Control>) -> El {
        let options = opts
            .iter()
            .map(|(value, label, icon)| {
                let mut o = Opt::new(label.clone()).icon(icon.clone());
                o.active = values.is_some_and(|c| value_text(&c.value) == *value);
                o
            })
            .collect();
        // An index past the end selects nothing, for a value no option holds.
        let index = selected.unwrap_or(usize::MAX);
        w::group(options, index, true).ring(self.ring(key, opts.len(), selected)).wrap().stop(key.to_owned()).on(format!("pick:{key}"))
    }

    /// A press writes the value and moves only the ring: the selection
    /// follows once the device reports it.
    fn press(&mut self, fx: &Effect, key: &str, index: usize) {
        let Some((opts, selected)) = choice(&fx.store.devices.earbuds, key) else { return };
        let Some(o) = opts.get(index) else { return };
        self.rings.insert(key.to_owned(), index);
        if Some(index) != selected {
            set(fx, key, o.0.clone());
        }
    }

    fn step_range(fx: &Effect, ctl: &Control, direction: f64) {
        if let Value::Num(n) = ctl.value {
            set(fx, &ctl.key, num(n + direction * ctl.step));
        }
    }
}

/// One part's picture (`duo`, `case`, `left`, `right`) in a `size` square;
/// none without `FS_EARBUDS_ART`, a set, or that part.
fn art(dev: &Device, part: &str, size: f64) -> Option<crate::scene::Bitmap> {
    if dev.art.is_empty() {
        return None;
    }
    let dir = PathBuf::from(std::env::var_os("FS_EARBUDS_ART")?);
    icons::load_at(&dir.join(format!("{}_{part}.webp", dev.art)), size.round().max(1.0) as u32, crate::scene::raster_scale())
}

fn percent(level: f64) -> String {
    format!("{}%", num(level))
}

/// The product picture beside the name and what it is doing; a headphone's
/// one battery sits under them, its level over a track.
fn hero(s: &fs_theme::tokens::Space, dev: &Device, picture: crate::scene::Bitmap) -> El {
    let size = s.control_height * 3.0;
    let mut words = vec![w::text(&dev.name).size(Type::Subtitle).elide()];
    if !dev.state_line.is_empty() {
        words.push(w::text(&dev.state_line).size(Type::BodySmall).ink(Ink::Dim).elide());
    }
    if let [b] = dev.batteries.as_slice()
        && b.id == BatteryId::Single
    {
        let mut level = vec![w::text(percent(b.level)).size(Type::Title).weight(Weight::Medium)];
        if b.charging {
            level.push(w::icon("zap").ink(Ink::Dim));
        }
        words.push(w::space(s.row_gap));
        words.push(w::row(s.xxs, level));
        words.push(w::track(b.level / 100.0));
    }
    w::row(s.section_gap, vec![w::picture(Some(picture), size), w::column(s.xxs, words).fill().mid()]).fill()
}

/// A tile per bud and the case: its picture, its level over a track, a
/// bolt while it charges, and "In ear" under a bud that is.
fn battery_tiles(s: &fs_theme::tokens::Space, dev: &Device) -> El {
    let size = s.control_height * 1.75;
    let tiles = dev
        .batteries
        .iter()
        .map(|b| {
            let part = match b.id {
                BatteryId::Left => "left",
                BatteryId::Right => "right",
                BatteryId::Case => "case",
                BatteryId::Single => "duo",
            };
            let mut level = vec![w::text(percent(b.level)).size(Type::Body).weight(Weight::Medium)];
            if b.charging {
                level.push(w::icon("zap").ink(Ink::Dim));
            }
            let word = if b.in_ear == Some(true) { format!("{} · In ear", b.label) } else { b.label.clone() };
            let parts = vec![
                w::picture(art(dev, part, size), size).centred(),
                w::row(s.xxs, level).centred(),
                w::track(b.level / 100.0),
                w::caption(word).centred(),
            ];
            w::cell(w::column(s.xxs, parts).fill())
        })
        .collect::<Vec<_>>();
    w::grid(tiles.len().max(1), s.row_gap, tiles)
}

fn toggle_row(s: &fs_theme::tokens::Space, ctl: &Control) -> El {
    let on = ctl.value == Value::Bool(true);
    let mut words = vec![w::label(&ctl.label).elide()];
    if !ctl.hint.is_empty() {
        words.push(w::text(&ctl.hint).size(Type::BodySmall).ink(Ink::Dim).elide());
    }
    let parts = vec![w::column(s.xxs, words).fill(), w::switch(on).on(format!("toggle:{}", ctl.key))];
    w::cell(w::row(s.icon_gap, parts).fill()).ghost().interactive().stop(ctl.key.clone()).on(format!("toggle:{}", ctl.key))
}

fn range_row(s: &fs_theme::tokens::Space, ctl: &Control) -> El {
    let top = vec![w::label(&ctl.label).elide(), w::value(model::range_text(Some(ctl))).size(Type::Body).weight(Weight::Medium).ink(Ink::Fg)];
    let track = w::slider(model::range_fraction(Some(ctl))).on(format!("range:{}", ctl.key));
    w::cell(w::column(s.xxs, vec![w::row(s.icon_gap, top).fill(), track])).ghost().stop(ctl.key.clone())
}

/// The plain battery rows, for a device with no pictures.
fn battery_section(s: &fs_theme::tokens::Space, dev: &Device) -> Option<El> {
    let batteries = model::battery_rows(Some(dev));
    if batteries.is_empty() {
        return None;
    }
    let rows = batteries
        .iter()
        .map(|b| {
            let mut value = Vec::new();
            if !b.hint.is_empty() {
                value.push(w::section_label(s, &b.hint, None, false).ink(Ink::Dim));
            }
            value.push(w::value(percent(b.level)).size(Type::Body).weight(Weight::Medium).ink(Ink::Fg));
            let top = vec![w::label(&b.label).elide(), w::row(s.icon_gap, value)];
            w::cell(w::column(s.xxs, vec![w::row(s.icon_gap, top).fill(), w::track(b.level / 100.0)])).ghost()
        })
        .collect();
    Some(w::column(s.row_gap, vec![w::section_label(s, "Battery", None, true), w::column(0.0, rows)]))
}

impl Panel for Earbuds {
    fn id(&self) -> &'static str {
        "earbuds"
    }

    fn title(&self, _: &View) -> String {
        "Earbuds".into()
    }

    fn icon(&self, _: &View) -> String {
        "headphones".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_wide
    }

    fn opened(&mut self) {
        self.rings.clear();
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let e = &v.store.devices.earbuds;
        let dev = e.active();
        let mut out = Vec::new();
        if !e.available() {
            out.push(w::section_label(s, "No daemon", None, true));
        } else if dev.is_none() {
            out.push(w::section_label(s, "No earbuds connected", None, true));
        }
        if e.devices().len() > 1
            && let Some((opts, selected)) = choice(e, DEVICE)
        {
            out.push(w::column(s.row_gap, vec![w::section_label(s, "Device", None, true), self.group(DEVICE, &opts, selected, None)]));
        }
        let Some(dev) = dev else { return w::column(s.section_gap, out) };
        match art(dev, "duo", s.control_height * 3.0) {
            Some(picture) => {
                out.push(hero(s, dev, picture));
                if !dev.batteries.iter().all(|b| b.id == BatteryId::Single) {
                    out.push(battery_tiles(s, dev));
                }
            }
            None => {
                out.push(w::hero(
                    s,
                    w::Hero {
                        glyph: "headphones".into(),
                        title: dev.name.clone(),
                        meta: dev.state_line.clone(),
                        readout: String::new(),
                        trailing: None,
                        rail: None,
                        rail_on: None,
                    },
                ));
                out.extend(battery_section(s, dev));
            }
        }
        for section in model::sections(Some(dev)) {
            let mut rows = Vec::new();
            let mut prev: Option<ControlKind> = None;
            for ctl in &section.controls {
                // Rows abut; a group is a boxed control and keeps `rowGap`.
                if prev.is_some_and(|p| p == ControlKind::Choice || ctl.kind == ControlKind::Choice) {
                    rows.push(w::space(s.row_gap));
                }
                prev = Some(ctl.kind);
                rows.push(match ctl.kind {
                    ControlKind::Choice => {
                        let (opts, selected) = choice(e, &ctl.key).unwrap_or_default();
                        self.group(&ctl.key, &opts, selected, Some(ctl))
                    }
                    ControlKind::Toggle => toggle_row(s, ctl),
                    ControlKind::Range => range_row(s, ctl),
                });
            }
            out.push(w::column(s.row_gap, vec![w::section_label(s, &section.section, None, true), w::column(0.0, rows)]));
        }
        w::column(s.section_gap, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let Some((verb, key)) = ev.on.split_once(':') else { return };
        let dev = fx.store.devices.earbuds.active();
        match (verb, &ev.what) {
            ("pick", What::Pick(i)) => self.press(fx, key, *i),
            ("toggle", What::Toggle(_) | What::Click) => {
                if let Some(ctl) = model::control(dev, key) {
                    set(fx, key, (ctl.value != Value::Bool(true)).to_string());
                }
            }
            ("range", What::Fraction(f)) => {
                if let Some(ctl) = model::control(dev, key) {
                    set(fx, key, num(ctl.min + f * (ctl.max - ctl.min)));
                }
            }
            ("range", What::Wheel(d)) => {
                if let Some(ctl) = model::control(dev, key) {
                    Self::step_range(fx, ctl, *d as f64);
                }
            }
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        let e = &fx.store.devices.earbuds;
        if let Some((opts, selected)) = choice(e, stop) {
            let at = self.ring(stop, opts.len(), selected);
            self.press(fx, stop, at);
            return;
        }
        if let Some(ctl) = model::control(e.active(), stop).filter(|c| c.kind == ControlKind::Toggle) {
            set(fx, stop, (ctl.value != Value::Bool(true)).to_string());
        }
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        let e = &fx.store.devices.earbuds;
        if let Some((opts, selected)) = choice(e, stop) {
            let at = self.ring(stop, opts.len(), selected) as i32 + direction;
            self.rings.insert(stop.to_owned(), at.clamp(0, (opts.len() as i32 - 1).max(0)) as usize);
            return;
        }
        if let Some(ctl) = model::control(e.active(), stop).filter(|c| c.kind == ControlKind::Range) {
            Self::step_range(fx, ctl, direction as f64);
        }
    }

    fn steps(&self) -> bool {
        true
    }
}
