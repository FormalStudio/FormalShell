//! Things under the pointer's hand: a window moved or resized with Super, a
//! tile lifted out of its tape and dropped into a column gap, data dragged
//! between windows, and what holding any of them still over a hot spot does.

use formalwm_core::{color::Rgba, geom::Rect as Box2, model::Layer};
use shojiwm_rs::{
    prelude::*,
    ssd::{WindowMoveEventSnapshot, WindowMovePhaseSnapshot, WindowResizeEventSnapshot},
};

use crate::{
    engine::{Engine, color, enter},
    overview::Hit,
    windows,
};

/// How long a drag rests on a hot spot before the spot acts.
const HOLD_MS: f64 = 450.0;
/// The top-left corner that opens the overview, and the strip at a tape's
/// side that scrolls it, in pixels.
const CORNER: f64 = 12.0;
const EDGE: f64 = 24.0;
/// Pixels a second a tape scrolls while a drag rests at its edge.
const EDGE_SPEED: f64 = 1400.0;
const EDGE_TICK_MS: f64 = 16.0;
/// Above every layer the model stacks.
const DRAG_Z: i32 = 900_000;

/// A window drawn where the hand has it instead of where the scene does.
pub struct Drag {
    pub id: String,
    /// The size its client has.
    pub size: (f64, f64),
    /// Where on the window the hand has it, as a share of its size.
    pub grip: (f64, f64),
    /// The pointer, global.
    pub at: (f64, f64),
}

#[derive(Clone, Debug, Default, PartialEq)]
enum Zone {
    #[default]
    None,
    Corner,
    Edge(i8),
    Workspace(String),
    Window(String),
}

#[derive(Default)]
pub struct Hold {
    /// Data is being dragged between clients.
    pub data_drag: bool,
    /// A window is being moved by the pointer.
    pub window_drag: bool,
    zone: Zone,
    timer: Option<TimerHandle>,
}

pub fn setup() {
    COMPOSITOR.event.on_window_move(|window, event| {
        let event = event.clone();
        enter(move |engine| engine.window_move(window, &event));
    });
    COMPOSITOR.event.on_window_resize(|window, event| {
        let event = event.clone();
        enter(move |engine| engine.window_resize(window, &event));
    });
    COMPOSITOR.event.on_pointer_move_async(|event| {
        let at = (event.position.x, event.position.y);
        enter(move |engine| engine.pointer_moved(at));
    });
    COMPOSITOR.dnd.on_start(|drag| {
        enter(|engine| engine.hold.data_drag = true);
        drag.on_end(|_| {
            enter(|engine| {
                engine.hold.data_drag = false;
                engine.hold_end();
                engine.focus();
                engine.regrab_overview();
            })
        });
        show_card(&drag);
    });
}

