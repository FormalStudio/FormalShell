//! A window's frame and its way into and out of the model.

use std::{cell::Cell, rc::Rc};

use formalwm_core::{
    geom::Rect as Box2,
    model::{Big, NewWindow, Placement},
    rules,
};
use shojiwm_rs::{
    prelude::*,
    ssd::{
        WindowActivateRequestEventSnapshot, WindowDecorationModeSnapshot,
        WindowFullscreenRequestEventSnapshot, WindowMaximizeRequestEventSnapshot,
    },
};

use crate::engine::{Engine, Signals, Win, enter};

pub static RECT: WindowStateKey<Rect> =
    WindowStateKey::new("rect", |_| Rect::new(0.0, 0.0, 960.0, 600.0));
pub static XFORM: WindowStateKey<ManagedTransform> = WindowStateKey::new("xform", |_| REST);
pub static OPACITY: WindowStateKey<f64> = WindowStateKey::new("opacity", |_| 0.0);
pub static Z: WindowStateKey<i32> = WindowStateKey::new("z", |_| 0);
pub static OUTPUTS: WindowStateKey<Option<Vec<String>>> = WindowStateKey::new("outputs", |_| None);
/// Not drawn: off its output, or not placed yet.
pub static IDLE: WindowStateKey<bool> = WindowStateKey::new("idle", |_| true);
/// Takes input: on screen and not under the overview.
pub static LIVE: WindowStateKey<bool> = WindowStateKey::new("live", |_| false);
pub static FULL: WindowStateKey<bool> = WindowStateKey::new("full", |_| false);
/// The client is told the rect's size; off for a float until it has shown
/// the size it wants.
pub static FORCE: WindowStateKey<bool> = WindowStateKey::new("force", |_| true);

/// The desk behind this window is blurred.
pub static BLUR: WindowStateKey<bool> = WindowStateKey::new("blur", |_| false);
/// Its rule keeps the pointer inside it while it has the keyboard.
pub static CONFINE: WindowStateKey<bool> = WindowStateKey::new("confine", |_| false);

const REST: ManagedTransform = ManagedTransform {
    origin_x: 0.0,
    origin_y: 0.0,
    translate_x: 0.0,
    translate_y: 0.0,
    scale_x: 1.0,
    scale_y: 1.0,
};

/// Hyprland's `popin`: a closing window shrinks to this.
const POP_OUT: f64 = 0.9;

/// How long a held window has to draw itself at its float's size before it
/// is shown at whatever size it has.
const HOLD_MS: f64 = 300.0;

fn blend(from: Color, to: Color, t: f64) -> Color {
    let channel = |from: u8, to: u8| {
        (from as f64 + (to as f64 - from as f64) * t.clamp(0.0, 1.0)).round() as u8
    };
    Color {
        r: channel(from.r, to.r),
        g: channel(from.g, to.g),
        b: channel(from.b, to.b),
        a: channel(from.a, to.a),
    }
}

