//! The window owners: which owner a configure, a frame callback or a close
//! belongs to, the one order they come off screen, present, wake, rescale,
//! report and take the store topics they read in, and the other order keys,
//! pastes and the pointer reach them in.

use std::time::Instant;

use serde_json::{Map, Value, json};
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;

use super::{App, Holder, Press, Target, heads, overview, screensaver::SaverInput};
use crate::store::Topic;
use crate::surfaces;
use crate::surfaces::modal::Part;
use crate::surfaces::bar::cell::Button;
use crate::surfaces::panel::host::Key;
use crate::surfaces::sidebar::minimap;
use crate::surfaces::tray_menu::Hit;

/// One window owner's part in the lifecycle. Its state lives on `App`
/// beside what it draws with, so every hook takes the whole of it.
pub(super) trait Owner: Sync {
    /// What the surface is, when it is one of this owner's.
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target>;
    fn configure(&self, _: &mut App, _: &WlSurface, _: (i32, i32)) {}
    fn frame(&self, _: &mut App, _: &WlSurface, _: Instant) {}
    fn closed(&self, _: &mut App, _: &WlSurface) {}
    /// Cards whose exit has run come off screen, ahead of every present.
    fn finished(&self, _: &mut App, _: Instant) {}
    fn present(&self, _: &mut App, _: Instant) {}
    /// The next moment something of its own starts by itself.
    fn wake(&self, _: &App, _: Instant) -> Option<Instant> {
        None
    }
    /// A surface moved to another scale. Only an owner that presents when
    /// told has anything to do.
    fn rescaled(&self, _: &mut App) {}
    fn output_gone(&self, _: &mut App, _: &WlOutput) {}
    /// Its blocks of `debug dump`.
    fn report(&self, _: &App, _: &mut Map<String, Value>) {}
    /// Its surfaces' commit and callback counts, for the exit line.
    fn counts(&self, _: &App, _: &mut Vec<String>) {}
    /// The store topics it redraws or reacts on, as it stands now.
    fn reads(&self, _: &App, _: Topic) -> bool {
        false
    }
    /// A topic it reads moved.
    fn changed(&self, _: &mut App, _: Topic) {}
    /// A key while it holds the keyboard. True when it took the key.
    fn key(&self, _: &mut App, _: &KeyEvent, _repeat: bool) -> bool {
        false
    }
    /// The clipboard's text a field asked for, while it holds the keyboard.
    /// True when it took the text.
    fn paste(&self, _: &mut App, _: &str) -> bool {
        false
    }
    /// A pointer event, on `target` as far as the walk has resolved it.
    fn pointer(&self, _: &mut App, _: &PointerEvent, _: Option<Target>) -> Pointer {
        Pointer::Pass
    }
}

/// What an owner did with a pointer event.
pub(super) enum Pointer {
    /// On to the next owner, and to the strip's own handling past the last.
    Pass,
    Taken,
    /// It moved the surface under the event to another owner (a spare bar
    /// promoted), so the walk goes on with the target resolved again.
    Rerouted,
}

/// The present order: every output's head, the live bar, the cards hanging
/// off it, the launcher before the atlas, the overlays, and the lock and its
/// veils last. While the boot's lock still owes its splash, `App::present`
/// draws that alone.
pub(super) static LIFECYCLE: &[&dyn Owner] = &[
    &Heads,
    &Bar,
    &Cards,
    &Launcher,
    &Atlas,
    &Switcher,
    &Overview,
    &Saver,
    &Tooltip,
    &Toasts,
    &Osd,
    &Headset,
    &Profiles,
    &Dock,
    &JoinPreview,
    &Zones,
    &Scrim,
    &Region,
    &Polkit,
    &Overlay,
    &Lock,
    &Veil,
    &Caffeinate,
    &HotCorners,
    &Reveal,
];

/// The order keys, pastes and the pointer reach the owners in, which is not
/// the present order: the screensaver's dismissing key, then the lock, the
/// polkit dialog, the modals with the atlas ahead of the launcher, the
/// switcher, the overview, the sidebar's keys, and the bar's cards last. An
/// owner without a hook passes. For the pointer the order decides only
/// between owners that act on another's surface: the profile pill under the
/// launcher's card and a spare bar promoted under the pointer. What every
/// owner passes goes to `App::pointer_rest`.
pub(super) static INPUT: &[&dyn Owner] = &[
    &Saver,
    &Lock,
    &HotCorners,
    &Reveal,
    &Outgoing,
    &Heads,
    &Polkit,
    &Overlay,
    &Region,
    &Atlas,
    &Profiles,
    &Dock,
    &Launcher,
    &Switcher,
    &Overview,
    &Bar,
    &Headset,
    &Cards,
    &Toasts,
];

/// A store change to every owner reading it, in the present order.
pub(super) fn changed(app: &mut App, topic: Topic) {
    for o in LIFECYCLE {
        if o.reads(app, topic) {
            o.changed(app, topic);
        }
    }
}

/// The device slices, which the osd, the headset card and the launcher's
/// device routes all read again on any of them.
pub(super) const DEVICES: &[Topic] = &[
    Topic::Bluetooth,
    Topic::Network,
    Topic::Audio,
    Topic::Power,
    Topic::Earbuds,
    Topic::PadLights,
    Topic::Profiles,
    Topic::Revealed,
    Topic::Logind,
];

/// Every owner's blocks of `debug dump`, and the two that span owners:
/// the modal cards on screen and the windows each card captures.
pub(super) fn report(app: &App, dump: &mut Map<String, Value>) {
    dump.insert("modals".into(), app.modals().map(surfaces::modal::Modal::report).collect());
    dump.insert("captures".into(), app.captures_report());
    for o in LIFECYCLE {
        o.report(app, dump);
    }
}

/// The owner of `surface`. A surface has one owner, so the walk's order
/// decides nothing here.
pub(super) fn of(app: &App, surface: &WlSurface) -> Option<&'static dyn Owner> {
    LIFECYCLE.iter().copied().find(|o| o.owns(app, surface).is_some())
}

/// Whose surface a pointer event landed on, a dropped panel surface's
/// included.
pub(super) fn target(app: &App, surface: &WlSurface) -> Option<Target> {
    LIFECYCLE.iter().find_map(|o| o.owns(app, surface)).or_else(|| Outgoing.owns(app, surface))
}

struct Heads;