/// The card that rides the pointer while data is dragged: the client's
/// icon, what a drop would do, in the theme's surface colours. A drop
/// nobody takes flies back to where the drag started. Its size and place
/// ride the spatial clocks, its opacity the effects ones.
fn show_card(drag: &DragSession) {
    let Ok((theme, enabled)) =
        crate::engine::ask(|engine| Ok((engine.theme.clone(), engine.theme.chrome.motion.enabled)))
    else {
        return;
    };
    let families = theme.chrome.motion.families;
    let curve = |[x1, y1, x2, y2]: [f64; 4]| cubic_bezier(x1, y1, x2, y2);
    let ms = move |ms: f64| if enabled { ms } else { 1.0 };
    let role = |name: &str| color(Rgba::resolve(name, &theme.colors).unwrap_or(Rgba::BLACK));
    let (surface, line, text, accent) = (
        role("background"),
        role("border"),
        role("primaryForeground"),
        role("primary"),
    );
    let rounding = theme.chrome.rounding.max(0) as f64;

    let appear = Animation::new(0.0);
    let fade = Animation::new(0.0);
    let finish = Animation::new(0.0);
    appear.start(
        AnimationOptions::to(1.0, ms(families.spatial_fast.ms))
            .from(0.0)
            .easing(curve(families.spatial_fast.curve)),
    );
    fade.start(
        AnimationOptions::to(1.0, ms(families.effects_fast.ms))
            .from(0.0)
            .easing(curve(families.effects_fast.curve)),
    );
    let ended = drag.ended();
    let origin = drag.origin();
    let position = move |axis: fn(DragPoint) -> f64| {
        derive(move || match ended.get() {
            None => 0.0,
            Some(end) if end.accepted => axis(end.position),
            Some(end) => {
                let (from, to) = (axis(end.position), axis(origin));
                from + (to - from) * finish.get()
            }
        })
    };
    let card = {
        let drag = drag.clone();
        move || {
            let badge = match drag.action().get() {
                DragAction::Copy => Some("+"),
                DragAction::Move => Some("→"),
                DragAction::Ask => Some("?"),
                DragAction::None => drag.target().get().is_some().then_some("×"),
            };
            let mut card = Flex::row()
                .style(
                    Style::new()
                        .padding(8.0)
                        .gap(8.0)
                        .border_radius(rounding)
                        .background(Color { a: 235, ..surface })
                        .border(1.0, line)
                        .align_items(AlignItems::Center),
                )
                .child(drag.icon_slot(None::<Element>));
            if let Some(badge) = badge {
                card = card.child(
                    Flex::column()
                        .style(
                            Style::new()
                                .size(18.0, 18.0)
                                .border_radius(9.0)
                                .background(accent)
                                .align_items(AlignItems::Center)
                                .justify_content(JustifyContent::Center),
                        )
                        .child(
                            Label::new(badge)
                                .style(Style::new().color(text).font_size(12.0).font_weight(700)),
                        ),
                );
            }
            card
        }
    };
    let view = drag.show(
        DragViewOptions::new()
            .anchor(derive(move || {
                if ended.get().is_some() {
                    DragViewAnchor::Global
                } else {
                    DragViewAnchor::Pointer
                }
            }))
            .x(position(|point| point.x))
            .y(position(|point| point.y))
            .opacity(derive(move || fade.get()))
            .scale(derive(move || 0.9 + 0.1 * appear.get()))
            .content(card),
    );
    drag.on_end(move |end| {
        let leave = move || {
            fade.start_then(
                AnimationOptions::to(0.0, ms(families.effects_fast.ms))
                    .easing(curve(families.effects_fast.curve)),
                move |_| view.dispose(),
            );
        };
        if end.accepted {
            return leave();
        }
        finish.start_then(
            AnimationOptions::to(1.0, ms(families.spatial.ms))
                .from(0.0)
                .easing(curve(families.spatial.curve)),
            move |_| leave(),
        );
    });
}

impl Engine {
    /// Writes the dragged window's place, which the scene does not have.
    pub fn present_drag(&mut self) {
        let Some(drag) = &self.drag else {
            return;
        };
        let Some(win) = self.windows.get(&drag.id) else {
            return;
        };
        let border = self.signals.look.get_untracked().border;
        let window = win.handle;
        let (w, h) = drag.size;
        let rect = self.drag_box(drag);
        let (sx, sy) = (rect.w / w.max(1.0), rect.h / h.max(1.0));
        batch(|| {
            window.state(&windows::RECT).set(Rect::new(
                rect.x - border * sx,
                rect.y - border * sy,
                (w + 2.0 * border).round(),
                (h + 2.0 * border).round(),
            ));
            window.state(&windows::XFORM).set(ManagedTransform {
                origin_x: 0.0,
                origin_y: 0.0,
                translate_x: 0.0,
                translate_y: 0.0,
                scale_x: sx,
                scale_y: sy,
            });
            window.state(&windows::Z).set(DRAG_Z);
            window.state(&windows::OUTPUTS).set(None);
            window.state(&windows::IDLE).set(false);
            window.state(&windows::LIVE).set(false);
            window
                .state(&windows::OPACITY)
                .set(self.opacity_of(&drag.id, false));
        });
    }

    /// The box a dragged window's client is drawn in: under the overview
    /// as small as the windows around it, held where the hand took it.
    pub fn drag_box(&self, drag: &Drag) -> Box2 {
        let zoom = self
            .output_at(drag.at)
            .and_then(|output| self.lens(&output))
            .map_or(1.0, |(lens, _)| lens.zoom);
        let (w, h) = (drag.size.0 * zoom, drag.size.1 * zoom);
        Box2::new(
            drag.at.0 - drag.grip.0 * w,
            drag.at.1 - drag.grip.1 * h,
            w,
            h,
        )
    }

