//! Activity's bar cell, first in the Indicators rail: the lead group's ring
//! and title, and `+N` for the groups behind it. Hidden while nothing is
//! live, so it measures once and never commits at idle. A failure leads,
//! then running work, then what just finished. A ring with
//! progress repaints only when a stream line moves it, and a ring with
//! none turns in place. A click opens the Activity panel.

use crate::store::Topic;
use crate::surfaces::activity;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Tone, View};
use crate::ui::el::Ring;

#[derive(Clone, Debug, Default, PartialEq)]
struct Shown {
    ring: Option<Ring>,
    title: String,
    detail: String,
    others: usize,
    failed: bool,
}

#[derive(Default)]
pub struct Activity {
    shown: Shown,
}

impl Cell for Activity {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Activity]
    }

    fn read(&mut self, env: &Env) -> bool {
        let groups = env.store.activity.activity.groups();
        let rank = |g: &fs_activity::Group| match g.state() {
            fs_activity::State::Failed => 0,
            fs_activity::State::Running => 1,
            _ => 2,
        };
        let shown = groups.iter().min_by_key(|g| rank(g)).map_or_else(Shown::default, |g| {
            let lead = g.lead();
            Shown {
                ring: Some(activity::ring(lead)),
                title: lead.title.clone(),
                detail: activity::detail(lead),
                others: groups.len() - 1,
                failed: g.state() == fs_activity::State::Failed,
            }
        });
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let s = &self.shown;
        let Some(ring) = s.ring else { return View::hidden() };
        let mut parts = vec![Part::Ring(ring), Part::Name { text: s.title.clone(), max: look.narrow / 2.0, dim: false }];
        if s.others > 0 {
            parts.push(Part::Label { text: format!("+{}", s.others), weight: None });
        }
        let tip = if s.detail.is_empty() { s.title.clone() } else { format!("{}: {}", s.title, s.detail) };
        View::new(parts, look.xs).tone(if s.failed { Tone::Destructive } else { Tone::Rest }).panel("activity").tooltip(tip)
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel("activity")
    }
}