impl Owner for Heads {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.head_part(surface).map(|(i, part)| Target::Head(i, part))
    }

    /// A press on another output's catcher closes what a click outside
    /// closes; a spare bar under the pointer becomes the live one, and the
    /// event goes on to it.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        match target {
            Some(Target::Head(_, heads::Part::Catcher)) => {
                if matches!(e.kind, PointerEventKind::Press { .. }) {
                    app.dismiss();
                }
                Pointer::Taken
            }
            Some(Target::Head(i, heads::Part::Bar))
                if match e.kind {
                    PointerEventKind::Press { .. } => app.press_spare(i),
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => app.promote(i),
                    _ => false,
                } =>
            {
                Pointer::Rerouted
            }
            _ => Pointer::Pass,
        }
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        if let Some((i, part)) = app.head_part(surface) {
            app.head_configure(i, part, width, height);
        }
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, now: Instant) {
        if let Some((i, part)) = app.head_part(surface) {
            app.head_frame(i, part, now);
        }
    }

    fn closed(&self, app: &mut App, surface: &WlSurface) {
        if let Some((i, part)) = app.head_part(surface) {
            app.head_closed(i, part);
        }
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_heads(live!(app).dirty, now);
    }

    fn wake(&self, app: &App, now: Instant) -> Option<Instant> {
        app.spares_wake(now)
    }

    fn rescaled(&self, app: &mut App) {
        app.update_backdrops();
    }

    fn output_gone(&self, app: &mut App, output: &WlOutput) {
        app.drop_head(output);
    }

    /// A depth map written: the desktop clocks cut their digits again.
    fn reads(&self, _: &App, topic: Topic) -> bool {
        topic == Topic::Depth
    }

    fn changed(&self, app: &mut App, _: Topic) {
        app.desktop_clock_changed();
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("backdrop".into(), app.backdrop_state());
        dump.insert("desktopClock".into(), app.desktop_clock_state());
    }
}

/// The live bar's window.
struct Bar;

impl Owner for Bar {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        live!(app).surface.as_ref().is_some_and(|s| s.layer.wl_surface() == surface).then_some(Target::Bar)
    }

    /// Every slice but the launcher's own inputs and the inboxes other
    /// owners drain: its cells declare which of them they show, and the
    /// strip's paint, spares, backdrops and rims follow any of them.
    fn reads(&self, _: &App, topic: Topic) -> bool {
        !matches!(
            topic,
            Topic::Menu | Topic::Screensaver | Topic::Capture | Topic::Launch | Topic::Clipboard | Topic::Localsend | Topic::Mirror | Topic::Picker
        )
    }

    /// Settings and plugins rebuild the strip and a theme restyles it, both
    /// reading everything again; any other topic re-reads the cells.
    fn changed(&self, app: &mut App, topic: Topic) {
        match topic {
            Topic::Config | Topic::Plugins => app.apply_config(),
            Topic::Theme => app.set_bar_theme(),
            _ => app.refresh_bar(Some(topic)),
        }
    }

    /// The sidebar's map while it holds the keyboard.
    fn key(&self, app: &mut App, event: &KeyEvent, _: bool) -> bool {
        app.sidebar_key(event)
    }

    /// A press and release on a cell clicks it, the sidebar's map drags
    /// between the two, and a wheel over a cell is that cell's. Its hover
    /// goes on to the shared handling.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        let (x, y) = e.position;
        let held = matches!(app.pressed, Some((Target::Bar, _)));
        match e.kind {
            PointerEventKind::Motion { .. } if held => {
                let moved = live!(app).bar.sidebar_motion((x, y));
                if moved != minimap::Moved::Still {
                    live!(app).dirty = true;
                }
                if moved == minimap::Moved::Overview {
                    app.overview_action(overview::Action::Open);
                }
                Pointer::Pass
            }
            PointerEventKind::Press { .. } if target == Some(Target::Bar) => {
                app.pressed = live!(app).bar.hit(x, y).map(|i| (Target::Bar, Press::Slot(i)));
                if let Some((_, Press::Slot(i))) = app.pressed
                    && live!(app).bar.sidebar_press(i, (x, y))
                {
                    live!(app).dirty = true;
                }
                Pointer::Taken
            }
            PointerEventKind::Release { button, .. } if held => {
                let Some((_, Press::Slot(i))) = app.pressed.take() else { return Pointer::Taken };
                if let Some(commands) = live!(app).bar.sidebar_release((x, y)) {
                    app.minimap_drop(commands);
                } else if live!(app).bar.clickable(i) {
                    let action = live!(app).bar.click(i, crate::input::button(button), x, y, &app.store);
                    let anchor = live!(app).bar.slot_anchor(i);
                    app.act(action, anchor);
                }
                Pointer::Taken
            }
            PointerEventKind::Axis { vertical, horizontal, .. } if target == Some(Target::Bar) => {
                let (v, h) = (crate::scroll::notches(&vertical), crate::scroll::notches(&horizontal));
                if (v != 0.0 || h != 0.0)
                    && let Some(i) = live!(app).bar.hit(x, y)
                {
                    let action = live!(app).bar.wheel(i, v, h, &app.store);
                    let anchor = live!(app).bar.slot_anchor(i);
                    app.act(action, anchor);
                    if live!(app).bar.sidebar().is_some() {
                        live!(app).dirty = true;
                        app.request_paint();
                    }
                }
                Pointer::Taken
            }
            _ => Pointer::Pass,
        }
    }

    fn configure(&self, app: &mut App, _: &WlSurface, (width, height): (i32, i32)) {
        let current = live!(app).bar.scene.size;
        let w = if width > 0 { width } else { current.w };
        let h = if height > 0 { height } else { current.h };
        live!(app).bar.resize(w, h);
        let size = live!(app).bar.scene.size;
        let win = &mut live!(app);
        if let Some(s) = &mut win.surface {
            let first = !s.configured;
            if first {
                crate::trace::phase("bar configured");
            }
            s.configure(size.w, size.h);
            // The frame drawn while this configure was on its way
            // goes out now, ahead of anything else this turn holds.
            if first && s.drawn_ahead() && size == current {
                s.present(&mut win.bar.scene, false, &app.qh);
            }
        }
        app.sync_bar_input();
        // The size the bar was laid out and drawn at ahead of this
        // configure needs nothing more; only a new one is re-read.
        if size != current {
            app.refresh_bar(None);
            // The sidebar clock frosts the picture under the window's
            // new place.
            app.sync_desktop_clocks();
        }
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        let Some(s) = &mut live!(app).surface else { return };
        // A callback only lets the next frame go; what that frame
        // draws is the present's to decide (a spinner needs no new
        // layout of the strip).
        if s.landed(now) {
            live!(app).dirty = true;
        }
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        live!(app).surface = None;
        live!(app).zones.clear();
    }

    fn present(&self, app: &mut App, now: Instant) {
        if live!(app).bar.sidebar_hand_over(now) {
            live!(app).dirty = true;
            app.overview_action(super::overview::Action::Open);
        }
        let qh = app.qh.clone();
        let animating = live!(app).bar.animating(&app.kit, now);
        let stepping = animating || live!(app).bar.marquee_due(&app.kit, now);
        if live!(app).dirty || (stepping && !live!(app).bar.step_in_place(&mut app.kit, now)) {
            live!(app).bar.layout(&mut app.kit, &app.store, now);
            live!(app).dirty = false;
            app.sync_sidebar_keys();
            app.sidebar_software();
            app.sync_bar_input();
        }
        let win = &mut live!(app);
        if let Some(s) = &mut win.surface {
            s.present(&mut win.bar.scene, animating, &qh);
        }
        if !app.faces_warmed && live!(app).surface.as_ref().is_some_and(|s| s.mapped()) {
            app.faces_warmed = true;
            crate::trace::phase("bar mapped");
            crate::trace::thread_ticks();
            app.warm_faces();
            app.prefault_launcher();
            app.launch.warm = true;
            app.sync_shards();
        }
    }

    fn wake(&self, app: &App, now: Instant) -> Option<Instant> {
        live!(app).bar.wake(&app.kit, now)
    }

    /// The strip and its line, `debug join`'s join, the frame ring and the
    /// sidebar's clock.
    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        let bar = &live!(app).bar;
        let (edge, screen) = (bar.edge().as_str(), app.bar_output_name());
        let line: Vec<Value> = bar.line_rects().iter().map(|r| json!(r)).collect();
        let reach = fs_js::json_number(app.store.theme.theme.card_radius());
        let join = app.debug_join().map(|(x, width)| json!({"edge": edge, "x": x, "width": width, "reach": reach, "screen": screen}));
        dump.insert("bar".into(), json!([{"screen": screen, "edge": edge, "line": line, "paint": bar.paint_state(&app.store)}]));
        dump.insert("join".into(), join.unwrap_or(Value::Null));
        dump.insert("frame".into(), bar.frame_state());
        dump.insert("sidebarClock".into(), app.sidebar_clock_state());
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(live!(app).surface.iter().map(|s| s.report()));
    }
}

