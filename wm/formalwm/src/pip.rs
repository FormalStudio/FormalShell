//! Picture in picture under a hand: the pointer drags one and lets it fly to
//! a corner or behind an edge, two fingers on the touchpad or the screen
//! resize it about the point they hold, and a finger on the screen moves it
//! once it has left the slop of a tap. Where one rests, and the maths of all
//! of it, is `formalwm_core::pip`.

use std::collections::BTreeMap;

use formalwm_core::{
    geom::Rect as Box2,
    model::Layer,
    pip::{self, Corner, Limits, Rest, Screen, Side, Tracker},
    rules,
};
use shojiwm_rs::{
    prelude::*,
    ssd::{
        GesturePinchEventSnapshot, GestureSwipePhaseSnapshot, TouchEventSnapshot,
        TouchPhaseSnapshot, WindowMovePhaseSnapshot,
    },
};

use crate::engine::{Engine, enter};

#[derive(Default)]
pub struct Pip {
    /// Where each picture the engine placed rests. One a rule placed has no
    /// entry until a hand moves it.
    rests: BTreeMap<String, Rest>,
    /// The corner the last picture was left in and the width it was pinched
    /// to: the next one opens like it.
    corner: Option<Corner>,
    width: Option<f64>,
    tracker: Tracker,
    pinch: Option<Pinch>,
    touch: Touch,
    /// The fingers on the screen, by their id.
    points: BTreeMap<i32, (f64, f64)>,
}

struct Pinch {
    id: String,
    start: Box2,
    /// The point the fingers took hold of, and how far they carried it.
    focus: (f64, f64),
    pan: (f64, f64),
    limits: Limits,
}

#[derive(Default)]
enum Touch {
    #[default]
    Idle,
    /// Not a picture's, or over with: nothing until every finger is up.
    Spent,
    /// One finger on a picture, inside the slop: its client has the touch.
    Pending {
        id: String,
        finger: i32,
        start: (f64, f64),
    },
    Drag {
        id: String,
        finger: i32,
        /// From the picture's corner to the finger.
        grip: (f64, f64),
    },
    /// Two fingers on a picture; `self.pinch` has the rest.
    Pinch {
        fingers: (i32, i32),
        /// How far apart they came down, and the point between them.
        span: f64,
        middle: (f64, f64),
    },
}

impl Pip {
    /// No finger of a touch is the engine's.
    #[cfg(test)]
    pub fn untouched(&self) -> bool {
        matches!(self.touch, Touch::Idle)
    }
}

pub fn setup() {
    COMPOSITOR.event.on_gesture_pinch_async(|event| {
        let event = event.clone();
        enter(move |engine| engine.pinch(&event));
    });
    COMPOSITOR.event.on_touch_async(|event| {
        let event = event.clone();
        enter(move |engine| engine.touch(&event));
    });
}

fn between(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

fn apart(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn whole(size: (f64, f64)) -> (f64, f64) {
    (size.0.round(), size.1.round())
}

/// What a rule says against a window being a picture: it unpins it, puts
/// it in an overlay or says so outright. One that tiles it is seen where
/// the rules are resolved.
pub fn allowed(effect: &rules::Effect) -> bool {
    effect.pip != Some(false) && effect.pin != Some(false) && effect.overlay.is_none()
}

/// What a window says about being a picture.
pub fn hints<'a>(window: Window, title: &'a str, xwayland: bool) -> pip::Hints<'a> {
    let x11 = window.x11().get_untracked();
    pip::Hints {
        title,
        xwayland,
        above: x11.is_some_and(|state| state.above),
        sticky: x11.is_some_and(|state| state.sticky),
        fixed_aspect: x11.is_some_and(|state| state.fixed_aspect),
    }
}

impl Engine {
    fn pip_screens(&self) -> Vec<Screen> {
        let outputs: Vec<(Box2, Box2)> = self
            .model
            .outputs
            .iter()
            .map(|output| (output.geo.rect, output.geo.usable))
            .collect();
        let options = &self.model.options;
        pip::screens(&outputs, options.gaps_out + options.border)
    }

