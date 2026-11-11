//! The overview: every output zoomed out onto its stack of workspaces, the
//! windows live and scaled where they are, the input the engine's own while
//! it is open.

use formalwm_core::{
    chord::Mods,
    geom::Rect as Box2,
    model::Home,
    motion::{Spring, SpringParams, project},
    overview::Lens,
    tape::Side,
};
use fs_wm_proto::Motion;
use shojiwm_rs::prelude::*;

use crate::{
    drag::Drag,
    engine::{Engine, enter},
};

/// The progress spring's unit: it runs from 0 to 1 over about a thousand
/// pixels of zoom.
const UNIT: f64 = 0.001;
/// How far the pointer travels with the button down before it is a drag.
const DRAG_START: f64 = 8.0;
/// Wheel or two-finger travel that walks one workspace.
const SCROLL_STEP: f64 = 40.0;
const BTN_LEFT: u32 = 0x110;

pub struct Overview {
    pub open: bool,
    pub progress: Spring,
    /// Fingers have it.
    pub held: bool,
    pub grab: Option<InputGrab>,
    press: Option<Press>,
    scroll: f64,
}

struct Press {
    window: Option<String>,
    at: (f64, f64),
}

/// What is under a point of the open overview.
#[derive(Debug, PartialEq)]
pub enum Hit {
    Window {
        id: String,
        workspace: String,
        x: f64,
    },
    Workspace {
        id: String,
        x: f64,
    },
    /// Between two workspaces of an output, or past its ends.
    Between {
        output: String,
        index: usize,
    },
    Nothing,
}

impl Default for Overview {
    fn default() -> Self {
        Self {
            open: false,
            progress: Spring::at(0.0),
            held: false,
            grab: None,
            press: None,
            scroll: 0.0,
        }
    }
}

impl Overview {
    pub fn resting(&self) -> bool {
        self.held || self.progress.resting()
    }

    pub fn step(&mut self, dt: f64, motion: &Motion) {
        if self.held {
            return;
        }
        if motion.enabled {
            self.progress.step_in(
                SpringParams::from_family(&motion.families.spatial),
                dt,
                UNIT,
            );
        } else {
            self.progress.jump(self.progress.target);
        }
    }
}

impl Engine {
    pub fn lens(&self, output: &str) -> Option<(Lens, Box2)> {
        let progress = self.overview.progress.value;
        if progress <= 0.0 {
            return None;
        }
        let area = self.model.output(output)?.geo.rect;
        Some((Lens::new(area.w, area.h, progress), area))
    }

    /// `open` absent toggles.
    pub fn set_overview(&mut self, open: Option<bool>) {
        let open = open.unwrap_or(!self.overview.open);
        self.overview.held = false;
        self.overview.progress.target = if open { 1.0 } else { 0.0 };
        if !self.theme.chrome.motion.enabled {
            self.overview.progress.jump(self.overview.progress.target);
        }
        if open != self.overview.open {
            self.overview.open = open;
            self.overview.press = None;
            self.overview.scroll = 0.0;
            if open {
                self.grab_overview();
            } else {
                if let Some(grab) = self.overview.grab.take() {
                    grab.release();
                }
                // A drag of the overview's own ends with it; one the core
                // is running goes on over the workspace now shown.
                if !self.hold.window_drag {
                    self.drag = None;
                }
            }
        }
        self.present();
        self.kick();
        self.publish();
    }

    /// The overview takes the keyboard and the pointer's buttons. Not while
    /// something is being dragged: that drag has the pointer and must keep
    /// it to be dropped.
    fn grab_overview(&mut self) {
        if self.hold.data_drag || self.hold.window_drag || self.overview.grab.is_some() {
            return;
        }
        let options = InputGrabOptions::new()
            .on_key(|event| {
                let (key, code) = (event.key.clone(), event.keycode);
                if event.state != InputGrabState::Pressed {
                    return enter(move |engine| engine.run_release(&key));
                }
                let mods = Mods {
                    logo: event.modifiers.logo,
                    shift: event.modifiers.shift,
                    ctrl: event.modifiers.ctrl,
                    alt: event.modifiers.alt,
                };
                enter(move |engine| engine.overview_key(&key, code, mods));
            })
            .on_pointer_motion(|event| {
                let at = (event.position.x, event.position.y);
                enter(move |engine| engine.overview_motion(at));
            })
            .on_pointer_button(|event| {
                if event.button != BTN_LEFT {
                    return;
                }
                let pressed = event.state == InputGrabState::Pressed;
                let at = (event.position.x, event.position.y);
                enter(move |engine| engine.overview_button(pressed, at));
            })
            .on_scroll(|event| {
                let travel = event
                    .discrete_y
                    .map_or(event.delta_y, |steps| steps * SCROLL_STEP);
                enter(move |engine| engine.overview_scroll(travel));
            })
            .on_swipe(|event| {
                let event = event.clone();
                enter(move |engine| engine.swipe(&event));
            })
            .on_cancel(|_| {
                enter(|engine| {
                    engine.overview.grab = None;
                    engine.set_overview(Some(false));
                })
            });
        self.overview.grab = Some(COMPOSITOR.input.grab(options));
    }