pub fn setup(signals: Signals) {
    COMPOSITOR
        .window
        .decoration(|_, _| WindowDecorationModeSnapshot::Server);
    COMPOSITOR
        .window
        .kde_default_decoration(|_| WindowDecorationModeSnapshot::Server);

    COMPOSITOR.window.composition(move |window| {
        // Read here on purpose: each one changes what the frame is made of.
        let look = signals.look.get();
        let full = window.state(&FULL).get();
        let blurred = window.state(&BLUR).get();

        let managed = ManagedWindow::new()
            .rect(window.state(&RECT))
            .visible_outputs(window.state(&OUTPUTS))
            .opacity(window.state(&OPACITY))
            .transform(window.state(&XFORM))
            .z_index(window.state(&Z))
            .force_rect_size(window.state(&FORCE))
            .tiled(true)
            .idle(window.state(&IDLE))
            .interactive(window.state(&LIVE))
            .confine_pointer(window.state(&CONFINE));
        // No frame at all, so the client's buffer can go straight to the
        // plane.
        if full {
            return managed.allow_tearing(true).child(ClientWindow::new());
        }

        // Hyprland's `border` leaf: the colours cross over, they do not flip.
        let focused = window.is_focused();
        let lit = |focused: bool| if focused { 1.0 } else { 0.0 };
        let mix = Animation::new(lit(focused.get_untracked()));
        let (ms, [x1, y1, x2, y2]) = look.tint;
        effect(move || {
            let to = lit(focused.get());
            if ms < 1.0 {
                mix.set(to);
            } else if mix.get_untracked() != to {
                mix.start(AnimationOptions::to(to, ms).easing(cubic_bezier(x1, y1, x2, y2)));
            }
        });
        let mut style = Style::new()
            .border(
                look.border,
                derive(move || blend(look.inactive, look.active, mix.get())),
            )
            .border_radius(look.rounding)
            .padding(0.0);
        if let Some((active, inactive)) = look.shadow {
            style = style.box_shadow(derive(move || {
                vec![BoxShadow {
                    color: blend(inactive.color, active.color, mix.get()),
                    ..if mix.get() > 0.5 { active } else { inactive }
                }]
            }));
        }
        // A node of the frame and not the core's `behind` slot: under a
        // window drawn below full opacity that slot shows the window again.
        let client = if blurred {
            ShaderEffect::new(crate::layers::blur()).child(ClientWindow::new())
        } else {
            ClientWindow::new()
        };
        managed.child(
            WindowBorder::new()
                .style(style)
                .resize_hit_area(8, 14)
                .child(client),
        )
    });

    COMPOSITOR.event.on_open(|window| {
        let scope = untrack(Scope::root).child();
        enter(move |engine| engine.adopt(window, scope));
        // After the turn: the effect's first run enters the engine.
        scope.effect(move || {
            let title = window.title().get();
            let app_id = window.app_id().get().unwrap_or_default();
            let _ = window.foreign_toplevel_id().get();
            let _ = window.tag().get();
            enter(move |engine| engine.renamed(window, &title, &app_id));
        });
        scope.effect(move || {
            let _ = window.size_constraints().get();
            enter(move |engine| engine.bounded(window));
        });
        // `vrr` 3 follows what a fullscreen client says it shows.
        scope.effect(move || {
            let _ = window.content_type().get();
            enter(|engine| engine.sync_fullscreen());
        });
    });
    COMPOSITOR
        .event
        .on_initial_configure(|window| enter(move |engine| engine.place(window)));
    COMPOSITOR
        .event
        .on_first_commit(|window| enter(move |engine| engine.first_commit(window)));
    COMPOSITOR
        .event
        .on_start_close(|window| enter(move |engine| engine.closing(window)));
    COMPOSITOR
        .event
        .on_close(|window| enter(move |engine| engine.closed(window)));
    COMPOSITOR.event.on_focus(|window, focused| {
        if focused {
            enter(move |engine| engine.focused(window));
        }
    });
    COMPOSITOR.event.on_window_fullscreen_request(
        |window, event: &WindowFullscreenRequestEventSnapshot| {
            let on = event.fullscreen;
            enter(move |engine| engine.set_big(window, Big::Fullscreen, on));
        },
    );
    COMPOSITOR.event.on_window_maximize_request(
        |window, event: &WindowMaximizeRequestEventSnapshot| {
            let on = event.maximized;
            enter(move |engine| engine.set_big(window, Big::Maximized, on));
        },
    );
    COMPOSITOR.event.on_window_minimize_request(|_, _| {});
    COMPOSITOR.event.on_window_activate_request(
        |window, _: &WindowActivateRequestEventSnapshot| {
            enter(move |engine| engine.activated(window));
        },
    );
}

impl Engine {
    fn adopt(&mut self, window: Window, scope: Scope) {
        self.windows.insert(
            window.id(),
            Win {
                handle: window,
                effect: rules::Effect::default(),
                placed: false,
                sized: false,
                held: None,
                asked: None,
                scope,
            },
        );
    }

    fn facts(&self, window: Window) -> (String, String, bool) {
        (
            window.app_id().get_untracked().unwrap_or_default(),
            window.title().get_untracked(),
            window.is_xwayland().get_untracked(),
        )
    }

