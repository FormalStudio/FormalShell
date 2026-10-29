//! The quake console: one terminal
//! that drops down over the current workspace and parks on the special
//! workspace again with its session still running. Visibility is read off
//! the compositor, never stored, so a restarted shell adopts the console
//! already running.
//!
//! The special workspace is Hyprland's `special:formalshell-console` and
//! mango's tag 0, the only special tag it has.
//!
//! The two waits poll because nothing signals them: a freshly spawned
//! window carries no id this shell knows until the compositor reports it
//! again, and Hyprland sends no event when a resize lands. Both are bounded
//! and run only while a show is in flight.

use std::time::Duration;

use calloop::RegistrationToken;
use calloop::timer::{TimeoutAction, Timer};
use fs_chrome::types::Edge;
use serde_json::{Value as Json, json};

use super::App;
use crate::services::compositor::{self, Command};

const TICK: Duration = Duration::from_millis(100);
const DEFAULT_APP_ID: &str = "dev.formalshell.console";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

fn clamp_share(share: f64) -> f64 {
    if share.is_finite() { share.clamp(0.2, 1.0) } else { 0.5 }
}

/// geometry.js's `consoleGeometry`: `screen` is the output's logical box.
pub fn geometry(screen: (f64, f64, f64, f64), bar: Insets, share: f64, margin: f64) -> Option<Rect> {
    let (sx, sy, sw, sh) = screen;
    if sw <= 0.0 || sh <= 0.0 {
        return None;
    }
    let gap = margin.round().max(0.0);
    let (top, bottom, left, right) = (bar.top.max(0.0), bar.bottom.max(0.0), bar.left.max(0.0), bar.right.max(0.0));
    let usable = sh - top - bottom - gap;
    Some(Rect {
        x: (sx + left + gap).round() as i64,
        y: (sy + top + gap).round() as i64,
        width: (sw - left - right - gap * 2.0).round().max(1.0) as i64,
        height: ((usable * clamp_share(share)).round() - gap).max(1.0) as i64,
    })
}

struct Placing {
    id: String,
    target: Rect,
    placed: bool,
    placed_at: u32,
    was_parked: bool,
}

#[derive(Default)]
pub struct Console {
    spawning: bool,
    await_app_id: String,
    attempts: u32,
    placing: Option<Placing>,
    timer: Option<RegistrationToken>,
}

impl App {
    fn console_app_id(&self) -> String {
        self.store.config.str("console.appId").unwrap_or(DEFAULT_APP_ID).to_owned()
    }

    fn console_command(&self) -> Vec<String> {
        match self.store.config.get("console.command") {
            Some(Json::Array(items)) => items.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect(),
            Some(_) => Vec::new(),
            None => vec!["ghostty".into(), format!("--class={DEFAULT_APP_ID}")],
        }
    }

    fn console_window_id(&self, app_id: &str) -> String {
        let windows = &self.store.compositor.snapshot.windows;
        windows.iter().find(|w| w.app_id == app_id).map(|w| w.id.clone()).unwrap_or_default()
    }

    fn console_available(&self) -> bool {
        self.store.compositor.snapshot.available
    }

    fn console_showing(&self) -> bool {
        let id = self.console_window_id(&self.console_app_id());
        !id.is_empty() && !compositor::is_window_parked(&self.store.compositor, &id)
    }

    pub fn console_status(&self) -> String {
        let app_id = self.console_app_id();
        json!({
            "available": self.console_available(),
            "appId": app_id,
            "windowId": self.console_window_id(&app_id),
            "visible": self.console_showing(),
            "spawning": self.console.spawning,
        })
        .to_string()
    }

    pub fn console_toggle(&mut self) {
        if self.console_showing() { self.console_hide() } else { self.console_show() }
    }

    pub fn console_show(&mut self) {
        if !self.console_available() {
            eprintln!("console: no compositor that can park and place a window");
            return;
        }
        let id = self.console_window_id(&self.console_app_id());
        if !id.is_empty() {
            self.console_reveal(id);
            return;
        }
        if self.console.spawning {
            return;
        }
        let argv = self.console_command();
        if argv.is_empty() {
            eprintln!("console: console.command is not set");
            return;
        }
        self.console.spawning = true;
        self.console.await_app_id = self.console_app_id();
        self.console.attempts = 0;
        compositor::spawn(&argv);
        self.console_tick();
    }

    /// ConsoleService.runOnce: `script` in a console window of its own,
    /// under `<appId>.run` so it never becomes the quake console.
    pub fn console_run_once(&mut self, script: &str) {
        use fs_info::notifications::Urgency;
        let fail = |app: &mut Self, body: &str| {
            app.store.notifications.notify("CONSOLE UNAVAILABLE", body, Urgency::Normal);
            crate::surfaces::changed(app, crate::store::Topic::Notifications);
        };
        if !self.console_available() {
            return fail(self, "this compositor cannot place a floating window");
        }
        if self.console.spawning {
            return;
        }
        let argv = self.console_command();
        if argv.is_empty() {
            return fail(self, "console.command is not set");
        }
        let app_id = self.console_app_id();
        let run_id = format!("{app_id}.run");
        let Some(spawn) = fs_system::console::one_off_argv(Some(&argv), &app_id, &run_id, script) else {
            return fail(self, &format!("console.command has to name {app_id}"));
        };
        self.console.spawning = true;
        self.console.await_app_id = run_id;
        self.console.attempts = 0;
        compositor::spawn(&spawn);
        self.console_tick();
    }

