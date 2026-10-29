//! A hero for the focused output, then one row per
//! output (an on/off switch, the scale, mirror and HDR lines, mode, make
//! and model, the card driving it, and a scale track), the backlight and
//! DDC monitors, a switch per output that can do HDR, and the one mirror
//! switch over the whole set.
//!
//! One flat cursor walks outputs, brightness rows, HDR rows and the mirror
//! row in that order. Enter switches an output, an HDR row or the mirror;
//! Left and Right step the scale or the percent.
//!
//! Nothing here reconfigures an output on open: every write hangs off a
//! click, a wheel notch or a key. While open it holds the display source,
//! which re-reads the outputs every 5s (the compositor announces no
//! disabled output) and detects the DDC monitors once.

use fs_system::display::outputs::{self, Output};

use super::{Effect, Panel, View};
use crate::services::display::{self, Brightness};
use crate::services::compositor::{self, Command};
use crate::services::wants::{Source, Want};
use crate::store::{Store, Topic};
use crate::services::compositor::OutputsState;
use crate::ui::{El, Event, Ink, Type, What, w};

/// The brightness step, the wheel's and the keyboard's.
const STEP: f64 = 5.0;

pub struct Display {
    _want: Want,
}

impl Display {
    pub fn new() -> Self {
        Self { _want: Want::new(Source::Display) }
    }
}

fn sorted(store: &Store) -> Vec<Output> {
    outputs::sort_outputs(&store.compositor.outputs)
}

fn hdr_names(store: &Store, rows: &[Output]) -> Vec<String> {
    rows.iter().filter(|r| r.enabled && display::hdr_supported(store, &r.name)).map(|r| r.name.clone()).collect()
}

fn focused(store: &Store) -> &str {
    &store.compositor.snapshot.focused_output_name
}

fn mirror_actionable(store: &Store, rows: &[Output]) -> bool {
    !rows.is_empty() && display::config_available() && outputs::mirror_plan(rows, focused(store)).ok
}

fn set_scale(row: &Output, scale: f64) {
    if outputs::quantize_scale(scale) != outputs::quantize_scale(row.scale) {
        compositor::send(Command::SetOutputScale(row.name.clone(), scale));
    }
}

fn toggle_output(rows: &[Output], name: &str) {
    if !display::enable_available() || !outputs::can_toggle(rows, name) {
        return;
    }
    if let Some(row) = outputs::find_output(rows, name) {
        compositor::send(Command::SetOutputEnabled(name.to_owned(), !row.enabled));
    }
}

/// MIRROR is one action over the set: on points every other lit output at
/// the plan's primary, off clears every output mirroring anything.
fn set_mirror(store: &Store, rows: &[Output], on: bool) {
    if !display::config_available() {
        return;
    }
    if !on {
        for name in outputs::mirrored_names(rows) {
            compositor::send(Command::SetOutputMirror(name, String::new()));
        }
        return;
    }
    let plan = outputs::mirror_plan(rows, focused(store));
    if plan.ok {
        for target in plan.targets {
            compositor::send(Command::SetOutputMirror(target, plan.primary.clone()));
        }
    }
}

fn set_brightness(fx: &Effect, d: &Brightness, percent: f64) {
    let (id, max) = (d.id.clone(), d.max);
    fx.service(move |ctx| display::set_percent(ctx, &id, percent, max));
}

fn dim(text: impl Into<String>, v: &View) -> El {
    w::section_label(&v.theme.space, &text.into(), None, false).ink(Ink::Dim)
}

fn value_line(text: String) -> El {
    w::value(text).elide()
}

