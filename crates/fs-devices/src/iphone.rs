// Portions from omarchy-iphone (MIT, Copyright (c) 2026 kbbahaPro)

//! Pure model for the iPhone mirror (plan at
//! `docs/superpowers/plans/2026-09-28-m75-iphone.md`). Ported from
//! `shell/Iphone/model.js`.
//!
//! The wire is `omarchy-iphone-bridge listen` (omarchy-iphone @ 586f37d,
//! bin/omarchy-iphone-bridge): one JSON object per stdout line, `type` one of
//! history, notification, dismiss, status, pairingCode, advertising, forgot,
//! error, hci. packaging/iphone-bridge-bond.patch adds `paired`/`address` to status
//! and the `forgot` event. A notification's `id` is ancs4linux's
//! per-connection id (the ANCS uid offset by a random base chosen per
//! connection), so it only means anything inside the `session` it arrived in.

use std::sync::LazyLock;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::Regex;
use serde_json::{Value as Json, json};
use unicode_normalization::UnicodeNormalization;

use crate::bluetooth;
use fs_js::{self as js, SPACE};

pub mod category {
    pub const OTHER: i64 = 0;
    pub const INCOMING_CALL: i64 = 1;
    pub const MISSED_CALL: i64 = 2;
    pub const VOICEMAIL: i64 = 3;
    pub const SOCIAL: i64 = 4;
}

pub const DEFAULT_FOCUS_WINDOW: f64 = 900.0;
pub const FOCUS_MODES: [&str; 3] = ["respect", "hide", "ignore"];

pub fn category_label(category: i64) -> &'static str {
    match category {
        1 => "Incoming call",
        2 => "Missed call",
        3 => "Voicemail",
        4 => "Social",
        5 => "Schedule",
        6 => "Email",
        7 => "News",
        8 => "Health",
        9 => "Finance",
        10 => "Location",
        11 => "Entertainment",
        _ => "",
    }
}

/// settings.json's default `iphone.notifications.dedupe`.
pub fn default_dedupe() -> Json {
    json!([{ "phone": "com.apple.MobileSMS", "local": ["Messages", "es.canarycoders.messages"], "window": 30 }])
}

/// The bridge's notification object, every field defaulted. `positive_action`
/// and `negative_action` are the phone's own labels ("Clear", "Answer",
/// "Decline"), "" when the phone offers no such action.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notification {
    pub id: i64,
    pub bundle_id: String,
    pub app_name: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub device_name: String,
    pub device_handle: String,
    pub positive_action: String,
    pub negative_action: String,
    pub category: i64,
    pub category_count: i64,
    pub silent: bool,
    pub important: bool,
    pub preexisting: bool,
    pub session: i64,
    /// Epoch seconds, the phone's own when ANCS sent one.
    pub ts: f64,
}

/// The bridge's `status` line. `installed` is left out on purpose: the bridge
/// answers it by looking for ancs4linux-observer under /usr/bin and
/// /usr/local/bin, which is false on every NixOS host. `observer` (the daemon
/// owning its bus name) is the honest signal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub observer: bool,
    /// The GATT link ANCS and AMS ride is up, over LE or BR/EDR.
    pub connected: bool,
    /// Any link at all: an audio link with GATT down still counts.
    pub linked: bool,
    pub paired: bool,
    pub address: String,
    pub device_name: String,
    /// 0..100, -1 when unknown.
    pub battery: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Notification(Notification),
    History(Vec<Notification>),
    Dismiss { id: i64 },
    Status(Status),
    PairingCode { code: String },
    Advertising { hci: String, name: String },
    Forgot { address: String },
    Error { message: String },
}

fn s(raw: &Json, key: &str) -> String {
    js::opt_str(js::field(raw, key))
}

fn n(raw: &Json, key: &str) -> i64 {
    js::int(js::field(raw, key))
}

fn is_true(raw: &Json, key: &str) -> bool {
    js::field(raw, key) == Some(&Json::Bool(true))
}

fn notification(raw: &Json) -> Notification {
    let app_id = s(raw, "appId");
    let app_name = match s(raw, "appName") {
        name if !name.is_empty() => name,
        _ if !app_id.is_empty() => app_id.clone(),
        _ => "iPhone".to_string(),
    };
    let ts = js::to_number(js::field(raw, "ts"));
    Notification {
        id: n(raw, "id"),
        bundle_id: app_id,
        app_name,
        title: s(raw, "title"),
        subtitle: s(raw, "subtitle"),
        body: s(raw, "body"),
        device_name: s(raw, "deviceName"),
        device_handle: s(raw, "deviceHandle"),
        positive_action: s(raw, "positiveAction"),
        negative_action: s(raw, "negativeAction"),
        category: n(raw, "category"),
        category_count: n(raw, "categoryCount"),
        silent: is_true(raw, "silent"),
        important: is_true(raw, "important"),
        preexisting: is_true(raw, "preexisting"),
        session: n(raw, "session"),
        ts: if ts > 0.0 { ts } else { 0.0 },
    }
}

/// One stdout line to one event, or `None` for a blank line, bad JSON or a
/// type this shell does not act on.
pub fn parse_event(line: &str) -> Option<Event> {
    let text = js::trim(line);
    if text.is_empty() {
        return None;
    }
    let raw: Json = serde_json::from_str(text).ok()?;
    if !js::is_object_like(&raw) {
        return None;
    }
    match js::field(&raw, "type")?.as_str()? {
        "notification" => Some(Event::Notification(notification(&raw))),
        "history" => Some(Event::History(match js::field(&raw, "items") {
            Some(Json::Array(items)) => items.iter().filter(|i| js::is_object_like(i)).map(notification).collect(),
            _ => Vec::new(),
        })),
        "dismiss" => Some(Event::Dismiss { id: n(&raw, "id") }),
        "status" => {
            let battery = js::to_number(js::field(&raw, "battery"));
            Some(Event::Status(Status {
                observer: is_true(&raw, "observer"),
                connected: is_true(&raw, "connected"),
                linked: is_true(&raw, "linked"),
                paired: is_true(&raw, "paired"),
                address: s(&raw, "address").to_uppercase(),
                device_name: s(&raw, "deviceName"),
                battery: if battery.is_finite() && battery >= 0.0 { battery.min(100.0) } else { -1.0 },
            }))
        }
        "pairingCode" => Some(Event::PairingCode { code: s(&raw, "code") }),
        "advertising" => Some(Event::Advertising { hci: s(&raw, "hci"), name: s(&raw, "name") }),
        "forgot" => Some(Event::Forgot { address: s(&raw, "address").to_uppercase() }),
        "error" => Some(Event::Error { message: explain_error(&s(&raw, "message")) }),
        _ => None,
    }
}

static NOT_CONNECTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i-u)\bNot connected\b").unwrap());
static REFUSED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?-u)org\.bluez\.Error\.(AuthenticationFailed|AuthenticationRejected|NotPermitted)\b").unwrap()
});
static GDBUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"GDBus\.Error:[A-Za-z0-9_.]+: ").unwrap());

