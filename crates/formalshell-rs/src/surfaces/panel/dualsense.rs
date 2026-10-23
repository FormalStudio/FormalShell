//! A read-only readout, the header's "Read only" chip
//! saying so, since the owner's host units own the lightbar and LED writes.
//! No controller is the dim "No controller" row; past it the hero carries
//! the battery as its readout and rail, and a Status row holds the lightbar
//! colour and the lit player LEDs, each half only while sysfs had it. The
//! cursor walks the rows and activates nothing.

use fs_system::dualsense::state_line;
use fs_theme::color::Rgba;

use super::{Panel, View};
use crate::store::Topic;
use crate::ui::el::Kind;
use crate::ui::{El, Ink, Type, Weight, w};

#[derive(Default)]
pub struct Dualsense;

fn half(s: &fs_theme::tokens::Space, title: &str, glyph: &str, middle: El, value: String, key: &str) -> El {
    let line = w::row(s.icon_gap, vec![w::icon(glyph), middle, w::space(0.0).fill(), w::value(value).size(Type::Body).weight(Weight::Medium).ink(Ink::Fg)]).fill();
    w::cell(w::column(s.xxs, vec![w::heading(s, title, None, false), line])).ghost().stop(key)
}

impl Panel for Dualsense {
    fn id(&self) -> &'static str {
        "dualsense"
    }

    fn title(&self, _: &View) -> String {
        "DualSense".into()
    }

    fn icon(&self, _: &View) -> String {
        "gamepad-2".into()
    }

    fn subtitle(&self, _: &View) -> String {
        "Controller".into()
    }

    fn status(&self, v: &View) -> String {
        v.store.devices.power.dualsense.as_ref().map_or_else(|| "no controller".into(), |s| format!("battery {}%", s.percent))
    }

    fn accent(&self) -> super::Accent {
        super::Accent::Secondary
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let s = &v.theme.space;
        vec![w::cell(w::section_label(s, "Read only", None, false)).cell_state(|c| c.chip = true)]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let Some(supply) = &v.store.devices.power.dualsense else {
            return w::column(s.section_gap, vec![w::section_label(s, "No controller", None, true)]);
        };
        let hero = w::hero_with(
            s,
            w::Hero {
                glyph: "gamepad-2".into(),
                title: "DualSense".into(),
                meta: state_line(Some(supply)),
                readout: format!("{}%", supply.percent),
                trailing: None,
                rail: Some(supply.percent as f64 / 100.0),
                rail_on: None,
                tab: "battery".into(),
            },
            false,
            Type::DisplayLarge,
            None,
        )
        .stop("battery");
        let mut out = vec![hero];
        let lights = &v.store.devices.lights;
        let mut halves = Vec::new();
        if let Some(hex) = &lights.lightbar {
            let fill = Rgba::parse(hex).unwrap_or(Rgba::TRANSPARENT);
            let body = v.theme.font_size.body;
            halves.push(half(s, "Lightbar", "lightbulb", w::swatch(fill, body, body, v.theme.radii.sm), hex.clone(), "lightbar"));
        }
        if let Some(lit) = lights.player_leds {
            let primary = v.theme.colors.get("primary");
            let muted = v.theme.colors.get("mutedForeground");
            let pips = (0..5)
                .map(|i| El::new(Kind::Swatch { color: if i < lit { primary } else { muted }, w: s.md, h: s.md, radius: s.md / 2.0, border: false }))
                .collect();
            halves.push(half(s, "Player LEDs", "circle-dot", w::row(s.sm, pips), format!("{lit} / 5"), "leds"));
        }
        if !halves.is_empty() {
            let halves = halves.into_iter().map(El::fill).collect();
            out.push(w::column(s.row_gap, vec![w::heading(s, "Status", None, true), w::row(s.row_gap, halves).fill()]));
        }
        w::column(s.section_gap, out)
    }
}