    /// Gives a new window its place by the rules.
    fn place(&mut self, window: Window) {
        let id = window.id();
        if self.windows.get(&id).is_none_or(|win| win.placed) {
            return;
        }
        let (app_id, title, _) = self.facts(window);
        let effect = self.effect(window);
        let floating = effect.float == Some(true) || effect.overlay.is_some();
        let workspace = effect.workspace.map(|number| number.to_string());
        let output = workspace
            .as_deref()
            .and_then(|id| self.model.workspace(id))
            .map_or(self.model.focused_output.clone(), |ws| ws.output.clone());
        let area = self
            .model
            .output(&output)
            .map_or((1280.0, 720.0), |output| {
                (output.geo.rect.w, output.geo.rect.h)
            });
        let float_rect = rules::float_rect(&effect, area, None);
        let sized = effect.size.is_some();
        // Electron apps ask to be maximized before they map; Hyprland's
        // scrolling layout opens them as a column the width of the view.
        let asked = self.windows.get(&id).and_then(|win| win.asked);
        window.state(&FORCE).set(!floating || sized);
        window.state(&BLUR).set(self.blurs(&effect));
        window
            .state(&CONFINE)
            .set(effect.confine_pointer == Some(true));
        self.model.open(
            NewWindow {
                id: id.clone(),
                title,
                app_id,
                pid: window.pid().map_or(0, i64::from),
                size: None,
            },
            Placement {
                workspace,
                overlay: effect.overlay.clone(),
                floating,
                float_rect,
                pinned: effect.pin.unwrap_or(false),
                fullscreen: window.is_fullscreen().get_untracked()
                    || asked == Some(Big::Fullscreen),
            },
        );
        if asked == Some(Big::Maximized) {
            self.model.set_big(&id, Some(Big::Maximized));
        }
        let _ = self.model.set_max_size(&id, max_size(window));
        if let Some(win) = self.windows.get_mut(&id) {
            win.effect = effect;
            win.placed = true;
            win.sized = !floating || sized;
        }
        window.set_close_animation_duration(self.scene.close_ms() as u64);
        self.commit();
    }

    /// The rules' sum for the window as it is now. A window the client
    /// marks as a dialog floats unless a rule says otherwise, as under
    /// Hyprland.
    fn effect(&self, window: Window) -> rules::Effect {
        let (app_id, title, xwayland) = self.facts(window);
        rules::resolve(
            &self.settings.rules,
            &rules::Facts {
                app_id: &app_id,
                title: &title,
                xwayland,
                tag: &window.tag().get_untracked().unwrap_or_default(),
                floating: window.suggests_float().get_untracked(),
            },
        )
    }

    /// Applies the rules again to what the client said by its first
    /// buffer, as Hyprland does at map: GTK 3 sets a dialog's fixed size
    /// only after the first configure, so Electron's `showErrorBox` looks
    /// resizable when it is placed. Nothing of the window is drawn yet, so
    /// a changed outcome places it again from the start, with every effect
    /// a placement takes (float, size, position, workspace, overlay, pin).
    /// Returns whether a tile became a float.
    fn reclassify(&mut self, window: Window) -> bool {
        let id = window.id();
        let Some(was) = self.model.window(&id).map(|rec| rec.floating) else {
            return false;
        };
        let effect = self.effect(window);
        if self.windows.get(&id).is_none_or(|win| win.effect == effect) {
            return false;
        }
        self.model.close(&id);
        if let Some(win) = self.windows.get_mut(&id) {
            win.placed = false;
        }
        self.place(window);
        self.scene.land(&id);
        !was && self.model.window(&id).is_some_and(|rec| rec.floating)
    }

