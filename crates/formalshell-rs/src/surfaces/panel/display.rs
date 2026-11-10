//! One row per
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

use super::{Effect, Nav, Out, Panel, View, quick};
use crate::services::display::{self, Brightness};
use crate::services::compositor::{self, Command};
use crate::services::wants::Source;
use crate::store::{Store, Topic};
use crate::services::compositor::OutputsState;
use crate::ui::{El, Event, Ink, On, Type, Weight, What, w};

/// A press on an output's or a brightness device's control, by its name or
/// id; `Output` is an output's row as a cursor stop.
enum Press {
    Output(String),
    Enable(String),
    Scale(String),
    Hdr(String),
    Brightness(String),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Press::Output(name) => write!(f, "output:{name}"),
            Press::Enable(name) => write!(f, "enable:{name}"),
            Press::Scale(name) => write!(f, "scale:{name}"),
            Press::Hdr(name) => write!(f, "hdr:{name}"),
            Press::Brightness(id) => write!(f, "brightness:{id}"),
        }
    }
}

/// The brightness step, the wheel's and the keyboard's.
const STEP: f64 = 5.0;

pub struct Display;

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
    if !display::config_available() || !outputs::can_toggle(rows, name) {
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

fn output_row(v: &View, rows: &[Output], row: &Output, main: &str) -> El {
    let s = &v.theme.space;
    let store = v.store;
    let configurable = row.enabled && display::config_available();
    let mut head = vec![
        w::icon(if row.enabled { "monitor" } else { "monitor-off" }).size(Type::Heading).ink(if row.name == focused(store) { Ink::Primary } else { Ink::Fg }),
        w::label(&row.name).weight(Weight::Medium).elide().fill(),
    ];
    let toggles = display::config_available() && outputs::can_toggle(rows, &row.name);
    head.push(w::switch(row.enabled).enabled(toggles).on(Press::Enable(row.name.clone())));

    let mut lines = vec![w::row(s.panel_gap, head).fill().height(s.panel_row).mid()];
    if !row.enabled {
        lines.push(w::caption("Disabled").elide());
    }
    if !row.mirror_of.is_empty() {
        lines.push(w::caption(format!("Mirrors {}", row.mirror_of)).elide());
    }
    let reason = display::hdr_reason(store, &row.name);
    if row.enabled && !display::hdr_supported(store, &row.name) && reason != "Checking" {
        lines.push(w::caption(format!("HDR unavailable: {reason}")).elide());
    }
    if rows.len() > 1 && !main.is_empty() && row.name == main {
        lines.push(w::caption("Main display").elide());
    }
    let mode = outputs::mode_label(row);
    if !mode.is_empty() {
        lines.push(w::caption(mode).mono().elide());
    }
    let identity = outputs::describe(row);
    if !identity.is_empty() {
        lines.push(w::caption(identity).mono().elide());
    }
    let card = outputs::output_card_label(&row.name, &store.display.cards);
    if !card.is_empty() {
        lines.push(w::caption(card).mono().elide());
    }
    if configurable {
        let slider = w::slider(outputs::fraction_for_scale(row.scale)).on(Press::Scale(row.name.clone()));
        lines.push(w::meter(s, None, "Scale", vec![w::panel_value(outputs::format_scale(row.scale))], Some(slider)));
    }
    w::panel_cell(s, w::column(s.xxs, lines).fill().pad(0.0, 0.0, 0.0, s.panel_gap)).stop(Press::Output(row.name.clone()))
}

fn brightness_row(v: &View, d: &Brightness) -> El {
    let track = w::slider(d.percent as f64 / 100.0).on(Press::Brightness(d.id.clone()));
    w::panel_meter(&v.theme.space, Some("sun"), &d.label, vec![w::panel_value(format!("{}%", d.percent))], Some(track)).stop(Press::Brightness(d.id.clone()))
}

impl Panel for Display {
    fn title(&self, _: &View) -> String {
        "Display".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Compositor, Topic::Display, Topic::State, Topic::Config]
    }

    fn wants(&self, _: &Store) -> Vec<Source> {
        vec![Source::Display]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let store = v.store;
        let rows = sorted(store);
        let mut parts = Vec::new();

        if rows.is_empty() {
            let text = match store.compositor.outputs_state {
                OutputsState::Failed => "Cannot read outputs",
                OutputsState::Ok => "No outputs",
                OutputsState::Unknown => "Loading",
            };
            parts.push(w::panel_empty(s, text));
        } else {
            let main = display::main_output(store);
            let list = rows.iter().map(|r| output_row(v, &rows, r, &main)).collect();
            parts.push(w::panel_section_n(s, "Outputs", rows.len(), list, ""));
        }

        let devices = store.display.devices();
        let bright = devices.iter().map(|d| brightness_row(v, d)).collect();
        parts.push(w::panel_section_n(s, "Brightness", devices.len(), bright, "No backlight"));

        let hdr = hdr_names(store, &rows);
        if !hdr.is_empty() {
            let list = hdr
                .iter()
                .map(|n| w::panel_switch(s, "sun", n.as_str(), None, display::hdr_on(store, n), Press::Hdr(n.to_string())).stop(Press::Hdr(n.to_string())))
                .collect();
            parts.push(w::panel_section_n(s, "HDR", hdr.len(), list, ""));
        }

        if !rows.is_empty() {
            let plan = outputs::mirror_plan(&rows, focused(store));
            let empty = if display::config_available() { "Single display" } else { "Mirror unsupported" };
            let mut mirror = Vec::new();
            if mirror_actionable(store, &rows) {
                let on = !outputs::mirrored_names(&rows).is_empty();
                let source = if on { outputs::mirror_source(&rows) } else { plan.primary.clone() };
                mirror.push(w::panel_switch(s, "copy", source, None, on, "mirror").stop("mirror"));
            }
            parts.push(w::panel_section(s, "Mirror", mirror, empty));
        }

        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let store = fx.store;
        let rows = sorted(store);
        match (ev.on.get::<Press>(), &ev.what) {
            (Some(Press::Enable(name)), What::Toggle(_)) => toggle_output(&rows, name),
            (Some(Press::Scale(name)), What::Fraction(f)) => {
                if let Some(row) = outputs::find_output(&rows, name).filter(|r| r.enabled) {
                    set_scale(row, outputs::scale_for_fraction(*f));
                }
            }
            (Some(Press::Scale(name)), What::Wheel(d)) => {
                if let Some(row) = outputs::find_output(&rows, name).filter(|r| r.enabled) {
                    set_scale(row, outputs::step_scale(row.scale, (*d).signum() as f64));
                }
            }
            (Some(Press::Brightness(id)), What::Fraction(f)) => {
                if let Some(d) = store.display.devices().iter().find(|d| &d.id == id) {
                    set_brightness(fx, d, f * 100.0);
                }
            }
            (Some(Press::Brightness(id)), What::Wheel(n)) => {
                if let Some(d) = store.display.devices().iter().find(|d| &d.id == id) {
                    set_brightness(fx, d, d.percent as f64 + (*n).signum() as f64 * STEP);
                }
            }
            (Some(Press::Hdr(name)), What::Toggle(on)) => fx.push(Out::Quick(quick::Run::HdrOutput(name.clone(), *on))),
            (None, What::Toggle(on)) if ev.on == "mirror" => set_mirror(store, &rows, *on),
            _ => {}
        }
    }

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        let store = fx.store;
        let rows = sorted(store);
        match stop.get::<Press>() {
            Some(Press::Output(name)) => toggle_output(&rows, name),
            Some(Press::Hdr(name)) => fx.push(Out::Quick(quick::Run::HdrOutput(name.clone(), !display::hdr_on(store, name)))),
            None if stop == "mirror" => set_mirror(store, &rows, outputs::mirrored_names(&rows).is_empty()),
            _ => {}
        }
    }

    fn step(&mut self, stop: &On, direction: i32, fx: &mut Effect) {
        let store = fx.store;
        let rows = sorted(store);
        match stop.get::<Press>() {
            Some(Press::Output(name)) => {
                if let Some(row) = outputs::find_output(&rows, name).filter(|r| r.enabled) {
                    set_scale(row, outputs::step_scale(row.scale, direction as f64));
                }
            }
            Some(Press::Brightness(id)) => {
                if let Some(d) = store.display.devices().iter().find(|d| &d.id == id) {
                    set_brightness(fx, d, d.percent as f64 + direction as f64 * STEP);
                }
            }
            _ => {}
        }
    }

    fn nav(&self, _: Option<&On>) -> Nav {
        Nav::STEPS
    }
}