/// BlueZ's errors reach the bridges as GLib's own rendering, e.g.
/// "subscribe: g-io-error-quark: GDBus.Error:org.bluez.Error.Failed: Not
/// connected (36)". The two BlueZ states a person can act on get a sentence;
/// anything else keeps its step and BlueZ's message without the D-Bus
/// wrapping.
pub fn explain_error(message: &str) -> String {
    let text = js::trim(message);
    if text.is_empty() {
        return "Unknown error".into();
    }
    if NOT_CONNECTED.is_match(text) {
        return "The iPhone is not connected over Bluetooth LE".into();
    }
    if REFUSED.is_match(text) {
        return "The iPhone refused this laptop's keys. Pair again".into();
    }
    GDBUS.replace_all(&text.replace("g-io-error-quark: ", ""), "").into_owned()
}

/// Whether `pair` has to drop BlueZ's bond first: BlueZ holds one for the
/// phone but it is not connected, which is what a phone that forgot this
/// laptop looks like from here. A phone still linked over any bearer is
/// using that bond, so it keeps it.
pub fn forget_before_pair(status: Option<&Status>) -> bool {
    status.is_some_and(|s| s.paired && !s.connected && !s.linked && !s.address.is_empty())
}

/// Newest first, one row per id, capped. ANCS resends an id when the phone
/// modifies a notification, which replaces the row rather than adding one.
pub fn upsert(items: &[Notification], entry: Notification, limit: usize) -> Vec<Notification> {
    let id = entry.id;
    let mut next = vec![entry];
    next.extend(items.iter().filter(|e| e.id != id).cloned());
    next.truncate(limit);
    next
}

pub fn remove_by_id(items: &[Notification], id: i64) -> Vec<Notification> {
    items.iter().filter(|e| e.id != id).cloned().collect()
}

/// What iOS's Focus asked for a notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A normal arrival.
    Toast,
    /// The centre's history with no toast and no sound.
    Quiet,
    /// Not mirrored.
    Drop,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Toast => "toast",
            Verdict::Quiet => "quiet",
            Verdict::Drop => "drop",
        }
    }
}

/// `silent` is the only signal there is (ANCS's EventFlagSilent, set on
/// everything a Focus holds back).
pub fn focus_verdict(entry: Option<&Notification>, mode: &str) -> Verdict {
    match entry {
        Some(e) if e.silent => match mode {
            "ignore" => Verdict::Toast,
            "hide" => Verdict::Drop,
            _ => Verdict::Quiet,
        },
        _ => Verdict::Toast,
    }
}

/// `iphone.notifications`: the enable switch, the blocked bundle ids and the
/// Focus mode.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteConfig {
    pub enable: bool,
    pub block: Vec<String>,
    pub focus: String,
}

impl Default for RouteConfig {
    fn default() -> RouteConfig {
        RouteConfig { enable: true, block: Vec::new(), focus: String::new() }
    }
}

/// [`focus_verdict`] plus the gates in front of it. A replayed notification
/// (`preexisting`, the phone's backlog on every reconnect) stays in the recent
/// list only: it already happened, and a reconnect would otherwise refill the
/// centre with everything still on the lock screen.
pub fn route(entry: &Notification, cfg: &RouteConfig) -> Verdict {
    if !cfg.enable || is_blocked(entry, &cfg.block) || entry.preexisting {
        return Verdict::Drop;
    }
    focus_verdict(Some(entry), &cfg.focus)
}

pub fn is_blocked(entry: &Notification, block: &[String]) -> bool {
    let id = entry.bundle_id.to_lowercase();
    !id.is_empty() && block.iter().any(|b| b.to_lowercase() == id)
}

// Bidi isolates and marks iOS wraps contact names in, zero-width joiners and
// the BOM: invisible, and present on one side of a pair and not the other.
static INVISIBLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x{200b}-\x{200f}\x{202a}-\x{202e}\x{2060}-\x{2069}\x{feff}]").unwrap());
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("[{SPACE}]+")).unwrap());

fn clean(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    let visible = INVISIBLE.replace_all(&nfc, "");
    let quotes: String = visible
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201c}' | '\u{201d}' => '"',
            c => c,
        })
        .collect();
    js::trim(&WHITESPACE.replace_all(&quotes, " ")).to_lowercase()
}

/// The comparison key for one sender and one message.
pub fn normalise(title: &str, body: &str) -> String {
    format!("{}\u{0}{}", clean(title), clean(body))
}

// Every (sender, body) reading a notification supports. The two clients do not
// put the sender in the same place: es.canarycoders.messages sends the chat
// title as the summary and, in a group, "Sender: text" as the body
// (crates/core/src/notify.rs), while ANCS carries the sender in `title` or
// `subtitle`. A pair matches when any reading of one equals any reading of
// the other.
static SENDER_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^([^:\n]{{1,64}}):[{SPACE}]+([\s\S]+)$")).unwrap());

fn readings(sender: &str, subtitle: &str, body: &str) -> Vec<String> {
    let mut keys = vec![normalise(sender, body)];
    if !js::trim(subtitle).is_empty() {
        keys.push(normalise(subtitle, body));
    }
    if let Some(m) = SENDER_PREFIX.captures(body) {
        keys.push(normalise(&m[1], &m[2]));
    }
    keys
}

fn overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|k| b.contains(k))
}

/// One `iphone.notifications.dedupe` rule: a phone bundle id, the local
/// clients that raise the same notification and how long they stay comparable.
#[derive(Clone, Debug, PartialEq)]
pub struct DedupeRule {
    pub phone: String,
    pub local: Vec<String>,
    pub window_ms: f64,
}

/// settings.json's list, anything malformed dropped rather than guessed at.
pub fn dedupe_rules(raw: &Json) -> Vec<DedupeRule> {
    let Json::Array(rules) = raw else { return Vec::new() };
    rules
        .iter()
        .filter(|r| js::is_object_like(r))
        .filter(|r| !s(r, "phone").is_empty())
        .filter_map(|r| {
            let Some(Json::Array(local)) = js::field(r, "local") else { return None };
            let window = js::to_number(js::field(r, "window"));
            Some(DedupeRule {
                phone: s(r, "phone").to_lowercase(),
                local: local.iter().map(|l| js::opt_str(Some(l)).to_lowercase()).filter(|l| !l.is_empty()).collect(),
                window_ms: (if window.is_finite() && window > 0.0 { window } else { 30.0 }) * 1000.0,
            })
        })
        .collect()
}

fn rule_for<'a>(rules: &'a [DedupeRule], bundle_id: &str) -> Option<&'a DedupeRule> {
    let id = bundle_id.to_lowercase();
    rules.iter().find(|r| r.phone == id)
}

/// The phone fields a centre entry carries under `phone`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhoneFields {
    pub bundle_id: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

/// One entry of the notification centre, as far as dedupe reads it. A phone
/// entry has `source == "iphone"` and its bridge fields under `phone`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CentreEntry {
    pub id: String,
    pub app_name: String,
    pub desktop_entry: String,
    pub summary: String,
    pub body: String,
    pub source: String,
    pub local: bool,
    pub phone: Option<PhoneFields>,
    pub arrived_at: f64,
}