/// The cards hanging off the bar: the panel host's card, a handoff's
/// outgoing one, and a tray item's menu. One owner, because the menu
/// presents ahead of the two hosts and comes off screen after them.
struct Cards;

impl Owner for Cards {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        if app.panel.as_ref().is_some_and(|p| p.win.owns(surface)) {
            Some(Target::Panel)
        } else if app.tray_menu.as_ref().is_some_and(|p| p.win.owns(surface)) {
            Some(Target::Menu)
        } else {
            app.outgoing.as_ref().is_some_and(|p| p.win.owns(surface)).then_some(Target::Outgoing)
        }
    }

    /// A theme restyles every card window on screen, a tray change reaches
    /// the item's menu, the compositor the Spaces peek, and the open panel
    /// redraws on what it declares.
    fn reads(&self, app: &App, topic: Topic) -> bool {
        matches!(topic, Topic::Theme | Topic::Tray | Topic::AppMenu | Topic::Compositor) || app.panel.as_ref().is_some_and(|p| p.module.reads().contains(&topic))
    }

    fn changed(&self, app: &mut App, topic: Topic) {
        match topic {
            Topic::Theme => app.restyle_cards(),
            Topic::Tray => app.tray_changed(),
            Topic::AppMenu => app.appmenu_changed(),
            Topic::Compositor => app.preview_changed(),
            _ => {}
        }
        if app.panel.as_ref().is_some_and(|p| topic == Topic::Theme || p.module.reads().contains(&topic)) {
            app.panel_dirty = true;
        }
    }

    /// Every key nothing ahead took: a tray item's menu over its panel
    /// while it is up, else the open panel.
    fn key(&self, app: &mut App, event: &KeyEvent, repeat: bool) -> bool {
        let editing = app.panel.as_ref().is_some_and(|h| h.editing());
        let key = match event.keysym {
            Keysym::Escape | Keysym::Tab | Keysym::ISO_Left_Tab | Keysym::Return | Keysym::KP_Enter if editing => match event.keysym {
                Keysym::Escape => Key::Escape,
                Keysym::Tab if !app.mods.shift => Key::Tab(1),
                Keysym::Tab | Keysym::ISO_Left_Tab => Key::Tab(-1),
                _ => Key::Activate,
            },
            sym if editing => Key::Edit(crate::input::field_key(sym), app.mods, event.utf8.clone()),
            _ => match crate::input::panel_key(event.keysym, event.utf8.clone(), repeat, app.mods.shift) {
                Some(key) => key,
                None => return true,
            },
        };
        if app.tray_menu.as_ref().is_some_and(|p| p.win.card.is_open()) {
            match key {
                Key::Escape => app.close_tray_menu(),
                Key::Move(0, dy) => drop(app.tray_menu_cursor(dy)),
                Key::Activate => drop(app.tray_menu_activate()),
                _ => {}
            }
            return true;
        }
        let Some(h) = &mut app.panel else { return true };
        let outs = h.key(key, &app.store, &app.runtime);
        app.panel_outs(Holder::Host, outs);
        true
    }

    /// Into the open panel's field, while one is being edited.
    fn paste(&self, app: &mut App, text: &str) -> bool {
        let Some(h) = app.panel.as_mut().filter(|h| h.editing()) else { return false };
        let outs = h.paste(text.to_owned(), &app.store, &app.runtime);
        app.panel_outs(Holder::Host, outs);
        true
    }

    /// The panel takes its whole gesture; a tray item's menu its rows and
    /// close button, a press outside its card closing it. Their hover goes
    /// on to the shared handling.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        let (x, y) = e.position;
        match (e.kind.clone(), target) {
            (PointerEventKind::Press { .. }, Some(Target::Panel)) => {
                if let Some(h) = &mut app.panel {
                    h.press(x, y, &app.store, &app.runtime);
                }
                app.pressed = Some((Target::Panel, Press::Whole));
                app.panel_dirty = true;
                Pointer::Taken
            }
            (PointerEventKind::Press { .. }, Some(Target::Menu)) => match app.tray_menu.as_ref().and_then(|p| p.menu_hit(x, y)) {
                Some(hit) => {
                    app.pressed = Some((Target::Menu, Press::Menu(hit)));
                    Pointer::Taken
                }
                None if app.tray_menu.as_ref().is_some_and(|p| !p.win.on_card(x, y)) => {
                    app.app_menu_press_off_card(x, y);
                    Pointer::Taken
                }
                None => {
                    app.pressed = None;
                    Pointer::Taken
                }
            },
            (PointerEventKind::Release { button, .. }, _) => match app.pressed {
                Some((Target::Panel, _)) => {
                    app.pressed = None;
                    let button = crate::input::button(button);
                    let outs = app.panel.as_mut().map_or(Vec::new(), |h| h.release(x, y, button, &app.store, &app.runtime));
                    app.panel_outs(Holder::Host, outs);
                    Pointer::Taken
                }
                Some((Target::Menu, Press::Menu(hit))) => {
                    app.pressed = None;
                    if crate::input::button(button) != Button::Left {
                        return Pointer::Taken;
                    }
                    let outcome = match (app.tray_menu.as_mut().and_then(|p| p.menu.as_mut()), hit) {
                        (Some(_), Hit::Close) => {
                            app.close_tray_menu();
                            return Pointer::Taken;
                        }
                        (Some(m), Hit::Row(i)) => m.activate(i),
                        (None, _) => return Pointer::Taken,
                    };
                    app.menu_outcome(outcome);
                    Pointer::Taken
                }
                _ => Pointer::Pass,
            },
            (PointerEventKind::Axis { time, vertical, horizontal, source }, Some(Target::Panel)) => {
                let (dx, dy) = (crate::scroll::notches(&horizontal), crate::scroll::notches(&vertical));
                let travel = crate::scroll::travel(&vertical, source, time);
                if let Some(h) = app.panel.as_mut().filter(|_| dx != 0.0 || dy != 0.0 || travel != crate::scroll::Travel::None) {
                    let outs = h.scroll(x, y, dx, dy, travel, &app.store, &app.runtime);
                    app.panel_outs(Holder::Host, outs);
                }
                app.panel_dirty = true;
                Pointer::Taken
            }
            (PointerEventKind::Axis { vertical, horizontal, .. }, Some(Target::Menu)) => {
                let v = crate::scroll::notches(&vertical);
                if (v != 0.0 || crate::scroll::notches(&horizontal) != 0.0)
                    && let Some(m) = app.open_menu_mut()
                {
                    m.scroll_notches(v);
                }
                Pointer::Taken
            }
            _ => Pointer::Pass,
        }
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, _: (i32, i32)) {
        if let Some(p) = app.tray_menu.as_mut().filter(|p| p.win.owns(surface)) {
            p.win.configure();
            return;
        }
        let Some(h) = [&mut app.panel, &mut app.outgoing].into_iter().flatten().find(|h| h.win.owns(surface)) else { return };
        h.win.configure();
        app.panel_dirty = true;
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, now: Instant) {
        if let Some(p) = app.tray_menu.as_mut().filter(|p| p.win.owns(surface)) {
            p.win.frame(now);
            app.sync_join();
            return;
        }
        let theme = &app.store.theme.theme;
        let h = if app.panel.as_ref().is_some_and(|p| p.win.owns(surface)) { &mut app.panel } else { &mut app.outgoing };
        let Some(h) = h else { return };
        let cut = h.win.frame(now) && h.mapped(theme, now);
        // The card that took this one's place is moving: the
        // outgoing window goes on this tick, while the two still
        // stand exactly on top of each other.
        if cut && let Some(old) = &mut app.outgoing {
            old.handed = true;
        }
        app.panel_dirty = true;
        app.sync_join();
    }

    fn closed(&self, app: &mut App, surface: &WlSurface) {
        if app.panel.as_ref().is_some_and(|p| p.win.owns(surface)) {
            app.panel = None;
            app.sync_join();
        } else if app.tray_menu.as_ref().is_some_and(|p| p.win.owns(surface)) {
            app.tray_menu = None;
        } else {
            app.outgoing = None;
            app.sync_join();
        }
    }

    fn finished(&self, app: &mut App, now: Instant) {
        if let Some(p) = app.panel.take_if(|p| p.finished(now)) {
            app.dropped.push(p.win.surface.layer.wl_surface().clone());
            app.sync_join();
            app.sync_bar_open(now);
            crate::trace::mapped(crate::trace::Surface::Panel(None), crate::trace::State::Unmapped);
        }
        if let Some(p) = app.outgoing.take_if(|p| p.finished(now)) {
            app.dropped.push(p.win.surface.layer.wl_surface().clone());
            app.sync_join();
        }
        if app.tray_menu.as_ref().is_some_and(|p| p.win.finished(now)) {
            app.tray_menu = None;
            crate::trace::mapped(crate::trace::Surface::TrayMenu, crate::trace::State::Unmapped);
        }
    }

    fn present(&self, app: &mut App, now: Instant) {
        let qh = app.qh.clone();
        app.step_menu(now);
        if let Some(p) = &mut app.tray_menu {
            p.layout(&mut app.kit);
            let morphing = p.menu.as_ref().is_some_and(|m| m.morph.running(now));
            let animating = p.win.animating(now) || morphing;
            p.win.present(animating, &qh);
        }
        let strip = app.bar_input();
        let theme = &app.store.theme.theme;
        for h in [&mut app.outgoing, &mut app.panel].into_iter().flatten() {
            if h.prime_until.is_some_and(|t| t <= now) {
                h.primed();
            }
            let animating = h.animating(now) || h.content_animating(now);
            if app.panel_dirty || animating {
                h.sync_region(&app.compositor, &strip);
                h.layout(&app.store, theme, &mut app.kit, now);
            }
            let animating = h.animating(now) || h.content_animating(now);
            h.win.surface.looping = !h.settling(now);
            h.win.present(animating, &qh);
        }
        app.panel_dirty = false;
    }

    /// The hosts' keyboard primes and their content's own clocks, and the
    /// Spaces peek's, which opens and closes the panel.
    fn wake(&self, app: &App, now: Instant) -> Option<Instant> {
        [&app.panel, &app.outgoing]
            .into_iter()
            .flatten()
            .flat_map(|h| [h.prime_until, h.wake.filter(|w| *w > now)])
            .chain([app.preview_deadline()])
            .flatten()
            .min()
    }

    fn rescaled(&self, app: &mut App) {
        app.panel_dirty = true;
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("panel".into(), app.panel.as_ref().map_or(Value::Null, |h| h.report()));
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.panel.as_ref().map(|p| p.win.surface.report()));
        out.extend(app.outgoing.as_ref().map(|p| p.win.surface.report()));
        out.extend(app.tray_menu.as_ref().map(|p| p.win.surface.report()));
    }
}

