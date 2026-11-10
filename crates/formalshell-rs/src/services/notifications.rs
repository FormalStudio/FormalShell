//! Notifications and reminders: `fs-notifd` serving
//! `org.freedesktop.Notifications` on the service thread, and fs-info's
//! three-tier reducer held here on the UI thread, which is its only writer.
//!
//! The server never closes anything on its own: every close the reducer
//! decides (a toast timing out, a dismiss) goes back to it as a [`Cmd`], so
//! the sender hears the right reason. A close the sender asked for, or the
//! one an action implies, comes in as [`Diff::Closed`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::{Duration, Instant};

use fs_info::notifications::{self as model, Action, AddOpts, Notif, Patch, Urgency};
use fs_info::reminders;
use fs_system::appicon::{DesktopEntry, Window, by_class};
use fs_js as js;
use fs_notifd::{CloseReason, Event, Image, ImageData, Server};
use fs_types::NotificationId;
use serde_json::{Value, json};

use super::icons;
use super::iphone;
use super::state::{self, Field};
use crate::runtime::{Ctx, now_ms};
use crate::services::appicon::{applications_dirs, scan_launchable};
use crate::store;

/// Omarchy's duration bands: low 5s, normal 8s, a sender's own timeout
/// honoured inside them up to 30s.
const LOW_MS: i64 = 5000;
const NORMAL_MS: i64 = 8000;
const MAX_MS: i64 = 30000;
/// How far a hovered toast's deadline is pushed each time it comes due.
const HOVER_HOLD_MS: i64 = 1000;

/// The size a themed icon is picked for, near the card's heading type it is
/// drawn at.
const ICON_PICK: u32 = 32;

fn timeout_for(urgency: Urgency, expire_timeout: i32) -> i64 {
    let floor = if urgency == Urgency::Low { LOW_MS } else { NORMAL_MS };
    MAX_MS.min(floor.max(i64::from(expire_timeout.max(0))))
}

/// One arrival or replace, as the reducer takes it.
pub struct Arrival {
    pub notif: Notif,
    pub timeout_ms: i64,
    /// The picture the icon order picks, decoded on the pool.
    pub icon: Option<icons::Raw>,
    pub replaced: bool,
}

pub enum Diff {
    Arrived(Box<Arrival>),
    /// The sender closed it, or an action closed it on the server's side.
    Closed(NotificationId),
    /// The bus name could not be had: another daemon owns it.
    Unavailable(String),
    /// A change a surface without write access to the store asked for.
    Op(Op),
    /// A notification mirrored off the iPhone; `quiet` is Focus's "history,
    /// no toast".
    Phone { record: iphone::Notification, quiet: bool },
    /// The phone withdrew one: its card goes without telling it back.
    DropPhone(i64),
}

/// A shell-authored toast's action: its key, its label, and what picking it
/// runs.
#[derive(Clone, Debug)]
pub struct LocalAction {
    pub key: String,
    pub label: String,
    pub run: Local,
}

/// What a shell-authored action runs once picked.
#[derive(Clone, Debug)]
pub enum Local {
    /// An argv the compositor spawns.
    Spawn(Vec<String>),
    /// A capture follow-up, which `wayland::capture_host` runs on the next
    /// pass of the loop.
    Capture(super::capture::Act),
}

