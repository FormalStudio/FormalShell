//! The notification history, a panel off the bar's line with the
//! pending tier over the seen one, each a column of grouped rows newest
//! where its section reads first. The host owns the card, its scroll and
//! its cursor; opening it suppresses the toast stack and closing it files
//! everything pending as seen (`wayland/toasts.rs`).
//!
//! Panels see the store read-only, so every change a row asks for goes
//! back to the UI thread as a notifications diff.
//!
//! Finished Activity folds here (`surfaces/activity.rs`): an `Activity`
//! section over the rest, the newest [`FOLDED`] rows and a `Clear`.

use fs_theme::palette::ColorRole;
use fs_info::notifications::{self as model, Group, Urgency};

use super::{Effect, Out, Panel, View};
use crate::services::activity::{self as activity_service, Input};
use crate::runtime::now_ms;
use crate::services::notifications::Op;
use crate::surfaces::activity;
use crate::store::Topic;
use crate::ui::el::Kind;
use crate::ui::{El, Event, Ink, On, Weight, What, w};

/// A press on one entry, by the entry's group key.
enum Press {
    Dismiss(String),
    Row(String),
    /// An action's key, then the group's.
    Action(String, String),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Press::Dismiss(key) => write!(f, "dismiss:{key}"),
            Press::Row(key) => write!(f, "row:{key}"),
            Press::Action(action, key) => write!(f, "action:{action}:{key}"),
        }
    }
}

pub const ID: &str = "notifications-center";

const FILTERS: [&str; 3] = ["All", "This PC", "iPhone"];
/// Folded Activity rows shown; the panel holds more.
const FOLDED: usize = 5;

/// Which source the list shows, an index into [`FILTERS`].
#[derive(Default)]
pub struct Center {
    filter: usize,
}

impl Center {
    fn shows(&self, g: &Group) -> bool {
        match self.filter {
            1 => g.entry.source != "iphone",
            2 => g.entry.source == "iphone",
            _ => true,
        }
    }
}

fn send(fx: &mut Effect, op: Op) {
    fx.push(Out::apply(op));
}

fn row(v: &View, g: &Group, unread: bool, now: i64) -> El {
    let s = &v.theme.space;
    let e = &g.entry;
    let key = model::group_key(g);
    let critical = e.urgency == Urgency::Critical;
    let mut head = Vec::new();
    if unread {
        let d = s.sm;
        head.push(El::new(Kind::Swatch { color: v.theme.colors.get(ColorRole::Primary), w: d, h: d, radius: d / 2.0, border: false }));
    }
    let chip = if critical { w::panel_chip_ink(s, "triangle-alert", false, Ink::Destructive) } else { w::panel_chip(s, "bell", false) };
    head.push(w::caption(e.app_name.clone()).weight(Weight::Semibold).ink(Ink::Fg).elide());
    head.push(w::caption(w::notification_meta(g, now)).mono());
    head.push(w::spacer());
    head.push(w::icon_button("x").tip("Dismiss").on(Press::Dismiss(key.to_owned())));
    let (chip, words) = w::notification(s, g, chip, w::row(s.icon_gap, head), None, |action| Press::Action(action.to_owned(), key.to_owned()));
    w::panel_cell(s, w::row(s.panel_gap, vec![chip, words]).fill().pad(0.0, s.md, 0.0, s.md)).on(Press::Row(key.to_owned())).stop(key)
}

/// Pending oldest first, seen newest first, each grouped.
fn sections(v: &View) -> (Vec<Group>, Vec<Group>) {
    let m = &v.store.notifications.model;
    let mut past = m.past.clone();
    past.reverse();
    (model::group_entries(&m.pending), model::group_entries(&past))
}

fn find(v: &crate::store::Store, key: &str) -> Option<Group> {
    let m = &v.notifications.model;
    let mut past = m.past.clone();
    past.reverse();
    model::group_entries(&m.pending).into_iter().chain(model::group_entries(&past)).find(|g| model::group_key(g) == key)
}

fn activate_row(fx: &mut Effect, key: &str) {
    let Some(g) = find(fx.store, key) else { return };
    if g.actions.iter().any(|a| a.key == "default") {
        send(fx, Op::Invoke(g.id, "default".into()));
    }
}

