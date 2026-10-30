//! Activity: what is running on the machine right now, from the three live
//! activity sources a Linux desktop has (FormalOS plan, decisions 6 and 12):
//! fspec's operation stream, `com.canonical.Unity.LauncherEntry` and the
//! notification spec's `value` hint, and the shell's own app launches until
//! their window maps. One list of items, grouped by source.
//!
//! Nothing piles up: a finished item stays [`FOLD_MS`] and then folds into
//! the history; a failed one stays until [`Activity::seen`]. Time is passed
//! in, in milliseconds since the epoch, so the model has no clock of its own.

pub mod fspec;
pub mod launcher;
pub mod progress;

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Value, json};

use launcher::Entry;
use progress::{Fraction, Progress};

/// How long a finished item stays before it folds into the history.
pub const FOLD_MS: i64 = 4000;
/// Folded items kept, newest first.
pub const HISTORY: usize = 50;
/// How long a launch waits for its window before it reads as failed.
pub const LAUNCH_MS: i64 = 15_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Fspec,
    LauncherEntry,
    Notification,
    /// An app the shell spawned, until a window of it maps.
    Launch,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Fspec => "fspec",
            Origin::LauncherEntry => "launcherEntry",
            Origin::Notification => "notification",
            Origin::Launch => "launch",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Succeeded,
    Failed,
    /// Ended without saying how: a LauncherEntry hiding its progress, a
    /// notification dropping its `value` or closed by its sender.
    Done,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Succeeded => "succeeded",
            State::Failed => "failed",
            State::Done => "done",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Unique across origins: `fspec:<op id>`, `launcher:<desktop id>`,
    /// `notification:<id>`, `launch:<desktop id>`.
    pub id: String,
    pub origin: Origin,
    /// The grouping key: one row per source.
    pub source: String,
    /// fspec's kind (`apply`, `update`, `rollback`, `run`, or one this
    /// reader does not know), `progress`, `notification` or `launch`
    /// otherwise.
    pub kind: String,
    /// The desktop entry id the item belongs to, where there is one.
    pub app: Option<String>,
    pub title: String,
    /// What it works on now: fspec's `item` or phase, a notification's body.
    pub detail: String,
    pub phase: Option<String>,
    pub progress: Progress,
    pub state: State,
    pub urgent: bool,
    pub keys: Vec<String>,
    /// The finished sentence: fspec's `result.summary` or `error`.
    pub summary: Option<String>,
    pub started_ms: i64,
    pub updated_ms: i64,
    /// When this reader saw it end.
    pub ended_ms: Option<i64>,
}

impl Item {
    /// What the item is about, for a line too narrow for the whole title:
    /// an fspec install's app name (fspec titles one `Installing <name>`,
    /// FormalSpec's `app.rs`), else the title.
    pub fn subject(&self) -> &str {
        match self.kind.as_str() {
            "install" => self.title.strip_prefix("Installing ").unwrap_or(&self.title),
            _ => &self.title,
        }
    }

    fn due(&self, now: i64) -> bool {
        match (self.state, self.ended_ms) {
            (State::Running | State::Failed, _) | (_, None) => false,
            (_, Some(t)) => now >= t + FOLD_MS,
        }
    }

    fn fold_at(&self) -> Option<i64> {
        match self.state {
            State::Running | State::Failed => None,
            _ => self.ended_ms.map(|t| t + FOLD_MS),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "origin": self.origin.as_str(),
            "source": self.source,
            "kind": self.kind,
            "app": self.app,
            "title": self.title,
            "detail": self.detail,
            "phase": self.phase,
            "state": self.state.as_str(),
            "urgent": self.urgent,
            "progress": self.progress.to_json(),
            "keys": self.keys,
            "summary": self.summary,
            "started": self.started_ms,
            "updated": self.updated_ms,
            "ended": self.ended_ms,
        })
    }
}

/// A LauncherEntry's count and urgency, for the app's own icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub count: Option<i64>,
    pub urgent: bool,
}

/// A notification as Activity reads it.
pub struct Notice {
    pub id: u32,
    /// The sender's desktop entry, else its app name.
    pub app: String,
    pub title: String,
    pub body: String,
    pub value: Option<Fraction>,
    pub urgent: bool,
}

