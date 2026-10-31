//! What is running and what just ended: `Now`, every live group as a
//! row (`surfaces/activity.rs`), then `Earlier`, the history. Enter opens a
//! row, `x` dismisses it, `r` retries a failed fspec run or update. Without
//! fspec the last line says so; LauncherEntry and notification progress
//! still show.

use super::{Effect, Panel, View};
use crate::store::Topic;
use crate::surfaces::activity::{self, Verb};
use crate::ui::{El, Event, w};

const EARLIER: usize = 10;

#[derive(Default)]
pub struct Activity {
    /// The row `activity open` asked for, for the host to put the cursor on.
    cursor: Option<String>,
}

/// An fspec row's open asks for this panel, which is already up.
fn run(verb: Verb, source: &str, fx: &mut Effect) {
    let _ = activity::act(fx.store, fx.runtime, verb, source);
}

impl Panel for Activity {
    fn id(&self) -> &'static str {
        "activity"
    }

    fn title(&self, _: &View) -> String {
        "Activity".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Activity]
    }

    fn call(&mut self, verb: &str, arg: &str) -> Option<String> {
        (verb == "select").then(|| {
            self.cursor = Some(arg.to_owned());
            "ok".into()
        })
    }

    fn take_cursor(&mut self) -> Option<String> {
        self.cursor.take()
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let a = &v.store.activity;
        let running = activity::list(v.theme, v.store, usize::MAX);
        let earlier = activity::history(v.theme, v.store, EARLIER);
        let mut parts = Vec::new();
        if running.is_empty() && earlier.is_empty() {
            parts.push(w::panel_empty(s, "Nothing running"));
        }
        if !running.is_empty() {
            let mut col = vec![w::panel_heading(s, "Now")];
            col.extend(running.into_iter().map(|r| super::center::inset(s, r)));
            parts.push(w::column(s.panel_gap, col));
        }
        if !earlier.is_empty() {
            let mut col = vec![w::panel_heading(s, "Earlier")];
            col.extend(earlier.into_iter().map(|r| super::center::inset(s, r)));
            parts.push(w::column(s.panel_gap, col));
        }
        let f = &a.fspec;
        let unavailable = match &f.error {
            _ if !f.installed => Some("fspec is not installed: installs and updates do not show here".to_owned()),
            Some(err) => Some(err.clone()),
            None => None,
        };
        if let Some(line) = unavailable {
            parts.push(w::panel_text(s, w::caption(line).pad_top(s.panel_section)));
        }
        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let Some((verb, source)) = Verb::parse(&ev.on) {
            run(verb, source, fx);
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        run(Verb::Open, stop, fx);
    }

    fn delete(&mut self, stop: &str, fx: &mut Effect) {
        run(Verb::Dismiss, stop, fx);
    }

    fn key(&mut self, stop: Option<&str>, text: &str, fx: &mut Effect) {
        if let (Some(stop), "r") = (stop, text) {
            run(Verb::Retry, stop, fx);
        }
    }
}
