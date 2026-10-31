// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! Pond's settings page: six square tiles, two by three (Appearance, Bar,
//! Desktop, System, Bluetooth, Information), each a glyph over its name,
//! the first four opening a page of settings (`settings_page`), beside a
//! column of round session buttons (lock, suspend, reboot, power off).
//! A tile hands over to the panel it names; a session
//! button closes the card and asks the shell or logind, restart and shut
//! down only on a second press, as the launcher's rows do.

use super::{Effect, Panel, View};
use crate::services::power;
use crate::store::Topic;
use crate::ui::{El, Event, Size, Type, Weight, What, w};

#[derive(Default)]
pub struct Settings {
    /// The restart or shut down button pressed once.
    armed: Option<&'static str>,
}

const TILES: [(&str, &str, &str, &str); 6] = [
    ("appearance", "paint-bucket", "Appearance", "settings:appearance"),
    ("bar", "panel-top", "Bar", "settings:bar"),
    ("desktop", "monitor", "Desktop", "settings:desktop"),
    ("system", "hexagon", "System", "settings:system"),
    ("bluetooth", "bluetooth", "Bluetooth", "bluetooth"),
    ("info", "info", "Information", "info"),
];

const SESSION: [(&str, &str, &str); 4] =
    [("lock", "lock", "Lock"), ("suspend", "moon", "Suspend"), ("reboot", "rotate-cw", "Restart"), ("poweroff", "power", "Shut down")];

impl Panel for Settings {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn title(&self, _: &View) -> String {
        "Settings".into()
    }

    fn header(&self) -> bool {
        false
    }

    fn reads(&self) -> &'static [Topic] {
        &[]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_menu
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let tile_h = s.huge * 4.0;
        let rows = TILES.len().div_ceil(2) as f64;
        let tiles = TILES
            .iter()
            .map(|(id, glyph, name, _)| {
                let face = w::column(
                    0.0,
                    vec![
                        w::icon(*glyph).size(Type::DisplayLarge),
                        w::space(tile_h - s.xl * 2.0 - s.huge * 2.0),
                        w::text(*name).size(Type::Display).weight(Weight::Medium).elide(),
                    ],
                )
                .pad(s.lg, s.xl, s.lg, s.xl);
                w::cell(face)
                    .cell_state(|c| {
                        c.group = true;
                        c.inset = Some((0.0, 0.0));
                    })
                    .height(tile_h)
                    .interactive()
                    .stop(format!("tile:{id}"))
                    .on(format!("tile:{id}"))
            })
            .collect();
        let gap = s.sm;
        let button_h = (tile_h * rows + gap * (rows - 1.0) - gap * 3.0) / 4.0;
        let session = SESSION
            .iter()
            .map(|(id, glyph, tip)| {
                let armed = self.armed == Some(*id);
                let side = button_h - s.lg * 2.0;
                let chip = w::cell(w::row(0.0, vec![w::spacer(), w::icon(*glyph).size(Type::Display), w::spacer()]).fill().height(side))
                    .cell_state(|c| {
                        c.glyph = true;
                        c.round = true;
                        c.open = armed;
                        c.inset = Some((0.0, 0.0));
                    })
                    .width(Size::Px(side))
                    .height(side);
                w::cell(w::row(0.0, vec![w::spacer(), chip, w::spacer()]).fill().height(button_h))
                    .cell_state(|c| {
                        c.group = true;
                        c.open = armed;
                        c.inset = Some((0.0, 0.0));
                    })
                    .height(button_h)
                    .interactive()
                    .tip(*tip)
                    .stop(format!("session:{id}"))
                    .on(format!("session:{id}"))
            })
            .collect();
        w::row(gap, vec![w::grid(2, gap, tiles), w::column(gap, session).width(Size::Px(s.huge * 2.5))])
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let What::Click = ev.what {
            self.go(&ev.on.clone(), fx);
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        self.go(stop, fx);
    }

    /// The cursor leaving an armed button disarms it, as the launcher's
    /// confirm rows do on a selection change.
    fn reached(&mut self, stop: &str, _fx: &mut Effect) {
        self.disarm_off(Some(stop));
    }

    /// The pointer leaving it too, so a click, a move away and a click
    /// back arms again.
    fn pointed(&mut self, stop: Option<&str>) {
        self.disarm_off(stop);
    }
}

impl Settings {
    fn disarm_off(&mut self, stop: Option<&str>) {
        if self.armed.is_some_and(|id| stop.and_then(|s| s.strip_prefix("session:")) != Some(id)) {
            self.armed = None;
        }
    }

    fn go(&mut self, on: &str, fx: &mut Effect) {
        let armed = self.armed.take();
        if let Some(id) = on.strip_prefix("tile:") {
            if let Some(t) = TILES.iter().find(|t| t.0 == id) {
                fx.panel = Some(t.3);
            }
            return;
        }
        let Some(&(id, ..)) = on.strip_prefix("session:").and_then(|id| SESSION.iter().find(|b| b.0 == id)) else { return };
        if id == "lock" {
            fx.call = Some(("lock", "lock"));
            return;
        }
        if matches!(id, "reboot" | "poweroff") && armed != Some(id) {
            self.armed = Some(id);
            return;
        }
        if let Some(action) = power::Action::parse(id) {
            power::request(action);
            fx.close = true;
        }
    }
}
