//! Drawn from the active device alone, never branching on
//! vendor. A device with a picture set (`Device::art`) leads with its
//! product picture and gives each bud and the case a tile of its own.
//!
//! A `choice` of up to four entries is one segmented row, a tier of options
//! (`ControlOption::tier`) folded into one entry with its members on a
//! second row while it is picked; a longer choice is a dropdown whose
//! options float over the panel. A `toggle`
//! is a `Switch` row, a `range` a track, each under its control's section in
//! the order the adapter listed them. No backend source at all is the dim
//! "No daemon" row; a source with no device is "No earbuds connected"; with
//! more than one device a choice of them heads the panel.
//!
//! One cursor walks the device choice and then every control. On a
//! segmented row Left and Right pick the neighbouring entry; on the device
//! group they move its ring and Enter presses the button under it; on a
//! range they step by the control's `step`. Every write goes through
//! `Earbuds::plan`, the same allow-list the `earbuds set` IPC uses (M77).
//! The panel holds the service while it is open (`wayland.rs`'s sync_open).

use std::collections::HashMap;

use fs_devices::earbuds::{self as model, BatteryId, Control, ControlKind, Device, Value};

use super::{Effect, Panel, View};
use crate::services::devices::{self, earbuds};
use crate::store::{self, Topic};
use crate::ui::el::Opt;
use crate::ui::{El, Event, Ink, Type, Weight, What, w};

const DEVICE: &str = "__device";
/// The most entries a choice shows as one segmented row.
const SEGMENTS: usize = 4;
/// The stop suffix of a tier's member row.
const TIER: &str = "#tier";

#[derive(Default)]
pub struct Earbuds {
    /// Control key to its ring's place, once the ring has moved.
    rings: HashMap<String, usize>,
    /// The choice whose dropdown is open, "" for none.
    menu: String,
    /// Control key to the tier member last picked, so picking the tier
    /// again returns to it.
    members: HashMap<String, String>,
}

/// The device choice's options and selected index.
fn devices(v: &earbuds::Earbuds) -> (Vec<(String, String, String)>, Option<usize>) {
    let active = v.active().map(|d| d.key.clone());
    let opts: Vec<_> = v.devices().iter().map(|d| (d.key.clone(), d.name.clone(), String::new())).collect();
    let index = opts.iter().position(|o| Some(&o.0) == active.as_ref());
    (opts, index)
}

/// One entry of a choice as the panel shows it: an option alone, or a tier
/// with its members' option indices.
struct Entry {
    label: String,
    tier: bool,
    members: Vec<usize>,
}

fn entries(ctl: &Control) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for (i, o) in ctl.options.as_deref().unwrap_or_default().iter().enumerate() {
        match out.last_mut() {
            Some(e) if e.tier && !o.tier.is_empty() && e.label == o.tier => e.members.push(i),
            _ => out.push(Entry { label: if o.tier.is_empty() { o.label.clone() } else { o.tier.clone() }, tier: !o.tier.is_empty(), members: vec![i] }),
        }
    }
    out
}

/// The entry holding the selected option.
fn picked(ctl: &Control, es: &[Entry]) -> Option<usize> {
    let sel = model::option_index(Some(ctl))?;
    es.iter().position(|e| e.members.contains(&sel))
}