fn is_local_for(rule: &DedupeRule, entry: &CentreEntry) -> bool {
    if entry.source == "iphone" || entry.local {
        return false;
    }
    let name = entry.app_name.to_lowercase();
    let desktop = entry.desktop_entry.to_lowercase();
    rule.local.iter().any(|l| *l == name || (!desktop.is_empty() && *l == desktop))
}

fn within(rule: &DedupeRule, entry: &CentreEntry, now: f64) -> bool {
    (now - entry.arrived_at).abs() <= rule.window_ms
}

/// A phone arrival against the notification centre's entries: the local entry
/// it duplicates, or `None`. The local one wins, so `Some` means the phone
/// arrival is dropped. `rules` is [`dedupe_rules`]'s output.
pub fn dedupe<'a>(
    rules: &[DedupeRule],
    phone_entry: &Notification,
    local_entries: &'a [CentreEntry],
    now: f64,
) -> Option<&'a CentreEntry> {
    let rule = rule_for(rules, &phone_entry.bundle_id)?;
    let phone_keys = readings(&phone_entry.title, &phone_entry.subtitle, &phone_entry.body);
    local_entries.iter().find(|local| {
        is_local_for(rule, local)
            && within(rule, local, now)
            && overlap(&phone_keys, &readings(&local.summary, "", &local.body))
    })
}

/// The other direction: a local arrival against the centre's phone entries.
/// Answers the ids of the phone entries it supersedes, which the caller
/// removes.
pub fn superseded(rules: &[DedupeRule], local_entry: &CentreEntry, entries: &[CentreEntry], now: f64) -> Vec<String> {
    let local_keys = readings(&local_entry.summary, "", &local_entry.body);
    entries
        .iter()
        .filter(|entry| entry.source == "iphone")
        .filter_map(|entry| {
            let phone = entry.phone.as_ref()?;
            let rule = rule_for(rules, &phone.bundle_id)?;
            (is_local_for(rule, local_entry)
                && within(rule, entry, now)
                && overlap(&local_keys, &readings(&phone.title, &phone.subtitle, &phone.body)))
            .then(|| entry.id.clone())
        })
        .collect()
}

/// One live arrival, for the Focus heuristic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrival {
    pub at: f64,
    pub silent: bool,
}

/// The Focus heuristic. No accessory can read the phone's Focus state, so "in
/// Focus" means the newest live arrival inside the last `window_sec` was
/// silent: a silent one in the window and no audible one since. `arrivals` is
/// in any order.
pub fn in_focus(arrivals: &[Arrival], now: f64, window_sec: f64) -> bool {
    let window_ms = (if window_sec > 0.0 { window_sec } else { DEFAULT_FOCUS_WINDOW }) * 1000.0;
    let mut newest: Option<&Arrival> = None;
    for a in arrivals {
        if now - a.at > window_ms || a.at > now {
            continue;
        }
        if newest.is_none_or(|n| a.at >= n.at) {
            newest = Some(a);
        }
    }
    newest.is_some_and(|n| n.silent)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DndState {
    pub owned: bool,
    pub focus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DndStep {
    pub owned: bool,
    pub focus: bool,
    /// `Some(on)` to write DND, `None` to leave it.
    pub set: Option<bool>,
}

/// `iphone.notifications.syncDnd`, one step. `prev` is the previous step's
/// state (start from the default). Only edges act, so a DND the owner clears
/// mid-Focus stays cleared; and only a DND this function turned on is ever
/// turned off, so one set by hand, before or during Focus, is never touched.
pub fn sync_dnd_step(prev: DndState, focus: bool, dnd: bool, enabled: bool) -> DndStep {
    let active = enabled && focus;
    let mut owned = prev.owned && dnd;
    let mut set = None;
    if active && !prev.focus && !dnd {
        set = Some(true);
        owned = true;
    } else if !active && prev.focus && owned {
        set = Some(false);
        owned = false;
    }
    DndStep { owned, focus: active, set }
}

// One-time codes. A bare number is not a code: one of the context words has to
// be present, which keeps prices, order numbers and years off the clipboard.
static CODE_CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i-u)\b(code|otp|passcode|pass ?code|verification|verify|verifica|2fa|two[- ]factor|one[- ]time|security|auth(entication)?|token|pin)\b",
    )
    .unwrap()
});
static CODE_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u)\b([0-9]{3})[- ]([0-9]{3})\b").unwrap());
static CODE_PLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u)\b([0-9]{4,8})\b").unwrap());
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(19|20)[0-9][0-9]$").unwrap());

pub fn extract_code(text: &str) -> String {
    if !CODE_CONTEXT.is_match(text) {
        return String::new();
    }
    if let Some(m) = CODE_SPLIT.captures(text) {
        return format!("{}{}", &m[1], &m[2]);
    }
    if let Some(m) = CODE_PLAIN.captures(text) {
        let code = &m[1];
        if !(code.len() == 4 && YEAR.is_match(code)) {
            return code.to_string();
        }
    }
    String::new()
}

/// Bundle id to an icon name from shell/Theme/icons.js. A call's category
/// outranks the app, since the phone app raises all three kinds.
pub fn app_icon(bundle_id: &str, category: i64) -> &'static str {
    match category {
        category::INCOMING_CALL => return "phone-incoming",
        category::MISSED_CALL => return "phone-missed",
        category::VOICEMAIL => return "voicemail",
        _ => {}
    }
    match bundle_id.to_lowercase().as_str() {
        "com.apple.mobilesms" => "message-circle",
        "com.apple.mobilephone" => "phone",
        "com.apple.facetime" => "video",
        "com.apple.mobilemail" => "mail",
        "com.apple.mobilecal" => "calendar",
        "com.apple.reminders" => "list-todo",
        "com.apple.music" => "music",
        "com.apple.health" => "heart-pulse",
        "com.apple.maps" => "map",
        "com.apple.news" => "newspaper",
        "com.apple.passbook" => "wallet",
        "net.whatsapp.whatsapp" => "message-circle",
        "org.whispersystems.signal" => "message-circle",
        "ph.telegra.telegraph" => "send",
        "com.hammerandchisel.discord" => "message-square",
        "com.tinyspeck.chatlyio" => "hash",
        "com.google.gmail" => "mail",
        "com.microsoft.office.outlook" => "mail",
        "com.spotify.client" => "music",
        _ => "smartphone",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub key: &'static str,
    pub label: String,
}

/// The bridge's own record, which dedupe and the dismiss sync read.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoneInfo {
    pub id: i64,
    pub bundle_id: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub icon: &'static str,
    pub category: i64,
    pub session: i64,
    pub device_handle: String,
    pub positive_action: String,
    pub negative_action: String,
    pub silent: bool,
}

/// The fields `NotificationService.notifyPhone()` hands the notification
/// model's `add()`.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationOut {
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: u8,
    pub actions: Vec<Action>,
    pub category: &'static str,
    pub phone: PhoneInfo,
}

