//! CPU, memory and one row per GPU card, each a section
//! label over a `display` mono figure and a track, then the screen
//! `display.outputPriority` resolves to, then the hinge into the
//! launcher's full monitor view. A figure nobody can measure yet is `--`,
//! a machine with no card is one dim "No GPU" row, and a card with no
//! unprivileged counter is "No metrics" with no track under it.

use fs_system::display::priority;
use fs_system::monitor::format::pct;

use super::{Effect, Panel, View};
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Ink, Type, Variant, Weight, w};

pub struct Monitor {
    _want: Want,
}

impl Monitor {
    pub fn new() -> Self {
        Self { _want: Want::new(Source::Monitor) }
    }
}

struct Metric {
    label: String,
    figure: String,
    fill: Option<f64>,
    state: &'static str,
}

fn fill(fraction: Option<f64>) -> Option<f64> {
    fraction.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0))
}

fn metrics(v: &View) -> Vec<Metric> {
    let m = &v.store.info.monitor;
    let mut out = vec![
        Metric { label: "CPU".into(), figure: pct(m.cpu.aggregate), fill: fill(m.cpu.aggregate), state: "" },
    ];
    let memory = m.mem.as_ref().map(|mem| mem.used_fraction);
    out.push(Metric { label: "Memory".into(), figure: pct(memory), fill: fill(memory), state: "" });
    if m.cards.is_empty() {
        out.push(Metric { label: "GPU".into(), figure: String::new(), fill: None, state: "No GPU" });
    }
    for card in &m.cards {
        let busy = card.record.metrics.busy.filter(|_| card.record.metrics.available);
        out.push(Metric {
            label: format!("GPU {}", card.name),
            figure: busy.map_or_else(String::new, |b| pct(Some(b))),
            fill: fill(busy),
            state: if busy.is_some() { "" } else { "No metrics" },
        });
    }
    out
}

fn main_output(v: &View) -> String {
    let names: Vec<String> = v.store.compositor.outputs.iter().filter(|o| o.enabled).map(|o| o.name.clone()).collect();
    let list = priority::priority_list(v.store.config.get("display.outputPriority").unwrap_or(&serde_json::Value::Null));
    priority::resolve_main_output(&names, &list, &v.store.compositor.snapshot.focused_output_name, "")
}

impl Panel for Monitor {
    fn id(&self) -> &'static str {
        "monitor"
    }

    fn title(&self, _: &View) -> String {
        "Monitor".into()
    }

    fn icon(&self, _: &View) -> String {
        "activity".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info, Topic::Compositor, Topic::Config]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_wide
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let mut parts = Vec::new();
        for (i, m) in metrics(v).into_iter().enumerate() {
            let mut line = Vec::new();
            if !m.figure.is_empty() {
                line.push(w::text(m.figure).size(Type::Display).mono().weight(Weight::Semibold));
            }
            if !m.state.is_empty() {
                line.push(w::section_label(s, m.state, None, false).ink(Ink::Dim));
            }
            let mut content = vec![w::row(s.icon_gap, line)];
            if let Some(f) = m.fill {
                content.push(w::track(f));
            }
            parts.push(w::column(
                s.row_gap,
                vec![
                    w::heading(s, &m.label, None, true),
                    w::cell(w::column(s.xxs, content).fill()).ghost().interactive().stop(format!("metric{i}")),
                ],
            ));
        }
        let name = main_output(v);
        parts.push(w::column(
            s.row_gap,
            vec![
                w::heading(s, "Main display", None, true),
                w::cell(w::text(if name.is_empty() { "--".into() } else { name }).mono().weight(Weight::Medium))
                    .ghost()
                    .interactive()
                    .stop("main"),
            ],
        ));
        parts.push(w::separator());
        parts.push(w::icon_text_button("external-link", "Open monitor").variant(Variant::Ghost).stop("open").on("open").width(crate::ui::Size::Hug));
        w::column(s.section_gap, parts)
    }

    fn event(&mut self, ev: &crate::ui::Event, fx: &mut Effect) {
        if ev.on == "open" {
            fx.summon = Some("monitor");
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if stop == "open" {
            fx.summon = Some("monitor");
        }
    }
}
