// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! Pond's Battery card under the panel's header: a dim "Battery" with the
//! charge state at the far end, the percent at hero size beside the draw
//! and the time left, a thick bar
//! for the charge, then "Power profile" with the active one named over the
//! profiles as three equal buttons, one `ButtonGroup`. The group is the
//! panel's one cursor stop: Left and Right walk its buttons and Enter
//! applies the one under the ring. Under them the power flow diagram where
//! the machine reports a supply; a machine with neither battery nor supply
//! says "No power sources".

mod diagram;

use std::cell::Cell;

use fs_system::power::flow::{self, Extras, Link};
use fs_system::power::model::{self, charge_threshold_active, format_duration, format_rate};
use fs_theme::tokens::Space;
use fs_types::Percent;
use fs_upower::{DeviceState, Profile};

use super::{Effect, Nav, Panel, View};
use diagram::{Flow, FlowNode, FlowPort};
use crate::services::devices;
use crate::services::wants::Source;
use crate::store::{Store, Topic};
use crate::ui::el::Opt;
use crate::ui::{El, Event, Ink, Type, Weight, What, w};

const PROFILES: [Profile; 3] = [Profile::PowerSaver, Profile::Balanced, Profile::Performance];
const ICONS: [&str; 3] = ["leaf", "gauge", "zap"];

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

/// What the battery card shows, read off UPower by the panel or made up
/// by the gallery's sample.
pub struct Card {
    pub level: Percent,
    /// The words at the card's far end, and whether they are the lit ink.
    pub state: Option<(&'static str, bool)>,
    pub rate: Option<String>,
    pub time: Option<String>,
    pub details: Vec<String>,
}

/// The battery half of Pond's card.
pub fn battery_card(s: &Space, c: &Card) -> El {
    let mut head = vec![w::text("Battery").size(Type::Title).weight(Weight::Normal).ink(Ink::Dim).fill()];
    if let Some((words, lit)) = c.state {
        head.push(w::text(words).size(Type::Title).weight(Weight::Normal).ink(if lit { Ink::Primary } else { Ink::Dim }));
    }
    let mut side = Vec::new();
    if let Some(r) = &c.rate {
        side.push(w::text(r.clone()).size(Type::Title).weight(Weight::Normal).ink(Ink::Muted));
    }
    if let Some(t) = &c.time {
        side.push(w::text(t.clone()).size(Type::Title).weight(Weight::Normal).ink(Ink::Dim));
    }
    let percent = w::text(format!("{}%", c.level.rounded())).size(Type::Hero).weight(Weight::Medium).fill();
    let readout = w::row(s.md, vec![percent, w::column(s.xxs, side)]);
    let mut parts = vec![w::row(0.0, head).fill(), readout.fill(), w::track(c.level.as_fraction()).height(s.panel_icon)];
    if !c.details.is_empty() {
        parts.push(w::caption(c.details.join(" \u{00b7} ")));
    }
    w::column(s.md, parts)
}

/// The "Power profile" caption, the active one named at its far end, over
/// the three buttons.
pub fn profiles(s: &Space, active: usize, ring: usize, performance: bool) -> El {
    let options = (0..3)
        .map(|i| {
            let mut o = Opt::new("").icon(ICONS[i]);
            o.enabled = PROFILES[i] != Profile::Performance || performance;
            o
        })
        .collect();
    w::panel_control(s, w::panel_heading_with(s, "Power profile", PROFILES[active].label()), w::group(options, active, true).squares().ring(ring).on("profile").stop("profile"))
}

#[derive(Default)]
pub struct Power {
    /// Where the group's ring sits: the active profile on a fresh open.
    cursor: Cell<Option<usize>>,
}

fn active(v: &View) -> usize {
    v.store.profiles.as_ref().and_then(|p| PROFILES.iter().position(|x| *x == p.active)).unwrap_or(0)
}

/// The card off UPower's battery and the supply's charge limit.
fn card(v: &View, b: &devices::power::Battery, held: bool) -> Card {
    let power = &v.store.power;
    let charging = b.state == DeviceState::Charging;
    let state = if held {
        Some(("Holding at limit", false))
    } else if power.on_battery {
        None
    } else if b.state == DeviceState::FullyCharged || b.level.as_percent() >= 100.0 {
        Some(("Charged", true))
    } else {
        Some(("Charging", true))
    };
    let left = if charging { b.time_to_full } else { b.time_to_empty };
    let time = left.filter(|_| !held).map(|t| format!("{} {}", format_duration(t), if charging { "to full" } else { "left" }));
    let limit = v
        .store
        .power_flow
        .snapshot
        .supplies
        .iter()
        .find(|s| s.kind == "Battery")
        .and_then(|s| s.limit)
        .and_then(model::charge_limit);
    let mut details = Vec::new();
    if let Some(h) = b.health {
        details.push(format!("Health {}%", h.rounded()));
    }
    if b.size_wh > 0.0 {
        details.push(format!("{:.1} Wh", b.size_wh));
    }
    if let Some(l) = limit {
        details.push(format!("Limit {l}%"));
    }
    Card { level: b.level, state, rate: (b.rate > 0.0).then(|| format_rate(b.rate)), time, details }
}

impl Panel for Power {
    fn title(&self, _: &View) -> String {
        "Power".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Power, Topic::Profiles, Topic::PowerFlow, Topic::Monitor, Topic::Iphone, Topic::Config]
    }

