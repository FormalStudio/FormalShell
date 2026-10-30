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

use fs_info::notifications::{self as model, Group, Urgency};

use super::{Effect, Panel, View};
use crate::services::activity::{self as activity_service, Input};
use crate::services::notifications::{Diff, Op, now_ms};
use crate::surfaces::activity;
use crate::store::{self, Topic};
use crate::ui::el::Kind;
use crate::ui::{El, Event, Ink, Type, Variant, Weight, What, w};

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

fn send(fx: &Effect, op: Op) {
    fx.service(move |ctx| ctx.publish(store::Diff::Notifications(Diff::Op(op))));
}

fn row(v: &View, g: &Group, unread: bool, now: i64) -> El {
    let s = &v.theme.space;
    let e = &g.entry;
    let key = model::group_key(g);
    let critical = e.urgency == Urgency::Critical;
    let rel = model::rel_time(now, e.arrived_at);
    let meta = if g.count > 1 { format!("{rel}  x{}", g.count) } else { rel };
    let mut head = Vec::new();
    if unread {
        let d = s.sm;
        head.push(El::new(Kind::Swatch { color: v.theme.colors.get("primary"), w: d, h: d, radius: d / 2.0, border: false }));
    }
    let mark = w::icon(if critical { "triangle-alert" } else { "bell" }).ink(if critical { Ink::Destructive } else { Ink::Muted });
    head.push(if e.source == "iphone" { mark.badge("smartphone") } else { mark });
    head.push(w::section_label(s, &e.app_name, None, false));
    head.push(w::caption(meta).mono().fill());
    head.push(w::icon_button("x").tip("Dismiss").on(format!("dismiss:{key}")));
    let mut parts = vec![w::row(s.icon_gap, head), w::para(e.summary.clone(), Type::Body, Weight::Medium, Ink::Fg, 2)];
    let body = model::sanitize_body(&e.body, &e.app_name, &e.app_icon);
    if !body.is_empty() {
        parts.push(w::para(body, Type::BodySmall, Weight::Normal, Ink::Muted, 2));
    }
    let actions = model::button_actions(e);
    if !actions.is_empty() {
        let buttons = actions
            .iter()
            .map(|a| w::button(a.label.clone()).variant(Variant::Outline).on(format!("action:{}:{key}", a.key)))
            .collect();
        parts.push(w::row(s.sm, buttons));
    }
    w::cell(w::column(s.row_gap, parts)).ghost().interactive().on(format!("row:{key}")).stop(key)
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

fn activate_row(fx: &Effect, key: &str) {
    let Some(g) = find(fx.store, key) else { return };
    if g.actions.iter().any(|a| a.key == "default") {
        send(fx, Op::Invoke(g.id.clone(), "default".into()));
    }
}

impl Panel for Center {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self, _: &View) -> String {
        "Notifications".into()
    }

    fn icon(&self, _: &View) -> String {
        String::new()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Notifications, Topic::Activity]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_wide
    }

    fn closable(&self) -> bool {
        false
    }

    /// The relative times recompute off a slow clock, never off the reducer's tick.
    fn wake(&self, _: &View) -> Option<std::time::Instant> {
        Some(std::time::Instant::now() + std::time::Duration::from_secs(30))
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let s = &v.theme.space;
        vec![
            w::section_label(s, "DND", None, false),
            w::switch(v.store.notifications.model.dnd).on("dnd").stop("dnd"),
            w::space(s.section_gap),
            w::button("Clear all").variant(Variant::Ghost).on("clear").stop("clear"),
        ]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let now = now_ms();
        let (pending, seen) = sections(v);
        let phone = pending.iter().chain(&seen).any(|g| g.entry.source == "iphone");
        let mut out = Vec::new();
        if phone || self.filter != 0 {
            out.push(w::segmented(FILTERS.iter().map(|f| (*f).to_owned()).collect(), self.filter).on("filter").stop("filter"));
        }
        let pending: Vec<Group> = pending.into_iter().filter(|g| self.shows(g)).collect();
        let seen: Vec<Group> = seen.into_iter().filter(|g| self.shows(g)).collect();
        let folded = if self.filter == 2 { Vec::new() } else { activity::history(v.theme, v.store, FOLDED) };
        let any_folded = !folded.is_empty();
        if any_folded {
            let head = w::row(
                s.sm,
                vec![w::heading(s, "Activity", None, false).fill(), w::button("Clear").variant(Variant::Ghost).on("activity-clear").stop("activity-clear")],
            )
            .fill();
            let mut col = vec![head];
            col.extend(folded);
            out.push(w::column(s.row_gap, col));
        }
        if pending.is_empty() && seen.is_empty() && !any_folded {
            out.push(w::section_label(s, "No notifications", None, false));
            return w::column(s.section_gap, out);
        }
        let count = |rows: &[Group]| rows.iter().map(|g| g.count).sum::<usize>();
        let mut section = |title: &str, count: usize, rows: &[Group], unread: bool| {
            let mut col = vec![w::heading(s, title, Some(count), false)];
            for (i, g) in rows.iter().enumerate() {
                if i > 0 {
                    col.push(w::separator());
                }
                col.push(row(v, g, unread, now));
            }
            out.push(w::column(s.row_gap, col));
        };
        if !pending.is_empty() {
            section("Pending", count(&pending), &pending, true);
        }
        if !seen.is_empty() {
            section("Seen", count(&seen), &seen, false);
        }
        w::column(s.section_gap, out)
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
            _ => {
                if let Some(key) = on.strip_prefix("dismiss:") {
                    if let Some(g) = find(fx.store, key) {
                        send(fx, Op::DismissGroup(g.member_ids.clone()));
                    }
                } else if let Some(key) = on.strip_prefix("row:") {
                    activate_row(fx, key);
                } else if let Some(rest) = on.strip_prefix("action:") {
                    if let Some((action, key)) = rest.split_once(':') {
                        if let Some(g) = find(fx.store, key) {
                            send(fx, Op::Invoke(g.id.clone(), action.to_owned()));
                        }
                    }
                }
            }
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        match stop {
            "filter" => self.filter = (self.filter + 1) % FILTERS.len(),
            "dnd" => send(fx, Op::SetDnd(!fx.store.notifications.model.dnd)),
            "clear" => send(fx, Op::ClearHistory),
            "activity-clear" => activity_service::send(Input::ClearHistory),
            key => activate_row(fx, key),
        }
    }

    fn delete(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(g) = find(fx.store, stop) {
            send(fx, Op::DismissGroup(g.member_ids.clone()));
        }
    }

    fn key(&mut self, _: Option<&str>, text: &str, fx: &mut Effect) {
        if text == "d" {
            send(fx, Op::SetDnd(!fx.store.notifications.model.dnd));
        }
    }
}
