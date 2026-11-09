//! Three-tier notification state machine: popups (visible toasts), then
//! pending (unseen, waiting in the history center), then past (seen, pruned
//! after 15 minutes). Every function takes state in and returns state out;
//! the clock is always an argument and the input is never mutated.
//!
//! Times are milliseconds since the epoch.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use fs_js as js;
pub use fs_types::Urgency;

/// Caps GROUPS on screen, not raw entries: five repeats of one notification
/// must not evict four unrelated toasts.
pub(crate) const MAX_POPUPS: usize = 4;
pub(crate) const DEFAULT_TIMEOUT_MS: i64 = 6000;
pub const PAST_TTL_MS: i64 = 15 * 60 * 1000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub key: String,
    pub label: String,
}

/// What the server layer hands `add`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notif {
    pub id: String,
    pub app_name: String,
    pub app_icon: String,
    pub desktop_entry: String,
    pub summary: String,
    pub body: String,
    pub urgency: Urgency,
    pub actions: Vec<Action>,
    pub image: String,
    /// Set by the server layer from the sender's app info, never inferred
    /// from urgency alone.
    pub sender_is_notify_send: bool,
    pub local: bool,
    /// "iphone" for a notification mirrored off the phone, "" otherwise.
    pub source: String,
    /// The phone bridge's own record, for a mirrored notification.
    pub phone: Option<Phone>,
}

/// What a card mirrored off the iPhone keeps of the bridge's record: the
/// fields dedupe compares, and the handle, id and session an action on the
/// card is sent back with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phone {
    pub id: i64,
    pub session: i64,
    pub bundle_id: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub device_handle: String,
    pub negative_action: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub app_name: String,
    pub app_icon: String,
    /// The sender's `desktop-entry` hint, kept beside `app_icon` because the
    /// icon lookup falls back to that entry's own icon when neither the image
    /// nor the app icon resolves.
    pub desktop_entry: String,
    pub summary: String,
    pub body: String,
    pub urgency: Urgency,
    pub actions: Vec<Action>,
    pub image: String,
    pub local: bool,
    pub source: String,
    pub phone: Option<Phone>,
    pub arrived_at: i64,
    pub seen_at: Option<i64>,
    /// `None` is "never scheduled" (an entry filed straight into pending),
    /// `Some(0)` a sticky popup that never times out.
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub popups: Vec<Entry>,
    pub pending: Vec<Entry>,
    pub past: Vec<Entry>,
    pub dnd: bool,
    pub next_expiry: Option<i64>,
}

/// A repeat collapsed into one row: a copy of the group's newest member with
/// the group's size and every member id, oldest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub entry: Entry,
    pub count: usize,
    pub member_ids: Vec<String>,
}

impl Deref for Group {
    type Target = Entry;

    fn deref(&self) -> &Entry {
        &self.entry
    }
}

#[derive(Clone, Debug, Default)]
pub struct AddOpts {
    /// Files the entry straight into pending, the way DND does, for an
    /// arrival the sender asked to be delivered without a toast.
    pub quiet: bool,
    pub timeout_ms: Option<i64>,
}

/// The fields a `replaces_id` update can change.
#[derive(Clone, Debug, Default)]
pub struct Patch {
    pub app_name: Option<String>,
    pub app_icon: Option<String>,
    pub desktop_entry: Option<String>,
    pub summary: Option<String>,
    pub body: Option<String>,
    pub urgency: Option<Urgency>,
    pub actions: Option<Vec<Action>>,
    pub image: Option<String>,
    pub phone: Option<Phone>,
}

impl State {
    pub fn new() -> State {
        State::default()
    }
}

/// Omarchy's narrow bypass: only urgency=critical notifications from
/// notify-send itself get through DND. A chat app marking its messages
/// critical does not qualify. A shell-authored local entry (the critical
/// battery warning) earns the same bypass on its own honest `local` marker,
/// it never claims to be notify-send.
pub fn bypasses_dnd(notif: &Notif) -> bool {
    notif.urgency == Urgency::Critical && (notif.sender_is_notify_send || notif.local)
}