    /// A tile that became a float by its first buffer may have drawn it
    /// at the size it was `told` as a tile rather than its own. With no
    /// size rule that buffer would set the float's size, so the window stays
    /// out of sight until it draws itself again at a size it picks, asked
    /// by a configure with no size. A window that drew a size of its own (a
    /// fixed-size dialog) is shown at it.
    fn hold(&mut self, window: Window, told: Option<(f64, f64)>) -> bool {
        let id = window.id();
        let Some(win) = self.windows.get(&id) else {
            return false;
        };
        let drawn = window.position().get_untracked();
        let drawn = (drawn.width, drawn.height);
        let at_tile =
            told.is_some_and(|(w, h)| (drawn.0 - w).abs() < 1.0 || (drawn.1 - h).abs() < 1.0);
        if win.sized || !at_tile {
            return false;
        }
        let scope = win.scope;
        let done = Rc::new(Cell::new(false));
        scope.effect(move || {
            if done.get() {
                return;
            }
            let now = window.position().get();
            if (now.width, now.height) != drawn {
                done.set(true);
                enter(move |engine| engine.release(window));
            }
        });
        let timer = set_timeout(HOLD_MS, move || enter(move |engine| engine.release(window)));
        if let Some(win) = self.windows.get_mut(&id) {
            win.held = Some(timer);
        }
        window.state(&OPACITY).set(0.0);
        window.state(&LIVE).set(false);
        true
    }

    /// A held window has drawn itself at its float's size, or ran out of
    /// time: it is shown.
    fn release(&mut self, window: Window) {
        let id = window.id();
        let Some(timer) = self.windows.get_mut(&id).and_then(|win| win.held.take()) else {
            return;
        };
        timer.cancel();
        self.settle(window);
    }