pub fn notification_id(id: u32) -> String {
    format!("notification:{id}")
}

fn launcher_id(app: &str) -> String {
    format!("launcher:{app}")
}

fn launch_id(app: &str) -> String {
    format!("launch:{app}")
}

/// A running launch's end, when its window has not mapped by then.
fn launch_due(item: &Item) -> Option<i64> {
    (item.origin == Origin::Launch && item.state == State::Running).then_some(item.started_ms + LAUNCH_MS)
}

/// The items sharing one source, in the order they started.
pub struct Group<'a> {
    pub source: &'a str,
    pub items: Vec<&'a Item>,
}

impl Group<'_> {
    pub fn state(&self) -> State {
        let any = |s| self.items.iter().any(|i| i.state == s);
        if any(State::Failed) {
            State::Failed
        } else if any(State::Running) {
            State::Running
        } else {
            self.items.last().map_or(State::Done, |i| i.state)
        }
    }

    pub fn urgent(&self) -> bool {
        self.items.iter().any(|i| i.urgent)
    }

    /// The item a one-row view shows: a failure, else the oldest running
    /// one, else the last to end.
    pub fn lead(&self) -> &Item {
        let by = |s| self.items.iter().find(|i| i.state == s);
        by(State::Failed)
            .or_else(|| by(State::Running))
            .or_else(|| self.items.iter().max_by_key(|i| i.ended_ms))
            .unwrap_or(&self.items[0])
    }
}

/// How a failed fspec operation is run again: what its stream line names
/// is all there is to go on, so only a `run` (its package), an `update`
/// (its spec dir) and an `install` or `uninstall` (its search id) can be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retry {
    pub cwd: Option<String>,
    pub argv: Vec<String>,
}