fn make_entry(notif: &Notif, now: i64, expires_at: Option<i64>) -> Entry {
    Entry {
        id: notif.id.clone(),
        app_name: notif.app_name.clone(),
        app_icon: notif.app_icon.clone(),
        desktop_entry: notif.desktop_entry.clone(),
        summary: notif.summary.clone(),
        body: notif.body.clone(),
        urgency: notif.urgency,
        actions: notif.actions.clone(),
        image: notif.image.clone(),
        local: notif.local,
        source: notif.source.clone(),
        phone: notif.phone.clone(),
        arrived_at: now,
        seen_at: None,
        expires_at,
    }
}

/// `None` once there are no timed popups left; a sticky (`Some(0)`) critical
/// popup never contributes a deadline.
fn recompute_next_expiry(popups: &[Entry]) -> Option<i64> {
    popups
        .iter()
        .filter_map(|p| p.expires_at.filter(|t| *t != 0))
        .min()
}

/// Identical means same appName (case- and surrounding-whitespace-insensitive)
/// plus same summary. Body is deliberately out of the key: the case grouping
/// exists for is a chat app firing one summary with a different body per
/// message, and keying on body too would degenerate to no grouping at all
/// there. The NUL separator can't appear in either field, so no
/// appName/summary pair can collide with a different one. The source leads
/// the key: a phone mirror of an app shares its name with the desktop client
/// of the same app, and folding the two into one card would hide which device
/// a message is on.
pub fn group_key(entry: &Entry) -> String {
    format!(
        "{}\u{0}{}\u{0}{}",
        entry.source,
        js::trim(&entry.app_name).to_lowercase(),
        entry.summary
    )
}

/// Collapses repeats into one row each. Group order follows each group's
/// newest member's position in `entries`, so a repeat moves its whole group to
/// the newest slot instead of adding a row, and a newest-first input stays
/// newest-first on the way out. The newest member is the one with the strictly
/// greatest `arrived_at`; a tie keeps whichever came first in `entries`.
///
/// Grouping is derived here, never stored: every entry keeps its own server
/// id and its own `expires_at`, so each member still times out on its own
/// clock and its sender is told when it closes.
pub fn group_entries(entries: &[Entry]) -> Vec<Group> {
    struct Acc {
        members: Vec<usize>,
        newest: usize,
    }
    let mut groups: Vec<Acc> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();

    for (index, entry) in entries.iter().enumerate() {
        match by_key.get(&group_key(entry)) {
            None => {
                by_key.insert(group_key(entry), groups.len());
                groups.push(Acc { members: vec![index], newest: index });
            }
            Some(&g) => {
                let acc = &mut groups[g];
                if entry.arrived_at > entries[acc.newest].arrived_at {
                    acc.newest = index;
                }
                acc.members.push(index);
            }
        }
    }

    groups.sort_by_key(|g| g.newest);

    groups
        .into_iter()
        .map(|mut g| {
            g.members
                .sort_by_key(|&i| (entries[i].arrived_at, i));
            Group {
                entry: entries[g.newest].clone(),
                count: g.members.len(),
                member_ids: g.members.iter().map(|&i| entries[i].id.clone()).collect(),
            }
        })
        .collect()
}

pub fn add(state: &State, notif: &Notif, now: i64, opts: &AddOpts) -> State {
    if opts.quiet || (state.dnd && !bypasses_dnd(notif)) {
        let mut next = state.clone();
        next.pending.push(make_entry(notif, now, None));
        return next;
    }

    let timeout = opts.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    let expires_at = if notif.urgency == Urgency::Critical { 0 } else { now + timeout };
    let mut popups = state.popups.clone();
    popups.push(make_entry(notif, now, Some(expires_at)));
    let mut pending = state.pending.clone();

    // The whole oldest GROUP moves to pending together, order preserved in
    // both tiers, so a repeat never costs an unrelated toast its slot.
    let groups = group_entries(&popups);
    if groups.len() > MAX_POPUPS {
        let evicted: HashSet<&str> = groups[..groups.len() - MAX_POPUPS]
            .iter()
            .flat_map(|g| g.member_ids.iter().map(String::as_str))
            .collect();
        pending.extend(popups.iter().filter(|p| evicted.contains(p.id.as_str())).cloned());
        popups.retain(|p| !evicted.contains(p.id.as_str()));
    }

    State {
        next_expiry: recompute_next_expiry(&popups),
        popups,
        pending,
        ..state.clone()
    }
}