pub enum Op {
    SetDnd(bool),
    DismissGroup(Vec<NotificationId>),
    /// The centre's "Clear all": pending and seen both go.
    ClearHistory,
    /// Every toast and every pending entry gone; the seen tier stays.
    Clear,
    ClearPending,
    DismissAll,
    MarkAllSeen,
    /// Toasts closed off the stack, not out of the history.
    DismissPopups(Vec<NotificationId>),
    Invoke(NotificationId, String),
    /// A toast's action button: its action runs and the toast goes.
    Pick(NotificationId, String),
    /// A click on a toast's body: the group's newest entry runs its default
    /// action, if it has one, and the group's toasts go.
    Open(Vec<NotificationId>),
    /// The newest toast's default action, then that toast dismissed.
    InvokeLast,
    /// `notifications expand on`, the rig's stand-in for hovering the stack.
    Expand(bool),
    /// The pointer over these toasts, or off them: their countdown holds.
    Hover(Vec<NotificationId>, bool),
    /// A shell-authored entry from a service on its own thread.
    Notify(String, String, Urgency),
    /// The same with actions, each a command run when it is picked, and a
    /// picture's URL ([`State::newest_local`] names the entry).
    NotifyRunning { summary: String, body: String, urgency: Urgency, actions: Vec<LocalAction>, image: String },
    /// A shell-authored entry's picture, decoded.
    Picture(NotificationId, icons::Raw),
    /// [`State::newest_reminder`] names the entry.
    SetReminder { seconds: u64, message: String },
    ClearReminders,
    /// The pending reminders as one toast, the bar's reminder indicator.
    ShowReminders,
    /// state.json's dnd and reminders, as the file now reads.
    Sync(bool, Value),
    /// `iphone.notifications.dedupe`'s rules, as settings.json now reads.
    Dedupe(Vec<fs_devices::iphone::DedupeRule>),
    /// The notification centre opened or shut, however it was.
    CenterOpen(bool),
    /// The clock: whatever came due by this unix millisecond runs.
    Tick(i64),
}

impl From<Op> for store::Diff {
    fn from(op: Op) -> Self {
        store::Diff::Notifications(Diff::Op(op))
    }
}