fn output_row(v: &View, rows: &[Output], row: &Output, main: &str) -> El {
    let s = &v.theme.space;
    let store = v.store;
    let configurable = row.enabled && display::config_available();
    let mut head = vec![
        w::icon(if row.enabled { "monitor" } else { "monitor-off" }).ink(if row.name == focused(store) { Ink::Primary } else { Ink::Fg }),
        w::label(&row.name).elide(),
    ];
    if configurable {
        head.push(w::value(outputs::format_scale(row.scale)));
    }
    let toggles = display::enable_available() && outputs::can_toggle(rows, &row.name);
    head.push(w::switch(row.enabled).enabled(toggles).on(format!("enable:{}", row.name)));

    let mut lines = vec![w::row(s.icon_gap, head).fill()];
    if !row.enabled {
        lines.push(dim("Disabled", v));
    }
    if !row.mirror_of.is_empty() {
        lines.push(dim(format!("Mirrors {}", row.mirror_of), v));
    }
    let reason = display::hdr_reason(store, &row.name);
    if row.enabled && !display::hdr_supported(store, &row.name) && reason != "Checking" {
        lines.push(dim(format!("HDR unavailable: {reason}"), v));
    }
    if rows.len() > 1 && !main.is_empty() && row.name == main {
        lines.push(dim("Main display", v));
    }
    let mode = outputs::mode_label(row);
    if !mode.is_empty() {
        lines.push(value_line(mode));
    }
    let identity = outputs::describe(row);
    if !identity.is_empty() {
        lines.push(value_line(identity));
    }
    let card = outputs::output_card_label(&row.name, &store.display.cards);
    if !card.is_empty() {
        lines.push(value_line(card));
    }
    if configurable {
        lines.push(w::slider(outputs::fraction_for_scale(row.scale)).on(format!("scale:{}", row.name)));
    }
    w::cell(w::column(s.xs, lines).fill()).ghost().interactive().stop(format!("output:{}", row.name))
}

fn brightness_row(v: &View, d: &Brightness) -> El {
    let s = &v.theme.space;
    let head = w::row(s.icon_gap, vec![w::icon("sun"), w::label(&d.label).elide(), w::value(format!("{}%", d.percent))]).fill();
    let track = w::slider(d.percent as f64 / 100.0).on(format!("brightness:{}", d.id));
    w::cell(w::column(s.xs, vec![head, track]).fill()).ghost().interactive().stop(format!("brightness:{}", d.id))
}

fn switch_row(v: &View, glyph: &str, name: &str, checked: bool, on: String, stop: String) -> El {
    let s = &v.theme.space;
    let row = w::row(s.icon_gap, vec![w::icon(glyph), w::text(name).mono().elide(), w::switch(checked).on(on)]).fill();
    w::cell(row).ghost().interactive().stop(stop)
}