    /// Where a float of `size` goes on `output` (global): over the middle of
    /// its parent when the parent is on the same output, else the output's
    /// middle; kept on the output either way. Local to the output.
    fn centred(&self, window: Window, output: Box2, size: (f64, f64)) -> Box2 {
        let (w, h) = size;
        let parent = window.parent_id().get_untracked().and_then(|parent| {
            self.model
                .layout()
                .into_iter()
                .find(|placed| placed.id == parent)
                .filter(|placed| {
                    self.model.output_of(&window.id()).as_deref() == Some(&placed.output)
                })
                .map(|placed| placed.rect.translate(-output.x, -output.y))
        });
        let (cx, cy) = match parent {
            Some(rect) => (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
            None => (output.w / 2.0, output.h / 2.0),
        };
        let x = (cx - w / 2.0).clamp(0.0, (output.w - w).max(0.0));
        let y = (cy - h / 2.0).clamp(0.0, (output.h - h).max(0.0));
        Box2::new(x.round(), y.round(), w, h)
    }

    /// The client has drawn itself: a float that was not told a size keeps
    /// the one it chose, where its rule, its parent or the centre puts it.
    fn first_commit(&mut self, window: Window) {
        self.place(window);
        let id = window.id();
        let told = self
            .model
            .layout()
            .into_iter()
            .find(|placed| placed.id == id)
            .map(|placed| (placed.rect.w, placed.rect.h));
        if self.reclassify(window) && self.hold(window, told) {
            return self.commit();
        }
        self.settle(window);
    }

    fn settle(&mut self, window: Window) {
        let id = window.id();
        let Some(win) = self.windows.get(&id) else {
            return;
        };
        self.scene.arrive(&id);
        if win.sized {
            return self.commit();
        }
        let natural = window.position().get_untracked();
        let effect = win.effect.clone();
        if let Some(win) = self.windows.get_mut(&id) {
            win.sized = true;
        }
        window.state(&FORCE).set(true);
        let output = self
            .model
            .output_of(&id)
            .and_then(|name| self.model.output(&name))
            .map(|output| output.geo.rect);
        if let Some(output) = output
            && natural.width >= 1.0
            && natural.height >= 1.0
        {
            let (w, h) = (natural.width.min(output.w), natural.height.min(output.h));
            let local = rules::float_rect(&effect, (output.w, output.h), Some((w, h)))
                .unwrap_or_else(|| self.centred(window, output, (w, h)));
            if self.model.place(&id, local.translate(output.x, output.y)) {
                self.scene.sync(&self.model);
                self.scene.land(&id);
            }
        }
        self.commit();
    }

    fn renamed(&mut self, window: Window, title: &str, app_id: &str) {
        let id = window.id();
        if self.windows.get(&id).is_some_and(|win| win.placed) {
            self.model.set_title(&id, title);
            self.model.set_app_id(&id, app_id);
        }
        self.publish();
    }

    fn bounded(&mut self, window: Window) {
        if self.model.set_max_size(&window.id(), max_size(window)) {
            self.commit();
        }
    }

    /// The compositor keeps the window's last picture for as long as it was
    /// told and plays this on it; the rest of the tape closes up meanwhile.
    fn closing(&mut self, window: Window) {
        let id = window.id();
        let ms = self.scene.close_ms() as u64;
        if ms > 0 {
            let rect = window.rect();
            let (dw, dh) = (rect.width * (1.0 - POP_OUT), rect.height * (1.0 - POP_OUT));
            // `windowsOut` and `fadeOut`: the shrink rides spatialFast for
            // as far as the fade on effectsFast lets it be seen.
            let families = self.theme.chrome.motion.families;
            let curve = |[x1, y1, x2, y2]: [f64; 4]| cubic_bezier(x1, y1, x2, y2);
            window.schedule_animation(
                ManagedAnimation::new("close")
                    .rect(
                        Some(Rect::new(0.0, 0.0, 0.0, 0.0)),
                        Rect::new(dw / 2.0, dh / 2.0, -dw, -dh),
                        families.spatial_fast.ms as u64,
                        curve(families.spatial_fast.curve),
                        AnimationMode::Add,
                    )
                    .opacity(
                        Some(1.0),
                        0.0,
                        ms,
                        curve(families.effects_fast.curve),
                        AnimationMode::Multiply,
                    ),
            );
        }
        self.forget(&id);
    }

    fn closed(&mut self, window: Window) {
        let id = window.id();
        self.forget(&id);
        if let Some(win) = self.windows.remove(&id) {
            win.scope.dispose();
        }
    }

    fn forget(&mut self, id: &str) {
        if self.drag.as_ref().is_some_and(|drag| drag.id == id) {
            self.drag = None;
        }
        self.window_gone(id);
        if self.model.window(id).is_none() {
            return;
        }
        self.model.close(id);
        if let Some(win) = self.windows.get_mut(id) {
            win.placed = false;
        }
        self.commit();
    }

    /// A click, or a client the compositor let take the keyboard.
    pub(crate) fn focused(&mut self, window: Window) {
        let id = window.id();
        self.asked_focus = Some(id.clone());
        if self.model.focused_window() == Some(&id) {
            return;
        }
        // The compositor hands the keyboard back to the window that last had
        // it when a layer lets go of it. One out of view is not shown for
        // that: the keyboard goes back to where the model has it.
        if !self.model.in_view(&id) {
            self.focus();
            return;
        }
        if self.model.focus_window(&id) {
            self.commit();
        }
    }

    /// `focus_on_activate`: a window that asks for attention gets it.
    fn activated(&mut self, window: Window) {
        if self.model.focus_window(&window.id()) {
            self.commit();
        }
    }

    fn set_big(&mut self, window: Window, kind: Big, on: bool) {
        let id = window.id();
        if let Some(win) = self.windows.get_mut(&id)
            && !win.placed
        {
            win.asked = match (on, win.asked) {
                (true, _) => Some(kind),
                (false, Some(asked)) if asked == kind => None,
                (false, asked) => asked,
            };
            return;
        }
        let now = self.model.big_of(&id);
        let changed = match (on, now) {
            (true, now) if now != Some(kind) => self.model.set_big(&id, Some(kind)),
            (false, Some(now)) if now == kind => self.model.set_big(&id, None),
            _ => false,
        };
        if changed {
            self.commit();
        }
    }
}

/// The most the client says it will draw, 0 on an axis it leaves free.
fn max_size(window: Window) -> Option<(f64, f64)> {
    let max = window.size_constraints().get_untracked().max?;
    Some((f64::from(max.width), f64::from(max.height)))
}