#[derive(Default)]
pub struct State {
    pub model: model::State,
    /// Why there is no server, when there is none.
    pub unavailable: Option<String>,
    pub center_open: bool,
    /// `notifications expand on`, the rig's stand-in for hovering the stack.
    pub stack_expanded: bool,
    /// Popup ids under the pointer: their countdown holds.
    pub hovered: HashSet<NotificationId>,
    /// Each entry's picture, keyed by its id; none means the bell.
    pub icons: HashMap<NotificationId, icons::Raw>,
    local_serial: u64,
    /// [`Local::Capture`] actions picked, in the order they were picked.
    pub capture_picked: Vec<super::capture::Act>,
    /// What shell-authored toasts' actions run, by entry id.
    local_commands: HashMap<NotificationId, Vec<LocalAction>>,
    /// state.json's dnd as last seen, so a setDnd answered here before its
    /// write lands is not rolled back by the old value.
    dnd_seen: Option<bool>,
    pub reminders: Vec<reminders::Entry>,
    reminders_seen: Option<Value>,
    reminder_serial: u64,
    newest_reminder: Option<String>,
    pub phone: iphone::Mirror,
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let now = now_ms();
        match diff {
            Diff::Arrived(a) => {
                let Arrival { notif, timeout_ms, icon, replaced } = *a;
                match icon {
                    Some(raw) => self.icons.insert(notif.id, raw),
                    None => self.icons.remove(&notif.id),
                };
                let known = self.find(notif.id).is_some();
                if replaced && known {
                    let patch = Patch {
                        app_name: Some(notif.app_name.clone()),
                        app_icon: Some(notif.app_icon.clone()),
                        desktop_entry: Some(notif.desktop_entry.clone()),
                        summary: Some(notif.summary.clone()),
                        body: Some(notif.body.clone()),
                        urgency: Some(notif.urgency),
                        actions: Some(notif.actions.clone()),
                        image: Some(notif.image.clone()),
                        phone: None,
                    };
                    self.model.update(notif.id, &patch, now);
                } else {
                    let opts = AddOpts { quiet: false, timeout_ms: Some(timeout_ms) };
                    self.model.add(&notif, now, &opts);
                    self.phone.local_arrived(&mut self.model, notif.id, now);
                }
                self.after_change();
                true
            }
            Diff::Closed(id) => {
                self.model.dismiss_one(id);
                self.after_change();
                true
            }
            Diff::Unavailable(why) => {
                self.unavailable = Some(why);
                true
            }
            Diff::Phone { record, quiet } => {
                self.phone.phone_arrived(&mut self.model, &record, quiet, now);
                true
            }
            Diff::DropPhone(id) => {
                let dropped = iphone::withdraw(&mut self.model, id);
                self.after_change();
                dropped
            }
            Diff::Op(op) => self.op(op),
        }
    }

    fn op(&mut self, op: Op) -> bool {
        match op {
            Op::SetDnd(on) => self.set_dnd(on),
            Op::DismissGroup(ids) => self.dismiss_group(&ids),
            Op::ClearHistory => {
                self.clear_pending();
                let past: Vec<NotificationId> = self.model.past.iter().map(|e| e.id).collect();
                self.dismiss_group(&past);
            }
            Op::Clear => {
                self.dismiss_all();
                self.clear_pending();
            }
            Op::ClearPending => self.clear_pending(),
            Op::DismissAll => self.dismiss_all(),
            Op::MarkAllSeen => self.mark_all_seen(),
            Op::DismissPopups(ids) => self.dismiss_popup_group(&ids),
            Op::Invoke(id, key) => self.invoke_action(id, &key),
            Op::Pick(id, key) => {
                self.invoke_action(id, &key);
                self.dismiss_popup(id);
            }
            Op::Open(ids) => {
                if let Some(&id) = ids.last()
                    && self.find(id).is_some_and(|e| e.actions.iter().any(|a| a.key == "default"))
                {
                    self.invoke_action(id, "default");
                }
                self.dismiss_popup_group(&ids);
            }
            Op::InvokeLast => self.invoke_last(),
            Op::Expand(on) => self.stack_expanded = on,
            Op::Hover(ids, on) => {
                self.set_hovered(&ids, on);
                // Only the countdown reads it: nothing redraws.
                return false;
            }
            Op::Notify(summary, body, urgency) => self.notify(&summary, &body, urgency),
            Op::NotifyRunning { summary, body, urgency, actions, image } => {
                self.notify_local(&summary, &body, urgency, actions, image);
            }
            Op::Picture(id, raw) => {
                self.icons.insert(id, raw);
            }
            Op::SetReminder { seconds, message } => self.set_reminder(seconds, &message),
            Op::ClearReminders => {
                if self.reminders.is_empty() {
                    return false;
                }
                self.store_reminders(Vec::new());
            }
            Op::ShowReminders => self.show_reminders(),
            Op::Sync(dnd, reminders) => return self.sync(dnd, reminders),
            Op::Dedupe(rules) => {
                self.phone.rules = rules;
                return false;
            }
            Op::CenterOpen(open) => {
                if self.center_open == open {
                    return false;
                }
                self.center_open = open;
                // Closing the centre files everything pending as seen,
                // however the card was closed (Escape, a click outside,
                // showHistory).
                if !open {
                    self.mark_all_seen();
                }
            }
            Op::Tick(now) => return self.tick(now),
        }
        true
    }

    /// Every entry the model holds, tier by tier, for `debug dump`'s
    /// `toasts`: a leg reads what one said rather than only how many there
    /// were.
    pub fn tiers(&self) -> Value {
        let m = &self.model;
        let entries: Vec<Value> = [("popup", &m.popups), ("pending", &m.pending), ("past", &m.past)]
            .into_iter()
            .flat_map(|(tier, list)| {
                list.iter().map(move |e| {
                    let actions: Vec<&str> = e.actions.iter().map(|a| a.label.as_str()).collect();
                    json!({"tier": tier, "summary": e.summary, "body": e.body, "urgency": u8::from(e.urgency), "actions": actions, "arrivedAt": e.arrived_at})
                })
            })
            .collect();
        Value::Array(entries)
    }

    pub fn find(&self, id: NotificationId) -> Option<&model::Entry> {
        self.all().find(|e| e.id == id)
    }

    fn all(&self) -> impl Iterator<Item = &model::Entry> {
        let m = &self.model;
        m.popups.iter().chain(&m.pending).chain(&m.past)
    }

    fn phone_dismissed(&self, id: NotificationId) {
        if let Some(e) = self.find(id) {
            iphone::card_closed(e);
        }
    }

    /// Drops pictures no entry holds any more.
    fn after_change(&mut self) {
        if self.icons.is_empty() {
            return;
        }
        let live: HashSet<NotificationId> = {
            let m = &self.model;
            m.popups.iter().chain(&m.pending).chain(&m.past).map(|e| e.id).collect()
        };
        self.icons.retain(|k, _| live.contains(k));
    }

    /// Mirrors state.json's dnd and reminders where they moved in the
    /// file. True when either did.
    fn sync(&mut self, dnd: bool, list: Value) -> bool {
        let mut moved = false;
        if self.dnd_seen != Some(dnd) {
            self.dnd_seen = Some(dnd);
            moved |= self.model.dnd != dnd;
            self.model.set_dnd(dnd);
        }
        if self.reminders_seen.as_ref() != Some(&list) {
            self.reminders = reminders::normalize(&list);
            self.reminders_seen = Some(list);
            moved = true;
        }
        moved
    }

    fn set_dnd(&mut self, on: bool) {
        self.model.set_dnd(on);
        state::set(vec![Field::Dnd(on)]);
    }

    /// A shell-authored entry, which earns the
    /// DND bypass at urgency 2 on its own `local` marker.
    fn notify(&mut self, summary: &str, body: &str, urgency: Urgency) {
        self.notify_with(summary, body, urgency, Vec::new(), String::new());
    }

    /// The same with actions and an image; the id comes back so the
    /// caller can match an invoked action to its entry.
    fn notify_with(&mut self, summary: &str, body: &str, urgency: Urgency, actions: Vec<Action>, image: String) -> NotificationId {
        self.local_serial += 1;
        let id = NotificationId::Local(self.local_serial);
        let notif = Notif {
            id,
            app_name: "formalshell".into(),
            summary: summary.into(),
            body: body.into(),
            urgency,
            actions,
            image,
            local: true,
            ..Default::default()
        };
        self.model.add(&notif, now_ms(), &AddOpts::default());
        id
    }

    /// [`State::notify_with`] whose actions run what they carry when picked.
    fn notify_local(&mut self, summary: &str, body: &str, urgency: Urgency, actions: Vec<LocalAction>, image: String) {
        let shown = actions.iter().map(|a| Action { key: a.key.clone(), label: a.label.clone() }).collect();
        let id = self.notify_with(summary, body, urgency, shown, image);
        if !actions.is_empty() {
            self.local_commands.insert(id, actions);
        }
    }

    fn dismiss_popup(&mut self, id: NotificationId) {
        self.hovered.remove(&id);
        if self.model.popups.iter().any(|p| p.id == id) {
            self.phone_dismissed(id);
        }
        self.model.dismiss_popup(id, now_ms());
        close(id, CloseReason::Dismissed);
    }

    fn dismiss_popup_group(&mut self, ids: &[NotificationId]) {
        for &id in ids {
            self.dismiss_popup(id);
        }
    }

    fn dismiss_one(&mut self, id: NotificationId) {
        self.phone_dismissed(id);
        self.model.dismiss_one(id);
        self.after_change();
    }

    fn dismiss_group(&mut self, ids: &[NotificationId]) {
        for &id in ids {
            self.dismiss_one(id);
        }
    }

    fn dismiss_all(&mut self) {
        self.model.dismiss_all();
        self.after_change();
    }

    fn clear_pending(&mut self) {
        self.model.clear_pending();
        self.after_change();
    }

    pub fn mark_all_seen(&mut self) {
        self.model.mark_all_seen(now_ms());
    }

    fn set_hovered(&mut self, ids: &[NotificationId], hovered: bool) {
        for id in ids {
            if hovered {
                self.hovered.insert(*id);
            } else {
                self.hovered.remove(id);
            }
        }
    }

    fn invoke_action(&mut self, id: NotificationId, key: &str) {
        match id {
            NotificationId::Local(_) => {
                let Some(a) = self.local_commands.get(&id).and_then(|acts| acts.iter().find(|a| a.key == key)) else { return };
                match &a.run {
                    Local::Spawn(argv) => super::compositor::spawn(argv),
                    Local::Capture(act) => self.capture_picked.push(act.clone()),
                }
            }
            NotificationId::Phone { .. } => {
                if let Some(phone) = self.find(id).and_then(iphone::phone_of) {
                    iphone::card_invoke(phone, key == "positive");
                }
            }
            NotificationId::Server(id) => invoke(id, key),
        }
    }

    /// The front toast's whole group, the one its close button closes:
    /// `None` when nothing is popped up.
    pub fn front_group(&self) -> Option<Vec<NotificationId>> {
        let groups = model::group_entries(&self.model.popups);
        model::stack_order(&groups).into_iter().next().map(|g| g.member_ids)
    }

    fn invoke_last(&mut self) {
        let Some(target) = model::invoke_target(&self.model).cloned() else { return };
        if target.actions.iter().any(|a| a.key == "default") {
            self.invoke_action(target.id, "default");
        }
        if self.model.popups.iter().any(|p| p.id == target.id) {
            self.dismiss_popup(target.id);
        } else {
            self.dismiss_one(target.id);
        }
    }

    /// The next instant something here starts on its own: a toast timing
    /// out, a seen entry aging out of the past tier, a reminder coming due.
    pub fn wake(&self, now: Instant) -> Option<Instant> {
        let past = self.model.past.iter().map(|p| p.seen_at.unwrap_or(p.arrived_at) + model::PAST_TTL_MS).min();
        let due = self.reminders.first().map(|r| r.due_at.ceil() as i64);
        [self.model.next_expiry, past, due].into_iter().flatten().min().map(|at| instant_at(at, now))
    }

    /// Runs whatever came due by `now`. True when anything moved.
    pub fn tick(&mut self, now: i64) -> bool {
        let mut moved = false;
        if self.model.next_expiry.is_some_and(|t| t <= now) {
            if !self.hovered.is_empty() {
                for p in &mut self.model.popups {
                    if self.hovered.contains(&p.id) && p.expires_at.is_some_and(|t| t != 0 && t <= now) {
                        p.expires_at = Some(now + HOVER_HOLD_MS);
                    }
                }
                self.model.next_expiry = self.model.popups.iter().filter_map(|p| p.expires_at.filter(|t| *t != 0)).min();
            }
            let before: Vec<NotificationId> = self.model.popups.iter().map(|p| p.id).collect();
            if self.model.expire(now) {
                for &id in before.iter().filter(|id| !self.model.popups.iter().any(|p| &p.id == *id)) {
                    self.hovered.remove(&id);
                    close(id, CloseReason::Expired);
                }
                moved = true;
            }
        }
        if self.model.prune_past(now) {
            self.after_change();
            moved = true;
        }
        let split = reminders::due(&self.reminders, now as f64);
        if !split.fired.is_empty() {
            for e in &split.fired {
                self.notify("Reminder", &e.message, Urgency::Critical);
            }
            self.store_reminders(split.remaining);
            moved = true;
        }
        moved
    }

    fn store_reminders(&mut self, list: Vec<reminders::Entry>) {
        let value = Value::Array(list.iter().map(reminder_json).collect());
        self.reminders = list;
        state::set(vec![Field::Reminders(value)]);
    }

    fn set_reminder(&mut self, seconds: u64, message: &str) {
        self.reminder_serial += 1;
        let entry = reminders::make_entry(seconds, message, now_ms() as f64, self.reminder_serial);
        self.newest_reminder = Some(entry.id.clone());
        let list = reminders::add(&self.reminders, entry);
        self.store_reminders(list);
    }

    /// The entry the last [`Op::SetReminder`] stored, while it is pending.
    pub fn newest_reminder(&self) -> Option<&reminders::Entry> {
        let id = self.newest_reminder.as_deref()?;
        self.reminders.iter().find(|e| e.id == id)
    }

    /// The id the last shell-authored entry was given.
    pub fn newest_local(&self) -> NotificationId {
        NotificationId::Local(self.local_serial)
    }

    fn show_reminders(&mut self) {
        if self.reminders.is_empty() {
            self.notify("Reminders", "None pending", Urgency::Normal);
            return;
        }
        let lines = reminders::summary_lines(&self.reminders, now_ms() as f64).join("\n");
        self.notify("Reminders", &lines, Urgency::Normal);
    }

    /// The IPC reminder `status` reply.
    pub fn reminder_status(&self) -> Value {
        let now = now_ms() as f64;
        let items: Vec<Value> = self
            .reminders
            .iter()
            .map(|e| {
                let secs = reminders::remaining_seconds(e, now);
                json!({
                    "id": e.id,
                    "message": e.message,
                    "dueAt": js::json_number(e.due_at),
                    "remainingSeconds": js::json_number(secs),
                    "remaining": reminders::countdown_label(secs),
                })
            })
            .collect();
        json!({"count": self.reminders.len(), "reminders": items})
    }
}