struct Launcher;

impl Owner for Launcher {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.launcher_part(surface).map(Target::Launcher)
    }

    fn reads(&self, app: &App, topic: Topic) -> bool {
        app.launcher_reads(topic)
    }

    fn changed(&self, app: &mut App, topic: Topic) {
        app.launcher_changed(topic);
    }

    fn key(&self, app: &mut App, event: &KeyEvent, repeat: bool) -> bool {
        app.launcher_key(event, repeat)
    }

    fn paste(&self, app: &mut App, text: &str) -> bool {
        app.launcher_paste(text)
    }

    /// Its card's events. A scrim's go on to the strip's handling.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        if app.launcher_pointer(e, target) { Pointer::Taken } else { Pointer::Pass }
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        if let Some(part) = app.launcher_part(surface) {
            app.launcher_configure(part, width, height);
        }
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, now: Instant) {
        if let Some(part) = app.launcher_part(surface) {
            app.launcher_frame(part, now);
        }
    }

    /// Its card closed takes the scrims with it.
    fn closed(&self, app: &mut App, surface: &WlSurface) {
        if app.launcher_part(surface) == Some(surfaces::modal::Part::Card) {
            app.launcher_closed();
        }
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_launcher(now);
    }

    fn rescaled(&self, app: &mut App) {
        app.launcher.dirty = true;
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.launch.win.as_ref().map(|w| w.shown.modal.win.surface.report()));
    }
}