    /// Takes the input an open overview should have and has not: after a
    /// drag that kept it, or a reload that ended the grab.
    pub fn regrab_overview(&mut self) {
        if self
            .overview
            .grab
            .as_ref()
            .is_some_and(InputGrab::is_active)
        {
            return;
        }
        self.overview.grab = None;
        if self.overview.open {
            self.grab_overview();
        }
    }

    fn overview_key(&mut self, key: &str, code: u32, mods: Mods) {
        let plain = mods == Mods::default();
        let moved = match key {
            "Escape" | "Return" if plain => return self.set_overview(Some(false)),
            "Up" if plain => self.model.focus_workspace_relative(-1),
            "Down" if plain => self.model.focus_workspace_relative(1),
            "Left" if plain => self.model.focus_column(Side::Prev),
            "Right" if plain => self.model.focus_column(Side::Next),
            _ => return self.run_chord(key, code, mods),
        };
        if moved {
            self.commit();
        }
    }

    fn overview_scroll(&mut self, travel: f64) {
        self.overview.scroll += travel;
        let steps = (self.overview.scroll / SCROLL_STEP).trunc();
        if steps == 0.0 {
            return;
        }
        self.overview.scroll -= steps * SCROLL_STEP;
        if self.model.focus_workspace_relative(steps as isize) {
            self.commit();
        }
    }

    fn overview_button(&mut self, pressed: bool, at: (f64, f64)) {
        if pressed {
            let window = match self.overview_hit(at, None) {
                Hit::Window { id, .. } => Some(id),
                _ => None,
            };
            self.overview.press = Some(Press { window, at });
            return;
        }
        let Some(press) = self.overview.press.take() else {
            return;
        };
        if let Some(drag) = self.drag.take() {
            self.overview_drop(&drag, at);
            return;
        }
        match self.overview_hit(at, None) {
            Hit::Window { id, .. } if press.window.as_ref() == Some(&id) => {
                self.model.focus_window(&id);
            }
            Hit::Workspace { id, .. } => {
                self.model.focus_workspace(&id);
            }
            _ => {}
        }
        self.commit();
        self.set_overview(Some(false));
    }

    fn overview_motion(&mut self, at: (f64, f64)) {
        let Some(press) = &self.overview.press else {
            return;
        };
        if self.drag.is_none() {
            let Some(id) = press.window.clone() else {
                return;
            };
            if (at.0 - press.at.0).hypot(at.1 - press.at.1) < DRAG_START {
                return;
            }
            let Some(visual) = self.scene.visual(&id) else {
                return;
            };
            let Some((lens, area)) = self.lens(&visual.output) else {
                return;
            };
            let drawn = lens
                .map(visual.rect.translate(-area.x, -area.y), visual.lane)
                .translate(area.x, area.y);
            self.drag = Some(Drag {
                id,
                size: visual.size,
                grip: (
                    (press.at.0 - drawn.x) / drawn.w.max(1.0),
                    (press.at.1 - drawn.y) / drawn.h.max(1.0),
                ),
                at,
            });
        }
        if let Some(drag) = &mut self.drag {
            drag.at = at;
        }
        self.present_drag();
    }

    /// A window let go over the overview: onto the workspace under it, at
    /// the column gap under it, or onto a new workspace between two.
    pub fn overview_drop(&mut self, drag: &Drag, at: (f64, f64)) {
        let id = drag.id.clone();
        let tiled = self
            .model
            .window(&id)
            .is_some_and(|window| !window.floating);
        let target = match self.overview_hit(at, Some(&id)) {
            Hit::Window { workspace, x, .. } => Some((workspace, Some(x))),
            Hit::Workspace { id, x } => Some((id, Some(x))),
            Hit::Between { output, index } => {
                Some((self.model.insert_workspace(&output, index), None))
            }
            Hit::Nothing => None,
        };
        if let Some((workspace, x)) = target {
            let before = x
                .filter(|_| tiled)
                .and_then(|x| self.column_before(&workspace, x, &id));
            let home = self
                .model
                .window(&id)
                .is_some_and(|window| window.home == Home::Workspace(workspace.clone()));
            match before {
                Some(Some(before)) => {
                    self.model.move_column(&id, Some(&before));
                }
                Some(None) => {
                    if !home {
                        self.model.move_to_workspace(&id, &workspace, false);
                    }
                    self.model.move_column(&id, None);
                }
                None if !home => {
                    self.model.move_to_workspace(&id, &workspace, false);
                }
                None => {}
            }
        }
        self.scene.sync(&self.model);
        if let Some(from) = self.undo_lens(&id, self.drag_box(drag)) {
            self.scene.carry(&id, from);
        }
        self.commit();
    }