fn apply(entry: &mut Entry, patch: &Patch) {
    if let Some(v) = &patch.app_name {
        entry.app_name.clone_from(v);
    }
    if let Some(v) = &patch.app_icon {
        entry.app_icon.clone_from(v);
    }
    if let Some(v) = &patch.desktop_entry {
        entry.desktop_entry.clone_from(v);
    }
    if let Some(v) = &patch.summary {
        entry.summary.clone_from(v);
    }
    if let Some(v) = &patch.body {
        entry.body.clone_from(v);
    }
    if let Some(v) = patch.urgency {
        entry.urgency = v;
    }
    if let Some(v) = &patch.actions {
        entry.actions.clone_from(v);
    }
    if let Some(v) = &patch.image {
        entry.image.clone_from(v);
    }
    if let Some(v) = &patch.phone {
        entry.phone = Some(v.clone());
    }
}

/// Applies a `replaces_id` update (the server mutates the existing
/// notification in place instead of emitting a new one) to whichever tier
/// currently holds the entry, without moving it between tiers or touching
/// `arrived_at`/`seen_at`. A still-popped-up entry gets `expires_at`
/// recomputed from the patched urgency exactly like `add` does, so a later
/// update isn't silently cut off by the original arrival's clock.
pub fn update(state: &State, id: &str, patch: &Patch, now: i64) -> State {
    let mut next = state.clone();
    if let Some(entry) = next.popups.iter_mut().find(|p| p.id == id) {
        apply(entry, patch);
        entry.expires_at = Some(if entry.urgency == Urgency::Critical {
            0
        } else {
            now + DEFAULT_TIMEOUT_MS
        });
        next.next_expiry = recompute_next_expiry(&next.popups);
        return next;
    }
    if let Some(entry) = next.pending.iter_mut().find(|p| p.id == id) {
        apply(entry, patch);
        return next;
    }
    if let Some(entry) = next.past.iter_mut().find(|p| p.id == id) {
        apply(entry, patch);
        return next;
    }
    next
}

pub fn expire(state: &State, now: i64) -> State {
    let (timed_out, popups): (Vec<Entry>, Vec<Entry>) = state.popups.iter().cloned().partition(|p| match p.expires_at {
        Some(0) => false,
        Some(t) => t <= now,
        None => now >= 0,
    });
    if timed_out.is_empty() {
        return state.clone();
    }
    let mut pending = state.pending.clone();
    pending.extend(timed_out);
    State {
        next_expiry: recompute_next_expiry(&popups),
        popups,
        pending,
        ..state.clone()
    }
}

/// Only removes from the popups tier, marking the entry seen on its way to
/// past: the toast surface's dismiss ("X" cell, or the click that
/// acknowledges a popup still on screen).
pub fn dismiss_popup(state: &State, id: &str, now: i64) -> State {
    let Some(idx) = state.popups.iter().position(|p| p.id == id) else {
        return state.clone();
    };
    let mut popups = state.popups.clone();
    let mut entry = popups.remove(idx);
    entry.seen_at = Some(now);
    let mut past = state.past.clone();
    past.push(entry);
    State {
        next_expiry: recompute_next_expiry(&popups),
        popups,
        past,
        ..state.clone()
    }
}

pub fn mark_all_seen(state: &State, now: i64) -> State {
    if state.pending.is_empty() {
        return state.clone();
    }
    let mut past = state.past.clone();
    past.extend(state.pending.iter().cloned().map(|mut e| {
        e.seen_at = Some(now);
        e
    }));
    State { pending: Vec::new(), past, ..state.clone() }
}

pub fn prune_past(state: &State, now: i64) -> State {
    let cutoff = now - PAST_TTL_MS;
    let kept: Vec<Entry> = state
        .past
        .iter()
        .filter(|p| p.seen_at.unwrap_or(p.arrived_at) >= cutoff)
        .cloned()
        .collect();
    if kept.len() == state.past.len() {
        return state.clone();
    }
    State { past: kept, ..state.clone() }
}