struct Atlas;

impl Owner for Atlas {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.atlas_part(surface).map(Target::Atlas)
    }

    /// Its inbox, the radio it plays, and the theme.
    fn reads(&self, _: &App, topic: Topic) -> bool {
        matches!(topic, Topic::Atlas | Topic::Media | Topic::Theme)
    }

    fn changed(&self, app: &mut App, topic: Topic) {
        app.atlas_changed(topic);
    }

    fn key(&self, app: &mut App, event: &KeyEvent, _: bool) -> bool {
        app.atlas_key(event)
    }

    fn paste(&self, app: &mut App, text: &str) -> bool {
        app.atlas_paste(text)
    }

    fn pointer(&self, app: &mut App, e: &PointerEvent, _: Option<Target>) -> Pointer {
        if app.atlas_pointer(e) { Pointer::Taken } else { Pointer::Pass }
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        if let Some(part) = app.atlas_part(surface) {
            app.atlas_configure(part, width, height);
        }
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, now: Instant) {
        if let Some(part) = app.atlas_part(surface) {
            app.atlas_frame(part, now);
        }
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.atlas_closed();
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_atlas(now);
    }

    fn wake(&self, app: &App, _: Instant) -> Option<Instant> {
        app.atlas_deadline()
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.atlas.win.as_ref().map(|w| w.shown.modal.win.surface.report()));
    }
}

struct Switcher;

impl Owner for Switcher {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.switcher.owns(surface).then_some(Target::Switcher)
    }

    /// The focused window whether or not it is up; while it is, every
    /// change, since its cells show windows, icons and titles off many
    /// slices.
    fn reads(&self, app: &App, topic: Topic) -> bool {
        topic == Topic::Compositor || app.switcher.model.open() || app.switcher.win.is_some()
    }

    fn changed(&self, app: &mut App, topic: Topic) {
        app.switcher_changed(topic);
    }

    fn key(&self, app: &mut App, event: &KeyEvent, _: bool) -> bool {
        app.switcher_key(event.keysym)
    }

    /// A press on the card closes it; its other events go on to the strip's
    /// handling.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        if target != Some(Target::Switcher) || !matches!(e.kind, PointerEventKind::Press { .. }) {
            return Pointer::Pass;
        }
        app.switcher_close();
        Pointer::Taken
    }

    fn configure(&self, app: &mut App, _: &WlSurface, (width, height): (i32, i32)) {
        app.switcher_configure(width, height);
    }

    fn frame(&self, app: &mut App, _: &WlSurface, _: Instant) {
        app.switcher_frame();
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.switcher_close();
    }

    fn present(&self, app: &mut App, _: Instant) {
        app.switcher_present();
    }

    fn wake(&self, app: &App, _: Instant) -> Option<Instant> {
        app.switcher_deadline()
    }

    fn rescaled(&self, app: &mut App) {
        app.switcher.dirty = true;
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.switcher.win.as_ref().map(|(s, _)| s.report()));
    }
}

struct Overview;

impl Owner for Overview {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.overview.owns(surface).then_some(Target::Overview)
    }

    /// Every change while it is open.
    fn reads(&self, app: &App, _: Topic) -> bool {
        app.overview.model.open()
    }

    fn changed(&self, app: &mut App, topic: Topic) {
        app.overview_changed(topic);
    }

    fn key(&self, app: &mut App, event: &KeyEvent, _: bool) -> bool {
        app.overview_key(event)
    }

    fn pointer(&self, app: &mut App, e: &PointerEvent, _: Option<Target>) -> Pointer {
        if app.overview_pointer(e) { Pointer::Taken } else { Pointer::Pass }
    }

    fn configure(&self, app: &mut App, _: &WlSurface, (width, height): (i32, i32)) {
        app.overview_configure(width, height);
    }

    fn frame(&self, app: &mut App, _: &WlSurface, _: Instant) {
        app.overview_frame();
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.overview_closed();
    }

    fn present(&self, app: &mut App, _: Instant) {
        app.overview_present();
    }

    fn wake(&self, app: &App, _: Instant) -> Option<Instant> {
        app.overview_deadline()
    }

    fn rescaled(&self, app: &mut App) {
        app.overview.dirty = true;
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("overview".into(), app.overview_status());
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.overview.win.as_ref().map(|(s, _, _)| s.report()));
    }
}

/// The screensaver's overlays, one per output not under the lock.
struct Saver;

impl Owner for Saver {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.saver_index(surface).map(|_| Target::Saver)
    }

    /// Its runs' frames, the idle state, the media guard and its settings.
    fn reads(&self, _: &App, topic: Topic) -> bool {
        matches!(topic, Topic::Screensaver | Topic::Caffeinate | Topic::Media | Topic::Config)
    }

    fn changed(&self, app: &mut App, topic: Topic) {
        match topic {
            Topic::Screensaver => app.saver_changed(),
            Topic::Caffeinate => app.saver_idle_changed(),
            Topic::Media => app.saver_update(),
            _ => app.saver_config(),
        }
    }

    /// A key pressed while it runs over an unlocked session dismisses it and
    /// goes no further; a repeat is not a press. Under the lock the lock's
    /// own key does it.
    fn key(&self, app: &mut App, _: &KeyEvent, repeat: bool) -> bool {
        if repeat || !app.saver.active || app.lock.locked {
            return false;
        }
        app.saver_input(None, SaverInput::Key);
        true
    }

    fn pointer(&self, app: &mut App, e: &PointerEvent, _: Option<Target>) -> Pointer {
        let Some(i) = app.saver_index(&e.surface) else { return Pointer::Pass };
        let input = match e.kind {
            PointerEventKind::Enter { serial } => match app.pointer.clone() {
                Some(p) => SaverInput::Enter(p, serial, e.position),
                None => return Pointer::Taken,
            },
            PointerEventKind::Motion { .. } => SaverInput::Motion(e.position),
            PointerEventKind::Press { .. } => SaverInput::Press,
            _ => return Pointer::Taken,
        };
        app.saver_input(Some(i), input);
        Pointer::Taken
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        app.saver_configure(surface, width, height);
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, _: Instant) {
        app.saver_frame_callback(surface);
    }

    fn closed(&self, app: &mut App, surface: &WlSurface) {
        app.saver_closed(surface);
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_saver(now);
    }

    fn output_gone(&self, app: &mut App, output: &WlOutput) {
        app.saver_output_gone(output);
    }
}

