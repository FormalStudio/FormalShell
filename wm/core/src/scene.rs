//! What is on screen between two states of the model. The model says where
//! every window rests; the scene carries each one there on the theme's
//! clocks. A tape's scroll and an output's place in its stack are springs
//! of their own, so a gesture can hold them and let go with a velocity. A
//! window that changes place keeps the spot it is drawn at and rides a
//! spring from there, whatever moved it.

use std::collections::BTreeMap;

use fs_wm_proto::{Families, Motion};

use crate::{
    geom::Rect,
    model::{Layer, Model, Placed, WsId},
    motion::{Spring, SpringParams, Tween},
    tape::WinId,
};

/// Hyprland's `popin`: an unstyled `windows` leaf opens a window from a
/// point five pixels across at its centre.
const OPEN_FROM: f64 = 0.0;
const OPEN_SIDE: f64 = 5.0;

/// A pixel in the units of a stack position or a scale, near enough: both
/// are fractions of something about a thousand pixels across.
const UNIT: f64 = 0.001;

#[derive(Clone, Debug)]
struct Actor {
    place: Placed,
    /// The workspace's place in its output's stack, and the output's rect.
    stack_index: f64,
    output_rect: Rect,
    /// Distance from where the model has it to where it is drawn.
    dx: Spring,
    dy: Spring,
    dw: Spring,
    dh: Spring,
    pop: Spring,
    presence: Tween,
}

/// One window as drawn this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Visual {
    pub id: WinId,
    pub output: String,
    pub workspace: Option<WsId>,
    /// Global logical: the box it is drawn in.
    pub rect: Rect,
    /// The size its client has; `rect` differs from it while it resizes or
    /// pops.
    pub size: (f64, f64),
    pub opacity: f64,
    /// It reaches into its output this frame.
    pub visible: bool,
    pub layer: Layer,
    pub order: usize,
    /// How many workspaces below the one its output shows its own sits, as
    /// drawn: 0.5 halfway through a switch. 0 for what rides no stack.
    pub lane: f64,
    /// At its place with nothing left to play.
    pub settled: bool,
}

#[derive(Clone, Debug)]
pub struct Scene {
    actors: BTreeMap<WinId, Actor>,
    scroll: BTreeMap<WsId, Spring>,
    /// The output each workspace is on, whose frames step its scroll.
    homes: BTreeMap<WsId, String>,
    stack: BTreeMap<String, Spring>,
    families: Families,
    enabled: bool,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new(&Motion::default())
    }
}

impl Scene {
    pub fn new(motion: &Motion) -> Self {
        Self {
            actors: BTreeMap::new(),
            scroll: BTreeMap::new(),
            homes: BTreeMap::new(),
            stack: BTreeMap::new(),
            families: motion.families,
            enabled: motion.enabled,
        }
    }

    pub fn set_motion(&mut self, motion: &Motion) {
        self.families = motion.families;
        self.enabled = motion.enabled;
        if !self.enabled {
            self.finish();
        }
    }

    fn spatial(&self) -> SpringParams {
        SpringParams::from_family(&self.families.spatial)
    }

    pub fn scroll_of(&self, workspace: &str) -> f64 {
        self.scroll
            .get(workspace)
            .map_or(0.0, |spring| spring.value)
    }

    /// An output's place in its stack, in workspaces: 1.5 is halfway from
    /// its second to its third.
    pub fn stack_of(&self, output: &str) -> f64 {
        self.stack.get(output).map_or(0.0, |spring| spring.value)
    }

    fn composed(&self, actor: &Actor) -> Rect {
        let mut rect = actor.place.rect;
        if actor.place.scrolls
            && let Some(workspace) = &actor.place.workspace
        {
            rect.x -= self.scroll_of(workspace);
        }
        match actor.place.layer {
            Layer::Pinned => {}
            Layer::Overlay => {
                if !actor.place.shown {
                    rect.y -= actor.output_rect.h;
                }
            }
            _ => {
                rect.y +=
                    (actor.stack_index - self.stack_of(&actor.place.output)) * actor.output_rect.h;
            }
        }
        rect
    }

    fn drawn(&self, actor: &Actor) -> Rect {
        let rect = self.composed(actor);
        Rect::new(
            rect.x + actor.dx.value,
            rect.y + actor.dy.value,
            (rect.w + actor.dw.value).max(1.0),
            (rect.h + actor.dh.value).max(1.0),
        )
    }

