//! The quake console: one terminal
//! that drops down over the current workspace and parks on the special
//! workspace again with its session still running. Visibility is read off
//! the compositor, never stored, so a restarted shell adopts the console
//! already running.
//!
//! The special workspace is Hyprland's `special:formalshell-console`.
//!
//! The two waits poll because nothing signals them: a freshly spawned
//! window carries no id this shell knows until the compositor reports it
//! again, and Hyprland sends no event when a resize lands. Both are bounded
//! and run only while a show is in flight.

use std::time::Duration;

use calloop::RegistrationToken;
use calloop::timer::{TimeoutAction, Timer};
use fs_types::{Edge, IRect, Insets, WindowId};
use serde_json::json;

use super::App;
use crate::services::compositor::{self, Command};
use crate::settings;

const TICK: Duration = Duration::from_millis(100);

fn clamp_share(share: f64) -> f64 {
    if share.is_finite() { share.clamp(0.2, 1.0) } else { 0.5 }
}

/// The console's box: `screen` is the output's logical box.
pub fn geometry(screen: (f64, f64, f64, f64), bar: Insets, share: f64, margin: f64) -> Option<IRect> {
    let (sx, sy, sw, sh) = screen;
    if sw <= 0.0 || sh <= 0.0 {
        return None;
    }
    let gap = margin.round().max(0.0);
    let (top, bottom, left, right) = (bar.top.max(0.0), bar.bottom.max(0.0), bar.left.max(0.0), bar.right.max(0.0));
    let usable = sh - top - bottom - gap;
    Some(IRect::new(
        (sx + left + gap).round() as i32,
        (sy + top + gap).round() as i32,
        (sw - left - right - gap * 2.0).round().max(1.0) as i32,
        ((usable * clamp_share(share)).round() - gap).max(1.0) as i32,
    ))
}

struct Placing {
    id: WindowId,
    target: IRect,
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
        settings::CONSOLE_APP_ID.get(&self.store.config).to_owned()
    }

    fn console_command(&self) -> Vec<String> {
        settings::console_command(&self.store.config)
    }

    fn console_window_id(&self, app_id: &str) -> WindowId {
        let windows = &self.store.compositor.snapshot.windows;
        windows.iter().find(|w| w.app_id == app_id).map(|w| w.id.clone()).unwrap_or_default()
    }

    fn console_available(&self) -> bool {
        self.store.compositor.snapshot.available
    }

    fn console_showing(&self) -> bool {
        let id = self.console_window_id(&self.console_app_id());
        !id.is_empty() && !compositor::is_parked(&self.store.compositor.snapshot, &id)
    }

    pub fn console_status(&self) -> serde_json::Value {
        let app_id = self.console_app_id();
        json!({
            "available": self.console_available(),
            "appId": app_id,
            "windowId": self.console_window_id(&app_id),
            "visible": self.console_showing(),
            "spawning": self.console.spawning,
        })
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

    /// Runs `script` in a console window of its own,
    /// under `<appId>.run` so it never becomes the quake console.
    pub fn console_run_once(&mut self, script: &str) {
        use fs_info::notifications::Urgency;
        let fail = |app: &mut Self, body: &str| {
            app.apply(crate::services::notifications::Op::Notify("CONSOLE UNAVAILABLE".into(), body.into(), Urgency::Normal));
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
        let c = self.chrome();
        Insets { top: c.inset(Edge::Top), bottom: c.inset(Edge::Bottom), left: c.inset(Edge::Left), right: c.inset(Edge::Right) }
    }

    /// Sized where nobody can see it, then brought in: the other order
    /// shows the terminal at its own default size until the placement lands.
    fn console_reveal(&mut self, id: WindowId) {
        let share = settings::CONSOLE_SHARE.get(&self.store.config);
        let margin = self.store.theme.theme.space.md;
        let Some(target) = self.console_screen().and_then(|s| geometry(s, self.console_insets(), share, margin)) else {
            eprintln!("console: no output to place the console on");
            return;
        };
        let was_parked = compositor::is_parked(&self.store.compositor.snapshot, &id);
        compositor::send(Command::FloatWindow(id.clone()));
        self.console.attempts = 0;
        self.console.placing = Some(Placing { id, target, placed: false, placed_at: 0, was_parked });
        self.console_tick();
    }

    fn console_tick(&mut self) {
        if self.console.timer.is_some() {
            return;
        }
        let token = self.handle.insert_source(Timer::from_duration(TICK), |_, _, app: &mut App| {
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
                x: f64::from(t.x),
                y: f64::from(t.y),
                width: f64::from(t.w),
                height: f64::from(t.h),
            });
            return true;
        }
        let sized = rect.is_some_and(|r| r.w == p.target.w && r.h == p.target.h);
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
        assert_eq!(r, IRect::new(10, 50, 1900, 505));
    }

    #[test]
    fn share_is_clamped_and_a_bad_screen_places_nothing() {
        let r = geometry((100.0, 0.0, 1000.0, 1000.0), Insets::default(), 5.0, 0.0).unwrap();
        assert_eq!(r, IRect::new(100, 0, 1000, 1000));
        let r = geometry((0.0, 0.0, 1000.0, 1000.0), Insets::default(), 0.01, 0.0).unwrap();
        assert_eq!(r.h, 200);
        assert!(geometry((0.0, 0.0, 0.0, 1000.0), Insets::default(), 0.5, 0.0).is_none());
    }
}
