//! The focus the engine does not ask a window for: the output under the
//! pointer, and the window the compositor means the keyboard for when that
//! is not the one the model has.

use shojiwm_rs::prelude::*;

use crate::engine::{Engine, enter};

pub fn setup() {
    // The press is the client's too.
    for (id, button) in [
        ("formalwm:press-left", "MouseLeft"),
        ("formalwm:press-right", "MouseRight"),
        ("formalwm:press-middle", "MouseMiddle"),
    ] {
        let options = KeyBindOptions::new().non_consuming();
        COMPOSITOR.key.bind_with(id, button, options, || {
            enter(|engine| {
                if let Some(at) = COMPOSITOR.cursor.get_position() {
                    engine.point_at((at.x, at.y));
                }
                engine.settle_focus();
            })
        });
    }
}

impl Engine {
    /// The output under the pointer is the focused one: on the way across
    /// and at a press, which is how one with no window to press on is
    /// reached. The keyboard stays where it is. Not while something is
    /// dragged, which belongs to the output it left until it is dropped.
    pub fn point_at(&mut self, at: (f64, f64)) {
        if self.hold.data_drag || self.hold.window_drag || self.drag.is_some() {
            return;
        }
        if let Some(output) = self.output_at(at)
            && self.model.focus_output(&output)
        {
            self.commit();
        }
    }

    /// Looks at which window has the keyboard once the compositor has
    /// answered what this turn asked of it. A tick of its scheduler reports
    /// the keyboard, then runs the timers, then does what was asked, so
    /// the answer is in the report of the tick after the first one.
    pub fn settle_focus(&mut self) {
        if std::mem::replace(&mut self.focus_pending, true) {
            return;
        }
        set_timeout(1.0, || {
            set_timeout(1.0, || {
                enter(|engine| {
                    engine.focus_pending = false;
                    engine.follow_keyboard();
                })
            });
        });
    }

    /// The window the compositor means the keyboard for, which is none
    /// under the lock screen or a layer that takes the keyboard for itself.
    pub fn meant_for(&self) -> Option<String> {
        let focus = COMPOSITOR.input.keyboard_focus().get_untracked();
        focus.and_then(|focus| focus.focused_window_id)
    }

    /// That window is the focused one, whatever was asked: a request the
    /// compositor refused or answered with another window, a press on the
    /// window that had the keyboard all along. Not before the model's own
    /// window was asked for, and not for a window out of view, which the
    /// model does not show for it.
    fn follow_keyboard(&mut self) {
        if self.hold.data_drag || self.asked_focus.as_ref() != self.model.focused_window() {
            return;
        }
        let Some(id) = self.meant_for() else {
            return;
        };
        if self.asked_focus.as_deref() != Some(id.as_str())
            && self.model.in_view(&id)
            && let Some(win) = self.windows.get(&id)
        {
            self.focused(win.handle);
        }
    }
}