    /// Takes the model's new state. Everything already on screen keeps the
    /// spot it is drawn at and heads for its new place from there.
    pub fn sync(&mut self, model: &Model) {
        for ws in &model.workspaces {
            let target = ws.tape.scroll;
            self.scroll
                .entry(ws.id.clone())
                .or_insert_with(|| Spring::at(target))
                .target = target;
        }
        self.scroll
            .retain(|id, _| model.workspaces.iter().any(|ws| &ws.id == id));
        self.homes = model
            .workspaces
            .iter()
            .map(|ws| (ws.id.clone(), ws.output.clone()))
            .collect();
        for output in &model.outputs {
            let target = model.stack_index(&output.active).unwrap_or(0) as f64;
            self.stack
                .entry(output.geo.name.clone())
                .or_insert_with(|| Spring::at(target))
                .target = target;
        }
        self.stack
            .retain(|name, _| model.outputs.iter().any(|o| &o.geo.name == name));

        let layout = model.layout();
        for place in &layout {
            let stack_index = place
                .workspace
                .as_ref()
                .and_then(|ws| model.stack_index(ws))
                .unwrap_or(0) as f64;
            let output_rect = model
                .output(&place.output)
                .map(|o| o.geo.rect)
                .unwrap_or_default();
            match self.actors.get(&place.id) {
                Some(actor) => {
                    let before = self.drawn(actor);
                    let mut next = actor.clone();
                    next.place = place.clone();
                    next.stack_index = stack_index;
                    next.output_rect = output_rect;
                    let after = self.composed(&next);
                    for (spring, offset) in [
                        (&mut next.dx, before.x - after.x),
                        (&mut next.dy, before.y - after.y),
                        (&mut next.dw, before.w - after.w),
                        (&mut next.dh, before.h - after.h),
                    ] {
                        spring.value = offset;
                        spring.target = 0.0;
                        if offset == 0.0 && spring.velocity == 0.0 {
                            spring.jump(0.0);
                        }
                    }
                    self.actors.insert(place.id.clone(), next);
                }
                None => {
                    let mut pop = Spring::at(OPEN_FROM);
                    pop.target = 1.0;
                    let mut presence = Tween::at(0.0);
                    presence.retarget(1.0, &self.families.effects);
                    self.actors.insert(
                        place.id.clone(),
                        Actor {
                            place: place.clone(),
                            stack_index,
                            output_rect,
                            dx: Spring::at(0.0),
                            dy: Spring::at(0.0),
                            dw: Spring::at(0.0),
                            dh: Spring::at(0.0),
                            pop,
                            presence,
                        },
                    );
                }
            }
        }
        self.actors
            .retain(|id, _| layout.iter().any(|place| &place.id == id));
        if !self.enabled {
            self.finish();
        }
    }

    /// How long the compositor keeps a closed window's last picture, in
    /// milliseconds: Hyprland drops it when `fadeOut` has run.
    pub fn close_ms(&self) -> f64 {
        if self.enabled {
            self.families.effects_fast.ms
        } else {
            0.0
        }
    }

    /// Lands everything where it is going.
    pub fn finish(&mut self) {
        for spring in self.scroll.values_mut().chain(self.stack.values_mut()) {
            spring.jump(spring.target);
        }
        for actor in self.actors.values_mut() {
            for spring in [&mut actor.dx, &mut actor.dy, &mut actor.dw, &mut actor.dh] {
                spring.jump(0.0);
            }
            actor.pop.jump(1.0);
            actor.presence.jump(1.0);
        }
    }

    /// Plays a window's way in from its start: its client has drawn it,
    /// which is the first there is to show.
    pub fn arrive(&mut self, id: &str) {
        if !self.enabled {
            return;
        }
        let effects = self.families.effects;
        if let Some(actor) = self.actors.get_mut(id) {
            actor.pop = Spring::at(OPEN_FROM);
            actor.pop.target = 1.0;
            actor.presence = Tween::at(0.0);
            actor.presence.retarget(1.0, &effects);
        }
    }

    /// Puts a window where it is going without playing the way there, for a
    /// window that is under the pointer's hand.
    pub fn land(&mut self, id: &str) {
        if let Some(actor) = self.actors.get_mut(id) {
            for spring in [&mut actor.dx, &mut actor.dy, &mut actor.dw, &mut actor.dh] {
                spring.jump(0.0);
            }
        }
    }

    /// Starts a window's way to its place from `from`, where a hand left
    /// it.
    pub fn carry(&mut self, id: &str, from: Rect) {
        let Some(rest) = self.actors.get(id).map(|actor| self.composed(actor)) else {
            return;
        };
        if !self.enabled {
            return;
        }
        if let Some(actor) = self.actors.get_mut(id) {
            for (spring, offset) in [
                (&mut actor.dx, from.x - rest.x),
                (&mut actor.dy, from.y - rest.y),
                (&mut actor.dw, from.w - rest.w),
                (&mut actor.dh, from.h - rest.h),
            ] {
                spring.value = offset;
                spring.target = 0.0;
            }
            actor.pop.jump(1.0);
        }
    }