impl Panel for Display {
    fn id(&self) -> &'static str {
        "display"
    }

    fn title(&self, _: &View) -> String {
        "Display".into()
    }

    fn icon(&self, _: &View) -> String {
        "monitor".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Compositor, Topic::Display, Topic::State, Topic::Config]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let store = v.store;
        let rows = sorted(store);
        let mut parts = Vec::new();

        if let Some(f) = outputs::find_output(&rows, focused(store)) {
            let hero = w::Hero {
                glyph: "monitor".into(),
                title: f.name.clone(),
                meta: outputs::mode_label(f),
                readout: String::new(),
                trailing: None,
                rail: None,
                rail_on: None,
            };
            parts.push(w::hero_with(s, hero, true, Type::Display, None));
        }

        if rows.is_empty() {
            let text = match store.compositor.outputs_state {
                OutputsState::Failed => "Cannot read outputs",
                OutputsState::Ok => "No outputs",
                OutputsState::Unknown => "Loading",
            };
            parts.push(w::section_label(s, text, None, true));
        } else {
            let main = display::main_output(store);
            let list = rows.iter().map(|r| output_row(v, &rows, r, &main)).collect();
            parts.push(w::section(s, "Outputs", Some(rows.len()), list));
        }

        let devices = store.display.devices();
        let mut bright = vec![w::heading(s, "Brightness", Some(devices.len()), true)];
        if devices.is_empty() {
            bright.push(w::section_label(s, "No backlight", None, true));
        } else {
            bright.push(w::column(0.0, devices.iter().map(|d| brightness_row(v, d)).collect()));
        }
        parts.push(w::column(s.row_gap, bright));

        let hdr = hdr_names(store, &rows);
        if !hdr.is_empty() {
            let list = hdr
                .iter()
                .map(|n| switch_row(v, "sun", n, display::hdr_on(store, n), format!("hdr:{n}"), format!("hdr:{n}")))
                .collect();
            parts.push(w::section(s, "HDR", Some(hdr.len()), list));
        }

        if !rows.is_empty() {
            let mut mirror = vec![w::heading(s, "Mirror", None, true)];
            let plan = outputs::mirror_plan(&rows, focused(store));
            if !display::config_available() {
                mirror.push(w::section_label(s, "Mirror unsupported", None, true));
            } else if !plan.ok {
                mirror.push(w::section_label(s, "Single display", None, true));
            }
            if mirror_actionable(store, &rows) {
                let on = !outputs::mirrored_names(&rows).is_empty();
                let source = if on { outputs::mirror_source(&rows) } else { plan.primary.clone() };
                mirror.push(switch_row(v, "copy", &source, on, "mirror".into(), "mirror".into()));
            }
            parts.push(w::column(s.row_gap, mirror));
        }

        w::column(s.section_gap, parts)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let store = fx.store;
        let rows = sorted(store);
        let (what, arg) = ev.on.split_once(':').unwrap_or((ev.on.as_str(), ""));
        match (what, &ev.what) {
            ("enable", What::Toggle(_)) => toggle_output(&rows, arg),
            ("scale", What::Fraction(f)) => {
                if let Some(row) = outputs::find_output(&rows, arg).filter(|r| r.enabled) {
                    set_scale(row, outputs::scale_for_fraction(*f));
                }
            }
            ("scale", What::Wheel(d)) => {
                if let Some(row) = outputs::find_output(&rows, arg).filter(|r| r.enabled) {
                    set_scale(row, outputs::step_scale(row.scale, (*d).signum() as f64));
                }
            }
            ("brightness", What::Fraction(f)) => {
                if let Some(d) = store.display.devices().iter().find(|d| d.id == arg) {
                    set_brightness(fx, d, f * 100.0);
                }
            }
            ("brightness", What::Wheel(n)) => {
                if let Some(d) = store.display.devices().iter().find(|d| d.id == arg) {
                    set_brightness(fx, d, d.percent as f64 + (*n).signum() as f64 * STEP);
                }
            }
            ("hdr", What::Toggle(on)) => {
                display::hdr_set(store, arg, *on);
            }
            ("mirror", What::Toggle(on)) => set_mirror(store, &rows, *on),
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        let store = fx.store;
        let rows = sorted(store);
        let (what, arg) = stop.split_once(':').unwrap_or((stop, ""));
        match what {
            "output" => toggle_output(&rows, arg),
            "hdr" => {
                display::hdr_set(store, arg, !display::hdr_on(store, arg));
            }
            "mirror" => set_mirror(store, &rows, outputs::mirrored_names(&rows).is_empty()),
            _ => {}
        }
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        let store = fx.store;
        let rows = sorted(store);
        let (what, arg) = stop.split_once(':').unwrap_or((stop, ""));
        match what {
            "output" => {
                if let Some(row) = outputs::find_output(&rows, arg).filter(|r| r.enabled) {
                    set_scale(row, outputs::step_scale(row.scale, direction as f64));
                }
            }
            "brightness" => {
                if let Some(d) = store.display.devices().iter().find(|d| d.id == arg) {
                    set_brightness(fx, d, d.percent as f64 + direction as f64 * STEP);
                }
            }
            _ => {}
        }
    }

    fn steps(&self) -> bool {
        true
    }
}
