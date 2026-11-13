//! The engine's one state and the turn every change takes through it: the
//! model moves, the scene follows, the windows' signals are written, the
//! socket hears about it.

use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, rc::Rc};

use formalwm_core::{
    color::Rgba,
    model::{Layer, Model, WorkspaceSpec},
    motion::Clocks,
    rules,
    scene::{Scene, Visual},
    settings::Settings,
};
use fs_wm_proto as proto;
use shojiwm_rs::{ipc::IpcServer, prelude::*};

use crate::{drag, gestures, handover, overview, stage, windows};

/// One window the compositor has: its handle and what the rules gave it.
pub struct Win {
    pub handle: Window,
    pub effect: rules::Effect,
    /// The model has it.
    pub placed: bool,
    /// A float without a size rule has taken the size its client chose.
    pub sized: bool,
    pub scope: Scope,
}

/// What a window's frame is drawn with. A change rebuilds every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub active: Color,
    pub inactive: Color,
    pub border: f64,
    pub rounding: f64,
    pub shadow: Option<(BoxShadow, BoxShadow)>,
    /// How long the colours take between focused and not, and on what
    /// curve; no time at all with motion off.
    pub tint: (f64, [f64; 4]),
}

/// The signals the compositions read; they never read the engine itself,
/// which may be in the middle of a turn when they run.
#[derive(Clone, Copy)]
pub struct Signals {
    pub look: Signal<Look>,
    pub stage: Signal<stage::Stage>,
}

pub struct Engine {
    pub dir: PathBuf,
    pub settings: Rc<Settings>,
    /// The workspaces as last given to the model.
    pub specs: Vec<WorkspaceSpec>,
    pub theme: proto::Theme,
    pub model: Model,
    pub scene: Scene,
    /// By the compositor's id, which is the model's.
    pub windows: BTreeMap<String, Win>,
    pub signals: Signals,
    pub ipc: Option<IpcServer>,
    pub driver: Option<PollHandle>,
    pub clocks: Clocks,
    pub state_pending: bool,
    pub last_state: String,
    pub asked_focus: Option<String>,
    pub focus_pending: bool,
    pub bind_ids: Vec<String>,
    pub escapes: BTreeMap<String, Vec<String>>,
    /// The shell asked for no cursor, or it has not arrived yet.
    pub cursor_hidden: bool,
    pub shell_arrived: bool,
    /// The pointer went idle over a window whose rule hides it then.
    pub cursor_idle: bool,
    pub idle_timer: Option<TimerHandle>,
    pub cursor: crate::cursor::Dynamic,
    pub gesture: gestures::Gesture,
    pub overview: overview::Overview,
    pub handover: handover::HandOver,
    pub drag: Option<drag::Drag>,
    pub hold: drag::Hold,
}

type Turn = Box<dyn FnOnce(&mut Engine)>;

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    static DEFERRED: RefCell<Vec<Turn>> = const { RefCell::new(Vec::new()) };
    /// What the output and input factories read; they run inside turns.
    static SETTINGS: RefCell<Rc<Settings>> = RefCell::new(Rc::new(Settings::default()));
}

pub fn install(engine: Engine) {
    SETTINGS.with(|cell| *cell.borrow_mut() = engine.settings.clone());
    ENGINE.with(|cell| *cell.borrow_mut() = Some(engine));
}

pub fn settings() -> Rc<Settings> {
    SETTINGS.with(|cell| cell.borrow().clone())
}

pub fn set_settings(settings: Rc<Settings>) {
    SETTINGS.with(|cell| *cell.borrow_mut() = settings);
}

/// What the turn put off, in the order it was asked; each may put off more.
fn run_deferred(engine: &mut Engine) {
    while let Some(next) = DEFERRED.with(|deferred| {
        let mut deferred = deferred.borrow_mut();
        (!deferred.is_empty()).then(|| deferred.remove(0))
    }) {
        next(engine);
    }
}

/// Runs `f` on the engine. Called from inside a turn, it runs when that
/// turn ends.
pub fn enter(f: impl FnOnce(&mut Engine) + 'static) {
    ENGINE.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => {
            let Some(engine) = guard.as_mut() else {
                return;
            };
            f(engine);
            run_deferred(engine);
        }
        Err(_) => DEFERRED.with(|deferred| deferred.borrow_mut().push(Box::new(f))),
    });
}

/// Runs `f` on the engine and answers with what it returns; an error when a
/// turn is already running, which a socket request never meets.
pub fn ask<R>(f: impl FnOnce(&mut Engine) -> Result<R, String>) -> Result<R, String> {
    ENGINE.with(|cell| {
        let mut guard = cell.try_borrow_mut().map_err(|_| "busy".to_owned())?;
        let engine = guard.as_mut().ok_or("not started")?;
        let result = f(engine);
        run_deferred(engine);
        result
    })
}