    /// The screen of the output a picture is on.
    fn pip_screen(&self, id: &str) -> Option<Screen> {
        let rect = self
            .model
            .output(&self.model.output_of(id)?)
            .map(|output| output.geo.rect)?;
        self.pip_screens()
            .into_iter()
            .find(|screen| screen.rect == rect)
    }

    /// A picture's rect in the model, global.
    fn pip_box(&self, id: &str) -> Option<Box2> {
        let window = self.model.window(id).filter(|window| window.pip)?;
        let origin = self.model.output(&self.model.output_of(id)?)?.geo.rect;
        Some(window.float_rect.translate(origin.x, origin.y))
    }

    /// Where a picture is drawn this frame: on its way somewhere, not where
    /// the model has it.
    fn pip_drawn(&self, id: &str) -> Option<Box2> {
        let rect = self.pip_box(id)?;
        Some(self.scene.visual(id).map_or(rect, |visual| visual.rect))
    }

    /// The picture drawn under a point, the topmost one.
    fn pip_at(&self, at: (f64, f64)) -> Option<String> {
        self.scene
            .frame()
            .iter()
            .rev()
            .find(|visual| visual.layer == Layer::Pip && visual.rect.contains(at.0, at.1))
            .map(|visual| visual.id.clone())
    }

    fn pip_stashed(&self, id: &str) -> bool {
        matches!(self.pip.rests.get(id), Some(Rest::Stash { .. }))
    }

    /// A picture belongs to the output it is over.
    fn pip_home(&mut self, id: &str, at: (f64, f64)) {
        let here = self.output_at(at);
        if here.is_none() || here == self.model.output_of(id) {
            return;
        }
        let workspace = here
            .and_then(|output| self.model.active_workspace(&output))
            .map(|ws| ws.id.clone());
        if let Some(workspace) = workspace {
            self.model.move_to_workspace(id, &workspace, false);
        }
    }

    /// The picture is at `rect`, under a hand at `at`.
    fn pip_follow(&mut self, id: &str, rect: Box2, at: (f64, f64), now: f64) {
        self.pip_home(id, at);
        self.model.place(id, rect);
        self.scene.sync(&self.model);
        self.scene.land(id);
        self.pip.tracker.push(at, now);
        self.present();
    }

    /// Sends a picture from where it is drawn to `rest` on `screen`, at the
    /// speed the hand had.
    fn pip_rest(&mut self, id: &str, screen: &Screen, rest: Rest, velocity: (f64, f64)) {
        let (Some(rect), Some(from)) = (self.pip_box(id), self.pip_drawn(id)) else {
            return;
        };
        self.pip_home(id, screen.rect.center());
        let limits = screen.limits(rect.w / rect.h.max(1.0));
        let size = whole(limits.size(limits.clamp(rect.w)));
        self.model.place(id, screen.rest(rest, size));
        match rest {
            // Out of the way: the keyboard goes back to what is in view.
            Rest::Stash { .. } => self.model.yield_focus(id),
            Rest::Corner(corner) => self.pip.corner = Some(corner),
        }
        self.pip.rests.insert(id.to_owned(), rest);
        self.scene.sync(&self.model);
        self.scene.carry(id, from);
        self.scene.fling(id, velocity);
        self.commit();
    }

    /// A hand let the picture go at `velocity`, pixels a second.
    fn pip_settle(&mut self, id: &str, velocity: (f64, f64)) {
        let Some(from) = self.pip_drawn(id) else {
            return;
        };
        let screens = self.pip_screens();
        match pip::land(&screens, from, velocity) {
            Some((index, rest)) => self.pip_rest(id, &screens[index], rest, velocity),
            None => self.commit(),
        }
    }

