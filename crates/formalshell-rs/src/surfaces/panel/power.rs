//! A hero for the battery (state icon, the state word, the
//! percent as the readout, the charge as the rail), a ledger of label and
//! mono value rows, the power flow diagram, then the profiles as one
//! `ButtonGroup`. The group is the panel's one cursor stop: Left and Right
//! walk its buttons and Enter applies the one under the ring. A machine
//! with no battery shows the honest "AC power" row instead of a 0%, and a
//! flow with nothing to draw is "No power sources".

use std::cell::Cell;

use fs_system::power::flow::{self, Extras, Link};
use fs_system::power::model::{
    self, DEFAULT_WARN_PCT, DeviceState, charge_state_label, charge_threshold_active, format_health_percent, format_wh, rate_row_label,
    rate_row_value, time_row_label, time_row_value,
};
use fs_upower::Profile;

use super::{Effect, Panel, View};
use crate::services::devices;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::el::{Flow, FlowNode, FlowPort, Opt};
use crate::ui::{El, Event, What, w};

const PROFILES: [Profile; 3] = [Profile::PowerSaver, Profile::Balanced, Profile::Performance];
const ICONS: [&str; 3] = ["leaf", "gauge", "zap"];
const NAMES: [&str; 3] = ["Power Saver", "Balanced", "Performance"];

/// The sheet of the flow model a draw reads: the same nodes Power/flow.js
/// builds, carried as plain strings.
pub fn flow_view(f: &flow::Flow, animate: bool) -> Flow {
    let node = |icon: &str, caption: &str, value: &str, detail: &str, dim: bool| FlowNode {
        icon: icon.into(),
        caption: caption.into(),
        value: value.into(),
        detail: detail.into(),
        dim,
    };
    Flow {
        nodes: [
            node(f.adapter.icon, f.adapter.caption, &f.adapter.value, &f.adapter.detail, !f.adapter.online),
            node(f.laptop.icon, f.laptop.caption, &f.laptop.value, &f.laptop.detail, false),
            node(f.battery.icon, f.battery.caption, &f.battery.value, &f.battery.detail, !f.battery.present),
        ],
        links: [
            i8::from(f.links.adapter),
            match f.links.battery {
                Link::In => 1,
                Link::Out => -1,
                Link::None => 0,
            },
        ],
        ports: f
            .ports
            .iter()
            .map(|p| FlowPort {
                name: p.name.clone(),
                label: p.label.clone(),
                detail: p.detail.clone(),
                direction: if p.powering {
                    1
                } else if p.supplying {
                    -1
                } else {
                    0
                },
            })
            .collect(),
        animate,
    }
}

pub struct Power {
    /// Where the group's ring sits: the active profile on a fresh open.
    cursor: Cell<Option<usize>>,
    _flow: Want,
    _monitor: Want,
    _iphone: Want,
}

impl Power {
    pub fn new() -> Self {
        Self { cursor: Cell::new(None), _flow: Want::new(Source::PowerFlow), _monitor: Want::new(Source::Monitor), _iphone: Want::new(Source::Iphone) }
    }
}

fn active(v: &View) -> usize {
    v.store.devices.profiles.as_ref().and_then(|p| PROFILES.iter().position(|x| *x == p.active)).unwrap_or(0)
}

/// A label and its mono value, `controlHeight` tall and flat.
fn stat(v: &View, label: &str, value: String) -> El {
    let s = &v.theme.space;
    w::cell(w::row(s.icon_gap, vec![w::section_label(s, label, None, false).elide(), w::text(value).mono()]).fill()).ghost()
}

