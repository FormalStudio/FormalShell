//! The sidebar holding the keyboard (`sidebar focus`): its layer takes the
//! keyboard exclusively while the minimap's cursor is up, and gives it back
//! on Escape or Enter.

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;

use super::App;
use crate::services::compositor::{self, Command};
use crate::surfaces::sidebar::minimap::Move;

impl App {
    /// The minimap's cursor up with the keyboard, or both given back.
    /// False under the bar layout.
    pub fn sidebar_focus(&mut self, on: bool) -> bool {
        let Some(sb) = self.bar.sidebar.as_mut() else { return false };
        if on {
            sb.map.take_keyboard();
        } else {
            sb.map.cursor = None;
        }
        if let Some(s) = &self.bar_surface {
            s.layer.set_keyboard_interactivity(if on { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
            s.layer.commit();
        }
        self.bar_dirty = true;
        self.request_paint();
        true
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
        match sb.map.step(dx, dy, carry) {
            Some(Move::Swap(a, b)) => compositor::send(Command::SwapWindows { a, b }),
            Some(Move::To(id, idx)) => compositor::send(Command::MoveWindowTo { id, idx, output: self.bar.output.clone() }),
            None => {}
        }
        self.bar_dirty = true;
        self.request_paint();
        true
    }

    /// A minimap drop's commands, in order, onto the bar's output.
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