    /// The window a column dropped at global x `x` of a workspace's tape
    /// goes before: `Some(None)` for the end of the tape, `None` when it
    /// would land where `moving` already is.
    pub fn column_before(&self, workspace: &str, x: f64, moving: &str) -> Option<Option<String>> {
        let ws = self.model.workspace(workspace)?;
        let strip = self.model.strip_of(&ws.output);
        let gap = ws.tape.gap_at(x - strip.x + ws.tape.scroll, strip.w);
        let own = ws
            .tape
            .find(moving)
            .filter(|(column, _)| ws.tape.columns[*column].windows.len() == 1)
            .map(|(column, _)| column);
        if own.is_some_and(|own| gap == own || gap == own + 1) {
            return None;
        }
        Some(
            ws.tape
                .columns
                .get(gap)
                .and_then(|column| column.windows.first().cloned()),
        )
    }

    /// The box the scene would have to draw for the lens to show `drawn`.
    pub fn undo_lens(&self, id: &str, drawn: Box2) -> Option<Box2> {
        let visual = self.scene.visual(id)?;
        let Some((lens, area)) = self.lens(&visual.output) else {
            return Some(drawn);
        };
        let (x, y) = lens.locate(drawn.x - area.x, drawn.y - area.y, visual.lane);
        Some(Box2::new(
            area.x + x,
            area.y + y + visual.lane * area.h,
            drawn.w / lens.zoom,
            drawn.h / lens.zoom,
        ))
    }

    /// What is under a global point. `skip` is a window being dragged,
    /// which is not under itself.
    pub fn overview_hit(&self, at: (f64, f64), skip: Option<&str>) -> Hit {
        let Some(output) = self
            .model
            .outputs
            .iter()
            .find(|output| output.geo.rect.contains(at.0, at.1))
        else {
            return Hit::Nothing;
        };
        let name = &output.geo.name;
        let area = output.geo.rect;
        let lens = Lens::new(area.w, area.h, self.overview.progress.value);
        let stack_at = self.scene.stack_of(name);
        let (px, py) = (at.0 - area.x, at.1 - area.y);
        let layout = self.model.layout();
        let stack = self.model.stack(name);
        for (index, ws) in stack.iter().enumerate() {
            let lane = index as f64 - stack_at;
            let panel = lens.panel(lane);
            if py < panel.y {
                return Hit::Between {
                    output: name.clone(),
                    index,
                };
            }
            if py > panel.bottom() {
                continue;
            }
            let (x, y) = lens.locate(px, py, lane);
            let mut windows: Vec<_> = layout
                .iter()
                .filter(|place| place.workspace.as_ref() == Some(&ws.id))
                .filter(|place| skip != Some(place.id.as_str()))
                .collect();
            windows.sort_by_key(|place| std::cmp::Reverse((place.layer, place.order)));
            let scroll = self.scene.scroll_of(&ws.id);
            for place in windows {
                let shift = if place.scrolls { scroll } else { 0.0 };
                if place
                    .rect
                    .translate(-area.x - shift, -area.y)
                    .contains(x, y)
                {
                    return Hit::Window {
                        id: place.id.clone(),
                        workspace: ws.id.clone(),
                        x: area.x + x,
                    };
                }
            }
            return if panel.contains(px, py) {
                Hit::Workspace {
                    id: ws.id.clone(),
                    x: area.x + x,
                }
            } else {
                Hit::Nothing
            };
        }
        Hit::Between {
            output: name.clone(),
            index: stack.len(),
        }
    }

    /// Four fingers: the overview under them, then let go with a velocity.
    pub fn overview_swipe(&mut self, total_y: f64, start: f64, height: f64) {
        self.overview.held = true;
        let progress = (start - total_y / (height * 0.5)).clamp(0.0, 1.0);
        self.overview.progress.jump(progress);
        self.present();
        self.kick();
    }

    pub fn overview_release(&mut self, velocity_y: f64, height: f64) {
        let span = height * 0.5;
        let velocity = -velocity_y / span;
        let landing = self.overview.progress.value + project(-velocity_y) / span;
        self.overview.held = false;
        self.set_overview(Some(landing > 0.5));
        if self.theme.chrome.motion.enabled {
            self.overview.progress.velocity = velocity;
        }
        self.kick();
    }
}