    /// The pointer has a picture: the compositor's move, started by its
    /// client or with Super held.
    pub fn pip_move(
        &mut self,
        id: &str,
        rect: Box2,
        pointer: (f64, f64),
        phase: WindowMovePhaseSnapshot,
        now: f64,
    ) {
        match phase {
            WindowMovePhaseSnapshot::Start => {
                self.pip.tracker = Tracker::default();
                self.pip_follow(id, rect, pointer, now);
            }
            WindowMovePhaseSnapshot::Update => self.pip_follow(id, rect, pointer, now),
            WindowMovePhaseSnapshot::End => {
                self.pip_follow(id, rect, pointer, now);
                self.pip_settle(id, self.pip.tracker.velocity(now));
            }
            WindowMovePhaseSnapshot::Cancel => self.pip_settle(id, (0.0, 0.0)),
        }
    }

    /// A press on what a stashed picture leaves on screen brings it back.
    pub fn pip_pressed(&mut self, at: (f64, f64)) {
        if let Some(id) = self.pip_at(at) {
            self.pip_unstash(&id);
        }
    }

    fn pip_unstash(&mut self, id: &str) {
        let Some(Rest::Stash { side, down }) = self.pip.rests.get(id).copied() else {
            return;
        };
        if let Some(screen) = self.pip_screen(id) {
            let corner = Rest::Corner(pip::unstash(side, down));
            self.pip_rest(id, &screen, corner, (0.0, 0.0));
        }
    }

    /// A new picture no rule gave a place: the size it asked for where that
    /// fits, in the corner the last one was left in.
    pub fn pip_open(&mut self, id: &str, natural: (f64, f64)) -> Option<Box2> {
        let screen = self.pip_screen(id)?;
        let limits = screen.limits(natural.0 / natural.1.max(1.0));
        let size = match self.pip.width {
            Some(width) => limits.size(limits.clamp(width)),
            None => pip::fit(natural, limits),
        };
        let rest = Rest::Corner(self.pip.corner.unwrap_or(Corner::BottomRight));
        self.pip.rests.insert(id.to_owned(), rest);
        Some(screen.rest(rest, whole(size)))
    }

    /// A window that had no title when it opened turns out to be a picture.
    pub fn pip_promote(&mut self, window: Window) {
        let id = window.id();
        if !self.windows.get(&id).is_some_and(|win| allowed(&win.effect)) {
            return;
        }
        self.model.set_floating(&id, Some(true));
        self.model.set_pinned(&id, true);
        if !self.model.set_pip(&id, true) {
            return;
        }
        let natural = window.position().get_untracked();
        if let Some(rect) = self.pip_open(&id, (natural.width.max(1.0), natural.height.max(1.0))) {
            self.model.place(&id, rect);
        }
        self.commit();
    }

    /// The outputs or what their layers leave free changed: every picture
    /// the engine placed goes back to its rest on the output it is on now.
    pub fn pip_reanchor(&mut self) {
        let rests: Vec<(String, Rest)> = self
            .pip
            .rests
            .iter()
            .map(|(id, rest)| (id.clone(), *rest))
            .collect();
        for (id, rest) in rests {
            let (Some(rect), Some(screen)) = (self.pip_box(&id), self.pip_screen(&id)) else {
                self.pip.rests.remove(&id);
                continue;
            };
            let rest = match rest {
                Rest::Stash { side, down }
                    if !match side {
                        Side::Left => screen.stash_left,
                        Side::Right => screen.stash_right,
                    } =>
                {
                    Rest::Corner(pip::unstash(side, down))
                }
                rest => rest,
            };
            let limits = screen.limits(rect.w / rect.h.max(1.0));
            let size = whole(limits.size(limits.clamp(rect.w)));
            if screen.rest(rest, size) != rect || self.pip.rests.get(&id) != Some(&rest) {
                self.pip_rest(&id, &screen, rest, (0.0, 0.0));
            }
        }
    }

    pub fn pip_gone(&mut self, id: &str) {
        self.pip.rests.remove(id);
        if self.pip.pinch.as_ref().is_some_and(|pinch| pinch.id == id) {
            self.pip.pinch = None;
        }
        let held = match &self.pip.touch {
            Touch::Pending { id: held, .. } | Touch::Drag { id: held, .. } => held == id,
            Touch::Pinch { .. } => self.pip.pinch.is_none(),
            Touch::Idle | Touch::Spent => false,
        };
        if held {
            self.pip.touch = Touch::Spent;
        }
    }

