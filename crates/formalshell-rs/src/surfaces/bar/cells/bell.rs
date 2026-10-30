//! Do-not-disturb and the pending count. A click opens or
//! shuts the notification centre, a right click flips DND.

use crate::store::Topic;
use crate::surfaces::panel::center;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, View};

#[derive(Default)]
pub struct Bell {
    dnd: bool,
    pending: usize,
    /// `bar.widgets.bell.showLabel`, on unless a user opts out: the count
    /// is this cell's only content past the glyph.
    label: bool,
}

impl Cell for Bell {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::State, Topic::Notifications, Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let n = &env.store.notifications.model;
        let label = env.store.config.bool("bar.widgets.bell.showLabel").unwrap_or(true);
        let next = (n.dnd, n.pending.len(), label);
        let changed = next != (self.dnd, self.pending, self.label);
        (self.dnd, self.pending, self.label) = next;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let tooltip = if self.dnd {
            "NOTIFICATIONS / DND ON".to_owned()
        } else if self.pending > 0 {
            format!("NOTIFICATIONS / {} PENDING", self.pending)
        } else {
            "NOTIFICATIONS / NONE PENDING".to_owned()
        };
        let mut parts = vec![Part::Icon { name: if self.dnd { "bell-off" } else { "bell" }.into(), dim: false, dot: self.pending > 0 }];
        if self.label && self.pending > 0 {
            parts.push(Part::Label { text: self.pending.to_string(), weight: None });
        }
        View::new(parts, look.xxs).panel(center::ID).tooltip(tooltip)
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right => Action::Dnd(!self.dnd),
            _ => Action::Center,
        }
    }
}
