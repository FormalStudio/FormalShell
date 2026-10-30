//! Activity's one row (`docs/superpowers/specs/2026-10-30-activity.md`):
//! a ring, the group's title, a line of detail and its actions. The panel,
//! the live toast and the notification centre all build from [`row`] and
//! [`folded`], and a sidebar card is [`list`] with a small `max`.
//!
//! A row's `on` actions are `open:<source>`, `dismiss:<source>`,
//! `retry:<source>` and `open-app:<source>`; [`event`] turns one into
//! [`act`], which the IPC verbs call too.

use fs_activity::{Group, Item, Origin, Retry, State};
use fs_theme::theme::Theme;

use crate::runtime::Runtime;
use crate::services::activity::{self as service, Input};
use crate::services::notifications::{self as notifications, Op, now_ms};
use crate::services::{compositor, menu};
use crate::store::{self, Store};
use crate::ui::el::Ring;
use crate::ui::{El, Ink, Type, Weight, w};

pub fn ring(item: &Item) -> Ring {
    match item.state {
        State::Running => item.progress.fraction().map_or(Ring::Unknown, |f| Ring::Progress(f.as_unit())),
        State::Failed => Ring::Failed,
        State::Succeeded | State::Done => Ring::Done,
    }
}

/// The line under the title: the error or result once it ended, else what
/// it works on and how far.
pub fn detail(item: &Item) -> String {
    if item.state != State::Running {
        return item.summary.clone().unwrap_or_else(|| item.detail.clone());
    }
    let what = if item.detail.is_empty() { item.phase.clone().unwrap_or_default() } else { item.detail.clone() };
    match item.progress.words() {
        Some(far) if what.is_empty() => far,
        Some(far) => format!("{what}, {far}"),
        None => what,
    }
}

/// One live group: its lead's ring, title and detail, the count past one
/// item, and its actions. `actions` off leaves the words alone (the bar's
/// tooltip, a narrow card).
pub fn row(theme: &Theme, store: &Store, g: &Group, actions: bool) -> El {
    let s = &theme.space;
    let lead = g.lead();
    let failed = g.state() == State::Failed;
    let mut head = vec![w::label(lead.title.clone()).elide().fill()];
    if g.items.len() > 1 {
        head.push(w::caption(format!("x{}", g.items.len())).mono().ink(Ink::Dim));
    }
    let words = w::column(
        s.xxs,
        vec![w::row(s.icon_gap, head).fill(), w::para(detail(lead), Type::BodySmall, Weight::Normal, if failed { Ink::Destructive } else { Ink::Muted }, 2)],
    )
    .fill();
    let mut parts = vec![w::ring(ring(lead)), words];
    if actions {
        if lead.state != State::Failed && crate::surfaces::packages::landed_app(store, g.source).is_some() {
            parts.push(w::icon_button("square-arrow-out-up-right").tip("Open").on(format!("open-app:{}", g.source)));
        }
        if lead.state == State::Failed && Retry::of(lead).is_some() {
            parts.push(w::icon_button("rotate-ccw").tip("Retry").on(format!("retry:{}", g.source)));
        }
        parts.push(w::icon_button("x").tip("Dismiss").on(format!("dismiss:{}", g.source)));
    }
    w::row(s.sm, parts).fill()
}

/// A folded item, newest first in the history: how it ended and when.
pub fn folded(theme: &Theme, item: &Item, now: i64) -> El {
    let s = &theme.space;
    let when = fs_info::notifications::rel_time(now, item.ended_ms.unwrap_or(item.updated_ms));
    let words = w::column(
        s.xxs,
        vec![
            w::row(s.icon_gap, vec![w::label(item.title.clone()).elide().fill(), w::caption(when).mono().ink(Ink::Dim)]).fill(),
            w::para(detail(item), Type::BodySmall, Weight::Normal, Ink::Muted, 2),
        ],
    )
    .fill();
    w::row(s.sm, vec![w::ring(ring(item)), words]).fill()
}