fn drop_ids(state: &State, wanted: &HashSet<&str>) -> State {
    let keep = |p: &&Entry| !wanted.contains(p.id.as_str());
    let popups: Vec<Entry> = state.popups.iter().filter(keep).cloned().collect();
    let pending: Vec<Entry> = state.pending.iter().filter(keep).cloned().collect();
    let past: Vec<Entry> = state.past.iter().filter(keep).cloned().collect();
    if popups.len() == state.popups.len()
        && pending.len() == state.pending.len()
        && past.len() == state.past.len()
    {
        return state.clone();
    }
    let next_expiry = if popups.len() == state.popups.len() {
        state.next_expiry
    } else {
        recompute_next_expiry(&popups)
    };
    State { popups, pending, past, next_expiry, ..state.clone() }
}

/// General-purpose delete by id from whichever tier holds it (the history
/// center's per-row dismiss, on pending or past rows). Unlike
/// `dismiss_popup`, this drops the entry outright rather than promoting it.
pub fn dismiss_one(state: &State, id: &str) -> State {
    drop_ids(state, &HashSet::from([id]))
}

/// `dismiss_one` for a whole group: the history center's per-row dismiss,
/// where one row can stand for several notifications.
pub fn dismiss_many<S: AsRef<str>>(state: &State, ids: &[S]) -> State {
    drop_ids(state, &ids.iter().map(AsRef::as_ref).collect())
}

pub fn dismiss_all(state: &State) -> State {
    if state.popups.is_empty() {
        return state.clone();
    }
    State { popups: Vec::new(), next_expiry: None, ..state.clone() }
}

pub fn clear_pending(state: &State) -> State {
    if state.pending.is_empty() {
        return state.clone();
    }
    State { pending: Vec::new(), ..state.clone() }
}

pub fn set_dnd(state: &State, on: bool) -> State {
    State { dnd: on, ..state.clone() }
}

/// Not a reducer step: the entry `invokeLast` acts on, the most recently
/// arrived notification still live in popups or pending.
pub fn invoke_target(state: &State) -> Option<&Entry> {
    state
        .popups
        .iter()
        .chain(&state.pending)
        .fold(None, |latest: Option<&Entry>, e| match latest {
            Some(l) if e.arrived_at <= l.arrived_at => Some(l),
            _ => Some(e),
        })
}

// Chromium-derived sender detection and the body prefix strip below are
// ported from omarchy's NotificationLogic.js (MIT, Copyright (c) David
// Heinemeier Hansson): GitHub web notifications (and any other Chromium
// browser notification) arrive with a URL-as-link or bare-URL line glued to
// the front of the body, which is what made GH notifs unreadable.
const CHROMIUM_MARKERS: [&str; 5] = ["chrom", "brave", "vivaldi", "microsoft-edge", "opera"];

pub fn is_chromium_derived(app_name: &str, app_icon: &str) -> bool {
    let source = format!("{app_name}\n{app_icon}").to_lowercase();
    CHROMIUM_MARKERS.iter().any(|m| source.contains(m))
}

/// `<img>` tags are always stripped (the card renders the notification's own
/// image through a dedicated icon slot, never inline in body text); the
/// Chromium URL-prefix strip only applies once `is_chromium_derived` matches.
pub fn sanitize_body(body: &str, app_name: &str, app_icon: &str) -> String {
    static IMG_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<img[^>]*>").unwrap());
    static LINK_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)^\s*<a\b[^>]*>\s*(?:https?://|www\.)?(?:[a-z0-9-]+\.)+[a-z]{2,}(?::[0-9]+)?(?:/[^<\s]*)?\s*</a>\s*",
        )
        .unwrap()
    });
    static BARE_URL_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^\s*(?:https?://|www\.)?(?:[a-z0-9-]+\.)+[a-z]{2,}(?::[0-9]+)?(?:/\S*)?\s+")
            .unwrap()
    });
    let text = IMG_TAG.replace_all(body, "");
    if !is_chromium_derived(app_name, app_icon) {
        return text.into_owned();
    }
    let text = LINK_PREFIX.replace(&text, "");
    BARE_URL_PREFIX.replace(&text, "").into_owned()
}