    /// Two fingers on the touchpad with the pointer over a picture.
    fn pinch(&mut self, event: &GesturePinchEventSnapshot) {
        let now = event.timestamp as f64;
        match event.phase {
            GestureSwipePhaseSnapshot::Begin => {
                let at = event.position.as_ref().map(|at| (at.x, at.y));
                if let Some(at) = at
                    && let Some(id) = self.pip_at(at)
                {
                    self.pinch_begin(id, at);
                }
            }
            GestureSwipePhaseSnapshot::Update => {
                if let Some(pinch) = self.pip.pinch.as_mut() {
                    pinch.pan.0 += event.delta_x;
                    pinch.pan.1 += event.delta_y;
                    self.pinch_to(event.scale, now);
                }
            }
            GestureSwipePhaseSnapshot::End => self.pinch_end(now, false),
            GestureSwipePhaseSnapshot::Cancel => self.pinch_end(now, true),
        }
    }

    fn pinch_begin(&mut self, id: String, focus: (f64, f64)) {
        if self.overview.open || self.pip_stashed(&id) {
            return;
        }
        let (Some(rect), Some(drawn), Some(screen)) =
            (self.pip_box(&id), self.pip_drawn(&id), self.pip_screen(&id))
        else {
            return;
        };
        // From where it is drawn: a pinch can catch it on its way.
        let start = Box2::new(drawn.x, drawn.y, rect.w, rect.h);
        self.pip.tracker = Tracker::default();
        self.pip.pinch = Some(Pinch {
            limits: screen.limits(rect.w / rect.h.max(1.0)),
            id,
            start,
            focus,
            pan: (0.0, 0.0),
        });
    }

    fn pinch_to(&mut self, scale: f64, now: f64) {
        let Some(pinch) = &self.pip.pinch else {
            return;
        };
        let id = pinch.id.clone();
        let shape = pip::pinch(pinch.start, scale, pinch.focus, pinch.pan, pinch.limits);
        let at = (pinch.focus.0 + pinch.pan.0, pinch.focus.1 + pinch.pan.1);
        self.pip.tracker.push(at, now);
        let (w, h) = whole((shape.rect.w, shape.rect.h));
        self.model
            .place(&id, Box2::new(shape.rect.x, shape.rect.y, w, h));
        self.scene.sync(&self.model);
        // Past a limit the client keeps the limit's size and the picture is
        // drawn stretched.
        self.scene.hold(&id, shape.drawn);
        self.present();
    }

    fn pinch_end(&mut self, now: f64, cancelled: bool) {
        let Some(pinch) = self.pip.pinch.take() else {
            return;
        };
        let from = self.pip_drawn(&pinch.id);
        if cancelled {
            self.model.place(&pinch.id, pinch.start);
            self.scene.sync(&self.model);
            if let Some(from) = from {
                self.scene.hold(&pinch.id, from);
            }
        } else {
            self.pip.width = self.pip_box(&pinch.id).map(|rect| rect.w);
        }
        self.scene.release(&pinch.id);
        let velocity = if cancelled {
            (0.0, 0.0)
        } else {
            self.pip.tracker.velocity(now)
        };
        self.pip_settle(&pinch.id, velocity);
    }

    /// A finger on the touchscreen, after its client has heard of it.
    fn touch(&mut self, event: &TouchEventSnapshot) {
        let now = event.timestamp as f64;
        let at = event.position.as_ref().map(|at| (at.x, at.y));
        match (event.phase, at) {
            (TouchPhaseSnapshot::Down, Some(at)) => self.touch_down(event.id, at),
            (TouchPhaseSnapshot::Motion, Some(at)) => self.touch_motion(event.id, at, now),
            (TouchPhaseSnapshot::Up, _) => self.touch_up(event.id, now, false),
            (TouchPhaseSnapshot::Cancel, _) => self.touch_up(event.id, now, true),
            _ => {}
        }
        if event.fingers == 0 {
            self.pip.touch = Touch::Idle;
            self.pip.points.clear();
        }
    }