fn option_value(ctl: &Control, i: usize) -> Option<String> {
    ctl.options.as_deref().unwrap_or_default().get(i).map(|o| value_text(&o.value))
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

    /// A press writes the device and moves only the ring: the selection
    /// follows once the service reports it.
    fn press(&mut self, fx: &Effect, index: usize) {
        let (opts, selected) = devices(&fx.store.devices.earbuds);
        let Some(o) = opts.get(index) else { return };
        self.rings.insert(DEVICE.to_owned(), index);
        if Some(index) != selected {
            set(fx, DEVICE, o.0.clone());
        }
    }

    /// Entry `index` of a choice: an option alone is written as is, a tier
    /// writes the member last picked in it, else its first.
    fn pick_entry(&mut self, fx: &Effect, key: &str, index: usize) {
        let Some(ctl) = model::control(fx.store.devices.earbuds.active(), key) else { return };
        let es = entries(ctl);
        let Some(e) = es.get(index) else { return };
        if picked(ctl, &es) == Some(index) {
            return;
        }
        let remembered = self.members.get(key).and_then(|v| e.members.iter().copied().find(|m| option_value(ctl, *m).as_ref() == Some(v)));
        if let Some(value) = option_value(ctl, remembered.unwrap_or(e.members[0])) {
            set(fx, key, value);
        }
    }

    /// Member `index` of the picked tier.
    fn pick_member(&mut self, fx: &Effect, key: &str, index: usize) {
        let Some(ctl) = model::control(fx.store.devices.earbuds.active(), key) else { return };
        let es = entries(ctl);
        let Some(m) = picked(ctl, &es).and_then(|at| es[at].members.get(index).copied()) else { return };
        if let Some(value) = option_value(ctl, m) {
            self.members.insert(key.to_owned(), value.clone());
            if model::option_index(Some(ctl)) != Some(m) {
                set(fx, key, value);
            }
        }
    }

    /// A choice's rows: segmented, with the picked tier's members under it,
    /// or a dropdown listing every option while it is open.
    fn choice_rows(&self, s: &fs_theme::tokens::Space, ctl: &Control) -> Vec<El> {
        let key = &ctl.key;
        let opts = ctl.options.as_deref().unwrap_or_default();
        let selected = model::option_index(Some(ctl));
        let es = entries(ctl);
        if es.len() <= SEGMENTS {
            let at = picked(ctl, &es);
            let mut rows = vec![w::segmented(es.iter().map(|e| e.label.clone()).collect(), at.unwrap_or(usize::MAX)).fill().stop(key.clone()).on(format!("seg:{key}"))];
            if let Some(e) = at.map(|a| &es[a]).filter(|e| e.tier) {
                let inner = e.members.iter().position(|m| Some(*m) == selected);
                let labels = e.members.iter().map(|m| opts[*m].label.clone()).collect();
                rows.push(w::segmented(labels, inner.unwrap_or(usize::MAX)).fill().stop(format!("{key}{TIER}")).on(format!("tier:{key}")));
            }
            return rows;
        }
        let open = self.menu == *key;
        let current = selected.map_or("Choose", |i| opts[i].label.as_str());
        let mut trigger = w::trigger(s, "", current, open).stop(key.clone()).on(format!("menu:{key}"));
        if open {
            let list = opts.iter().enumerate().map(|(i, o)| {
                let current = Some(i) == selected;
                let check = w::icon("check").size(Type::BodySmall).ink(if current { Ink::Fg } else { Ink::Color(fs_theme::color::Rgba::TRANSPARENT) });
                let stop = format!("opt:{key}:{i}");
                w::cell(w::row(s.icon_gap, vec![check, w::text(&o.label).size(Type::BodySmall).elide()])).selected(current).interactive().stop(stop.clone()).on(stop)
            });
            trigger = trigger.float(w::column(0.0, list.collect()));
        }
        vec![w::cell(w::row(s.icon_gap, vec![w::label(&ctl.label).elide(), w::spacer(), trigger]).fill()).ghost()]
    }

    fn toggle_menu(&mut self, key: &str) {
        self.menu = if self.menu == key { String::new() } else { key.to_owned() };
    }

    /// `key:index` from a dropdown row: writes that option and closes it.
    fn pick_option(&mut self, fx: &Effect, rest: &str) {
        self.menu.clear();
        let Some((key, i)) = rest.rsplit_once(':') else { return };
        let Some(ctl) = model::control(fx.store.devices.earbuds.active(), key) else { return };
        let Ok(i) = i.parse::<usize>() else { return };
        if model::option_index(Some(ctl)) != Some(i)
            && let Some(value) = option_value(ctl, i)
        {
            set(fx, key, value);
        }
    }

    fn step_range(fx: &Effect, ctl: &Control, direction: f64) {
        if let Value::Num(n) = ctl.value {
            set(fx, &ctl.key, num(n + direction * ctl.step));
        }
    }
}

fn art(dev: &Device, part: &str, size: f64) -> Option<crate::scene::Bitmap> {
    earbuds::picture(&dev.art, part, size)
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
    Some(w::column(s.row_gap, vec![w::panel_heading(s, "Battery"), w::column(0.0, rows)]))
}