impl Panel for Power {
    fn id(&self) -> &'static str {
        "power"
    }

    fn title(&self, _: &View) -> String {
        "Power".into()
    }

    fn icon(&self, v: &View) -> String {
        if v.store.devices.power.battery.is_some() { "battery" } else { "zap" }.into()
    }

    fn subtitle(&self, v: &View) -> String {
        if v.store.devices.power.battery.is_some() { "Battery" } else { "AC power" }.into()
    }

    fn status(&self, v: &View) -> String {
        let power = &v.store.devices.power;
        power.battery.as_ref().map_or_else(|| "on ac".into(), |b| format!("{}% {}", b.percent, if power.on_battery { "on battery" } else { "plugged in" }))
    }

    fn accent(&self) -> super::Accent {
        super::Accent::Tertiary
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices, Topic::Info, Topic::Config]
    }

    fn opened(&mut self) {
        self.cursor.set(None);
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let power = &v.store.devices.power;
        let mut parts = Vec::new();

        if let Some(b) = &power.battery {
            let charging = b.state == DeviceState::Charging;
            let held = charge_threshold_active(b.percent, b.state, b.rate, b.time_to_full, power.on_battery);
            let warn = v.store.config.f64("battery.warnPercent").unwrap_or(DEFAULT_WARN_PCT);
            let glyph = model::battery_icon(b.percent, power.on_battery, held, Some(warn));
            parts.push(w::hero_with(
                s,
                w::Hero {
                    glyph: String::new(),
                    title: "Battery".into(),
                    meta: charge_state_label(b.percent, b.state, power.on_battery, held).into(),
                    readout: format!("{}%", b.percent),
                    trailing: None,
                    rail: Some(b.percent / 100.0),
                    rail_on: None,
                    tab: charge_state_label(b.percent, b.state, power.on_battery, held).to_lowercase(),
                },
                false,
                crate::ui::Type::Display,
                Some(w::icon(glyph).size(crate::ui::Type::Heading).pulse(charging)),
            ));

            let flow_state = &v.store.info.power_flow;
            let limit = flow_state
                .snapshot
                .supplies
                .iter()
                .find(|s| s.kind == "Battery")
                .and_then(|s| s.limit)
                .and_then(|l| model::parse_charge_limit(&l.to_string()));
            let mut rows = vec![
                w::heading(s, "Battery", None, true),
                stat(v, "Capacity", format_health_percent(b.health.unwrap_or(0.0), b.health.is_some())),
                stat(v, "Size", format_wh(b.size_wh)),
                stat(v, time_row_label(charging), time_row_value(charging, b.time_to_full, b.time_to_empty)),
                stat(v, rate_row_label(charging, held), rate_row_value(b.rate, flow_state.cpu_w)),
            ];
            if let Some(l) = limit {
                rows.push(stat(v, "Charge limit", format!("{l}%")));
            }
            parts.push(w::column(s.row_gap, rows));
        } else {
            parts.push(w::section_label(s, "AC power", None, true));
        }

        let sample = &v.store.info.power_flow;
        let gpu_w = {
            let cards = &v.store.info.monitor.cards;
            let watts: Vec<f64> = cards.iter().filter_map(|c| c.record.metrics.power_w).collect();
            (!watts.is_empty()).then(|| watts.iter().sum())
        };
        let model = flow::build_flow(
            &sample.snapshot,
            &Extras { cpu_w: sample.cpu_w, gpu_w, iphone_percent: v.store.info.iphone.battery.map(|b| b * 100.0) },
        );
        let mut flow_rows = vec![w::heading(s, "Power flow", None, true)];
        flow_rows.push(if model.available {
            w::flow(flow_view(&model, true))
        } else {
            w::section_label(s, "No power sources", None, true)
        });
        parts.push(w::column(s.row_gap, flow_rows));

        let offered = v.store.devices.profiles.as_ref();
        let options = (0..3)
            .map(|i| {
                let mut o = Opt::new(NAMES[i]).icon(ICONS[i]);
                o.enabled = PROFILES[i] != Profile::Performance || offered.is_none_or(|p| p.has_performance());
                o
            })
            .collect();
        let at = self.cursor.get().unwrap_or_else(|| {
            let a = active(v);
            self.cursor.set(Some(a));
            a
        });
        parts.push(w::column(
            s.row_gap,
            vec![
                w::heading(s, "Profile", None, true),
                w::group(options, active(v), true).ring(at).on("profile").stop("profile"),
            ],
        ));
        w::column(s.section_gap, parts)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let ("profile", What::Pick(i)) = (ev.on.as_str(), &ev.what) {
            self.apply(*i, fx);
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if stop == "profile" {
            let at = self.cursor.get().unwrap_or(0);
            self.apply(at, fx);
        }
    }

    fn step(&mut self, stop: &str, direction: i32, _: &mut Effect) {
        if stop == "profile" {
            let at = self.cursor.get().unwrap_or(0) as i32 + direction;
            self.cursor.set(Some(at.clamp(0, 2) as usize));
        }
    }

    fn steps(&self) -> bool {
        true
    }
}

impl Power {
    fn apply(&self, index: usize, fx: &Effect) {
        let Some(profile) = PROFILES.get(index).copied() else { return };
        self.cursor.set(Some(index));
        fx.service(move |ctx| devices::run(ctx, devices::Op::Profile(profile)));
    }
}