struct Tooltip;

impl Owner for Tooltip {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.tip_card.as_ref().is_some_and(|c| c.surface.layer.wl_surface() == surface).then_some(Target::Tooltip)
    }

    fn configure(&self, app: &mut App, _: &WlSurface, _: (i32, i32)) {
        let Some(c) = &mut app.tip_card else { return };
        let size = c.scene.size;
        c.surface.configure(size.w, size.h);
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        let theme = &app.store.theme.theme;
        let Some(c) = &mut app.tip_card else { return };
        if c.surface.landed(now) {
            c.mapped(theme, now);
        }
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.tip_card = None;
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_tooltip(now);
    }

    fn wake(&self, app: &App, _: Instant) -> Option<Instant> {
        app.tips.wake()
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.tip_card.as_ref().map(|c| c.surface.report()));
    }
}

/// The toast stack, and the notification slice's own clock behind it.
struct Toasts;

impl Owner for Toasts {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.toasts.as_ref().is_some_and(|t| t.surface.layer.wl_surface() == surface).then_some(Target::Toasts)
    }

    fn reads(&self, _: &App, topic: Topic) -> bool {
        matches!(topic, Topic::Theme | Topic::Notifications | Topic::Activity)
    }

    /// A theme casts the column's shadow again.
    fn changed(&self, app: &mut App, topic: Topic) {
        if topic == Topic::Theme {
            app.toast_cast = None;
        }
        app.toasts_dirty = true;
    }

    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        if app.toasts_pointer(e, target) { Pointer::Taken } else { Pointer::Pass }
    }

    fn configure(&self, app: &mut App, _: &WlSurface, _: (i32, i32)) {
        let Some(t) = &mut app.toasts else { return };
        let size = t.scene.size;
        t.surface.configure(size.w, size.h);
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        let Some(t) = &mut app.toasts else { return };
        if t.surface.landed(now) {
            app.toasts_dirty = true;
        }
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.toasts = None;
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_toasts(now);
    }

    fn wake(&self, app: &App, now: Instant) -> Option<Instant> {
        let relative = app.toasts.as_ref().map(|t| t.rel_at + surfaces::toasts::REL_EVERY);
        [app.store.notifications.wake(now), relative].into_iter().flatten().min()
    }

    fn rescaled(&self, app: &mut App) {
        app.toasts_dirty = true;
    }

    /// What every toast said, and where each card on the stack sits.
    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        let cards = app.toast_cards().into_iter().map(|(key, r)| json!({"key": key, "rect": r})).collect();
        dump.insert("toasts".into(), app.store.notifications.tiers());
        dump.insert("toastCards".into(), cards);
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.toasts.as_ref().map(|t| t.surface.report()));
    }
}

struct Osd;

impl Owner for Osd {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.osd_owns(surface).then_some(Target::Osd)
    }

    /// The default sink's volume and mute, which it shows when they move.
    fn reads(&self, _: &App, topic: Topic) -> bool {
        DEVICES.contains(&topic)
    }

    fn changed(&self, app: &mut App, _: Topic) {
        app.osd_audio();
    }

    fn configure(&self, app: &mut App, _: &WlSurface, _: (i32, i32)) {
        app.osd_configure();
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        app.osd_frame(now);
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.osd.pill = None;
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.osd_present(now);
    }

    fn wake(&self, app: &App, _: Instant) -> Option<Instant> {
        app.osd_wake()
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.osd.pill.as_ref().map(|p| p.win.surface.report()));
    }
}

/// The headset connect card.
struct Headset;

impl Owner for Headset {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.headset_owns(surface).then_some(Target::Headset)
    }

    fn reads(&self, _: &App, topic: Topic) -> bool {
        DEVICES.contains(&topic)
    }

    fn changed(&self, app: &mut App, _: Topic) {
        app.headset_devices();
    }

    /// Its card's events, which go on to the strip's handling for the hover
    /// and the hand.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        app.headset_pointer(target, &e.kind);
        Pointer::Pass
    }

    fn configure(&self, app: &mut App, _: &WlSurface, _: (i32, i32)) {
        app.headset_configure();
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        app.headset_frame(now);
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.headset.card = None;
        app.sync_join();
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.headset_present(now);
    }

    fn wake(&self, app: &App, _: Instant) -> Option<Instant> {
        app.headset.wake()
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("headset".into(), app.headset_status());
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.headset.card.as_ref().map(|c| c.popup.win.surface.report()));
    }
}

/// The power profile pill beside the Settings tiles.
struct Profiles;

impl Owner for Profiles {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.profiles_owns(surface).then_some(Target::Profiles)
    }

    /// The launcher's card covers the pill, so the pill takes the card's
    /// events over itself. Its own surface's go on to the strip's handling
    /// for the hover and the hand.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        if target == Some(Target::Launcher(Part::Card))
            && let Some(at) = app.profiles_under(e.position)
        {
            app.profiles_pointer(Some(Target::Profiles), &e.kind, at);
            return Pointer::Taken;
        }
        if target == Some(Target::Profiles) {
            app.profiles_pointer(target, &e.kind, e.position);
        } else if !app.overview.owns(&e.surface) {
            // The overview comes later in this order for its keys, but its
            // own surface's events never reach the pill's.
            app.profiles_release_elsewhere(&e.kind);
        }
        Pointer::Pass
    }

    fn configure(&self, app: &mut App, _: &WlSurface, _: (i32, i32)) {
        app.profiles_configure();
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        app.profiles_frame(now);
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.profiles = None;
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.profiles_present(now);
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("profilesPill".into(), app.profiles_status());
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.profiles.as_ref().map(|p| p.popup.win.surface.report()));
    }
}

