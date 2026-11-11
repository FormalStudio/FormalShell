//! A window's frame and its way into and out of the model.

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
            .interactive(window.state(&LIVE));
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
            enter(move |engine| engine.renamed(window, &title, &app_id));
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
        let (app_id, title, xwayland) = self.facts(window);
        // A dialog floats unless a rule says otherwise, as it does under
        // Hyprland.
        let dialog =
            window.is_transient().get_untracked() || !window.is_resizable().get_untracked();
        let effect = rules::resolve(
            &self.settings.rules,
            &rules::Facts {
                app_id: &app_id,
                title: &title,
                xwayland,
                tag: &window.tag().get_untracked().unwrap_or_default(),
                floating: dialog,
            },
        );
        let floating = effect.float.unwrap_or(dialog) || effect.overlay.is_some();
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
        window.state(&FORCE).set(!floating || sized);
        window.state(&BLUR).set(self.blurs(&effect));
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
                fullscreen: window.is_fullscreen().get_untracked(),
            },
        );
        if let Some(win) = self.windows.get_mut(&id) {
            win.effect = effect;
            win.placed = true;
            win.sized = !floating || sized;
        }
        window.set_close_animation_duration(self.scene.close_ms() as u64);
        self.commit();
    }

    /// The client has drawn itself: a float that was not told a size keeps
    /// the one it chose, where its rule or the centre puts it.
    fn first_commit(&mut self, window: Window) {
        self.place(window);
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
            let local = rules::float_rect(&effect, (output.w, output.h), Some((w, h))).unwrap_or(
                Box2::new(
                    ((output.w - w) / 2.0).round(),
                    ((output.h - h) / 2.0).round(),
                    w,
                    h,
                ),
            );
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
    fn focused(&mut self, window: Window) {
        let id = window.id();
        self.asked_focus = Some(id.clone());
        if self.model.focused_window() == Some(&id) {
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