    /// Holds a tape at the model's scroll, under the fingers.
    pub fn hold_scroll(&mut self, workspace: &str) {
        if let Some(spring) = self.scroll.get_mut(workspace) {
            spring.jump(spring.target);
        }
    }

    /// Lets a held tape go at `velocity` px/s toward the model's scroll.
    pub fn release_scroll(&mut self, workspace: &str, velocity: f64) {
        if let Some(spring) = self.scroll.get_mut(workspace)
            && self.enabled
        {
            spring.velocity = velocity;
        }
    }

    /// Holds an output at `position` of its stack, under the fingers.
    pub fn hold_stack(&mut self, output: &str, position: f64) {
        if let Some(spring) = self.stack.get_mut(output) {
            spring.jump(position);
        }
    }

    /// Lets a held stack go at `velocity` workspaces per second.
    pub fn release_stack(&mut self, output: &str, velocity: f64) {
        if let Some(spring) = self.stack.get_mut(output) {
            if self.enabled {
                spring.velocity = velocity;
            } else {
                spring.jump(spring.target);
            }
        }
    }

    pub fn step(&mut self, dt: f64) {
        let spatial = self.spatial();
        for spring in self.scroll.values_mut() {
            spring.step(spatial, dt);
        }
        for spring in self.stack.values_mut() {
            spring.step_in(spatial, dt, UNIT);
        }
        for actor in self.actors.values_mut() {
            Self::step_actor(actor, spatial, dt);
        }
    }

    /// `step` for what `output` draws: its stack, the scroll of its
    /// workspaces and the windows placed on it. Each output steps on its own
    /// frames, so what it shows follows the curve at its own frame times.
    pub fn step_on(&mut self, output: &str, dt: f64) {
        let spatial = self.spatial();
        for (ws, spring) in &mut self.scroll {
            if self.homes.get(ws).is_some_and(|home| home == output) {
                spring.step(spatial, dt);
            }
        }
        if let Some(spring) = self.stack.get_mut(output) {
            spring.step_in(spatial, dt, UNIT);
        }
        for actor in self.actors.values_mut() {
            if actor.place.output == output {
                Self::step_actor(actor, spatial, dt);
            }
        }
    }

    fn step_actor(actor: &mut Actor, spatial: SpringParams, dt: f64) {
        for spring in [&mut actor.dx, &mut actor.dy, &mut actor.dw, &mut actor.dh] {
            spring.step(spatial, dt);
        }
        actor.pop.step_in(spatial, dt, UNIT);
        actor.presence.step(dt);
    }

    fn settled(&self, actor: &Actor) -> bool {
        [actor.dx, actor.dy, actor.dw, actor.dh, actor.pop]
            .iter()
            .all(Spring::resting)
            && actor.presence.resting(&self.families.effects)
    }

    /// Whether another frame would differ from this one.
    pub fn animating(&self) -> bool {
        self.scroll
            .values()
            .chain(self.stack.values())
            .any(|spring| !spring.resting())
            || self.actors.values().any(|actor| !self.settled(actor))
    }

    pub fn visual(&self, id: &str) -> Option<Visual> {
        self.actors.get(id).map(|actor| self.visual_of(actor))
    }

    fn visual_of(&self, actor: &Actor) -> Visual {
        let composed = self.composed(actor);
        let drawn = self.drawn(actor);
        let (cx, cy) = drawn.center();
        let pop = actor.pop.value;
        let side = |full: f64| (full * pop).max(OPEN_SIDE.min(full));
        let (w, h) = (side(drawn.w), side(drawn.h));
        let rect = Rect::new(cx - w / 2.0, cy - h / 2.0, w, h);
        let opacity = actor.presence.value(&self.families.effects).clamp(0.0, 1.0);
        let springs_rest = self
            .scroll
            .values()
            .chain(self.stack.values())
            .all(Spring::resting);
        Visual {
            id: actor.place.id.clone(),
            output: actor.place.output.clone(),
            workspace: actor.place.workspace.clone(),
            rect,
            size: (composed.w, composed.h),
            opacity,
            visible: opacity > 0.0 && rect.intersects(&actor.output_rect),
            layer: actor.place.layer,
            order: actor.place.order,
            lane: match actor.place.layer {
                Layer::Pinned | Layer::Overlay => 0.0,
                _ => actor.stack_index - self.stack_of(&actor.place.output),
            },
            settled: springs_rest && self.settled(actor),
        }
    }

    /// Every window as drawn now, bottom to top.
    pub fn frame(&self) -> Vec<Visual> {
        let mut visuals: Vec<Visual> = self
            .actors
            .values()
            .map(|actor| self.visual_of(actor))
            .collect();
        visuals.sort_by_key(|visual| (visual.layer, visual.order));
        visuals
    }
}