/// Every live group as a cursor stop, `max` of them (the rest as `+N`).
pub fn list(theme: &Theme, store: &Store, max: usize) -> Vec<El> {
    let groups = store.activity.activity.groups();
    let mut rows: Vec<El> = groups
        .iter()
        .take(max)
        .map(|g| w::cell(row(theme, store, g, true)).ghost().interactive().on(format!("open:{}", g.source)).stop(g.source.to_owned()))
        .collect();
    if groups.len() > max {
        rows.push(w::caption(format!("+{}", groups.len() - max)).mono().ink(Ink::Dim));
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Open,
    Dismiss,
    Retry,
    /// The app an install brought.
    OpenApp,
}

impl Verb {
    pub fn parse(on: &str) -> Option<(Verb, &str)> {
        let (verb, source) = on.split_once(':')?;
        let verb = match verb {
            "open" => Verb::Open,
            "dismiss" => Verb::Dismiss,
            "retry" => Verb::Retry,
            "open-app" => Verb::OpenApp,
            _ => return None,
        };
        Some((verb, source))
    }
}

/// What the caller still has to do itself.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    Nothing,
    /// An fspec row opens the Activity panel.
    Panel,
}

fn notify(runtime: Option<&Runtime>, op: Op) {
    if let Some(rt) = runtime {
        rt.service(move |ctx| ctx.publish(store::Diff::Notifications(notifications::Diff::Op(op))));
    }
}

fn notification(item: &Item) -> Option<String> {
    (item.origin == Origin::Notification).then(|| item.id.strip_prefix("notification:").unwrap_or(&item.id).to_owned())
}

/// The window of desktop entry `app`, else the entry launched.
fn focus_app(store: &Store, app: &str) {
    let id = app.trim_end_matches(".desktop");
    let window = store.compositor.snapshot.windows.iter().find(|w| w.app_id.eq_ignore_ascii_case(id) || w.initial_class.eq_ignore_ascii_case(id));
    match window {
        Some(win) => compositor::focus_window(&win.id),
        None => {
            if let Some(argv) = menu::launch_argv(&store.menu, id) {
                crate::services::launch::spawn(store, id, &argv);
            }
        }
    }
}

fn spawn(retry: Retry) {
    let argv = match retry.cwd {
        Some(dir) => {
            let mut argv: Vec<String> = ["sh", "-c", "cd \"$1\" && shift && exec \"$@\"", "sh"].map(String::from).to_vec();
            argv.push(dir);
            argv.extend(retry.argv);
            argv
        }
        None => retry.argv,
    };
    compositor::spawn(&argv);
}

/// Runs `verb` on the live group `source`. The error is the IPC reply.
pub fn act(store: &Store, runtime: Option<&Runtime>, verb: Verb, source: &str) -> Result<Done, String> {
    let activity = &store.activity.activity;
    let groups = activity.groups();
    let Some(g) = groups.iter().find(|g| g.source == source) else { return Err(format!("error: no activity from '{source}'")) };
    match verb {
        Verb::Open => {
            let lead = g.lead();
            match (lead.origin, lead.app.as_deref(), notification(lead)) {
                (Origin::Fspec, ..) => return Ok(Done::Panel),
                (Origin::LauncherEntry | Origin::Launch, Some(app), _) => focus_app(store, app),
                (Origin::Notification, _, Some(id)) => notify(runtime, Op::Invoke(id, "default".into())),
                _ => {}
            }
        }
        Verb::Dismiss => {
            let ids: Vec<String> = g.items.iter().filter_map(|i| notification(i)).collect();
            if !ids.is_empty() {
                notify(runtime, Op::DismissGroup(ids));
            }
            service::send(Input::Dismiss(source.to_owned()));
        }
        Verb::OpenApp => {
            let Some(app) = crate::surfaces::packages::landed_app(store, source) else { return Err(format!("error: '{source}' brought no app")) };
            focus_app(store, app);
            service::send(Input::Dismiss(source.to_owned()));
        }
        Verb::Retry => {
            let Some(retry) = activity.retry(source) else { return Err(format!("error: '{source}' cannot be retried")) };
            // The packages service follows its own installs' operations.
            if let Some(id) = source.strip_prefix("fspec:install:") {
                crate::surfaces::packages::install(store, id);
            } else if let Some(id) = source.strip_prefix("fspec:uninstall:") {
                crate::services::packages::send(crate::services::packages::Cmd::Uninstall(id.to_owned()));
            } else {
                spawn(retry);
            }
            service::send(Input::Dismiss(source.to_owned()));
        }
    }
    Ok(Done::Nothing)
}

/// The history, newest first, `max` rows.
pub fn history(theme: &Theme, store: &Store, max: usize) -> Vec<El> {
    let now = now_ms();
    store.activity.activity.history().take(max).map(|i| w::cell(folded(theme, i, now)).ghost().interactive().stop(i.id.clone())).collect()
}
