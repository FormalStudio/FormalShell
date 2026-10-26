//! One network icon, dim while stopped. Hidden until
//! the first poll resolves with the CLI present. The tooltip words the poll
//! state the status belongs to, never a daemon state nobody observed.

use crate::services::info::tailscale::{Poll, State};
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, View};

pub struct Tailscale {
    want: Want,
    state: State,
}

impl Default for Tailscale {
    fn default() -> Self {
        Self { want: Want::new(Source::Tailscale), state: State::default() }
    }
}

impl Cell for Tailscale {
    fn visible(&mut self, on: bool) {
        self.want.hold(on);
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.info.tailscale.clone();
        let changed = state != self.state;
        self.state = state;
        changed
    }

    fn view(&self, look: &Look) -> View {
        if matches!(self.state.poll, Poll::Unknown | Poll::Missing) {
            return View::hidden();
        }
        let running = self.state.status.as_ref().is_some_and(|s| s.running);
        let tooltip = match self.state.poll {
            Poll::NeedsLogin => "TAILSCALE / NEEDS LOGIN",
            Poll::Error => "TAILSCALE / NO TAILSCALE",
            Poll::Ok if running => "TAILSCALE / CONNECTED",
            Poll::Ok => "TAILSCALE / STOPPED",
            _ => "TAILSCALE",
        };
        View::new(vec![Part::Icon { name: "network".into(), dim: !running, dot: false }], look.xxs)
            .panel("tailscale")
            .tooltip(tooltip)
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel("tailscale")
    }
}