/// A reminder's message as typed, or `default` when it is blank.
pub fn reminder_message(message: &str, default: &str) -> String {
    let text = js::trim(message);
    if text.is_empty() { default } else { text }.to_owned()
}

fn reminder_json(e: &reminders::Entry) -> Value {
    json!({ "id": e.id, "message": e.message, "setAt": fs_js::json_number(e.set_at), "dueAt": fs_js::json_number(e.due_at) })
}

/// The instant `at` (epoch ms) falls on, on the monotonic clock.
fn instant_at(at: i64, now: Instant) -> Instant {
    let delta = at - now_ms();
    if delta <= 0 { now } else { now + Duration::from_millis(delta as u64) }
}

enum Cmd {
    Close(u32, CloseReason),
    Invoke(u32, String),
}

static CMD: OnceLock<async_channel::Sender<Cmd>> = OnceLock::new();

/// The largest scale any output draws at, as the UI thread last said: what
/// a themed icon is picked for.
static SCALE: LazyLock<(async_channel::Sender<f64>, async_channel::Receiver<f64>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread; only the latest is kept.
pub fn scale(largest: f64) {
    while SCALE.1.try_recv().is_ok() {}
    let _ = SCALE.0.try_send(largest);
}

fn close(id: NotificationId, reason: CloseReason) {
    if let (NotificationId::Server(id), Some(tx)) = (id, CMD.get()) {
        let _ = tx.try_send(Cmd::Close(id, reason));
    }
}

fn invoke(id: u32, key: &str) {
    if let Some(tx) = CMD.get() {
        let _ = tx.try_send(Cmd::Invoke(id, key.to_owned()));
    }
}

/// Every close the server confirms or the sender asks for ends the
/// notification's activity item, through here.
fn closed(id: u32) -> NotificationId {
    let id = NotificationId::Server(id);
    super::activity::notification_closed(id);
    id
}

/// The notification as Activity reads it: a `value` hint is its progress.
fn notice_of(n: &fs_notifd::Notification) -> fs_activity::Notice {
    let app = if n.desktop_entry.is_empty() { n.app_name.clone() } else { n.desktop_entry.clone() };
    fs_activity::Notice {
        id: NotificationId::Server(n.id),
        app,
        title: n.summary.clone(),
        body: n.body.clone(),
        value: n.value.map(|v| v.to_fraction()),
        urgent: n.urgency == fs_notifd::Urgency::Critical,
    }
}

/// The image hint: a decoded buffer gets a token of its
/// own, a path a file url, a name the icon provider's url.
fn image_of(n: &fs_notifd::Notification) -> (String, Option<Arc<ImageData>>) {
    match &n.image {
        None => (String::new(), None),
        Some(Image::Data(d)) => (format!("image://notification/{}", NotificationId::Server(n.id)), Some(Arc::new(d.clone()))),
        Some(Image::Path(p)) if p.starts_with('/') => (format!("file://{p}"), None),
        Some(Image::Path(p)) => (format!("image://icon/{p}"), None),
    }
}

/// The desktop entries the sender's icon is looked up in, rescanned when an
/// applications directory moved.
static ENTRIES: std::sync::Mutex<(Vec<Option<std::time::SystemTime>>, Vec<DesktopEntry>)> =
    std::sync::Mutex::new((Vec::new(), Vec::new()));

/// A card's `entry`: the `desktop-entry` hint's entry, else the
/// entry the sender's name heuristically matches, and its icon.
fn sender_icon(desktop_id: &str, app_name: &str) -> Option<String> {
    let dirs = applications_dirs();
    let stamp: Vec<_> = dirs.iter().map(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok()).collect();
    let mut cached = ENTRIES.lock().ok()?;
    if cached.0 != stamp || cached.1.is_empty() {
        cached.1 = scan_launchable(&dirs);
        cached.0 = stamp;
    }
    let by = |name: &str| {
        let win = Window { app_id: name.to_owned(), ..Window::default() };
        (!name.is_empty()).then(|| by_class(Some(&win), &cached.1)).flatten()
    };
    by(desktop_id).or_else(|| by(app_name)).map(|e| e.icon.clone())
}

/// The icon order, resolved to pixels: the notification's own image, its
/// app icon, then nothing (the card's bell). A themed name no theme
/// carries resolves to nothing rather than to a broken picture.
fn resolve_icon(image: &str, app_icon: &str, desktop_entry: &str, app_name: &str, pixels: Option<&ImageData>, largest: f64) -> Option<icons::Raw> {
    if let Some(d) = pixels {
        return icons::from_rgba(d.width as i32, d.height as i32, &d.rgba);
    }
    let source = fs_info::notification_icon::resolve(
        &fs_info::notification_icon::IconSource {
            image: image.into(),
            app_icon: app_icon.into(),
            desktop_entry: desktop_entry.into(),
            app_name: app_name.into(),
        },
        |name| if icons::lookup(name, "", ICON_PICK).is_some() { format!("image://icon/{name}") } else { String::new() },
        sender_icon,
    );
    if let Some(name) = source.strip_prefix("image://icon/") {
        return icons::named(name, "", icons::pick(ICON_PICK, largest));
    }
    let path = source.strip_prefix("file://")?;
    icons::load(std::path::Path::new(path))
}

fn arrival(n: &fs_notifd::Notification, replaced: bool) -> Arrival {
    let urgency = n.urgency;
    let (image, _) = image_of(n);
    Arrival {
        notif: Notif {
            id: NotificationId::Server(n.id),
            app_name: n.app_name.clone(),
            app_icon: n.app_icon.clone(),
            desktop_entry: n.desktop_entry.clone(),
            summary: n.summary.clone(),
            body: n.body.clone(),
            urgency,
            actions: n.actions.iter().map(|a| Action { key: a.key.clone(), label: a.label.clone() }).collect(),
            image,
            // Omarchy's rule: the CLI's literal default app name, nothing
            // inferred from urgency alone.
            sender_is_notify_send: n.app_name == "notify-send",
            ..Default::default()
        },
        timeout_ms: timeout_for(urgency, n.expire_timeout),
        icon: None,
        replaced,
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (server, events) = match Server::start(env!("CARGO_PKG_VERSION")).await {
        Ok(s) => s,
        Err(err) => {
            eprintln!("notifications: {err}");
            ctx.publish(store::Diff::Notifications(Diff::Unavailable(err.to_string())));
            return;
        }
    };
    let (tx, rx) = async_channel::unbounded();
    let _ = CMD.set(tx);
    let server = std::rc::Rc::new(server);
    let s = server.clone();
    let c = ctx.clone();
    ctx.spawn(async move {
        while let Ok(cmd) = rx.recv().await {
            match cmd {
                Cmd::Close(id, reason) => {
                    if let Ok(true) = s.close(id, reason).await {
                        closed(id);
                    }
                }
                Cmd::Invoke(id, key) => {
                    if let Ok(true) = s.invoke_action(id, &key, None).await {
                        c.publish(store::Diff::Notifications(Diff::Closed(closed(id))));
                    }
                }
            }
        }
    });
    let mut largest = 1.0;
    while let Ok(event) = events.recv().await {
        let (n, replaced) = match event {
            Event::Notified(n) => (n, false),
            Event::Replaced(n) => (n, true),
            Event::Closed { id } => {
                ctx.publish(store::Diff::Notifications(Diff::Closed(closed(id))));
                continue;
            }
        };
        super::activity::notification(notice_of(&n));
        let mut a = arrival(&n, replaced);
        let (image, pixels) = image_of(&n);
        let (app_icon, entry, name) = (n.app_icon.clone(), n.desktop_entry.clone(), n.app_name.clone());
        while let Ok(s) = SCALE.1.try_recv() {
            largest = s;
        }
        a.icon = ctx.pool().run(move || resolve_icon(&image, &app_icon, &entry, &name, pixels.as_deref(), largest)).await.flatten();
        ctx.publish(store::Diff::Notifications(Diff::Arrived(Box::new(a))));
    }
}

/// The IPC `status` reply, the centre's numbers handed in by the
/// surface that owns them.
pub fn status(s: &State, center: (f64, f64, bool)) -> Value {
    json!({
        "dnd": s.model.dnd,
        "pending": s.model.pending.len(),
        "popups": s.model.popups.len(),
        "centerOpen": s.center_open,
        "centerHeight": js::json_number(js::round(center.0)),
        "centerMaxHeight": js::json_number(js::round(center.1)),
        "centerCapped": center.2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_bands() {
        assert_eq!(timeout_for(Urgency::Low, -1), 5000);
        assert_eq!(timeout_for(Urgency::Normal, 0), 8000);
        assert_eq!(timeout_for(Urgency::Normal, 12000), 12000);
        assert_eq!(timeout_for(Urgency::Normal, 90000), 30000);
    }

    #[test]
    fn a_reminder_fires_through_dnd_into_the_popups() {
        let mut s = State::default();
        s.model.set_dnd(true);
        s.reminders = vec![reminders::make_entry(1, "m", 0.0, 1)];
        assert!(s.tick(5000));
        assert!(s.reminders.is_empty());
        assert_eq!(s.model.popups.len(), 1);
        assert_eq!(s.model.popups[0].summary, "Reminder");
    }

    fn sms(id: i64) -> iphone::Notification {
        iphone::Notification {
            id,
            bundle_id: "com.apple.MobileSMS".into(),
            app_name: "Messages".into(),
            title: "Alex".into(),
            body: "Still on for Friday?".into(),
            session: 77,
            ..Default::default()
        }
    }

    fn local(id: u32) -> Diff {
        Diff::Arrived(Box::new(Arrival {
            notif: Notif {
                id: NotificationId::Server(id),
                app_name: "Messages".into(),
                summary: "Alex".into(),
                body: "Still on for Friday?".into(),
                ..Default::default()
            },
            timeout_ms: NORMAL_MS,
            icon: None,
            replaced: false,
        }))
    }

    fn mirrored() -> State {
        let mut s = State::default();
        s.phone.rules = crate::settings::iphone_dedupe(&Value::Null);
        s
    }

    #[test]
    fn a_local_arrival_replaces_the_phone_card() {
        let mut s = mirrored();
        s.apply(Diff::Phone { record: sms(4012), quiet: false });
        assert_eq!(s.model.popups.len(), 1);
        s.apply(local(9));
        assert_eq!(s.model.popups.iter().map(|p| p.id).collect::<Vec<_>>(), [NotificationId::Server(9)]);
    }

    #[test]
    fn the_phone_copy_of_a_local_message_is_dropped() {
        let mut s = mirrored();
        s.apply(local(9));
        s.apply(Diff::Phone { record: sms(4012), quiet: false });
        assert_eq!(s.model.popups.len(), 1);
    }

    #[test]
    fn a_quiet_phone_arrival_files_into_pending_and_the_phone_can_withdraw_it() {
        let mut s = mirrored();
        s.apply(Diff::Phone { record: sms(4012), quiet: true });
        assert_eq!((s.model.popups.len(), s.model.pending.len()), (0, 1));
        assert_eq!(s.model.pending[0].source, "iphone");
        s.apply(Diff::DropPhone(4012));
        assert!(s.model.pending.is_empty());
    }

    #[test]
    fn a_resent_phone_id_updates_its_card() {
        let mut s = mirrored();
        s.apply(Diff::Phone { record: sms(4012), quiet: false });
        s.apply(Diff::Phone { record: iphone::Notification { body: "Moved to Saturday".into(), ..sms(4012) }, quiet: false });
        assert_eq!(s.model.popups.len(), 1);
        assert_eq!(s.model.popups[0].body, "Moved to Saturday");
    }

    #[test]
    fn a_hovered_toast_holds() {
        let mut s = State::default();
        s.notify("a", "b", Urgency::Normal);
        let id = s.model.popups[0].id;
        let at = s.model.next_expiry.unwrap();
        s.set_hovered(std::slice::from_ref(&id), true);
        s.tick(at);
        assert_eq!(s.model.popups.len(), 1);
        s.set_hovered(&[id], false);
        s.tick(at + HOVER_HOLD_MS);
        assert_eq!(s.model.popups.len(), 0);
    }
}