pub fn color(rgba: Rgba) -> Color {
    let [r, g, b, a] = rgba
        .0
        .map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
    Color { r, g, b, a }
}

pub fn look_of(theme: &proto::Theme) -> Look {
    let chrome = &theme.chrome;
    let resolve = |text: &str| Rgba::resolve(text, &theme.colors).unwrap_or(Rgba::BLACK);
    let shade = |text: &str| BoxShadow {
        spread: 0.0,
        ..shadow(
            chrome.shadow_offset[0] as f64,
            chrome.shadow_offset[1] as f64,
            chrome.shadow_range as f64 * 2.0,
            color(resolve(text)),
        )
    };
    Look {
        active: color(resolve(&chrome.border_color)),
        inactive: color(resolve(&theme.colors.border)),
        border: chrome.border_size.max(0) as f64,
        rounding: chrome.rounding.max(0) as f64,
        shadow: chrome.shadow.then(|| {
            (
                shade(&chrome.shadow_color),
                shade(&chrome.shadow_inactive_color),
            )
        }),
        tint: (
            if chrome.motion.enabled {
                chrome.motion.families.effects_slow.ms
            } else {
                0.0
            },
            chrome.motion.families.effects_slow.curve,
        ),
    }
}

fn z_of(layer: Layer, order: usize) -> i32 {
    let rank = match layer {
        Layer::Tile => 0,
        Layer::Maximized => 1,
        Layer::Zen => 2,
        Layer::Float => 3,
        Layer::Pinned => 4,
        Layer::Fullscreen => 5,
        Layer::Overlay => 6,
    };
    rank * 100_000 + order.min(99_999) as i32
}

impl Engine {
    /// After the model changed: everything that follows from it.
    pub fn commit(&mut self) {
        self.scene.sync(&self.model);
        self.sync_fullscreen();
        self.focus();
        self.present();
        self.kick();
        self.publish();
        self.publish_workspaces();
    }

    pub fn sync_fullscreen(&self) {
        crate::session::set_fullscreen(self.fullscreen());
    }

    /// By output, the content type of its fullscreen window: the one a rule
    /// gives it, else the one its client declares.
    fn fullscreen(&self) -> BTreeMap<String, String> {
        let content = |id: &String| {
            let win = self.windows.get(id)?;
            let declared = || match win.handle.content_type().get_untracked() {
                Some(WindowContentType::Photo) => "photo",
                Some(WindowContentType::Video) => "video",
                Some(WindowContentType::Game) => "game",
                None => "",
            };
            let ruled = win.effect.content.clone();
            Some(ruled.unwrap_or_else(|| declared().to_owned()))
        };
        self.model
            .outputs
            .iter()
            .filter_map(|output| {
                let id = self.model.fullscreen_on(&output.geo.name)?;
                Some((output.geo.name.clone(), content(id)?))
            })
            .collect()
    }

    /// Gives the keyboard to the window the model has focused. Not while
    /// data is dragged: a client that gains the keyboard is sent the
    /// selection, and foot drops the drag's offer for it, so the keyboard
    /// follows once the drag has ended.
    pub fn focus(&mut self) {
        let want = self.model.focused_window().cloned();
        if want == self.asked_focus || self.hold.data_drag {
            return;
        }
        let had = std::mem::replace(&mut self.asked_focus, want.clone());
        match want {
            // On request: the model has decided, and the compositor's
            // order of windows nobody has used yet would refuse an older
            // one over a newer.
            Some(id) => {
                if let Some(win) = self.windows.get(&id)
                    && self.meant_for().as_ref() != Some(&id)
                {
                    win.handle.focus_on_request();
                }
            }
            // Nothing in view to type into. The compositor keeps the keyboard
            // on a window as long as it is mapped, in view or not.
            None => {
                let asked = had.and_then(|id| self.windows.get(&id).map(|win| win.handle));
                for window in asked.into_iter().chain(COMPOSITOR.window.focused()) {
                    window.unfocus();
                }
            }
        }
        self.arm_idle_pointer();
        self.settle_focus();
    }

    pub fn opacity_of(&self, id: &str, full: bool) -> f64 {
        if full {
            return 1.0;
        }
        self.windows
            .get(id)
            .and_then(|win| win.effect.opacity)
            .unwrap_or(self.settings.opacity)
    }

