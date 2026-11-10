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

use super::{Effect, Out, Panel, Verb, View};
use crate::services::power;
use crate::ui::confirm::Armed;
use crate::store::Topic;
use crate::ui::{El, Event, On, Size, Type, Weight, What, w};

#[derive(Default)]
pub struct Settings {
    /// The restart or shut down button pressed once.
    armed: Armed<Session>,
}

/// A session button beside the tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Session {
    Lock,
    Power(power::Action),
}

impl Session {
    const ALL: [Session; 4] = [Session::Lock, Session::Power(power::Action::Suspend), Session::Power(power::Action::Reboot), Session::Power(power::Action::PowerOff)];

    fn id(self) -> &'static str {
        match self {
            Session::Lock => "lock",
            Session::Power(a) => a.id(),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Session::Lock => "lock",
            Session::Power(a) => a.icon(),
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Session::Lock => "Lock",
            Session::Power(power::Action::Suspend) => "Suspend",
            Session::Power(power::Action::Reboot) => "Restart",
            Session::Power(power::Action::PowerOff) => "Shut down",
        }
    }

    fn confirms(self) -> bool {
        match self {
            Session::Lock => false,
            Session::Power(a) => a.confirms(power::Seat::Session),
        }
    }
}

/// A tile by its row in [`TILES`], or a session button.
#[derive(Clone, Copy, PartialEq)]
enum Press {
    Tile(usize),
    Session(Session),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Press::Tile(i) => write!(f, "tile:{}", TILES[i].0),
            Press::Session(b) => write!(f, "session:{}", b.id()),
        }
    }
}

const TILES: [(&str, &str, &str, PanelId); 4] = [
    ("appearance", "paint-bucket", "Appearance", PanelId::SettingsAppearance),
    ("bluetooth", "bluetooth", "Bluetooth", PanelId::Bluetooth),
    ("system", "hexagon", "System", PanelId::SettingsSystem),
    ("info", "info", "Information", PanelId::Info),
];

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
        let session = Session::ALL
            .into_iter()
            .map(|b| {
                let armed = self.armed.is(&b);
                let side = button_h - s.lg * 2.0;
                let chip = w::cell(w::row(0.0, vec![w::spacer(), w::icon(b.icon()).size(Type::Display), w::spacer()]).fill().height(side))
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
                    .tip(b.tip())
                    .stop(Press::Session(b))
                    .on(Press::Session(b))
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
        let on = match stop.and_then(|s| s.get::<Press>()) {
            Some(&Press::Session(b)) => Some(b),
            _ => None,
        };
        self.armed.keep(on.as_ref());
    }

    fn go(&mut self, on: &On, fx: &mut Effect) {
        let b = match on.get::<Press>() {
            Some(&Press::Session(b)) => b,
            Some(&Press::Tile(i)) => {
                self.armed.clear();
                fx.push(Out::Panel(TILES[i].3, None));
                return;
            }
            None => {
                self.armed.clear();
                return;
            }
        };
        if b.confirms() && !self.armed.press(b) {
            return;
        }
        self.armed.clear();
        match b {
            Session::Lock => {
                fx.push(Out::Close);
                fx.push(Out::Do(Verb::Lock));
            }
            Session::Power(action) => {
                power::request(action);
                fx.push(Out::Close);
            }
        }
    }
}