/// now/Nm/Nh/Nd ago, each unit's own upper bound exclusive so an exact hour
/// reads "1h ago" rather than "60m ago".
pub fn rel_time(now_ms: i64, arrived_at_ms: i64) -> String {
    let diff = (now_ms - arrived_at_ms).max(0);
    let minute = 60 * 1000;
    let hour = 60 * minute;
    let day = 24 * hour;
    if diff < minute {
        "now".into()
    } else if diff < hour {
        format!("{}m ago", diff / minute)
    } else if diff < day {
        format!("{}h ago", diff / hour)
    } else {
        format!("{}d ago", diff / day)
    }
}

/// The actions a card draws as buttons. `default` is the freedesktop
/// activation hint, not a button: the spec lets a sender give it no label at
/// all, and every surface here already invokes it on a body click. Ghostty
/// sends exactly that pair, which is where the empty pill came from. A
/// labelless ordinary action is dropped for the same reason: an unlabelled
/// button says nothing and cannot be told apart from its neighbour.
pub fn button_actions(entry: &Entry) -> Vec<Action> {
    entry
        .actions
        .iter()
        .filter(|a| a.key != "default" && !js::trim(&a.label).is_empty())
        .cloned()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    TopRight,
    BottomRight,
    BottomLeft,
    TopLeft,
}

impl Position {
    pub fn as_str(self) -> &'static str {
        match self {
            Position::TopRight => "top-right",
            Position::BottomRight => "bottom-right",
            Position::BottomLeft => "bottom-left",
            Position::TopLeft => "top-left",
        }
    }
}

pub const DEFAULT_POSITION: Position = Position::BottomRight;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionSpec {
    pub name: Position,
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

/// Config-driven popup corner (`notifications.position`, one of the four
/// screen corners). An unknown or missing name falls back to the shipped
/// default rather than erroring.
pub fn position_spec(name: Option<&str>) -> PositionSpec {
    let pos = match name {
        Some("top-right") => Position::TopRight,
        Some("bottom-right") => Position::BottomRight,
        Some("bottom-left") => Position::BottomLeft,
        Some("top-left") => Position::TopLeft,
        _ => DEFAULT_POSITION,
    };
    let top = matches!(pos, Position::TopRight | Position::TopLeft);
    let right = matches!(pos, Position::TopRight | Position::BottomRight);
    PositionSpec {
        name: pos,
        top,
        bottom: !top,
        left: !right,
        right,
    }
}

/// What `stack_order` needs to rank a toast group.
pub trait Ranked {
    fn arrived_at(&self) -> i64;
    fn urgency(&self) -> Urgency;
}

impl Ranked for Entry {
    fn arrived_at(&self) -> i64 {
        self.arrived_at
    }

    fn urgency(&self) -> Urgency {
        self.urgency
    }
}

impl Ranked for Group {
    fn arrived_at(&self) -> i64 {
        self.entry.arrived_at
    }

    fn urgency(&self) -> Urgency {
        self.entry.urgency
    }
}

/// Collapsed-stack front-to-back order: newest group first, EXCEPT a
/// critical group always wins the front slot over a newer normal one,
/// "urgency outranks recency at a glance". Ties (two criticals) resolve by
/// recency like everything else. Index 0 is the front card, 1 and 2 the two
/// peek levels, the rest present only in the count the expanded stack
/// reveals.
pub fn stack_order<T: Ranked + Clone>(entries: &[T]) -> Vec<T> {
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.arrived_at()));
    match sorted.iter().position(|e| e.urgency() == Urgency::Critical) {
        None | Some(0) => sorted,
        Some(i) => {
            let front = sorted.remove(i);
            sorted.insert(0, front);
            sorted
        }
    }
}

/// Which freedesktop sound a notification asks for, under
/// `notifications.sound` (elementary's own map). Urgency outranks the
/// category: a critical notification is a warning whatever it is about, and an
/// instant message is the one category with a sound of its own. Everything
/// else, including a notification carrying no category hint at all, is the
/// plain information sound.
///
/// A name, never a path: `canberra-gtk-play -i` resolves it against the
/// installed sound theme, so a session with none plays nothing and says so
/// rather than this file guessing at a file name.
pub fn sound_name(urgency: Urgency, category: Option<&str>) -> &'static str {
    if urgency == Urgency::Critical {
        "dialog-warning"
    } else if category == Some("im.received") {
        "message-new-instant"
    } else {
        "dialog-information"
    }
}