/// `summary`/`body` are only what a card shows. A ringing call is critical so
/// it stays up until the phone says it ended; nothing here claims `local`, so
/// a critical phone entry still waits behind DND like any other app's.
pub fn to_notification(entry: &Notification) -> NotificationOut {
    let label = category_label(entry.category);
    let body = if entry.subtitle.is_empty() { entry.body.clone() } else { format!("{}\n{}", entry.subtitle, entry.body) };
    let mut actions = Vec::new();
    if !entry.positive_action.is_empty() {
        actions.push(Action { key: "positive", label: entry.positive_action.clone() });
    }
    if !entry.negative_action.is_empty() {
        actions.push(Action { key: "negative", label: entry.negative_action.clone() });
    }
    NotificationOut {
        app_name: entry.app_name.clone(),
        summary: if !entry.title.is_empty() {
            entry.title.clone()
        } else if !label.is_empty() {
            label.to_string()
        } else {
            entry.app_name.clone()
        },
        body,
        urgency: if entry.category == category::INCOMING_CALL || entry.important { 2 } else { 1 },
        actions,
        category: if entry.category == category::SOCIAL { "im.received" } else { "" },
        phone: PhoneInfo {
            id: entry.id,
            bundle_id: entry.bundle_id.clone(),
            title: entry.title.clone(),
            subtitle: entry.subtitle.clone(),
            body: entry.body.clone(),
            icon: app_icon(&entry.bundle_id, entry.category),
            category: entry.category,
            session: entry.session,
            device_handle: entry.device_handle.clone(),
            positive_action: entry.positive_action.clone(),
            negative_action: entry.negative_action.clone(),
            silent: entry.silent,
        },
    }
}

/// One `omarchy-iphone-ams` line. AMS rides the same BLE link as ANCS but is a
/// second GATT client with its own process; `Status.available` is whether the
/// phone's entity-update characteristic was found at all (false right after a
/// listen that then exits non-zero), and every `NowPlaying` line carries the
/// player's *whole* known state, not a diff, so a title-only change still
/// repeats the last `elapsed`/`playback` the caller already had.
#[derive(Clone, Debug, PartialEq)]
pub enum AmsEvent {
    Status { available: bool },
    NowPlaying { title: String, artist: String, album: String, duration: f64, elapsed: f64, playback: String, volume: f64 },
    Error { message: String },
}

/// One `omarchy-iphone-ams listen`/`command` stdout line (omarchy-iphone @
/// 586f37d, bin/omarchy-iphone-ams), or `None` for a blank line or bad JSON.
pub fn parse_ams_line(line: &str) -> Option<AmsEvent> {
    let text = js::trim(line);
    if text.is_empty() {
        return None;
    }
    let raw: Json = serde_json::from_str(text).ok()?;
    if !js::is_object_like(&raw) {
        return None;
    }
    match js::field(&raw, "type")?.as_str()? {
        "status" => Some(AmsEvent::Status { available: is_true(&raw, "available") }),
        "nowplaying" => {
            let duration = js::to_number(js::field(&raw, "duration"));
            let elapsed = js::to_number(js::field(&raw, "elapsed"));
            let volume = js::to_number(js::field(&raw, "volume"));
            Some(AmsEvent::NowPlaying {
                title: s(&raw, "title"),
                artist: s(&raw, "artist"),
                album: s(&raw, "album"),
                duration: if duration.is_finite() && duration > 0.0 { duration } else { 0.0 },
                elapsed: if elapsed.is_finite() && elapsed >= 0.0 { elapsed } else { 0.0 },
                playback: s(&raw, "playback"),
                volume: if volume.is_finite() && volume >= 0.0 { volume } else { -1.0 },
            })
        }
        "error" => Some(AmsEvent::Error { message: explain_error(&s(&raw, "message")) }),
        _ => None,
    }
}

static HANDLE_ADDRESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u)dev_([0-9A-Fa-f]{2}(?:_[0-9A-Fa-f]{2}){5})$").unwrap());

/// A BlueZ device object path ends in dev_AA_BB_CC_DD_EE_FF; that is the
/// device's address, which is how the phone is found among the Bluetooth
/// devices when no object path is exposed to match on directly.
pub fn address_from_handle(handle: &str) -> String {
    HANDLE_ADDRESS.captures(handle).map_or(String::new(), |m| m[1].replace('_', ":").to_uppercase())
}

/// The Bluetooth device that is the phone: its object path first (the
/// bridge's `deviceHandle` is exactly that), then the address encoded in it or
/// the address of the bond the bridge reports, then the bridge's reported
/// name. `None` when none match.
pub fn match_device<'a>(
    devices: &'a [bluetooth::Device],
    handle: &str,
    name: &str,
    bond_address: &str,
) -> Option<&'a bluetooth::Device> {
    let address = match address_from_handle(handle) {
        a if !a.is_empty() => a,
        _ => bond_address.to_uppercase(),
    };
    if !handle.is_empty()
        && let Some(d) = devices.iter().find(|d| d.dbus_path == handle)
    {
        return Some(d);
    }
    if !address.is_empty()
        && let Some(d) = devices.iter().find(|d| d.address.to_uppercase() == address)
    {
        return Some(d);
    }
    if !name.is_empty() {
        return devices.iter().find(|d| d.connected && (d.name == name || d.device_name == name));
    }
    None
}

/// Whether an action on this entry can still reach the phone. ancs4linux picks
/// a fresh id base per connection, so an id from an earlier session names a
/// notification the phone never had: the write succeeds and does nothing.
pub fn is_actionable(entry: Option<&Notification>, current_session: i64) -> bool {
    match entry {
        Some(e) if !e.device_handle.is_empty() => current_session == 0 || e.session == current_session,
        _ => false,
    }
}

// JS `encodeURIComponent` leaves these alone besides ASCII letters and digits.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

/// AMS carries no artwork, so the phone's cover is looked up on iTunes' search
/// by title and artist.
pub fn artwork_search_url(artist: &str, title: &str) -> String {
    let query = js::trim(&format!("{title} {artist}")).to_string();
    format!("https://itunes.apple.com/search?term={}&entity=song&limit=10", utf8_percent_encode(&query, URI_COMPONENT))
}

static ARTWORK_SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/[0-9]+x[0-9]+bb\.(jpg|png)$").unwrap());

