//! CPU, memory and one row per measured GPU card, each a section
//! label over a `display` mono figure and a track, then the screen
//! `display.outputPriority` resolves to, then the hinge into the
//! launcher's full monitor view. A figure nobody can measure yet is `--`,
//! a machine with no card is one dim "No GPU" row, and cards with no
//! unprivileged counter are left out, or folded into one "GPUs" row saying
//! so when none has one.

use fs_system::display::priority;
use fs_system::monitor::format::pct;

use super::{Effect, Out, Panel, View};
use crate::services::wants::Source;
use crate::store::{Store, Topic};
use crate::ui::{El, w};

pub struct Monitor;

struct Metric {
    label: String,
    figure: String,
    fill: Option<f64>,
    state: String,
}

fn fill(fraction: Option<f64>) -> Option<f64> {
    fraction.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0))
}

fn metrics(v: &View) -> Vec<Metric> {
    let m = &v.store.monitor;
    let mut out = vec![
        Metric { label: "CPU".into(), figure: pct(m.cpu.aggregate), fill: fill(m.cpu.aggregate), state: String::new() },
    ];
    let memory = m.mem.as_ref().map(|mem| mem.used_fraction);
    out.push(Metric { label: "Memory".into(), figure: pct(memory), fill: fill(memory), state: String::new() });
    if m.cards.is_empty() {
        out.push(Metric { label: "GPU".into(), figure: String::new(), fill: None, state: "No GPU".into() });
    }
    let measured: Vec<_> = m.cards.iter().filter_map(|c| c.record.metrics.busy.filter(|_| c.record.metrics.available).map(|b| (c, b))).collect();
    if measured.is_empty() && !m.cards.is_empty() {
        let n = m.cards.len();
        let state = if n == 1 { "1 card, no metrics".to_owned() } else { format!("{n} cards, no metrics") };
        out.push(Metric { label: "GPUs".into(), figure: String::new(), fill: None, state });
    }
    for (card, busy) in measured {
        out.push(Metric { label: format!("GPU {}", card.name), figure: pct(Some(busy)), fill: fill(Some(busy)), state: String::new() });
    }
    out
}

fn main_output(v: &View) -> String {
    let names: Vec<String> = v.store.compositor.outputs.iter().filter(|o| o.enabled).map(|o| o.name.clone()).collect();
    let list = crate::settings::output_priority(&v.store.config);
    priority::resolve_main_output(&names, &list, &v.store.compositor.snapshot.focused_output_name, "")
}

impl Panel for Monitor {
    fn title(&self, _: &View) -> String {
        "Monitor".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Monitor, Topic::Compositor, Topic::Config]
    }

    fn wants(&self, _: &Store) -> Vec<Source> {
        vec![Source::Monitor]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let mut parts = Vec::new();
        for (i, m) in metrics(v).into_iter().enumerate() {
            let mut values = Vec::new();
            if !m.state.is_empty() {
                values.push(w::caption(m.state));
            }
            if !m.figure.is_empty() {
                values.push(w::panel_value(m.figure));
            }
            parts.push(w::panel_meter(s, None, &m.label, values, m.fill.map(w::track)).stop(format!("metric{i}")));
        }
        let name = main_output(v);
        parts.push(w::panel_row(s, "monitor", "Main display", None, vec![w::panel_value(if name.is_empty() { "--".into() } else { name })]).stop("main"));
        parts.push(w::panel_footer(s, "external-link", "Open monitor").stop("open").on("open"));
        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &crate::ui::Event, fx: &mut Effect) {
        if ev.on == "open" {
            fx.push(Out::Summon("monitor"));
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if stop == "open" {
            fx.push(Out::Summon("monitor"));
        }
    }
}