/// The dock and the strip that brings it out.
struct Dock;

impl Owner for Dock {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        if app.dock_owns(surface) {
            Some(Target::Dock)
        } else {
            app.dock_strip_owns(surface).then_some(Target::DockEdge)
        }
    }

    /// The strip's events end here; the card's go on to the shared hover
    /// for its tooltip and the hand.
    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        match target {
            Some(Target::DockEdge) => {
                app.dock_strip_pointer(&e.kind);
                Pointer::Taken
            }
            Some(Target::Dock) => {
                app.dock_pointer(&e.kind, e.position);
                Pointer::Pass
            }
            _ => Pointer::Pass,
        }
    }

    fn reads(&self, _: &App, topic: Topic) -> bool {
        matches!(topic, Topic::Compositor | Topic::State | Topic::Config | Topic::Activity | Topic::AppIcon | Topic::AppMenu | Topic::Display)
    }

    fn changed(&self, app: &mut App, _: Topic) {
        app.dock_sync();
        app.dock_menu_changed();
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, size: (i32, i32)) {
        if app.dock_strip_owns(surface) {
            app.dock_strip_configure(size.0);
        } else {
            app.dock_configure();
        }
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        app.dock_frame(now);
    }

    fn closed(&self, app: &mut App, surface: &WlSurface) {
        if app.dock_owns(surface) {
            app.dock_closed();
        }
    }

    fn output_gone(&self, app: &mut App, output: &WlOutput) {
        app.dock_output_gone(output);
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.dock_present(now);
    }

    fn wake(&self, app: &App, _: Instant) -> Option<Instant> {
        app.dock_wake()
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("dock".into(), app.dock_status());
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.dock.card.as_ref().map(|d| d.popup.win.surface.report()));
    }
}

/// `debug join`'s card.
struct JoinPreview;

impl Owner for JoinPreview {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.join_preview.as_ref().is_some_and(|(s, _, _)| s.layer.wl_surface() == surface).then_some(Target::Preview)
    }

    fn configure(&self, app: &mut App, _: &WlSurface, _: (i32, i32)) {
        let Some((s, scene, _)) = &mut app.join_preview else { return };
        s.configure(scene.size.w, scene.size.h);
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        let Some((s, _, _)) = &mut app.join_preview else { return };
        s.landed(now);
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.join_preview = None;
    }

    fn present(&self, app: &mut App, _: Instant) {
        let qh = app.qh.clone();
        if let Some((surface, scene, _)) = &mut app.join_preview {
            surface.present(scene, false, &qh);
        }
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.join_preview.as_ref().map(|(s, _, _)| s.report()));
    }
}

/// The live bar's frame zones.
struct Zones;

impl Owner for Zones {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        live!(app).zones.iter().position(|(_, z)| z.layer.wl_surface() == surface).map(Target::Zone)
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        if let Some((_, z)) = live!(app).zones.iter_mut().find(|(_, z)| z.layer.wl_surface() == surface) {
            z.configure(width.max(1), height.max(1));
        }
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, _: Instant) {
        if let Some((_, z)) = live!(app).zones.iter_mut().find(|(_, z)| z.layer.wl_surface() == surface) {
            z.landed();
        }
    }

    fn present(&self, app: &mut App, _: Instant) {
        let qh = app.qh.clone();
        for (_, z) in &mut live!(app).zones {
            z.present(0.0, false, &qh);
        }
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(live!(app).zones.iter().map(|(_, z)| z.report()));
    }
}

struct Scrim;

impl Owner for Scrim {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.scrim.as_ref().is_some_and(|(_, s)| s.layer.wl_surface() == surface).then_some(Target::Scrim)
    }

    fn configure(&self, app: &mut App, _: &WlSurface, (width, height): (i32, i32)) {
        let Some((_, s)) = &mut app.scrim else { return };
        s.configure(width, height);
    }

    fn frame(&self, app: &mut App, _: &WlSurface, now: Instant) {
        let Some((scrim, s)) = &mut app.scrim else { return };
        s.landed();
        scrim.mapped(now);
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.scrim = None;
    }

    fn finished(&self, app: &mut App, now: Instant) {
        if app.scrim.as_ref().is_some_and(|(p, s)| s.mapped() && p.finished(now)) {
            app.scrim = None;
            crate::trace::mapped(crate::trace::Surface::Scrim, crate::trace::State::Unmapped);
        }
    }

    fn present(&self, app: &mut App, now: Instant) {
        let qh = app.qh.clone();
        if let Some((scrim, surface)) = &mut app.scrim {
            surface.present(scrim.fade(now), scrim.animating(now), &qh);
        }
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.scrim.as_ref().map(|(_, s)| s.report()));
    }
}

/// The capture region picker, one surface per output.
struct Region;

impl Owner for Region {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.picker_owner(surface).map(Target::Picker)
    }

    fn key(&self, app: &mut App, event: &KeyEvent, _: bool) -> bool {
        app.picker_key_event(event)
    }

    fn paste(&self, app: &mut App, text: &str) -> bool {
        app.picker_paste(text)
    }

    fn pointer(&self, app: &mut App, e: &PointerEvent, target: Option<Target>) -> Pointer {
        let Some(Target::Picker(i)) = target else { return Pointer::Pass };
        app.picker_pointer(i, e);
        Pointer::Taken
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        if let Some(i) = app.picker_owner(surface) {
            app.picker_configure(i, width, height);
        }
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, _: Instant) {
        if let Some(i) = app.picker_owner(surface) {
            app.picker_frame(i);
        }
    }

    fn closed(&self, app: &mut App, surface: &WlSurface) {
        if let Some(i) = app.picker_owner(surface) {
            app.picker_closed(i);
        }
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_picker(now);
    }

    fn rescaled(&self, app: &mut App) {
        for p in &mut app.pickers.surfaces {
            p.dirty = true;
        }
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.pickers.surfaces.iter().map(|p| p.surface.report()));
    }
}

/// The polkit agent's dialog.
struct Polkit;

impl Owner for Polkit {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.polkit_owns(surface).then_some(Target::Polkit)
    }

    fn reads(&self, _: &App, topic: Topic) -> bool {
        topic == Topic::Theme
    }

    fn changed(&self, app: &mut App, _: Topic) {
        if let Some(d) = &mut app.polkit {
            d.dirty = true;
        }
    }

    fn key(&self, app: &mut App, event: &KeyEvent, _: bool) -> bool {
        app.polkit_key(event)
    }

    fn paste(&self, app: &mut App, text: &str) -> bool {
        app.polkit_paste(text)
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        app.polkit_configure(surface, width, height);
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, _: Instant) {
        app.polkit_frame(surface);
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_polkit(now);
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.polkit.as_ref().map(|d| d.modal.win.surface.report()));
    }
}

