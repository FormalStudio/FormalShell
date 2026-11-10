// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! Pond's settings page: four square tiles, two by two (Appearance,
//! Bluetooth, System, Information), each a glyph over its name, Appearance
//! and System opening a page of settings (`settings_page`, Bar and Desktop
//! under Appearance), beside a column of round session buttons (lock,
//! suspend, reboot, power off) as tall as the grid.
//! A tile hands over to the panel it names; a session
//! button closes the card and asks the shell or logind, restart and shut
//! down only on a second press, as the launcher's rows do.

use fs_chrome::bar::panels::PanelId;

use super::{Effect, Out, Panel, View, quick};
use crate::services::power;
use crate::store::Topic;
use crate::ui::{El, Event, On, Size, Type, Weight, What, w};

#[derive(Default)]
pub struct Settings {
    /// The restart or shut down button pressed once, by its row.
    armed: Option<usize>,
}

/// A tile or a session button, by its row in [`TILES`] or [`SESSION`].
#[derive(Clone, Copy, PartialEq)]
enum Press {
    Tile(usize),
    Session(usize),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Press::Tile(i) => write!(f, "tile:{}", TILES[i].0),
            Press::Session(i) => write!(f, "session:{}", SESSION[i].0),
        }
    }
}

const TILES: [(&str, &str, &str, PanelId); 4] = [
    ("appearance", "paint-bucket", "Appearance", PanelId::SettingsAppearance),
    ("bluetooth", "bluetooth", "Bluetooth", PanelId::Bluetooth),
    ("system", "hexagon", "System", PanelId::SettingsSystem),
    ("info", "info", "Information", PanelId::Info),
];

const SESSION: [(&str, &str, &str); 4] =
    [("lock", "lock", "Lock"), ("suspend", "moon", "Suspend"), ("reboot", "rotate-cw", "Restart"), ("poweroff", "power", "Shut down")];

impl Panel for Settings {
    fn title(&self, _: &View) -> String {
        "Settings".into()
    }

    fn header(&self) -> bool {
        false
    }

    fn reads(&self) -> &'static [Topic] {
        &[]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let gap = s.sm;
        let column = s.huge * 2.5;
        // Square tiles, as Pond's are: half of what the card leaves beside
        // the session column.
        let tile_h = ((super::width_of(PanelId::Settings.desc().width, s) - w::text_inset(s) * 2.0 - column - gap * 2.0) / 2.0).floor();
        let rows = TILES.len().div_ceil(2) as f64;
        let tiles = TILES
            .iter()
            .enumerate()
            .map(|(i, (_, glyph, name, _))| {
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
                    .stop(Press::Tile(i))
                    .on(Press::Tile(i))
            })
            .collect();
        let button_h = (tile_h * rows + gap * (rows - 1.0) - gap * 3.0) / 4.0;
        let session = SESSION
            .iter()
            .enumerate()
            .map(|(i, (_, glyph, tip))| {
                let armed = self.armed == Some(i);
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
                    .stop(Press::Session(i))
                    .on(Press::Session(i))
            })
            .collect();
        w::panel_text(s, w::row(gap, vec![w::grid(2, gap, tiles), w::column(gap, session).width(Size::Px(column))]).fill())
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let What::Click = ev.what {
            self.go(&ev.on, fx);
        }
    }

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        self.go(stop, fx);
    }

    /// The cursor leaving an armed button disarms it, as the launcher's
    /// confirm rows do on a selection change.
    fn reached(&mut self, stop: &On, _fx: &mut Effect) {
        self.disarm_off(Some(stop));
    }

    /// The pointer leaving it too, so a click, a move away and a click
    /// back arms again.
    fn pointed(&mut self, stop: Option<&On>) {
        self.disarm_off(stop);
    }
}

impl Settings {
    fn disarm_off(&mut self, stop: Option<&On>) {
        if self.armed.is_some_and(|i| stop.and_then(|s| s.get::<Press>()) != Some(&Press::Session(i))) {
            self.armed = None;
        }
    }

    fn go(&mut self, on: &On, fx: &mut Effect) {
        let armed = self.armed.take();
        let i = match on.get::<Press>() {
            Some(&Press::Tile(i)) => {
                fx.push(Out::Panel(TILES[i].3, None));
                return;
            }
            Some(&Press::Session(i)) => i,
            None => return,
        };
        let id = SESSION[i].0;
        if id == "lock" {
            fx.push(Out::Close);
            fx.push(Out::Quick(quick::Run::Lock));
            return;
        }
        if matches!(id, "reboot" | "poweroff") && armed != Some(i) {
            self.armed = Some(i);
            return;
        }
        if let Some(action) = power::Action::parse(id) {
            power::request(action);
            fx.push(Out::Close);
        }
    }
}