impl Retry {
    pub fn of(item: &Item) -> Option<Retry> {
        if item.origin != Origin::Fspec {
            return None;
        }
        let arg = item.source.strip_prefix(&format!("fspec:{}:", item.kind)).filter(|a| !a.is_empty())?;
        match item.kind.as_str() {
            "run" => Some(Retry { cwd: None, argv: vec!["fspec".into(), "run".into(), "--detach".into(), arg.into()] }),
            "update" => Some(Retry { cwd: Some(arg.into()), argv: vec!["fspec".into(), "update".into()] }),
            "install" | "uninstall" => {
                Some(Retry { cwd: None, argv: vec!["fspec".into(), "app".into(), item.kind.clone(), arg.into(), "--detach".into()] })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Activity {
    live: Vec<Item>,
    history: VecDeque<Item>,
    badges: BTreeMap<String, Badge>,
    /// Sources whose live card was put away, with the group's state then:
    /// the card comes back once the state moves on.
    dismissed: BTreeMap<String, State>,
}

impl Activity {
    pub fn get(&self, id: &str) -> Option<&Item> {
        self.live.iter().find(|i| i.id == id)
    }

    pub fn live(&self) -> &[Item] {
        &self.live
    }

    pub fn history(&self) -> impl Iterator<Item = &Item> {
        self.history.iter()
    }

    pub fn badges(&self) -> &BTreeMap<String, Badge> {
        &self.badges
    }

    /// Replaces the item holding `item.id` whole, or adds it. An item that
    /// arrives ended is stamped `now`; one that was already ended keeps its
    /// stamp. Returns whether anything changed.
    pub fn upsert(&mut self, item: Item, now: i64) -> bool {
        let changed = self.put(item, now);
        self.settle_dismissed();
        changed
    }

    /// A dismissal lasts while its group holds the state it was put away
    /// in: once that moves on (a run ends, or ends and starts again before
    /// it folds) the card is back for good.
    fn settle_dismissed(&mut self) {
        let states: BTreeMap<String, State> = self.groups().iter().map(|g| (g.source.to_owned(), g.state())).collect();
        self.dismissed.retain(|source, state| states.get(source) == Some(state));
    }

    fn put(&mut self, mut item: Item, now: i64) -> bool {
        let at = self.live.iter().position(|i| i.id == item.id);
        if item.state != State::Running {
            item.ended_ms = at.and_then(|i| self.live[i].ended_ms).or(item.ended_ms).or(Some(now));
        } else {
            item.ended_ms = None;
        }
        match at {
            Some(i) if self.live[i] == item => false,
            Some(i) => {
                self.live[i] = item;
                true
            }
            None => {
                self.live.push(item);
                true
            }
        }
    }

    /// Ends a running item. False when it is not live or already ended.
    pub fn end(&mut self, id: &str, state: State, summary: Option<String>, now: i64) -> bool {
        let Some(item) = self.live.iter_mut().find(|i| i.id == id && i.state == State::Running) else { return false };
        item.state = state;
        item.urgent |= state == State::Failed;
        if summary.is_some() {
            item.summary = summary;
        }
        item.ended_ms = Some(now);
        self.settle_dismissed();
        true
    }

    /// Fails every launch whose window is overdue, and moves every item
    /// whose time is up into the history.
    pub fn fold(&mut self, now: i64) -> bool {
        let overdue: Vec<(String, String)> =
            self.live.iter().filter(|i| launch_due(i).is_some_and(|at| now >= at)).map(|i| (i.id.clone(), i.title.clone())).collect();
        let mut changed = false;
        for (id, title) in overdue {
            changed |= self.end(&id, State::Failed, Some(format!("{title} has not opened a window")), now);
        }
        self.fold_where(|i| i.due(now)) || changed
    }

    /// The failures have been looked at: they fold now.
    pub fn seen(&mut self) -> bool {
        self.fold_where(|i| i.state == State::Failed)
    }

    fn fold_where(&mut self, due: impl Fn(&Item) -> bool) -> bool {
        let before = self.live.len();
        let mut kept = Vec::with_capacity(before);
        for item in self.live.drain(..) {
            if due(&item) {
                self.history.push_front(item);
            } else {
                kept.push(item);
            }
        }
        self.live = kept;
        self.history.truncate(HISTORY);
        self.settle_dismissed();
        self.live.len() != before
    }

    /// Put a group away: a running one loses its card until its state
    /// changes, a finished or failed one folds now. False for a source
    /// nothing live holds.
    pub fn dismiss(&mut self, source: &str) -> bool {
        let Some(state) = self.groups().iter().find(|g| g.source == source).map(Group::state) else { return false };
        if state == State::Running {
            return self.dismissed.insert(source.to_owned(), state) != Some(state);
        }
        self.fold_where(|i| i.source == source)
    }

    /// The sources that get a live card: fspec and LauncherEntry groups
    /// (a notification with a `value` is its own card, a launch has the
    /// bar's ring alone), not put away.
    pub fn toasts(&self) -> Vec<&str> {
        self.groups()
            .into_iter()
            .filter(|g| g.items.iter().any(|i| !matches!(i.origin, Origin::Notification | Origin::Launch)))
            .filter(|g| self.dismissed.get(g.source) != Some(&g.state()))
            .map(|g| g.source)
            .collect()
    }

    /// How the source's failed operation runs again, where it can.
    pub fn retry(&self, source: &str) -> Option<Retry> {
        self.live.iter().filter(|i| i.source == source && i.state == State::Failed).find_map(Retry::of)
    }

    pub fn clear_history(&mut self) -> bool {
        let had = !self.history.is_empty();
        self.history.clear();
        had
    }

    /// When the next finished item folds or the next launch runs out, if
    /// any is waiting to.
    pub fn next_fold(&self) -> Option<i64> {
        self.live.iter().filter_map(|i| i.fold_at().or_else(|| launch_due(i))).min()
    }

    /// The shell spawned desktop entry `app`: running, with no progress to
    /// tell, until [`Activity::launched`] or [`LAUNCH_MS`] passes.
    pub fn launch(&mut self, app: &str, title: &str, now: i64) -> bool {
        self.upsert(
            Item {
                id: launch_id(app),
                origin: Origin::Launch,
                source: launch_id(app),
                kind: "launch".into(),
                app: Some(app.to_owned()),
                title: title.to_owned(),
                detail: "Opening".into(),
                phase: None,
                progress: Progress::Indeterminate,
                state: State::Running,
                urgent: false,
                keys: Vec::new(),
                summary: None,
                started_ms: now,
                updated_ms: now,
                ended_ms: None,
            },
            now,
        )
    }

    /// A window of `app` mapped: its launch goes, failed or not, seen
    /// into the history or not.
    pub fn launched(&mut self, app: &str) -> bool {
        let id = launch_id(app);
        let before = (self.live.len(), self.history.len());
        self.live.retain(|i| i.id != id);
        self.history.retain(|i| i.id != id);
        self.settle_dismissed();
        (self.live.len(), self.history.len()) != before
    }

    /// One LauncherEntry's merged properties. Progress while it shows a
    /// bar; the count and urgency as the app's badge.
    pub fn launcher(&mut self, app: &str, title: &str, entry: &Entry, now: i64) -> bool {
        let badge = (entry.count_visible || entry.urgent)
            .then(|| Badge { count: entry.count_visible.then_some(entry.count), urgent: entry.urgent });
        let mut changed = match badge {
            Some(b) if self.badges.get(app) != Some(&b) => {
                self.badges.insert(app.to_owned(), b);
                true
            }
            Some(_) => false,
            None => self.badges.remove(app).is_some(),
        };
        let id = launcher_id(app);
        if !entry.progress_visible {
            if let Some(item) = self.live.iter_mut().find(|i| i.id == id && i.state == State::Running) {
                item.urgent = entry.urgent;
            }
            return self.end(&id, State::Done, None, now) || changed;
        }
        let started = self.get(&id).filter(|i| i.state == State::Running).map_or(now, |i| i.started_ms);
        let progress = entry.progress.map_or(Progress::Indeterminate, Progress::Fraction);
        let updated = match self.get(&id) {
            Some(i) if i.state == State::Running && i.progress == progress && i.urgent == entry.urgent && i.title == title => {
                i.updated_ms
            }
            _ => now,
        };
        changed |= self.upsert(
            Item {
                id,
                origin: Origin::LauncherEntry,
                source: format!("app:{app}"),
                kind: "progress".into(),
                app: Some(app.to_owned()),
                title: title.to_owned(),
                detail: String::new(),
                phase: None,
                progress,
                state: State::Running,
                urgent: entry.urgent,
                keys: Vec::new(),
                summary: None,
                started_ms: started,
                updated_ms: updated,
                ended_ms: None,
            },
            now,
        );
        changed
    }

    /// The app that sent `app`'s entry left the bus.
    pub fn launcher_gone(&mut self, app: &str, now: i64) -> bool {
        let badge = self.badges.remove(app).is_some();
        self.end(&launcher_id(app), State::Done, None, now) || badge
    }

    /// A notification arrived or was replaced. One with a `value` is a
    /// running item; a replace without one ends it.
    pub fn notification(&mut self, n: Notice, now: i64) -> bool {
        let id = notification_id(n.id);
        let Some(value) = n.value else {
            let Some(item) = self.live.iter_mut().find(|i| i.id == id && i.state == State::Running) else { return false };
            item.title = n.title;
            item.detail = n.body;
            item.updated_ms = now;
            return self.end(&id, State::Done, None, now);
        };
        let started = self.get(&id).filter(|i| i.state == State::Running).map_or(now, |i| i.started_ms);
        self.upsert(
            Item {
                id,
                origin: Origin::Notification,
                source: format!("app:{}", n.app),
                kind: "notification".into(),
                app: (!n.app.is_empty()).then_some(n.app),
                title: n.title,
                detail: n.body,
                phase: None,
                progress: Progress::Fraction(value),
                state: State::Running,
                urgent: n.urgent,
                keys: Vec::new(),
                summary: None,
                started_ms: started,
                updated_ms: now,
                ended_ms: None,
            },
            now,
        )
    }

    /// The sender closed notification `id`.
    pub fn notification_closed(&mut self, id: u32, now: i64) -> bool {
        self.end(&notification_id(id), State::Done, None, now)
    }

    /// Live items by source: urgent groups first, then in the order their
    /// first item started, so rows hold their places while progress moves.
    pub fn groups(&self) -> Vec<Group<'_>> {
        let mut groups: Vec<Group<'_>> = Vec::new();
        let mut sorted: Vec<&Item> = self.live.iter().collect();
        sorted.sort_by_key(|i| i.started_ms);
        for item in sorted {
            match groups.iter_mut().find(|g| g.source == item.source) {
                Some(g) => g.items.push(item),
                None => groups.push(Group { source: &item.source, items: vec![item] }),
            }
        }
        groups.sort_by_key(|g| !g.urgent());
        groups
    }

    pub fn to_json(&self) -> Value {
        let groups: Vec<Value> = self
            .groups()
            .iter()
            .map(|g| {
                json!({
                    "source": g.source,
                    "count": g.items.len(),
                    "state": g.state().as_str(),
                    "urgent": g.urgent(),
                    "lead": g.lead().id,
                    "items": g.items.iter().map(|i| i.to_json()).collect::<Vec<_>>(),
                })
            })
            .collect();
        let badges: Vec<Value> =
            self.badges.iter().map(|(app, b)| json!({"app": app, "count": b.count, "urgent": b.urgent})).collect();
        json!({
            "groups": groups,
            "history": self.history.iter().map(Item::to_json).collect::<Vec<_>>(),
            "badges": badges,
            "toasts": self.toasts(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str, source: &str, state: &str, done: u64, started: i64) -> Item {
        let line = format!(
            r#"{{"v":1,"type":"update","op":{{"id":"{id}","kind":"apply","source":"{source}","title":"Applying","started":{started},"updated":{started},"state":"{state}","progress":{{"done":{done},"total":4,"unit":"resources"}},"error":"boom"}}}}"#
        );
        fspec::parse(&line).unwrap().item()
    }

    fn notice(id: u32, value: Option<u8>) -> Notice {
        Notice { id, app: "firefox".into(), title: "Downloading".into(), body: "a.iso".into(), value: value.map(Fraction::from_percent), urgent: false }
    }

    #[test]
    fn a_launch_runs_until_its_window_maps() {
        let mut a = Activity::default();
        assert!(a.launch("foot", "Foot", 1000));
        let item = a.get("launch:foot").unwrap();
        assert_eq!((item.state, item.progress.clone()), (State::Running, Progress::Indeterminate));
        assert!(a.toasts().is_empty(), "a launch raised a toast card");
        assert_eq!(a.next_fold(), Some(1000 + LAUNCH_MS));
        assert!(a.launched("foot"));
        assert!(a.live().is_empty());
        assert_eq!(a.history().count(), 0, "a launch that mapped went into the history");
        assert!(!a.launched("foot"));
    }

    #[test]
    fn a_launch_with_no_window_fails() {
        let mut a = Activity::default();
        a.launch("ghost", "Ghost", 0);
        assert!(!a.fold(LAUNCH_MS - 1));
        assert!(a.fold(LAUNCH_MS));
        let item = a.get("launch:ghost").unwrap();
        assert_eq!((item.state, item.urgent), (State::Failed, true));
        assert_eq!(item.summary.as_deref(), Some("Ghost has not opened a window"));
        assert_eq!(a.next_fold(), None, "a failed launch asked for a timer");
        assert!(a.seen());
        assert_eq!(a.history().next().unwrap().id, "launch:ghost");
    }

    #[test]
    fn a_late_window_takes_the_failure_back() {
        let mut a = Activity::default();
        a.launch("slow", "Slow", 0);
        a.fold(LAUNCH_MS);
        assert!(a.launched("slow"));
        assert!(a.live().is_empty());
        assert_eq!(a.history().count(), 0);

        a.launch("slow", "Slow", 0);
        a.fold(LAUNCH_MS);
        a.seen();
        assert!(a.launched("slow"), "a seen failure stayed in the history");
        assert_eq!(a.history().count(), 0);
    }

    #[test]
    fn a_line_replaces_the_item_whole() {
        let mut a = Activity::default();
        assert!(a.upsert(op("1", "fspec:apply:g815", "running", 1, 10), 100));
        assert!(!a.upsert(op("1", "fspec:apply:g815", "running", 1, 10), 150), "the same line twice changed something");
        assert!(a.upsert(op("1", "fspec:apply:g815", "running", 3, 10), 200));
        assert_eq!(a.live().len(), 1);
        assert_eq!(a.get("fspec:1").unwrap().progress.fraction().unwrap().as_unit(), 0.75);
    }

    #[test]
    fn finished_items_fold_after_four_seconds() {
        let mut a = Activity::default();
        a.upsert(op("1", "s", "running", 1, 10), 0);
        assert_eq!(a.next_fold(), None, "a running item asked for a timer");
        a.upsert(op("1", "s", "succeeded", 4, 10), 1000);
        assert_eq!(a.next_fold(), Some(1000 + FOLD_MS));
        a.upsert(op("1", "s", "succeeded", 4, 10), 3000);
        assert_eq!(a.next_fold(), Some(1000 + FOLD_MS), "a repeat of the final line restarted the clock");
        assert!(!a.fold(4999));
        assert!(a.fold(5000));
        assert!(a.live().is_empty());
        assert_eq!(a.history().next().unwrap().id, "fspec:1");
        assert_eq!(a.next_fold(), None);
    }

    #[test]
    fn failures_stay_until_seen() {
        let mut a = Activity::default();
        a.upsert(op("1", "s", "failed", 2, 10), 0);
        assert!(a.get("fspec:1").unwrap().urgent);
        assert_eq!(a.next_fold(), None);
        assert!(!a.fold(60_000));
        assert_eq!(a.live().len(), 1);
        assert!(a.seen());
        assert!(a.live().is_empty());
        assert!(!a.seen());
    }

    #[test]
    fn a_died_operation_reads_failed() {
        let mut a = Activity::default();
        a.upsert(op("1", "s", "running", 2, 10), 0);
        assert!(a.end("fspec:1", State::Failed, Some("ended without a result".into()), 500));
        let item = a.get("fspec:1").unwrap();
        assert_eq!((item.state, item.urgent, item.ended_ms), (State::Failed, true, Some(500)));
        assert!(!a.end("fspec:1", State::Failed, None, 600), "an ended item ended twice");
    }

    #[test]
    fn groups_by_source_urgent_first() {
        let mut a = Activity::default();
        a.upsert(op("1", "fspec:apply:g815", "running", 1, 10), 0);
        a.upsert(op("2", "fspec:update:spec", "running", 1, 20), 0);
        a.upsert(op("3", "fspec:apply:g815", "running", 0, 30), 0);
        let groups = a.groups();
        assert_eq!(groups.iter().map(|g| (g.source, g.items.len())).collect::<Vec<_>>(), [("fspec:apply:g815", 2), ("fspec:update:spec", 1)]);
        assert_eq!(groups[0].lead().id, "fspec:1");
        a.upsert(op("2", "fspec:update:spec", "failed", 1, 20), 0);
        let groups = a.groups();
        assert_eq!(groups[0].source, "fspec:update:spec");
        assert_eq!(groups[0].state(), State::Failed);
        let json = a.to_json();
        assert_eq!(json["groups"][1]["count"], 2);
        assert_eq!(json["groups"][0]["lead"], "fspec:2");
    }

    #[test]
    fn launcher_entry_progress_and_badge() {
        let mut a = Activity::default();
        let mut e = Entry::default();
        e.apply(launcher::Prop::Progress(0.25));
        e.apply(launcher::Prop::ProgressVisible(true));
        e.apply(launcher::Prop::Count(3));
        e.apply(launcher::Prop::CountVisible(true));
        assert!(a.launcher("firefox", "Firefox", &e, 100));
        assert!(!a.launcher("firefox", "Firefox", &e, 200), "an unchanged entry changed something");
        let item = a.get("launcher:firefox").unwrap();
        assert_eq!((item.source.as_str(), item.progress.fraction().unwrap().as_percent(), item.started_ms), ("app:firefox", 25, 100));
        assert_eq!(a.badges()["firefox"], Badge { count: Some(3), urgent: false });
        e.apply(launcher::Prop::Urgent(true));
        assert!(a.launcher("firefox", "Firefox", &e, 250));
        e.apply(launcher::Prop::ProgressVisible(false));
        e.apply(launcher::Prop::CountVisible(false));
        e.apply(launcher::Prop::Urgent(false));
        assert!(a.launcher("firefox", "Firefox", &e, 300));
        let item = a.get("launcher:firefox").unwrap();
        assert_eq!((item.state, item.urgent), (State::Done, false));
        assert!(a.badges().is_empty());
        e.apply(launcher::Prop::Urgent(true));
        assert!(a.launcher("firefox", "Firefox", &e, 400));
        assert!(a.launcher_gone("firefox", 500));
        assert!(a.badges().is_empty());
    }

    #[test]
    fn notification_value_updates_in_place_and_ends() {
        let mut a = Activity::default();
        assert!(!a.notification(notice(7, None), 0), "a plain notification became an item");
        assert!(a.notification(notice(7, Some(10)), 100));
        assert!(a.notification(notice(7, Some(60)), 200));
        let item = a.get("notification:7").unwrap();
        assert_eq!((item.progress.fraction().unwrap().as_percent(), item.started_ms, item.app.as_deref()), (60, 100, Some("firefox")));
        assert!(a.notification(Notice { title: "Downloaded".into(), ..notice(7, None) }, 300));
        let item = a.get("notification:7").unwrap();
        assert_eq!((item.state, item.title.as_str(), item.ended_ms), (State::Done, "Downloaded", Some(300)));
        a.notification(notice(8, Some(5)), 400);
        assert!(a.notification_closed(8, 500));
        assert!(!a.notification_closed(9, 500));
    }

    #[test]
    fn history_is_capped_newest_first() {
        let mut a = Activity::default();
        for n in 0..(HISTORY as u32 + 5) {
            a.notification(notice(n, Some(1)), 0);
            a.notification_closed(n, 0);
        }
        a.fold(FOLD_MS);
        assert_eq!(a.history().count(), HISTORY);
    }

    #[test]
    fn a_dismissed_card_comes_back_when_its_state_moves() {
        let mut a = Activity::default();
        a.upsert(op("1", "s", "running", 1, 10), 0);
        a.notification(notice(7, Some(40)), 0);
        assert_eq!(a.toasts(), vec!["s"], "a value notification got a second card");
        assert!(a.dismiss("s"));
        assert!(!a.dismiss("s"));
        assert!(a.toasts().is_empty());
        a.upsert(op("1", "s", "running", 3, 10), 100);
        assert!(a.toasts().is_empty(), "progress alone brought the card back");
        a.upsert(op("1", "s", "succeeded", 4, 10), 150);
        a.upsert(op("2", "s", "running", 0, 160), 160);
        assert_eq!(a.toasts(), vec!["s"], "a restart before the fold hid the card again");
        a.upsert(op("2", "s", "failed", 3, 160), 200);
        assert_eq!(a.toasts(), vec!["s"]);
        assert!(a.dismiss("s"), "dismissing a failure did not fold it");
        assert!(a.get("fspec:2").is_none());
        assert!(!a.dismiss("nothing"));
    }

    #[test]
    fn only_a_run_and_an_update_retry() {
        let line = |kind: &str, source: &str| {
            fspec::parse(&format!(r#"{{"v":1,"type":"update","op":{{"id":"{kind}","kind":"{kind}","source":"{source}","title":"t","started":1,"updated":1,"state":"failed","error":"boom"}}}}"#))
                .unwrap()
                .item()
        };
        let mut a = Activity::default();
        a.upsert(line("run", "fspec:run:jq"), 0);
        a.upsert(line("update", "fspec:update:/home/k/spec"), 0);
        a.upsert(line("apply", "fspec:apply:g815"), 0);
        assert_eq!(a.retry("fspec:run:jq").unwrap().argv, ["fspec", "run", "--detach", "jq"]);
        assert_eq!(a.retry("fspec:update:/home/k/spec"), Some(Retry { cwd: Some("/home/k/spec".into()), argv: vec!["fspec".into(), "update".into()] }));
        assert_eq!(a.retry("fspec:apply:g815"), None);
    }

    #[test]
    fn an_install_is_about_its_app() {
        let item = |kind: &str, title: &str| {
            fspec::parse(&format!(r#"{{"v":1,"type":"update","op":{{"id":"a","kind":"{kind}","source":"fspec:{kind}:x","title":"{title}","started":1,"updated":1,"state":"running"}}}}"#))
                .unwrap()
                .item()
        };
        assert_eq!(item("install", "Installing Firefox").subject(), "Firefox");
        assert_eq!(item("uninstall", "Uninstalling Firefox").subject(), "Uninstalling Firefox");
        assert_eq!(item("apply", "Applying g815").subject(), "Applying g815");
    }
}