    fn output_at(&self, at: (f64, f64)) -> Option<String> {
        self.model
            .outputs
            .iter()
            .find(|output| output.geo.rect.contains(at.0, at.1))
            .map(|output| output.geo.name.clone())
    }

    fn window_move(&mut self, window: Window, event: &WindowMoveEventSnapshot) {
        let id = window.id();
        let Some(record) = self.model.window(&id) else {
            return;
        };
        let floating = record.floating;
        let border = self.signals.look.get_untracked().border;
        let pointer = (event.current_pointer.x, event.current_pointer.y);
        let client = Box2::new(
            event.start_rect.x + event.delta.x,
            event.start_rect.y + event.delta.y,
            event.start_rect.width,
            event.start_rect.height,
        )
        .inset(border);
        let ending = matches!(
            event.phase,
            WindowMovePhaseSnapshot::End | WindowMovePhaseSnapshot::Cancel
        );
        self.hold.window_drag = !ending;
        if ending {
            self.hold_end();
        } else {
            self.hold_at(pointer, false);
        }

        if floating {
            if ending {
                let here = self.output_at(pointer);
                if here.is_some() && here != self.model.output_of(&id) {
                    let workspace = here
                        .and_then(|output| self.model.active_workspace(&output))
                        .map(|ws| ws.id.clone());
                    if let Some(workspace) = workspace {
                        self.model.move_to_workspace(&id, &workspace, true);
                    }
                }
            }
            self.model.place(&id, client);
            self.scene.sync(&self.model);
            self.scene.land(&id);
            if ending {
                self.commit();
                self.regrab_overview();
            } else {
                self.present();
            }
            return;
        }

        match event.phase {
            WindowMovePhaseSnapshot::Start => {
                if self.model.big_of(&id).is_some() || self.scene.visual(&id).is_none() {
                    return;
                }
                self.model.focus_window(&id);
                self.drag = Some(Drag {
                    id,
                    size: (client.w, client.h),
                    grip: (
                        (pointer.0 - client.x) / client.w.max(1.0),
                        (pointer.1 - client.y) / client.h.max(1.0),
                    ),
                    at: pointer,
                });
                self.commit();
                self.present_drag();
            }
            WindowMovePhaseSnapshot::Update => {
                if let Some(drag) = self.drag.as_mut().filter(|drag| drag.id == id) {
                    drag.at = pointer;
                    self.present_drag();
                }
            }
            WindowMovePhaseSnapshot::End | WindowMovePhaseSnapshot::Cancel => {
                let Some(drag) = self.drag.take().filter(|drag| drag.id == id) else {
                    return;
                };
                let dropped = event.phase == WindowMovePhaseSnapshot::End;
                self.drop_tile(&drag, dropped.then_some(pointer));
                self.regrab_overview();
            }
        }
    }

    /// A lifted tile comes down: into the column gap under the pointer on
    /// the workspace its output shows, or back where it was.
    fn drop_tile(&mut self, drag: &Drag, at: Option<(f64, f64)>) {
        let id = &drag.id;
        if let Some(at) = at
            && self.overview.open
        {
            return self.overview_drop(drag, at);
        }
        let workspace = at
            .and_then(|at| self.output_at(at))
            .and_then(|output| self.model.active_workspace(&output))
            .map(|ws| ws.id.clone());
        if let (Some(at), Some(workspace)) = (at, workspace)
            && let Some(before) = self.column_before(&workspace, at.0, id)
            && self.model.move_window(id, &workspace, before.as_deref())
        {
            self.model.focus_workspace(&workspace);
        }
        self.scene.sync(&self.model);
        let drawn = self.drag_box(drag);
        let from = self.undo_lens(id, drawn).unwrap_or(drawn);
        self.scene.carry(id, from);
        self.commit();
    }