/// Only a result by the same artist counts (a "Waves" by anyone else is not
/// this track's cover); one on the same album wins over the first. The 100px
/// thumbnail URL takes any size in its last path segment, so it is asked for
/// at 600px. "" for no usable result.
pub fn pick_artwork(body: &str, artist: &str, album: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<Json>(body) else { return String::new() };
    let Some(Json::Array(results)) = js::field(&parsed, "results") else { return String::new() };
    let want_artist = artist.to_lowercase();
    let want_album = album.to_lowercase();
    if want_artist.is_empty() {
        return String::new();
    }
    let mut first = String::new();
    for r in results {
        let have = s(r, "artistName").to_lowercase();
        let url = s(r, "artworkUrl100");
        if url.is_empty() || have.is_empty() || (!have.contains(&want_artist) && !want_artist.contains(&have)) {
            continue;
        }
        let url = ARTWORK_SIZE.replace(&url, "/600x600bb.$1").into_owned();
        if !want_album.is_empty() && s(r, "collectionName").to_lowercase() == want_album {
            return url;
        }
        if first.is_empty() {
            first = url;
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    // Lines in the exact shape omarchy-iphone-bridge's cmd_listen emits
    // (bin/omarchy-iphone-bridge @ 586f37d).
    const LINE_SMS: &str = r#"{"type": "notification", "id": 4012, "appId": "com.apple.MobileSMS", "appName": "Messages", "title": "Alex", "subtitle": "", "body": "Still on for Friday?", "deviceName": "Kyan's iPhone", "deviceHandle": "/org/bluez/hci0/dev_A1_B2_C3_D4_E5_F6", "positiveAction": null, "negativeAction": "Clear", "category": 4, "categoryCount": 2, "silent": false, "important": false, "preexisting": false, "session": 77, "ts": 1790000000.0}"#;
    const LINE_CALL: &str = r#"{"type": "notification", "id": 4013, "appId": "com.apple.mobilephone", "appName": "Phone", "title": "Mum", "subtitle": "", "body": "mobile", "deviceName": "Kyan's iPhone", "deviceHandle": "/org/bluez/hci0/dev_A1_B2_C3_D4_E5_F6", "positiveAction": "Answer", "negativeAction": "Decline", "category": 1, "categoryCount": 1, "silent": false, "important": true, "preexisting": false, "session": 77, "ts": 1790000001.0}"#;

    fn rules() -> Vec<DedupeRule> {
        dedupe_rules(&default_dedupe())
    }

    fn parse_notification(line: &str) -> Notification {
        match parse_event(line) {
            Some(Event::Notification(n)) => n,
            other => panic!("not a notification: {other:?}"),
        }
    }

    fn phone() -> Notification {
        parse_notification(LINE_SMS)
    }

    fn local(id: &str, summary: &str, body: &str, arrived_at: f64) -> CentreEntry {
        CentreEntry {
            id: id.into(),
            app_name: "Messages".into(),
            summary: summary.into(),
            body: body.into(),
            arrived_at,
            ..Default::default()
        }
    }

    // A notification-centre entry for a phone arrival, as notifyPhone adds it.
    fn phone_entry(record: &Notification, arrived_at: f64) -> CentreEntry {
        let m = to_notification(record);
        CentreEntry {
            id: format!("iphone-{}-{}", record.session, record.id),
            app_name: m.app_name,
            summary: m.summary,
            body: m.body,
            source: "iphone".into(),
            phone: Some(PhoneFields {
                bundle_id: m.phone.bundle_id,
                title: m.phone.title,
                subtitle: m.phone.subtitle,
                body: m.phone.body,
            }),
            arrived_at,
            ..Default::default()
        }
    }

    // --- parse_event -------------------------------------------------------

    #[test]
    fn parse_notification_reads_every_field() {
        let e = phone();
        assert_eq!(e.id, 4012);
        assert_eq!(e.bundle_id, "com.apple.MobileSMS");
        assert_eq!(e.app_name, "Messages");
        assert_eq!(e.title, "Alex");
        assert_eq!(e.body, "Still on for Friday?");
        assert_eq!(e.device_handle, "/org/bluez/hci0/dev_A1_B2_C3_D4_E5_F6");
        assert_eq!(e.positive_action, "");
        assert_eq!(e.negative_action, "Clear");
        assert_eq!(e.category, 4);
        assert!(!e.silent);
        assert_eq!(e.session, 77);
        assert_eq!(e.ts, 1790000000.0);
    }

    #[test]
    fn parse_notification_defaults_missing_fields() {
        let e = parse_notification(r#"{"type":"notification","id":5}"#);
        assert_eq!(e.bundle_id, "");
        assert_eq!(e.app_name, "iPhone");
        assert_eq!(e.title, "");
        assert_eq!(e.negative_action, "");
        assert!(!e.silent);
        assert_eq!(e.session, 0);
    }

    #[test]
    fn parse_history_drops_non_objects() {
        let line = format!(r#"{{"type":"history","items":[{LINE_SMS}, 3, null]}}"#);
        match parse_event(&line) {
            Some(Event::History(items)) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].id, 4012);
            }
            other => panic!("{other:?}"),
        }
    }

    fn status(line: &str) -> Status {
        match parse_event(line) {
            Some(Event::Status(s)) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parse_status_ignores_installed() {
        let e = status(r#"{"type":"status","observer":true,"connected":true,"deviceName":"Kyan's iPhone","battery":64,"installed":false}"#);
        assert!(e.observer);
        assert!(e.connected);
        assert_eq!(e.device_name, "Kyan's iPhone");
        assert_eq!(e.battery, 64.0);
        assert_eq!(status(r#"{"type":"status","battery":-1}"#).battery, -1.0);
    }

    // packaging/iphone-bridge-bond.patch's status: a bond BlueZ holds while LE is down.
    #[test]
    fn parse_status_bond() {
        let e = status(r#"{"type":"status","observer":true,"connected":false,"linked":true,"paired":true,"deviceName":"FormalPhone","address":"10:7d:c8:a5:3b:42","battery":-1}"#);
        assert!(!e.connected);
        assert!(e.linked);
        assert!(e.paired);
        assert_eq!(e.address, "10:7D:C8:A5:3B:42");
        let old = status(r#"{"type":"status","observer":true,"connected":true}"#);
        assert!(!old.paired);
        assert!(!old.linked);
        assert_eq!(old.address, "");
        assert_eq!(
            parse_event(r#"{"type":"forgot","address":"10:7d:c8:a5:3b:42"}"#),
            Some(Event::Forgot { address: "10:7D:C8:A5:3B:42".into() })
        );
    }

    #[test]
    fn forget_before_pair_only_for_a_disconnected_bond() {
        let st = |paired, connected, address: &str| Status { paired, connected, address: address.into(), ..Default::default() };
        assert!(forget_before_pair(Some(&st(true, false, "10:7D:C8:A5:3B:42"))));
        assert!(!forget_before_pair(Some(&st(true, true, "10:7D:C8:A5:3B:42"))));
        let linked = Status { paired: true, linked: true, address: "10:7D:C8:A5:3B:42".into(), ..Default::default() };
        assert!(!forget_before_pair(Some(&linked)));
        assert!(!forget_before_pair(Some(&st(false, false, ""))));
        assert!(!forget_before_pair(Some(&st(true, false, ""))));
        assert!(!forget_before_pair(None));
    }

    #[test]
    fn explain_error_gives_the_two_actionable_states_a_sentence() {
        assert_eq!(
            explain_error("subscribe: g-io-error-quark: GDBus.Error:org.bluez.Error.Failed: Not connected (36)"),
            "The iPhone is not connected over Bluetooth LE"
        );
        assert_eq!(
            explain_error("GDBus.Error:org.bluez.Error.AuthenticationFailed: Authentication Failed"),
            "The iPhone refused this laptop's keys. Pair again"
        );
        assert_eq!(
            explain_error("subscribe: g-io-error-quark: GDBus.Error:org.bluez.Error.InProgress: In Progress"),
            "subscribe: In Progress"
        );
        assert_eq!(explain_error("The phone rejected the command"), "The phone rejected the command");
        assert_eq!(explain_error(""), "Unknown error");
        assert_eq!(
            parse_ams_line(r#"{"type":"error","message":"subscribe: g-io-error-quark: GDBus.Error:org.bluez.Error.Failed: Not connected (36)"}"#),
            Some(AmsEvent::Error { message: "The iPhone is not connected over Bluetooth LE".into() })
        );
    }

    #[test]
    fn parse_small_events() {
        assert_eq!(parse_event(r#"{"type":"dismiss","id":4012}"#), Some(Event::Dismiss { id: 4012 }));
        assert_eq!(parse_event(r#"{"type":"pairingCode","code":"123456"}"#), Some(Event::PairingCode { code: "123456".into() }));
        assert_eq!(
            parse_event(r#"{"type":"advertising","hci":"hci0","name":"FormalShell"}"#),
            Some(Event::Advertising { hci: "hci0".into(), name: "FormalShell".into() })
        );
        assert_eq!(
            parse_event(r#"{"type":"error","message":"The phone rejected the command"}"#),
            Some(Event::Error { message: "The phone rejected the command".into() })
        );
    }

    #[test]
    fn parse_garbage_is_none() {
        assert_eq!(parse_event(""), None);
        assert_eq!(parse_event("   "), None);
        assert_eq!(parse_event("not json"), None);
        assert_eq!(parse_event("[1,2]"), None);
        assert_eq!(parse_event(r#"{"type":"hci","adapters":[]}"#), None);
    }

    // --- AMS now-playing -----------------------------------------------------
    //
    // Lines in the exact shape omarchy-iphone-ams's cmd_listen/cmd_command emit
    // (bin/omarchy-iphone-ams @ 586f37d): `nowplaying` always carries the
    // player's whole known state, not a diff.

    #[test]
    fn parse_ams_status() {
        assert_eq!(parse_ams_line(r#"{"type": "status", "available": true}"#), Some(AmsEvent::Status { available: true }));
        assert_eq!(parse_ams_line(r#"{"type": "status", "available": false}"#), Some(AmsEvent::Status { available: false }));
    }

    #[test]
    fn parse_ams_nowplaying() {
        let e = parse_ams_line(r#"{"type": "nowplaying", "title": "Talk", "artist": "Kraftwerk", "album": "The Man-Machine", "duration": 220.5, "player": "Music", "playback": "playing", "elapsed": 41.2, "volume": 0.6}"#);
        assert_eq!(
            e,
            Some(AmsEvent::NowPlaying {
                title: "Talk".into(),
                artist: "Kraftwerk".into(),
                album: "The Man-Machine".into(),
                duration: 220.5,
                elapsed: 41.2,
                playback: "playing".into(),
                volume: 0.6,
            })
        );
    }

    #[test]
    fn parse_ams_nowplaying_defaults() {
        let e = parse_ams_line(r#"{"type": "nowplaying", "title": "", "artist": "", "album": "", "duration": 0.0, "player": "", "playback": "", "elapsed": 0.0, "volume": -1.0}"#);
        match e {
            Some(AmsEvent::NowPlaying { title, duration, playback, volume, .. }) => {
                assert_eq!(title, "");
                assert_eq!(duration, 0.0);
                assert_eq!(playback, "");
                assert_eq!(volume, -1.0);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parse_ams_error_and_garbage() {
        assert_eq!(
            parse_ams_line(r#"{"type": "error", "message": "AMS not available (phone connected?)"}"#),
            Some(AmsEvent::Error { message: "AMS not available (phone connected?)".into() })
        );
        assert_eq!(parse_ams_line(""), None);
        assert_eq!(parse_ams_line("not json"), None);
        assert_eq!(parse_ams_line(r#"{"type":"hci"}"#), None);
    }

    // --- recent list -----------------------------------------------------

    #[test]
    fn upsert_replaces_a_modified_resend() {
        let with = |id, body: &str| Notification { id, body: body.into(), ..phone() };
        let a = with(1, "x");
        let b = with(2, "x");
        let mut list = upsert(&upsert(&[], a, 10), b, 10);
        list = upsert(&list, with(1, "edited"), 10);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, 1);
        assert_eq!(list[0].body, "edited");
        assert_eq!(upsert(&list, with(3, "x"), 2).len(), 2);
        assert_eq!(remove_by_id(&list, 2).len(), 1);
    }

    // --- Focus -----------------------------------------------------------

    #[test]
    fn focus_verdict_modes() {
        let loud = Notification { silent: false, ..phone() };
        let quiet = Notification { silent: true, ..phone() };
        let v = |e: &Notification, m: &str| focus_verdict(Some(e), m).as_str();
        assert_eq!(v(&loud, "respect"), "toast");
        assert_eq!(v(&quiet, "respect"), "quiet");
        assert_eq!(v(&quiet, "hide"), "drop");
        assert_eq!(v(&loud, "hide"), "toast");
        assert_eq!(v(&quiet, "ignore"), "toast");
        assert_eq!(v(&quiet, "bogus"), "quiet");
        assert_eq!(focus_verdict(None, "respect"), Verdict::Toast);
    }

    #[test]
    fn route_gates() {
        let mut cfg = RouteConfig { enable: true, block: vec!["COM.apple.mobilesms".into()], focus: "respect".into() };
        assert_eq!(route(&phone(), &cfg), Verdict::Drop);
        cfg.block = vec![];
        assert_eq!(route(&phone(), &cfg), Verdict::Toast);
        assert_eq!(route(&Notification { preexisting: true, ..phone() }, &cfg), Verdict::Drop);
        assert_eq!(route(&Notification { silent: true, ..phone() }, &cfg), Verdict::Quiet);
        cfg.enable = false;
        assert_eq!(route(&phone(), &cfg), Verdict::Drop);
    }

    #[test]
    fn in_focus_heuristic() {
        let now = 1000000.0;
        let a = |ago: f64, silent| Arrival { at: now - ago, silent };
        assert!(!in_focus(&[], now, 900.0));
        assert!(in_focus(&[a(1000.0, true)], now, 900.0));
        // An audible arrival after the silent one ends it.
        assert!(!in_focus(&[a(5000.0, true), a(1000.0, false)], now, 900.0));
        // And a silent one after an audible one starts it again.
        assert!(in_focus(&[a(5000.0, false), a(1000.0, true)], now, 900.0));
        // Aged out of the window.
        assert!(!in_focus(&[a(901000.0, true)], now, 900.0));
        assert!(in_focus(&[a(901000.0, false), a(1000.0, true)], now, 900.0));
    }

    fn step(prev: &DndStep, focus: bool, dnd: bool, enabled: bool) -> DndStep {
        sync_dnd_step(DndState { owned: prev.owned, focus: prev.focus }, focus, dnd, enabled)
    }

    fn start(focus: bool, dnd: bool, enabled: bool) -> DndStep {
        sync_dnd_step(DndState::default(), focus, dnd, enabled)
    }

    #[test]
    fn sync_dnd_turns_on_and_off_its_own() {
        let mut r = start(true, false, true);
        assert_eq!(r.set, Some(true));
        assert!(r.owned);
        // The write lands back as a DND change: nothing further.
        r = step(&r, true, true, true);
        assert_eq!(r.set, None);
        assert!(r.owned);
        r = step(&r, false, true, true);
        assert_eq!(r.set, Some(false));
        assert!(!r.owned);
    }

    #[test]
    fn sync_dnd_never_clears_a_hand_set_dnd() {
        // DND already on when Focus starts: not ours.
        let mut r = start(true, true, true);
        assert_eq!(r.set, None);
        assert!(!r.owned);
        r = step(&r, false, true, true);
        assert_eq!(r.set, None);
    }

    #[test]
    fn sync_dnd_hand_clear_mid_focus_sticks() {
        let mut r = start(true, false, true);
        assert_eq!(r.set, Some(true));
        // Owner turns DND off by hand while Focus still reads on.
        r = step(&r, true, false, true);
        assert_eq!(r.set, None);
        assert!(!r.owned);
        // ...and back on by hand: that one is theirs, so Focus ending leaves it.
        r = step(&r, true, true, true);
        assert!(!r.owned);
        r = step(&r, false, true, true);
        assert_eq!(r.set, None);
    }

    #[test]
    fn sync_dnd_disabled() {
        let r = start(true, false, false);
        assert_eq!(r.set, None);
        // Turning the key off while it owns DND hands it back.
        let r = sync_dnd_step(DndState { owned: true, focus: true }, true, true, false);
        assert_eq!(r.set, Some(false));
        assert!(!r.owned);
    }

    // --- dedupe ----------------------------------------------------------

    #[test]
    fn normalise_ignores_isolates_quotes_and_spacing() {
        assert_eq!(normalise("\u{2068}Alex\u{2069}", "It\u{2019}s   late "), normalise("alex", "it's late"));
        assert_ne!(normalise("Alex", "hi"), normalise("Alex", "hi!"));
        assert_ne!(normalise("ab", "c"), normalise("a", "bc"));
    }

    #[test]
    fn dedupe_rules_parse() {
        let rules = rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].phone, "com.apple.mobilesms");
        assert_eq!(rules[0].window_ms, 30000.0);
        assert_eq!(dedupe_rules(&json!([])).len(), 0);
        assert_eq!(dedupe_rules(&json!("nope")).len(), 0);
        assert_eq!(dedupe_rules(&json!([{ "phone": "x" }, { "phone": "y", "local": ["Y"], "window": -3 }]))[0].window_ms, 30000.0);
    }

    #[test]
    fn phone_arrival_dropped_for_live_local() {
        let now = 100000.0;
        let entries = [local("9", "Alex", "Still on for Friday?", now - 2000.0)];
        let hit = dedupe(&rules(), &phone(), &entries, now);
        assert_eq!(hit.unwrap().id, "9");
    }

    #[test]
    fn phone_arrival_kept_outside_window_or_mismatch() {
        let now = 100000.0;
        let r = rules();
        let d = |p: &Notification, e: CentreEntry, r: &[DedupeRule]| dedupe(r, p, &[e], now).is_none();
        assert!(d(&phone(), local("9", "Alex", "Still on for Friday?", now - 31000.0), &r));
        assert!(d(&phone(), local("9", "Alex", "Something else", now), &r));
        assert!(d(&phone(), local("9", "Sam", "Still on for Friday?", now), &r));
        assert!(d(
            &Notification { bundle_id: "net.whatsapp.WhatsApp".into(), ..phone() },
            local("9", "Alex", "Still on for Friday?", now),
            &r
        ));
        assert!(d(&phone(), local("9", "Alex", "Still on for Friday?", now), &[]));
    }

    #[test]
    fn dedupe_matches_desktop_entry_and_skips_other_sources() {
        let now = 100000.0;
        let r = rules();
        let with = |f: fn(&mut CentreEntry)| {
            let mut e = local("9", "Alex", "Still on for Friday?", now);
            f(&mut e);
            e
        };
        let hit = |e: CentreEntry| dedupe(&r, &phone(), &[e], now).is_some();
        assert!(hit(with(|e| {
            e.app_name = "whatever".into();
            e.desktop_entry = "es.canarycoders.messages".into();
        })));
        assert!(!hit(with(|e| e.app_name = "Slack".into())));
        // Shell-authored and other phone entries never count as the local copy.
        assert!(!hit(with(|e| e.local = true)));
        assert!(!hit(with(|e| e.source = "iphone".into())));
    }

    #[test]
    fn dedupe_group_chat_readings() {
        let now = 100000.0;
        let r = rules();
        // The desktop client: the group as summary, "Sender: text" as body.
        let group = [local("9", "Weekend", "Alex: Still on for Friday?", now)];
        // The phone with the sender as title and the group as subtitle.
        assert!(dedupe(&r, &Notification { subtitle: "Weekend".into(), ..phone() }, &group, now).is_some());
        // The phone with the group as title and the prefixed body.
        let p = Notification { title: "Weekend".into(), body: "Alex: Still on for Friday?".into(), ..phone() };
        assert!(dedupe(&r, &p, &group, now).is_some());
        // The phone with the group as title and the sender as subtitle.
        let p = Notification { title: "Weekend".into(), subtitle: "Alex".into(), ..phone() };
        assert!(dedupe(&r, &p, &group, now).is_some());
    }

    // The phone half of the replace path: a phone card in the centre, then the
    // desktop client raising the same message, names the phone entry as
    // superseded. (The reducer that removes it lives with the notifications.)
    #[test]
    fn local_arrival_supersedes_the_phone_card() {
        let now = 100000.0;
        let incoming = local("9", "Alex", "Still on for Friday?", now + 3000.0);
        let all = [phone_entry(&phone(), now), incoming.clone()];
        let stale = superseded(&rules(), &incoming, &all, now + 3000.0);
        assert_eq!(stale, ["iphone-77-4012"]);
    }

    #[test]
    fn local_arrival_supersedes_a_quiet_phone_entry_in_history() {
        let now = 100000.0;
        let quiet = Notification { silent: true, ..phone() };
        let incoming = local("9", "Alex", "Still on for Friday?", now + 1000.0);
        let stale = superseded(&rules(), &incoming, &[phone_entry(&quiet, now)], now + 1000.0);
        assert_eq!(stale.len(), 1);
    }

    #[test]
    fn superseded_leaves_unrelated_phone_entries() {
        let now = 100000.0;
        let entries = [
            phone_entry(&phone(), now - 40000.0),
            phone_entry(&Notification { id: 2, body: "Other".into(), ..phone() }, now),
            phone_entry(&Notification { id: 3, bundle_id: "net.whatsapp.WhatsApp".into(), ..phone() }, now),
            local("8", "Alex", "Still on for Friday?", now),
        ];
        assert_eq!(superseded(&rules(), &local("9", "Alex", "Still on for Friday?", now), &entries, now).len(), 0);
        let slack = CentreEntry { app_name: "Slack".into(), ..local("9", "Alex", "Still on for Friday?", now) };
        assert_eq!(superseded(&rules(), &slack, &[phone_entry(&phone(), now)], now).len(), 0);
    }

    // --- notification model hooks --------------------------------------------

    #[test]
    // The groupKey half lives in tests/iphone_notifications.rs, which has the
    // notification model to key them with.
    fn phone_and_local_entries_keep_their_sources_apart() {
        let a = phone_entry(&phone(), 1000.0);
        let b = local("9", "Alex", "different", 1000.0);
        assert_eq!(a.source, "iphone");
        assert_eq!(a.phone.as_ref().unwrap().body, "Still on for Friday?");
        assert_eq!(b.source, "");
    }

    // --- mapping ---------------------------------------------------------

    #[test]
    fn to_notification_maps_a_message_and_a_call() {
        let m = to_notification(&Notification { subtitle: "Weekend".into(), ..phone() });
        assert_eq!(m.summary, "Alex");
        assert_eq!(m.body, "Weekend\nStill on for Friday?");
        assert_eq!(m.urgency, 1);
        assert_eq!(m.category, "im.received");
        assert_eq!(m.actions.len(), 1);
        assert_eq!(m.actions[0].key, "negative");
        assert_eq!(m.actions[0].label, "Clear");
        assert_eq!(m.phone.body, "Still on for Friday?");
        assert_eq!(m.phone.icon, "message-circle");
        assert_eq!(m.phone.negative_action, "Clear");

        let call = to_notification(&parse_notification(LINE_CALL));
        assert_eq!(call.urgency, 2);
        assert_eq!(call.actions.iter().map(|a| a.key).collect::<Vec<_>>().join(","), "positive,negative");
        assert_eq!(call.phone.icon, "phone-incoming");

        assert_eq!(to_notification(&Notification { title: "".into(), category: 2, ..phone() }).summary, "Missed call");
        assert_eq!(to_notification(&Notification { title: "".into(), category: 0, ..phone() }).summary, "Messages");
    }

    #[test]
    fn app_icon_by_bundle_and_category() {
        assert_eq!(app_icon("com.apple.MobileSMS", 0), "message-circle");
        assert_eq!(app_icon("com.apple.mobilephone", 2), "phone-missed");
        assert_eq!(app_icon("com.apple.mobilephone", 3), "voicemail");
        assert_eq!(app_icon("com.example.unknown", 0), "smartphone");
        assert_eq!(app_icon("", 0), "smartphone");
    }

    #[test]
    fn extract_code_needs_a_context_word() {
        assert_eq!(extract_code("Your verification code is 482913"), "482913");
        assert_eq!(extract_code("123-456 is your Apple ID code"), "123456");
        assert_eq!(extract_code("Order 482913 has shipped"), "");
        assert_eq!(extract_code("Security alert since 2019"), "");
        assert_eq!(extract_code(""), "");
    }

    #[test]
    fn device_matching() {
        assert_eq!(address_from_handle("/org/bluez/hci0/dev_a1_B2_C3_D4_E5_F6"), "A1:B2:C3:D4:E5:F6");
        assert_eq!(address_from_handle(""), "");
        let by_path = bluetooth::Device {
            dbus_path: "/org/bluez/hci0/dev_A1_B2_C3_D4_E5_F6".into(),
            address: "x".into(),
            name: "a".into(),
            connected: true,
            ..Default::default()
        };
        let by_addr = bluetooth::Device { address: "a1:b2:c3:d4:e5:f6".into(), name: "b".into(), connected: true, ..Default::default() };
        let by_name = bluetooth::Device { name: "Kyan's iPhone".into(), connected: true, ..Default::default() };
        let handle = "/org/bluez/hci0/dev_A1_B2_C3_D4_E5_F6";
        let found = |devices: &[bluetooth::Device], h: &str, n: &str, b: &str| match_device(devices, h, n, b).cloned();
        assert_eq!(found(&[by_addr.clone(), by_path.clone()], handle, "", ""), Some(by_path));
        assert_eq!(found(&[by_name.clone(), by_addr.clone()], "/org/bluez/hci1/dev_A1_B2_C3_D4_E5_F6", "", ""), Some(by_addr));
        assert_eq!(found(std::slice::from_ref(&by_name), "", "Kyan's iPhone", ""), Some(by_name.clone()));
        let asleep = bluetooth::Device { name: "Kyan's iPhone".into(), connected: false, ..Default::default() };
        assert_eq!(found(&[asleep], "", "Kyan's iPhone", ""), None);
        assert_eq!(found(&[], "", "", ""), None);
        let bonded = bluetooth::Device { address: "10:7D:C8:A5:3B:42".into(), name: "FormalPhone".into(), ..Default::default() };
        assert_eq!(found(&[by_name, bonded.clone()], "", "FormalPhone", "10:7d:c8:a5:3b:42"), Some(bonded));
    }

    #[test]
    fn actionable_only_in_its_session() {
        let e = phone();
        assert!(is_actionable(Some(&e), 0));
        assert!(is_actionable(Some(&e), 77));
        assert!(!is_actionable(Some(&e), 78));
        assert!(!is_actionable(Some(&Notification { device_handle: "".into(), ..phone() }), 77));
        assert!(!is_actionable(None, 77));
    }

    #[test]
    fn artwork_search_url_puts_title_first() {
        assert_eq!(
            artwork_search_url("Daft Punk", "Get Lucky"),
            "https://itunes.apple.com/search?term=Get%20Lucky%20Daft%20Punk&entity=song&limit=10"
        );
    }

    #[test]
    fn pick_artwork_prefers_the_album_and_upsizes() {
        let body = json!({ "results": [
            { "artistName": "Someone Else", "collectionName": "RAM", "artworkUrl100": "https://x/a.jpg/100x100bb.jpg" },
            { "artistName": "Daft Punk", "collectionName": "Get Lucky (Remix)", "artworkUrl100": "https://x/b.jpg/100x100bb.jpg" },
            { "artistName": "Daft Punk, Pharrell Williams", "collectionName": "Random Access Memories", "artworkUrl100": "https://x/c.jpg/100x100bb.jpg" }
        ] })
        .to_string();
        assert_eq!(pick_artwork(&body, "Daft Punk", "Random Access Memories"), "https://x/c.jpg/600x600bb.jpg");
        assert_eq!(pick_artwork(&body, "Daft Punk", "Unknown"), "https://x/b.jpg/600x600bb.jpg");
    }

    #[test]
    fn pick_artwork_needs_the_same_artist() {
        let body = json!({ "results": [{ "artistName": "Other", "artworkUrl100": "https://x/a.jpg/100x100bb.jpg" }] }).to_string();
        assert_eq!(pick_artwork(&body, "Fixture Band", "Fixture Album"), "");
        assert_eq!(pick_artwork("nope", "Fixture Band", ""), "");
        assert_eq!(pick_artwork(&body, "", ""), "");
    }
}
