//! A branch icon and "prs/issues" in mono. gh missing
//! hides the cell until the first poll lands; a failed auth reads NO AUTH,
//! any other failure NO GH, never a stale count.

use crate::services::info::github::Poll;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, View};

pub struct Github {
    want: Want,
    poll: Poll,
    counts: (u64, u64),
    label: bool,
}

impl Default for Github {
    fn default() -> Self {
        Self { want: Want::new(Source::Github), poll: Poll::Unknown, counts: (0, 0), label: true }
    }
}

impl Cell for Github {
    fn visible(&mut self, on: bool) {
        self.want.hold(on);
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn read(&mut self, env: &Env) -> bool {
        let g = &env.store.info.github;
        let label = env.store.config.bool("bar.widgets.github.showLabel").unwrap_or(true);
        let next = (g.poll, (g.prs, g.issues), label);
        let changed = next != (self.poll, self.counts, self.label);
        (self.poll, self.counts, self.label) = next;
        changed
    }

    fn view(&self, look: &Look) -> View {
        if matches!(self.poll, Poll::Unknown | Poll::Missing) {
            return View::hidden();
        }
        let ok = self.poll == Poll::Ok;
        let mut parts = vec![Part::Icon { name: "git-branch".into(), dim: !ok, dot: false }];
        if self.label {
            parts.push(if ok {
                Part::Label { text: format!("{}/{}", self.counts.0, self.counts.1), weight: None }
            } else {
                Part::Meta { text: if self.poll == Poll::NoAuth { "NO AUTH" } else { "NO GH" }.into() }
            });
        }
        let tooltip = match self.poll {
            Poll::Ok => format!("GITHUB / {} PRS {} ISSUES", self.counts.0, self.counts.1),
            Poll::NoAuth => "GITHUB / NOT AUTHENTICATED".to_owned(),
            _ => "GITHUB / UNAVAILABLE".to_owned(),
        };
        View::new(parts, look.xs).panel("github").tooltip(tooltip)
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel("github")
    }
}