    fn wants(&self, _: &Store) -> Vec<Source> {
        vec![Source::PowerFlow, Source::Monitor]
    }

    fn opened(&mut self) {
        self.cursor.set(None);
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let mut parts = Vec::new();

        let sample = &v.store.power_flow;
        let gpu_w = {
            let cards = &v.store.monitor.cards;
            let watts: Vec<f64> = cards.iter().filter_map(|c| c.record.metrics.power_w).collect();
            (!watts.is_empty()).then(|| watts.iter().sum())
        };
        let held = v
            .store
            .power
            .battery
            .as_ref()
            .is_some_and(|b| charge_threshold_active(b.level, b.state, b.rate, b.time_to_full, v.store.power.on_battery));
        let model = flow::build_flow(
            &sample.snapshot,
            &Extras { cpu_w: sample.cpu_w, gpu_w, iphone_percent: v.store.iphone.battery.map(|b| b * 100.0), held },
        );
        match &v.store.power.battery {
            Some(b) => parts.push(w::panel_text(s, battery_card(s, &card(v, b, held)))),
            None => {
                if !model.available {
                    parts.push(w::panel_empty(s, "No power sources"));
                }
            }
        }

        let at = self.cursor.get().unwrap_or_else(|| {
            let a = active(v);
            self.cursor.set(Some(a));
            a
        });
        let performance = v.store.profiles.as_ref().is_none_or(|p| p.has_performance());
        parts.push(profiles(s, active(v), at, performance));
        if model.available {
            parts.push(w::panel_control(s, w::panel_heading(s, "Power flow"), El::custom(flow_view(&model, true)).fill()));
        }
        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &Event, _: &mut Effect) {
        if let ("profile", What::Pick(i)) = (ev.on.as_str(), &ev.what) {
            self.apply(*i);
        }
    }

    fn activate(&mut self, stop: &str, _: &mut Effect) {
        if stop == "profile" {
            let at = self.cursor.get().unwrap_or(0);
            self.apply(at);
        }
    }

    fn step(&mut self, stop: &str, direction: i32, _: &mut Effect) {
        if stop == "profile" {
            let at = self.cursor.get().unwrap_or(0) as i32 + direction;
            self.cursor.set(Some(at.clamp(0, 2) as usize));
        }
    }

    fn nav(&self, _: Option<&str>) -> Nav {
        Nav::STEPS
    }
}

impl Power {
    fn apply(&self, index: usize) {
        let Some(profile) = PROFILES.get(index).copied() else { return };
        self.cursor.set(Some(index));
        devices::power::send(devices::power::Cmd::Profile(profile));
    }
}
