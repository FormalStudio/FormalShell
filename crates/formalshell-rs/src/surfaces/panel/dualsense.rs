//! A read-only readout, a "Read only" line under it
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
use crate::ui::{El, Ink, w};

#[derive(Default)]
pub struct Dualsense;

fn status_row(s: &fs_theme::tokens::Space, title: &str, glyph: &str, middle: El, value: String, key: &str) -> El {
    w::panel_row(s, glyph, title, None, vec![middle, w::panel_value(value)]).stop(key)
}

impl Panel for Dualsense {
    fn id(&self) -> &'static str {
        "dualsense"
    }

    fn title(&self, _: &View) -> String {
        "DualSense".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let Some(supply) = &v.store.devices.power.dualsense else {
            return w::column(s.section_gap, vec![w::panel_empty(s, "No controller")]);
        };
        let hero = w::hero(
            s,
            w::Hero {
                glyph: "gamepad-2".into(),
                title: "DualSense".into(),
                meta: state_line(Some(supply)),
                readout: format!("{}%", supply.percent),
                trailing: None,
                rail: Some(supply.percent as f64 / 100.0),
                rail_on: None,
            },
        )
        .stop("battery");
        let mut out = vec![hero];
        let lights = &v.store.devices.lights;
        let mut rows = Vec::new();
        if let Some(hex) = &lights.lightbar {
            let fill = Rgba::parse(hex).unwrap_or(Rgba::TRANSPARENT);
            let body = v.theme.font_size.body;
            rows.push(status_row(s, "Lightbar", "lightbulb", w::swatch(fill, body, body, v.theme.radii.sm), hex.clone(), "lightbar"));
        }
        if let Some(lit) = lights.player_leds {
            let primary = v.theme.colors.get("primary");
            let muted = v.theme.colors.get("mutedForeground");
            let pips = (0..5)
                .map(|i| El::new(Kind::Swatch { color: if i < lit { primary } else { muted }, w: s.sm, h: s.sm, radius: s.sm / 2.0, border: false }))
                .collect();
            rows.push(status_row(s, "Player LEDs", "circle-dot", w::row(s.sm, pips), format!("{lit} / 5"), "leds"));
        }
        if !rows.is_empty() {
            out.push(w::panel_section(s, "Status", rows));
        }
        out.push(w::panel_text(s, w::caption("Read only").ink(Ink::Muted).pad_top(s.panel_section)));
        w::column(0.0, out)
    }
}