    pub fn console_hide(&mut self) {
        let id = self.console_window_id(&self.console_app_id());
        if id.is_empty() {
            return;
        }
        self.console.placing = None;
        compositor::send(Command::ParkWindow(id));
    }

    /// The focused output's logical box, or the first one's.
    fn console_screen(&self) -> Option<(f64, f64, f64, f64)> {
        let focused = &self.store.compositor.snapshot.focused_output_name;
        let infos: Vec<_> = self.outputs.outputs().filter_map(|o| self.outputs.info(&o)).collect();
        let info = infos.iter().find(|i| i.name.as_deref() == Some(focused.as_str())).or(infos.first())?;
        let (x, y) = info.logical_position.unwrap_or((0, 0));
        let (w, h) = info.logical_size?;
        Some((x as f64, y as f64, w as f64, h as f64))
    }

    fn console_insets(&self) -> Insets {
        let edge = self.bar.edge();
        let thickness = self.bar.thickness() as f64;
        let frame = if self.bar.framed() { self.bar.frame_thickness() } else { 0.0 };
        let inset = |e: Edge| if e == edge { thickness } else { frame };
        Insets { top: inset(Edge::Top), bottom: inset(Edge::Bottom), left: inset(Edge::Left), right: inset(Edge::Right) }
    }

    /// Sized where nobody can see it, then brought in: the other order
    /// shows the terminal at its own default size until the placement lands.
    fn console_reveal(&mut self, id: String) {
        let share = self.store.config.f64("console.share").unwrap_or(0.5);
        let margin = self.store.theme.theme.space.md;
        let Some(target) = self.console_screen().and_then(|s| geometry(s, self.console_insets(), share, margin)) else {
            eprintln!("console: no output to place the console on");
            return;
        };
        let was_parked = compositor::is_window_parked(&self.store.compositor, &id);
        compositor::send(Command::FloatWindow(id.clone()));
        self.console.attempts = 0;
        self.console.placing = Some(Placing { id, target, placed: false, placed_at: 0, was_parked });
        self.console_tick();
    }

    fn console_tick(&mut self) {
        if self.console.timer.is_some() {
            return;
        }
        let Some(handle) = &self.handle else { return };
        let token = handle.insert_source(Timer::from_duration(TICK), |_, _, app: &mut App| {
            if app.console_step() {
                TimeoutAction::ToDuration(TICK)
            } else {
                app.console.timer = None;
                TimeoutAction::Drop
            }
        });
        self.console.timer = token.ok();
    }

    /// One tick of whichever wait is running; false once neither is.
    fn console_step(&mut self) -> bool {
        compositor::send(Command::RefreshWindows);
        self.console.attempts += 1;
        let attempts = self.console.attempts;
        if self.console.spawning {
            let mapped = self.console_window_id(&self.console.await_app_id.clone());
            if !mapped.is_empty() {
                self.console.spawning = false;
                self.console_reveal(mapped);
                return true;
            }
            if attempts >= 50 {
                self.console.spawning = false;
                eprintln!("console: no window with app id {} opened in time", self.console.await_app_id);
                return false;
            }
            return true;
        }
        let Some(p) = &self.console.placing else { return false };
        let Some(win) = self.store.compositor.window(&p.id) else {
            if attempts >= 20 {
                self.console.placing = None;
                eprintln!("console: the console window went away while being placed");
                return false;
            }
            return true;
        };
        let rect = win.rect;
        let p = self.console.placing.as_mut().expect("placing");
        // The placement is absolute, so a backend with no rect
        // still gets placed after half a second rather than never.
        if !p.placed && (rect.is_some() || attempts >= 5) {
            p.placed = true;
            p.placed_at = attempts;
            let t = p.target;
            compositor::send(Command::PlaceFloatingWindow {
                id: p.id.clone(),
                x: t.x as f64,
                y: t.y as f64,
                width: t.width as f64,
                height: t.height as f64,
            });
            return true;
        }
        let sized = rect.is_some_and(|r| r.width == p.target.width && r.height == p.target.height);
        let unverifiable = rect.is_none() && attempts - p.placed_at >= 3;
        if p.placed && (sized || unverifiable || attempts >= 20) {
            if !sized && !unverifiable {
                eprintln!("console: the console did not settle in time, showing it anyway");
            }
            let p = self.console.placing.take().expect("placing");
            if p.was_parked {
                compositor::send(Command::UnparkWindow(p.id.clone()));
            }
            compositor::focus_window(&p.id);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_from_under_the_bar_over_its_share() {
        let bar = Insets { top: 40.0, ..Insets::default() };
        let r = geometry((0.0, 0.0, 1920.0, 1080.0), bar, 0.5, 10.0).unwrap();
        assert_eq!(r, Rect { x: 10, y: 50, width: 1900, height: 505 });
    }

    #[test]
    fn share_is_clamped_and_a_bad_screen_places_nothing() {
        let r = geometry((100.0, 0.0, 1000.0, 1000.0), Insets::default(), 5.0, 0.0).unwrap();
        assert_eq!(r, Rect { x: 100, y: 0, width: 1000, height: 1000 });
        let r = geometry((0.0, 0.0, 1000.0, 1000.0), Insets::default(), 0.01, 0.0).unwrap();
        assert_eq!(r.height, 200);
        assert!(geometry((0.0, 0.0, 0.0, 1000.0), Insets::default(), 0.5, 0.0).is_none());
    }
}