    fn window_resize(&mut self, window: Window, event: &WindowResizeEventSnapshot) {
        let id = window.id();
        let Some(record) = self.model.window(&id) else {
            return;
        };
        let border = self.signals.look.get_untracked().border;
        let rect = Box2::new(
            event.current_rect.x,
            event.current_rect.y,
            event.current_rect.width,
            event.current_rect.height,
        )
        .inset(border);
        if record.floating {
            self.model.place(&id, rect);
        } else {
            // A tile's width is its column's share of the strip.
            let Some(output) = self.model.output_of(&id) else {
                return;
            };
            let strip = self.model.strip_of(&output).w.max(1.0);
            self.model.focus_window(&id);
            self.model
                .set_column_width((rect.w + 2.0 * self.model.options.tile_inset()) / strip);
        }
        self.scene.sync(&self.model);
        self.scene.finish();
        self.commit();
    }

    fn pointer_moved(&mut self, at: (f64, f64)) {
        self.arm_idle_pointer();
        if self.hold.data_drag {
            self.hold_at(at, true);
        }
    }

    fn zone_at(&self, at: (f64, f64), data: bool) -> Zone {
        let Some(output) = self.model.output(&self.output_at(at).unwrap_or_default()) else {
            return Zone::None;
        };
        let area = output.geo.rect;
        if self.overview.open {
            let dragged = self.drag.as_ref().map(|drag| drag.id.as_str());
            let focused = self.model.focused_workspace().map(|ws| ws.id.clone());
            return match self.overview_hit(at, dragged) {
                Hit::Window { workspace, .. } | Hit::Workspace { id: workspace, .. }
                    if Some(&workspace) != focused.as_ref() =>
                {
                    Zone::Workspace(workspace)
                }
                _ => Zone::None,
            };
        }
        if at.0 - area.x < CORNER && at.1 - area.y < CORNER {
            return Zone::Corner;
        }
        if at.0 - area.x < EDGE {
            return Zone::Edge(-1);
        }
        if area.right() - at.0 < EDGE {
            return Zone::Edge(1);
        }
        if !data {
            return Zone::None;
        }
        self.scene
            .frame()
            .iter()
            .rev()
            .find(|visual| {
                visual.visible && visual.layer != Layer::Overlay && visual.rect.contains(at.0, at.1)
            })
            .map_or(Zone::None, |visual| Zone::Window(visual.id.clone()))
    }

    /// A drag is at `at`: starts or stops what resting there does.
    pub fn hold_at(&mut self, at: (f64, f64), data: bool) {
        let zone = self.zone_at(at, data);
        if zone == self.hold.zone {
            return;
        }
        if let Some(timer) = self.hold.timer.take() {
            timer.cancel();
        }
        self.hold.zone = zone.clone();
        self.hold.timer = match zone {
            Zone::None => None,
            Zone::Corner => Some(set_timeout(HOLD_MS, || {
                enter(|engine| {
                    engine.hold.zone = Zone::None;
                    engine.set_overview(Some(true));
                })
            })),
            Zone::Workspace(workspace) => Some(set_timeout(HOLD_MS, move || {
                let workspace = workspace.clone();
                enter(move |engine| {
                    engine.hold.zone = Zone::None;
                    if engine.model.focus_workspace(&workspace) {
                        engine.commit();
                    }
                    engine.set_overview(Some(false));
                })
            })),
            Zone::Window(id) => Some(set_timeout(HOLD_MS, move || {
                let id = id.clone();
                enter(move |engine| {
                    if engine.model.focus_window(&id) {
                        engine.commit();
                    }
                })
            })),
            Zone::Edge(side) => Some(set_interval(EDGE_TICK_MS, move || {
                enter(move |engine| engine.edge_scroll(side))
            })),
        };
    }

    pub fn hold_end(&mut self) {
        if let Some(timer) = self.hold.timer.take() {
            timer.cancel();
        }
        self.hold.zone = Zone::None;
    }

    fn edge_scroll(&mut self, side: i8) {
        let Some(workspace) = self.model.focused_workspace().map(|ws| ws.id.clone()) else {
            return;
        };
        let step = side as f64 * EDGE_SPEED * EDGE_TICK_MS / 1000.0;
        if self.model.scroll_by(&workspace, step) {
            self.scene.sync(&self.model);
            self.scene.hold_scroll(&workspace);
            self.present();
            self.present_drag();
        }
    }
}