impl Panel for Earbuds {
    fn id(&self) -> &'static str {
        "earbuds"
    }

    fn title(&self, _: &View) -> String {
        "Earbuds".into()
    }


    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_wide
    }

    fn opened(&mut self) {
        self.rings.clear();
        self.menu.clear();
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let e = &v.store.devices.earbuds;
        let dev = e.active();
        let mut out = Vec::new();
        if !e.available() {
            out.push(w::panel_empty(s, "No daemon"));
        } else if dev.is_none() {
            out.push(w::panel_empty(s, "No earbuds connected"));
        }
        if e.devices().len() > 1 {
            let (opts, selected) = devices(e);
            out.push(w::column(s.row_gap, vec![w::panel_heading(s, "Device"), self.group(DEVICE, &opts, selected, None)]));
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
                match ctl.kind {
                    ControlKind::Choice => {
                        for (i, row) in self.choice_rows(s, ctl).into_iter().enumerate() {
                            if i > 0 {
                                rows.push(w::space(s.xs));
                            }
                            rows.push(row);
                        }
                    }
                    ControlKind::Toggle => rows.push(toggle_row(s, ctl)),
                    ControlKind::Range => rows.push(range_row(s, ctl)),
                }
            }
            out.push(w::column(s.row_gap, vec![w::panel_heading(s, &section.section), w::column(0.0, rows)]));
        }
        w::column(s.section_gap, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let Some((verb, key)) = ev.on.split_once(':') else { return };
        if verb != "menu" && verb != "opt" {
            self.menu.clear();
        }
        let dev = fx.store.devices.earbuds.active();
        match (verb, &ev.what) {
            ("pick", What::Pick(i)) => self.press(fx, *i),
            ("seg", What::Pick(i)) => self.pick_entry(fx, key, *i),
            ("tier", What::Pick(i)) => self.pick_member(fx, key, *i),
            ("menu", _) => self.toggle_menu(key),
            ("opt", _) => self.pick_option(fx, key),
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
        if stop == DEVICE {
            let (opts, selected) = devices(e);
            let at = self.ring(DEVICE, opts.len(), selected);
            self.press(fx, at);
            return;
        }
        if let Some(rest) = stop.strip_prefix("opt:") {
            self.pick_option(fx, rest);
            return;
        }
        match model::control(e.active(), stop) {
            Some(ctl) if ctl.kind == ControlKind::Toggle => set(fx, stop, (ctl.value != Value::Bool(true)).to_string()),
            Some(ctl) if ctl.kind == ControlKind::Choice && entries(ctl).len() > SEGMENTS => self.toggle_menu(stop),
            _ => {}
        }
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        let e = &fx.store.devices.earbuds;
        if stop == DEVICE {
            let (opts, selected) = devices(e);
            let at = self.ring(DEVICE, opts.len(), selected) as i32 + direction;
            self.rings.insert(DEVICE.to_owned(), at.clamp(0, (opts.len() as i32 - 1).max(0)) as usize);
            return;
        }
        if let Some(key) = stop.strip_suffix(TIER) {
            let Some(ctl) = model::control(e.active(), key) else { return };
            let es = entries(ctl);
            let Some(members) = picked(ctl, &es).map(|at| es[at].members.clone()) else { return };
            let at = members.iter().position(|m| model::option_index(Some(ctl)) == Some(*m)).map_or(0, |i| i as i32 + direction);
            self.pick_member(fx, key, at.clamp(0, members.len() as i32 - 1) as usize);
            return;
        }
        let Some(ctl) = model::control(e.active(), stop) else { return };
        match ctl.kind {
            ControlKind::Range => Self::step_range(fx, ctl, direction as f64),
            ControlKind::Choice => {
                let es = entries(ctl);
                if es.len() <= SEGMENTS {
                    let at = picked(ctl, &es).map_or(0, |i| i as i32 + direction);
                    self.pick_entry(fx, stop, at.clamp(0, es.len() as i32 - 1) as usize);
                }
            }
            ControlKind::Toggle => {}
        }
    }

    fn steps(&self) -> bool {
        true
    }
}