/// A plugin's overlay card.
struct Overlay;

impl Owner for Overlay {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.overlay_owns(surface).then_some(Target::Overlay)
    }

    /// What its plugin prints, the plugin list and the theme.
    fn reads(&self, _: &App, topic: Topic) -> bool {
        matches!(topic, Topic::Plugins | Topic::PluginOutput | Topic::Theme)
    }

    fn changed(&self, app: &mut App, _: Topic) {
        app.overlay_changed();
    }

    fn key(&self, app: &mut App, event: &KeyEvent, _: bool) -> bool {
        app.overlay_key(event)
    }

    fn paste(&self, app: &mut App, text: &str) -> bool {
        app.overlay_paste(text)
    }

    fn pointer(&self, app: &mut App, e: &PointerEvent, _: Option<Target>) -> Pointer {
        if app.overlay_pointer(e) { Pointer::Taken } else { Pointer::Pass }
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        app.overlay_configure(surface, width, height);
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, _: Instant) {
        app.overlay_frame(surface);
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_overlay(now);
    }

    fn counts(&self, app: &App, out: &mut Vec<String>) {
        out.extend(app.plugin_overlay.as_ref().map(|w| w.modal.win.surface.report()));
    }
}

/// The session lock's surfaces, one per output. Their configures come
/// through the session lock, not the layer shell.
struct Lock;

impl Owner for Lock {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.lock_owns(surface).then_some(Target::Lock)
    }

    /// Every change while a session lock is up, since what it shows reads
    /// across the store.
    fn reads(&self, app: &App, _: Topic) -> bool {
        app.lock_session_up()
    }

    fn changed(&self, app: &mut App, topic: Topic) {
        app.lock_changed(topic);
    }

    fn key(&self, app: &mut App, event: &KeyEvent, repeat: bool) -> bool {
        app.lock_key(event, repeat)
    }

    fn paste(&self, app: &mut App, text: &str) -> bool {
        app.lock_paste(text)
    }

    /// It watches its surfaces' events and passes them all on.
    fn pointer(&self, app: &mut App, e: &PointerEvent, _: Option<Target>) -> Pointer {
        app.lock_pointer(e);
        Pointer::Pass
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, _: Instant) {
        app.lock_frame(surface);
    }

    fn present(&self, app: &mut App, _: Instant) {
        app.present_lock();
    }

    fn rescaled(&self, app: &mut App) {
        app.lock.dirty = true;
    }

    fn output_gone(&self, app: &mut App, output: &WlOutput) {
        app.lock_output_gone(output);
    }
}

/// The veils over an arrival, a departure and the unlock's hand-over.
struct Veil;

impl Owner for Veil {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.veil_owns(surface).then_some(Target::Veil)
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, (width, height): (i32, i32)) {
        app.veil_configure(surface, width, height);
    }

    fn frame(&self, app: &mut App, surface: &WlSurface, _: Instant) {
        app.veil_frame(surface);
    }

    fn closed(&self, app: &mut App, surface: &WlSurface) {
        app.veil_closed(surface);
    }

    fn present(&self, app: &mut App, now: Instant) {
        app.present_veils(now);
    }

    fn wake(&self, app: &App, now: Instant) -> Option<Instant> {
        app.veil_deadline(now)
    }

    fn output_gone(&self, app: &mut App, output: &WlOutput) {
        app.veil_output_gone(output);
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("veil".into(), app.veil_phase().into());
    }
}

/// The idle inhibitor's clear surface.
struct Caffeinate;

impl Owner for Caffeinate {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.caffeinate_owns(surface).then_some(Target::Caffeinate)
    }

    fn reads(&self, _: &App, topic: Topic) -> bool {
        topic == Topic::Caffeinate
    }

    /// The toggle and the cookies share its one inhibitor surface.
    fn changed(&self, app: &mut App, _: Topic) {
        app.hold_inhibitor();
    }

    fn configure(&self, app: &mut App, _: &WlSurface, _: (i32, i32)) {
        app.caffeinate_configure();
    }
}

/// The panel surfaces the shell has dropped while the compositor still
/// routes the pointer to one (`Target::Outgoing`); not a window of the
/// lifecycle's, since nothing is left to present.
struct Outgoing;

impl Owner for Outgoing {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.dropped.contains(surface).then_some(Target::Outgoing)
    }

    /// A pointer entering a surface the saver and the corners passed on has
    /// left every dropped one.
    fn pointer(&self, app: &mut App, e: &PointerEvent, _: Option<Target>) -> Pointer {
        if matches!(e.kind, PointerEventKind::Enter { .. }) {
            app.dropped.clear();
        }
        Pointer::Pass
    }
}

struct HotCorners;

impl Owner for HotCorners {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.hot_corner_owns(surface).then_some(Target::HotCorner)
    }

    fn pointer(&self, app: &mut App, e: &PointerEvent, _: Option<Target>) -> Pointer {
        if app.hot_corner_pointer(e) { Pointer::Taken } else { Pointer::Pass }
    }

    fn configure(&self, app: &mut App, surface: &WlSurface, _: (i32, i32)) {
        app.hot_corner_configure(surface);
    }

    fn output_gone(&self, app: &mut App, output: &WlOutput) {
        app.hot_corner_output_gone(output);
    }
}

/// The line along the bar's edge over a fullscreen Mac window
/// (`reveal.rs`). Its events are read ahead of every owner, so here they
/// stop.
struct Reveal;

impl Owner for Reveal {
    fn owns(&self, app: &App, surface: &WlSurface) -> Option<Target> {
        app.reveal_owns(surface).then_some(Target::RevealEdge)
    }

    fn pointer(&self, _: &mut App, _: &PointerEvent, target: Option<Target>) -> Pointer {
        if target == Some(Target::RevealEdge) { Pointer::Taken } else { Pointer::Pass }
    }

    fn configure(&self, app: &mut App, _: &WlSurface, size: (i32, i32)) {
        app.reveal_configure(size);
    }

    fn closed(&self, app: &mut App, _: &WlSurface) {
        app.reveal_closed();
    }

    /// A card going away with the pointer off the bar starts the hold.
    fn present(&self, app: &mut App, _: Instant) {
        app.hold_reveal();
    }

    fn output_gone(&self, app: &mut App, output: &WlOutput) {
        app.reveal_output_gone(output);
    }

    fn report(&self, app: &App, dump: &mut Map<String, Value>) {
        dump.insert("reveal".into(), app.reveal_report());
    }
}