    fn touch_down(&mut self, finger: i32, at: (f64, f64)) {
        self.pip.points.insert(finger, at);
        let on = self.pip_at(at);
        self.pip.touch = match std::mem::take(&mut self.pip.touch) {
            Touch::Idle => match on {
                Some(id) if self.overview.open => {
                    let _ = id;
                    Touch::Spent
                }
                Some(id) if self.pip_stashed(&id) => {
                    COMPOSITOR.input.take_touch();
                    self.pip_unstash(&id);
                    Touch::Spent
                }
                Some(id) => Touch::Pending {
                    id,
                    finger,
                    start: at,
                },
                None => Touch::Spent,
            },
            // A second finger on the picture the first one has.
            Touch::Pending {
                id, finger: first, ..
            }
            | Touch::Drag {
                id, finger: first, ..
            } if on.as_deref() == Some(id.as_str()) => {
                let other = self.pip.points.get(&first).copied().unwrap_or(at);
                COMPOSITOR.input.take_touch();
                self.pinch_begin(id, between(other, at));
                if self.pip.pinch.is_some() {
                    Touch::Pinch {
                        fingers: (first, finger),
                        span: apart(other, at).max(1.0),
                        middle: between(other, at),
                    }
                } else {
                    Touch::Spent
                }
            }
            other => other,
        };
    }

    fn touch_motion(&mut self, finger: i32, at: (f64, f64), now: f64) {
        self.pip.points.insert(finger, at);
        match &self.pip.touch {
            Touch::Pending {
                id,
                finger: first,
                start,
            } if *first == finger && apart(at, *start) > pip::SLOP => {
                let (id, start) = (id.clone(), *start);
                let Some(drawn) = self.pip_drawn(&id) else {
                    return;
                };
                // A drag from here on: the client hears no more of the touch.
                COMPOSITOR.input.take_touch();
                self.pip.tracker = Tracker::default();
                self.pip.touch = Touch::Drag {
                    id,
                    finger,
                    grip: (start.0 - drawn.x, start.1 - drawn.y),
                };
                self.touch_motion(finger, at, now);
            }
            Touch::Drag {
                id,
                finger: first,
                grip,
            } if *first == finger => {
                let (id, grip) = (id.clone(), *grip);
                if let Some(rect) = self.pip_box(&id) {
                    let rect = Box2::new(at.0 - grip.0, at.1 - grip.1, rect.w, rect.h);
                    self.pip_follow(&id, rect, at, now);
                }
            }
            Touch::Pinch {
                fingers,
                span,
                middle,
            } => {
                let (Some(a), Some(b)) = (
                    self.pip.points.get(&fingers.0),
                    self.pip.points.get(&fingers.1),
                ) else {
                    return;
                };
                let (scale, here) = (apart(*a, *b) / span, between(*a, *b));
                let pan = (here.0 - middle.0, here.1 - middle.1);
                if let Some(pinch) = self.pip.pinch.as_mut() {
                    pinch.pan = pan;
                }
                self.pinch_to(scale, now);
            }
            _ => {}
        }
    }

    fn touch_up(&mut self, finger: i32, now: f64, cancelled: bool) {
        self.pip.points.remove(&finger);
        self.pip.touch = match std::mem::take(&mut self.pip.touch) {
            Touch::Drag {
                id, finger: first, ..
            } if first == finger => {
                let velocity = if cancelled {
                    (0.0, 0.0)
                } else {
                    self.pip.tracker.velocity(now)
                };
                self.pip_settle(&id, velocity);
                Touch::Spent
            }
            Touch::Pinch { fingers, .. } if fingers.0 == finger || fingers.1 == finger => {
                self.pinch_end(now, cancelled);
                Touch::Spent
            }
            Touch::Pending { finger: first, .. } if first == finger => Touch::Spent,
            other => other,
        };
    }
}