impl Panel for Center {
    fn title(&self, _: &View) -> String {
        "Notifications".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Notifications, Topic::Activity]
    }

    fn width(&self, v: &View) -> Option<f64> {
        Some(v.theme.space.popup_width_wide)
    }

    fn closable(&self) -> bool {
        false
    }

    /// The relative times recompute off a slow clock, never off the reducer's tick.
    fn wake(&self, _: &View) -> Option<std::time::Instant> {
        Some(std::time::Instant::now() + std::time::Duration::from_secs(30))
    }

    fn actions(&self, v: &View) -> Vec<El> {
        vec![
            w::icon("bell-off").ink(Ink::Muted),
            w::switch(v.store.notifications.model.dnd).tip("Do not disturb").on("dnd").stop("dnd"),
            w::icon_button("trash-2").tip("Clear all").on("clear").stop("clear"),
        ]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let now = now_ms();
        let (pending, seen) = sections(v);
        let phone = pending.iter().chain(&seen).any(|g| g.entry.source == "iphone");
        let mut out = Vec::new();
        if phone || self.filter != 0 {
            out.push(w::panel_choice(s, FILTERS.iter().map(|f| (*f).to_owned()).collect(), self.filter).on("filter").stop("filter"));
        }
        let pending: Vec<Group> = pending.into_iter().filter(|g| self.shows(g)).collect();
        let seen: Vec<Group> = seen.into_iter().filter(|g| self.shows(g)).collect();
        let folded = if self.filter == 2 { Vec::new() } else { activity::history(v.theme, v.store, FOLDED) };
        let any_folded = !folded.is_empty();
        if any_folded {
            let head = w::panel_heading_action(s, "Activity", "Clear", "activity-clear");
            out.push(w::panel_section_with(s, head, folded.into_iter().map(|r| w::panel_inset(s, r)).collect(), ""));
        }
        if pending.is_empty() && seen.is_empty() && !any_folded {
            out.push(w::panel_empty(s, "No notifications"));
            return w::column(0.0, out);
        }
        let mut section = |title: &str, rows: &[Group], unread: bool| {
            let n = rows.iter().map(|g| g.count).sum();
            out.push(w::panel_section_n(s, title, n, rows.iter().map(|g| row(v, g, unread, now)).collect(), ""));
        };
        if !pending.is_empty() {
            section("Pending", &pending, true);
        }
        if !seen.is_empty() {
            section("Seen", &seen, false);
        }
        w::column(0.0, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let on = ev.on.as_str();
        match on {
            "filter" => {
                if let What::Pick(i) = ev.what {
                    self.filter = i.min(FILTERS.len() - 1);
                }
            }
            "dnd" => send(fx, Op::SetDnd(!fx.store.notifications.model.dnd)),
            "clear" => send(fx, Op::ClearHistory),
            "activity-clear" => activity_service::send(Input::ClearHistory),
            _ => match ev.on.get::<Press>() {
                Some(Press::Dismiss(key)) => {
                    if let Some(g) = find(fx.store, key) {
                        send(fx, Op::DismissGroup(g.member_ids.clone()));
                    }
                }
                Some(Press::Row(key)) => activate_row(fx, key),
                Some(Press::Action(action, key)) => {
                    if let Some(g) = find(fx.store, key) {
                        send(fx, Op::Invoke(g.id, action.clone()));
                    }
                }
                None => {}
            },
        }
    }

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        match stop.as_str() {
            "filter" => self.filter = (self.filter + 1) % FILTERS.len(),
            "dnd" => send(fx, Op::SetDnd(!fx.store.notifications.model.dnd)),
            "clear" => send(fx, Op::ClearHistory),
            "activity-clear" => activity_service::send(Input::ClearHistory),
            key => activate_row(fx, key),
        }
    }

    fn delete(&mut self, stop: &On, fx: &mut Effect) {
        if let Some(g) = find(fx.store, stop) {
            send(fx, Op::DismissGroup(g.member_ids.clone()));
        }
    }

    fn key(&mut self, _: Option<&On>, text: &str, fx: &mut Effect) {
        if text == "d" {
            send(fx, Op::SetDnd(!fx.store.notifications.model.dnd));
        }
    }
}
