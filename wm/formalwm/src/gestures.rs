//! Touchpad swipes: three fingers sideways drag the tape under them and
//! fling it, three fingers up or down walk the output's workspaces as far
//! as the fling carries, four fingers open and close the overview.

use formalwm_core::motion::project;
use shojiwm_rs::{
    prelude::*,
    ssd::{GestureSwipeEventSnapshot, GestureSwipePhaseSnapshot},
};

use crate::engine::{Engine, enter};

/// Travel before a swipe has picked its axis, in pixels.
const DECIDE: f64 = 6.0;
/// How far past its ends a stack can be pulled, in workspaces.
const OVERPULL: f64 = 0.25;

#[derive(Clone, Debug, Default, PartialEq)]
enum Mode {
    #[default]
    Idle,
    Undecided,
    Tape(String),
    Stack,
    Overview,
}

#[derive(Default)]
pub struct Gesture {
    mode: Mode,
    output: String,
    /// The stack position or the overview's progress the swipe started at.
    start: f64,
}

pub fn setup() {
    COMPOSITOR.event.on_gesture_swipe(|event| {
        let event = event.clone();
        enter(move |engine| engine.swipe(&event));
    });
}

impl Engine {
    pub fn swipe(&mut self, event: &GestureSwipeEventSnapshot) {
        match event.phase {
            GestureSwipePhaseSnapshot::Begin => {
                self.gesture = Gesture {
                    mode: Mode::Undecided,
                    output: event
                        .output_name
                        .clone()
                        .filter(|name| self.model.output(name).is_some())
                        .unwrap_or_else(|| self.model.focused_output.clone()),
                    start: 0.0,
                };
            }
            GestureSwipePhaseSnapshot::Update => self.swipe_update(event),
            GestureSwipePhaseSnapshot::End | GestureSwipePhaseSnapshot::Cancel => {
                self.swipe_end(event)
            }
        }
    }

    fn height(&self) -> f64 {
        self.model
            .output(&self.gesture.output)
            .map_or(1080.0, |output| output.geo.rect.h.max(1.0))
    }

    fn swipe_update(&mut self, event: &GestureSwipeEventSnapshot) {
        let output = self.gesture.output.clone();
        if self.gesture.mode == Mode::Undecided {
            let (x, y) = (event.total_x.abs(), event.total_y.abs());
            if x.max(y) < DECIDE {
                return;
            }
            let active = self.model.active_workspace(&output).map(|ws| ws.id.clone());
            (self.gesture.mode, self.gesture.start) = match (event.fingers, x > y, active) {
                (3, true, Some(workspace)) if !self.overview.open => (Mode::Tape(workspace), 0.0),
                (3, false, _) => (Mode::Stack, self.scene.stack_of(&output)),
                (fingers, false, _) if fingers >= 4 => {
                    (Mode::Overview, self.overview.progress.on(&output))
                }
                _ => (Mode::Idle, 0.0),
            };
            self.model.focus_output(&output);
        }
        match self.gesture.mode.clone() {
            Mode::Tape(workspace) => {
                if self.model.scroll_by(&workspace, -event.delta_x) {
                    self.scene.sync(&self.model);
                    self.scene.hold_scroll(&workspace);
                    self.present();
                }
            }
            Mode::Stack => {
                let last = self.model.stack(&output).len().saturating_sub(1) as f64;
                let position = (self.gesture.start - event.total_y / self.height())
                    .clamp(-OVERPULL, last + OVERPULL);
                self.scene.hold_stack(&output, position);
                self.present();
                self.kick();
            }
            Mode::Overview => {
                self.overview_swipe(event.total_y, self.gesture.start, self.height());
            }
            Mode::Idle | Mode::Undecided => {}
        }
    }

    fn swipe_end(&mut self, event: &GestureSwipeEventSnapshot) {
        let output = self.gesture.output.clone();
        let cancelled = event.phase == GestureSwipePhaseSnapshot::Cancel;
        let (vx, vy) = if cancelled {
            (0.0, 0.0)
        } else {
            (event.velocity_x, event.velocity_y)
        };
        match std::mem::take(&mut self.gesture.mode) {
            Mode::Tape(workspace) => {
                let scroll = self
                    .model
                    .workspace(&workspace)
                    .map_or(0.0, |ws| ws.tape.scroll);
                self.model.scroll_release(&workspace, scroll + project(-vx));
                self.scene.sync(&self.model);
                self.scene.release_scroll(&workspace, -vx);
                self.commit();
            }
            Mode::Stack => {
                let height = self.height();
                let last = self.model.stack(&output).len().saturating_sub(1) as f64;
                let landing = (self.scene.stack_of(&output) + project(-vy) / height)
                    .round()
                    .clamp(0.0, last);
                self.model
                    .focus_workspace_at(landing as i64 + 1, Some(&output));
                self.scene.sync(&self.model);
                self.scene.release_stack(&output, -vy / height);
                self.commit();
            }
            Mode::Overview => self.overview_release(vy, self.height()),
            Mode::Idle | Mode::Undecided => {}
        }
    }
}
