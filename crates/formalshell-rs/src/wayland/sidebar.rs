//! The sidebar holding the keyboard (`sidebar focus`): its layer takes the
//! keyboard exclusively while the minimap's cursor is up, and gives it back
//! on Escape or Enter.

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;

use super::App;
use crate::services::compositor::{self, Command};

impl App {
    /// The minimap's cursor up with the keyboard, or both given back.
    /// False under the bar layout. Taking it closes the launcher and the
    /// panels, which would otherwise keep the keys.
    pub fn sidebar_focus(&mut self, on: bool) -> bool {
        if self.bar.sidebar.is_none() {
            return false;
        }
        if on {
            if self.launcher.open {
                self.menu_close();
            }
            self.close_panels();
        }
        let Some(sb) = self.bar.sidebar.as_mut() else { return false };
        if on {
            sb.map.take_keyboard();
        } else {
            sb.map.cursor = None;
        }
        self.sync_sidebar_keys();
        self.bar_dirty = true;
        self.request_paint();
        true
    }

    /// The live bar's layer holds the keyboard exactly while the minimap's
    /// cursor is up. A bar re-created (collapse, a new edge) takes it over;
    /// one rebuilt has no cursor and lets go; a bar hidden under fullscreen
    /// gives the cursor up with its layer.
    pub(super) fn sync_sidebar_keys(&mut self) {
        let current = self.bar_surface.as_ref().map(|s| s.layer.wl_surface().clone());
        if current.is_none()
            && let Some(sb) = self.bar.sidebar.as_mut()
        {
            sb.map.cursor = None;
        }
        let want = current.is_some() && self.bar.sidebar.as_ref().is_some_and(|sb| sb.map.cursor.is_some());
        let have = self.sidebar_keys.is_some() && self.sidebar_keys == current;
        if want != have
            && let Some(s) = &self.bar_surface
        {
            s.layer.set_keyboard_interactivity(if want { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
            s.layer.commit();
        }
        self.sidebar_keys = if want { current } else { None };
    }

    /// A key while the sidebar holds the keyboard; false when it does not.
    pub(super) fn sidebar_key(&mut self, event: &KeyEvent) -> bool {
        let Some(sb) = self.bar.sidebar.as_mut().filter(|sb| sb.map.cursor.is_some()) else { return false };
        let carry = self.mods.shift;
        let (dx, dy) = match event.keysym {
            Keysym::Left => (-1, 0),
            Keysym::Right => (1, 0),
            Keysym::Up => (0, -1),
            Keysym::Down => (0, 1),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let action = sb.map.enter(&self.bar.output);
                self.sidebar_focus(false);
                let anchor = 0.0;
                self.act(action, anchor);
                return true;
            }
            Keysym::Escape => {
                self.sidebar_focus(false);
                return true;
            }
            _ => return true,
        };
        let moved = sb.map.step(dx, dy, carry);
        self.minimap_drop(moved.into_iter().collect());
        true
    }

    /// A minimap move's commands, in order, onto the bar's output.
    pub(super) fn minimap_drop(&mut self, commands: Vec<Command>) {
        for command in commands {
            compositor::send(match command {
                Command::MoveWindowTo { id, idx, .. } => Command::MoveWindowTo { id, idx, output: self.bar.output.clone() },
                other => other,
            });
        }
        self.bar_dirty = true;
        self.request_paint();
    }
}