    /// Writes this frame of the scene to the windows.
    pub fn present(&mut self) {
        let frame = self.scene.frame();
        let border = self.signals.look.get_untracked().border;
        batch(|| {
            for visual in &frame {
                let Some(win) = self.windows.get(&visual.id) else {
                    continue;
                };
                if self.drag.as_ref().is_some_and(|drag| drag.id == visual.id) {
                    continue;
                }
                self.write(win, visual, border);
            }
        });
        self.present_drag();
        crate::trace::frame(self, &frame);
        self.restage(&frame);
    }

    fn write(&self, win: &Win, visual: &Visual, border: f64) {
        let window = win.handle;
        // The core's first configure carries the rect's size whatever FORCE
        // says; no size at all is what lets the client pick its own.
        if !win.sized {
            let at = visual.rect;
            return window
                .state(&windows::RECT)
                .set(Rect::new(at.x, at.y, 0.0, 0.0));
        }
        let full = visual.layer == Layer::Fullscreen;
        let grow = if full { 0.0 } else { border };
        let (w, h) = visual.size;
        // Under the overview the scene's box goes through its lens.
        let lens = self
            .lens(&visual.output)
            .filter(|_| visual.layer != Layer::Overlay);
        let drawn = match lens {
            Some((lens, area)) => lens
                .map(visual.rect.translate(-area.x, -area.y), visual.lane)
                .translate(area.x, area.y),
            None => visual.rect,
        };
        let (sx, sy) = (drawn.w / w.max(1.0), drawn.h / h.max(1.0));
        let (mut x, mut y) = (drawn.x - grow * sx, drawn.y - grow * sy);
        if visual.settled && lens.is_none() {
            (x, y) = (x.round(), y.round());
        }
        window.state(&windows::RECT).set(Rect::new(
            x,
            y,
            (w + 2.0 * grow).round(),
            (h + 2.0 * grow).round(),
        ));
        window.state(&windows::XFORM).set(ManagedTransform {
            origin_x: 0.0,
            origin_y: 0.0,
            translate_x: 0.0,
            translate_y: 0.0,
            scale_x: sx,
            scale_y: sy,
        });
        window.state(&windows::FULL).set(full);
        window
            .state(&windows::OPACITY)
            .set(visual.opacity * self.opacity_of(&visual.id, full));
        window
            .state(&windows::Z)
            .set(z_of(visual.layer, visual.order));
        window
            .state(&windows::OUTPUTS)
            .set(Some(vec![visual.output.clone()]));
        // The overview shows the workspaces the output is not on as well.
        let shown = match lens {
            Some((_, area)) => visual.opacity > 0.0 && drawn.intersects(&area),
            None => visual.visible,
        };
        window.state(&windows::IDLE).set(!shown);
        let overview = self.overview.progress.on(&visual.output) > 0.0;
        window
            .state(&windows::LIVE)
            .set(visual.visible && !overview);
    }

    fn restage(&mut self, frame: &[Visual]) {
        let stage = stage::build(self, frame);
        self.signals.stage.set(stage);
    }

    pub fn animating(&self) -> bool {
        self.scene.animating() || !self.overview.resting()
    }

    /// Starts the frame clock when something has a way to go.
    pub fn kick(&mut self) {
        if self.driver.is_some() || !self.animating() {
            return;
        }
        self.clocks.reset();
        self.driver = Some(create_poll_for_each_output(1.0, |handle, output| {
            let now = handle.now_ms();
            let output = output.to_owned();
            enter(move |engine| engine.tick(&output, now));
        }));
    }

    /// `output`'s frame at `now`: what it draws steps on its own clock.
    fn tick(&mut self, output: &str, now: f64) {
        let Some(dt) = self.clocks.advance(output, now) else {
            return;
        };
        let outputs = &self.model.outputs;
        self.overview
            .progress
            .retain(|name| outputs.iter().any(|o| o.geo.name == name));
        self.scene.step_on(output, dt);
        self.overview.step(output, dt, &self.theme.chrome.motion);
        self.present();
        if !self.animating()
            && let Some(driver) = self.driver.take()
        {
            driver.cancel();
        }
    }

    /// The model's id for an id the socket was given.
    pub fn resolve(&self, public: &str) -> Result<String, String> {
        self.windows
            .iter()
            .find(|(id, win)| {
                win.handle.foreign_toplevel_id().get_untracked().as_deref() == Some(public)
                    || id.as_str() == public
            })
            .map(|(id, _)| id.clone())
            .ok_or_else(|| format!("no such window: {public}"))
    }

    /// The id the socket shows for a window: its foreign-toplevel one.
    pub fn public(&self, id: &str) -> String {
        self.windows
            .get(id)
            .and_then(|win| win.handle.foreign_toplevel_id().get_untracked())
            .unwrap_or_else(|| id.to_owned())
    }
}
