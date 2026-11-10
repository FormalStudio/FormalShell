// Portions from omarchy-iphone (MIT, Copyright (c) 2026 kbbahaPro)

//! The iPhone's bridge half: one long-lived
//! `omarchy-iphone-bridge listen` child turning ancs4linux's signals into
//! JSONL, restarted on a doubling backoff, and the verbs the panel and the
//! centre's cards drive it with: invoke, dismiss, clear, pair. No bridge on
//! PATH is `installed: false` and the cell never appears; a bridge whose
//! daemon does not own its bus name is not `observer`. Neither fakes a
//! phone. The Apple Media Service half is [`ams`](crate::services::ams).
//!
//! The service owns the phone's own history (`recent`), newest first, and
//! publishes the whole state as one snapshot. Each arrival the routing lets
//! through goes to the notification centre, where [`Mirror`] files it
//! against the local clients' copies on the UI thread.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_io::Timer;
use fs_dbus::Backoff;
use fs_devices::iphone::{self, Arrival, Event, Verdict};
pub use fs_devices::iphone::{Notification, phone_of};
use fs_info::notifications::{self as model, AddOpts, Patch, Phone};
use fs_types::{BtAddress, NotificationId};
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, FutureExt, StreamExt};
use serde_json::{Value, json};

use crate::runtime::Ctx;
use crate::services::config;
use crate::services::notifications;
use crate::services::proc::MISSING;
use crate::services::ams;
use crate::store;

const BRIDGE: &str = "omarchy-iphone-bridge";
const HISTORY_LIMIT: usize = 200;
const BASE_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// BlueZ's DiscoverableTimeout default, after which the classic side is
/// hidden anyway.
const PAIR_WINDOW: Duration = Duration::from_secs(180);
const FOCUS_TICK: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub installed: bool,
    pub observer: bool,
    pub connected: bool,
    pub device_name: String,
    /// 0..1, only while connected and the bridge has read one.
    pub battery: Option<f64>,
    pub unread: u32,
    /// BlueZ holds a bond for the phone at `bond_address` whether or not it
    /// is connected.
    pub bonded: bool,
    pub bond_address: Option<BtAddress>,
    /// Linked over any bearer, LE or not.
    pub linked: bool,
    /// Newest first, the bridge's own record per notification, including
    /// ones the Focus verdict kept out of the centre.
    pub recent: Vec<Notification>,
    /// The connection the newest notification belongs to.
    pub session: i64,
    pub pairing_code: String,
    pub advertising: bool,
    pub last_error: String,
    pub in_focus: bool,
}

impl State {
    /// The service's error, else the Apple Media Service's own (the two
    /// share one line, as IphoneService.lastError does).
    pub fn last_error<'a>(&'a self, ams: &'a ams::State) -> &'a str {
        if self.last_error.is_empty() { &ams.error } else { &self.last_error }
    }

    /// The IPC `status` reply: exactly what the panel renders from,
    /// `installed: false` and nothing else when the bridge has never
    /// answered a line.
    pub fn status(&self, ams: &ams::State) -> Value {
        if !self.installed {
            return json!({ "installed": false });
        }
        json!({
            "installed": true,
            "available": self.observer,
            "connected": self.connected,
            "bonded": self.bonded,
            "bondAddress": self.bond_address.map(|a| a.to_string()).unwrap_or_default(),
            "deviceName": self.device_name,
            "batteryAvailable": self.battery.is_some(),
            "battery": self.battery.map_or(json!(0), fs_js::json_number),
            "unread": self.unread,
            "inFocus": self.in_focus,
            "pairingCode": self.pairing_code,
            "advertising": self.advertising,
            "lastError": self.last_error(ams),
            "mediaAvailable": ams.available,
            "mediaTitle": ams.title,
            "recent": self.recent.iter().map(entry).collect::<Vec<_>>(),
        })
    }
}

fn entry(n: &Notification) -> Value {
    json!({
        "id": n.id,
        "bundleId": n.bundle_id,
        "appName": n.app_name,
        "title": n.title,
        "subtitle": n.subtitle,
        "body": n.body,
        "deviceName": n.device_name,
        "deviceHandle": n.device_handle,
        "positiveAction": n.positive_action,
        "negativeAction": n.negative_action,
        "category": n.category,
        "categoryCount": n.category_count,
        "silent": n.silent,
        "important": n.important,
        "preexisting": n.preexisting,
        "session": n.session,
        "ts": fs_js::json_number(n.ts),
    })
}

/// What the panel and IPC ask of the service.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Invoke { id: i64, positive: bool },
    Dismiss(i64),
    Clear,
    MarkRead,
    Pair,
}

static COMMANDS: OnceLock<(async_channel::Sender<Cmd>, async_channel::Receiver<Cmd>)> = OnceLock::new();

fn commands() -> &'static (async_channel::Sender<Cmd>, async_channel::Receiver<Cmd>) {
    COMMANDS.get_or_init(async_channel::unbounded)
}

/// Queues a command for the service; it runs while a cell or the panel
/// holds the iPhone source.
pub fn command(cmd: Cmd) {
    let _ = commands().0.try_send(cmd);
}

/// Each queue runs one at a time: re-running a child still running would
/// send the first and drop the rest.
type Queue = (async_channel::Sender<Vec<String>>, async_channel::Receiver<Vec<String>>);

/// The panel's or IPC's invokes: what they print is read as bridge lines and
/// a non-zero exit reaches `last_error`.
static PANEL: OnceLock<Queue> = OnceLock::new();
/// A centre card's invokes, whose outcome nothing reads. Their own worker, so
/// a slow panel invoke never holds a card's Answer or Decline.
static CARD: OnceLock<Queue> = OnceLock::new();

fn panel_queue() -> &'static Queue {
    PANEL.get_or_init(async_channel::unbounded)
}

fn card_queue() -> &'static Queue {
    CARD.get_or_init(async_channel::unbounded)
}

fn invoke_argv(handle: &str, id: i64, positive: bool) -> Vec<String> {
    [BRIDGE, "invoke", "--handle", handle, "--id", &id.to_string(), "--kind", if positive { "positive" } else { "negative" }].map(str::to_owned).to_vec()
}

/// An action picked on a card mirrored off the phone, sent with the card's
/// own record: the card outlives the panel's `recent` (Clear empties it).
pub fn card_invoke(phone: &Phone, positive: bool) {
    if !phone.device_handle.is_empty() {
        let _ = card_queue().0.try_send(invoke_argv(&phone.device_handle, phone.id, positive));
    }
}

/// The owner closed `entry`: a card mirrored off the phone is cleared there
/// too, through its own negative action when it offers one.
pub fn card_closed(entry: &model::Entry) {
    if let Some(phone) = iphone::phone_of(entry)
        && !phone.negative_action.is_empty()
    {
        card_invoke(phone, false);
    }
}

fn entries(m: &model::State) -> impl Iterator<Item = &model::Entry> {
    m.popups.iter().chain(&m.pending).chain(&m.past)
}

/// The centre's side of the mirror, held by the notification state on the
/// UI thread: `iphone.notifications.dedupe`, and what a phone or local
/// arrival does to the cards already there.
#[derive(Default)]
pub struct Mirror {
    rules: Vec<iphone::DedupeRule>,
    /// The key as last read, absent included.
    seen: Option<Option<Value>>,
}

impl Mirror {
    /// Reads `iphone.notifications.dedupe` whenever settings.json moved.
    pub fn sync_config(&mut self, raw: Option<&Value>) {
        if self.seen.as_ref().is_some_and(|seen| seen.as_ref() == raw) {
            return;
        }
        self.seen = Some(raw.cloned());
        let raw = raw.cloned().unwrap_or_else(iphone::default_dedupe);
        self.rules = iphone::dedupe_rules(&raw);
    }

    /// A phone arrival; `quiet` is Focus's "history, no toast". ANCS resends
    /// an id when the phone modifies a notification, which updates its card
    /// in place, and one a local client already raised is dropped.
    pub fn phone_arrived(&self, m: &mut model::State, record: &Notification, quiet: bool, now: i64) {
        let notif = iphone::to_notification(record);
        if entries(m).any(|e| e.id == notif.id) {
            let patch = Patch { summary: Some(notif.summary), body: Some(notif.body), phone: notif.phone, ..Patch::default() };
            m.update(notif.id, &patch, now);
            return;
        }
        if !self.rules.is_empty() && iphone::dedupe(&self.rules, record, entries(m), now as f64).is_some() {
            return;
        }
        m.add(&notif, now, &AddOpts { quiet, timeout_ms: None });
    }

    /// A local arrival, `id`, just added: the local client wins, so a phone
    /// card already up for the same message goes, and the phone keeps its
    /// own copy.
    pub fn local_arrived(&self, m: &mut model::State, id: NotificationId, now: i64) {
        let Some(entry) = entries(m).find(|e| e.id == id) else { return };
        let stale = iphone::superseded(&self.rules, entry, entries(m), now as f64);
        if !stale.is_empty() {
            m.dismiss_many(&stale);
        }
    }
}

/// The phone withdrew notification `id`: its cards go without telling it
/// back. True when there was one.
pub fn withdraw(m: &mut model::State, id: i64) -> bool {
    let ids: Vec<NotificationId> = entries(m).filter(|e| iphone::phone_of(e).is_some_and(|p| p.id == id)).map(|e| e.id).collect();
    m.dismiss_many(&ids);
    !ids.is_empty()
}

/// Whether `invoke` on `id` would be queued: the entry is known, the bridge
/// is there and the connection it arrived on is still the live one.
pub fn actionable(state: &State, id: i64) -> bool {
    state.installed && iphone::is_actionable(state.recent.iter().find(|e| e.id == id), state.session)
}

#[derive(Default)]
struct Session {
    state: State,
    known: HashSet<i64>,
    arrivals: Vec<Arrival>,
    /// The phone was seen unpaired since pairing began, so a bond that was
    /// already there is not mistaken for the phone accepting.
    saw_unpaired: bool,
    advertising_hci: String,
    /// Counts `advertising` lines, so a timer armed by an earlier one does
    /// not end a later pairing.
    pair_generation: u64,
    finish_pairing: bool,
    restart_timer: bool,
    /// What the notification mirror is handed, drained after each line.
    mirror: Vec<notifications::Diff>,
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

fn focus_window() -> f64 {
    crate::settings::IPHONE_FOCUS_WINDOW.positive(&config::current())
}

impl Session {
    /// Applies one bridge line; true when the state moved.
    fn event(&mut self, event: Event) -> bool {
        let before = self.state.clone();
        match event {
            Event::Status(s) => {
                self.state.observer = s.observer;
                self.state.connected = s.connected;
                self.state.bonded = s.paired;
                self.state.bond_address = s.address;
                self.state.linked = s.linked;
                self.state.device_name = s.device_name;
                self.state.battery = (s.connected && s.battery >= 0.0).then_some(s.battery / 100.0);
                if s.observer {
                    self.state.last_error.clear();
                }
                if self.state.advertising {
                    if !s.paired {
                        self.saw_unpaired = true;
                    } else if self.saw_unpaired {
                        self.finish_pairing = true;
                    }
                }
            }
            Event::History(items) => {
                self.known = items.iter().map(|n| n.id).collect();
                let block = crate::settings::iphone_block(&config::current());
                // The bridge appends every resend of an id to its history file,
                // oldest first, so the last record per id is the live one.
                let mut seen = HashSet::new();
                let mut kept: Vec<Notification> = items.into_iter().rev().filter(|n| seen.insert(n.id) && !iphone::is_blocked(n, &block)).collect();
                kept.sort_by(|a, b| b.ts.total_cmp(&a.ts));
                kept.truncate(HISTORY_LIMIT);
                self.state.recent = kept;
            }
            Event::Notification(entry) => {
                let cfg = config::current();
                let route = iphone::RouteConfig {
                    enable: crate::settings::IPHONE_NOTIFICATIONS.get(&cfg),
                    block: crate::settings::iphone_block(&cfg),
                    focus: crate::settings::IPHONE_FOCUS.get(&cfg).to_owned(),
                };
                match iphone::route(&entry, &route) {
                    Verdict::Drop => {}
                    v => self.mirror.push(notifications::Diff::Phone { record: entry.clone(), quiet: v == Verdict::Quiet }),
                }
                if !iphone::is_blocked(&entry, &route.block) {
                    if self.state.session != entry.session && entry.session != 0 {
                        self.state.session = entry.session;
                    }
                    let fresh = self.known.insert(entry.id);
                    if fresh {
                        if iphone::focus_verdict(Some(&entry), &route.focus) != Verdict::Drop {
                            self.state.unread += 1;
                        }
                        if !entry.preexisting {
                            self.arrivals.push(Arrival { at: now_ms(), silent: entry.silent });
                        }
                    }
                    self.state.recent = iphone::upsert(&self.state.recent, entry, HISTORY_LIMIT);
                }
            }
            Event::Dismiss { id } => {
                self.known.remove(&id);
                self.state.recent = iphone::remove_by_id(&self.state.recent, id);
                self.mirror.push(notifications::Diff::DropPhone(id));
            }
            Event::PairingCode { code } => self.state.pairing_code = code,
            Event::Advertising { hci, .. } => {
                self.advertising_hci = hci;
                self.state.advertising = true;
                if !self.state.bonded {
                    self.saw_unpaired = true;
                }
                self.pair_generation += 1;
                self.restart_timer = true;
            }
            Event::Forgot { address } => {
                if self.state.bond_address == address {
                    self.state.bonded = false;
                    self.state.bond_address = None;
                }
            }
            Event::Error { message } => {
                // The pair run failed before it advertised.
                if self.state.advertising && self.advertising_hci.is_empty() {
                    self.state.advertising = false;
                    self.saw_unpaired = false;
                }
                self.state.last_error = message;
            }
        }
        self.state != before
    }

    /// The Focus reading, aged: a silent arrival leaving the window ends it
    /// with no event to say so.
    fn age(&mut self) {
        let horizon = now_ms() - focus_window() * 1000.0;
        self.arrivals.retain(|a| a.at >= horizon);
        self.state.in_focus = iphone::in_focus(&self.arrivals, now_ms(), focus_window());
    }
}

fn on_path(name: &str) -> bool {
    super::info::monitor::on_path(name)
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|h| h.trim().to_owned()).ok().filter(|h| !h.is_empty()).unwrap_or_else(|| "FormalShell".into())
}

/// What the children the service starts tell the loop.
enum Msg {
    Line(String),
    /// A bridge action exited non-zero.
    ActionFailed,
    /// stderr text from the listening bridge.
    Stderr(String),
    /// The pairing window `generation` ran out.
    PairTimeout(u64),
}

enum Ev {
    Line(Option<String>),
    Cmd(Cmd),
    Msg(Msg),
    Tick,
}

fn spawn_bridge(args: &[&str]) -> std::io::Result<async_process::Child> {
    crate::services::proc::command(BRIDGE)
        .args(args)
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::piped())
        .kill_on_drop(false)
        .spawn()
}

/// One bridge run to its exit, its lines handed to the loop. `args` is the
/// whole argv, program first.
async fn run_child(args: Vec<String>, inbox: async_channel::Sender<Msg>, report_failure: bool) {
    let argv: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let Ok(mut child) = spawn_bridge(&argv) else { return };
    let mut lines = child.stdout.take().map(|o| BufReader::new(o).lines());
    if let Some(lines) = lines.as_mut() {
        while let Some(Ok(line)) = lines.next().await {
            let _ = inbox.send(Msg::Line(line)).await;
        }
    }
    let ok = child.status().await.is_ok_and(|s| s.success());
    if !ok && report_failure {
        let _ = inbox.send(Msg::ActionFailed).await;
    }
}

/// One bridge run to its exit with nothing read back.
async fn run_quiet(args: Vec<String>) {
    let _ = crate::services::proc::command(BRIDGE)
        .args(&args[1..])
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::null())
        .stderr(async_process::Stdio::null())
        .status()
        .await;
}

async fn listen(ctx: &Ctx, session: &mut Session, inbox: &(async_channel::Sender<Msg>, async_channel::Receiver<Msg>)) -> i32 {
    let child = crate::services::proc::command(BRIDGE)
        .args(["listen", "--limit", &HISTORY_LIMIT.to_string()])
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return MISSING,
        Err(_) => return -1,
    };
    let Some(stdout) = child.stdout.take() else { return -1 };
    let mut lines = BufReader::new(stdout).lines();
    let mut errs = child.stderr.take().map(|e| BufReader::new(e).lines());
    let cmds = &commands().1;
    session.age();
    let mut next_age: Option<Instant> = (!session.arrivals.is_empty()).then(|| Instant::now() + FOCUS_TICK);
    loop {
        let ev = async { Ev::Line(lines.next().await.and_then(Result::ok)) }
            .or(async { cmds.recv().await.map_or(Ev::Tick, Ev::Cmd) })
            .or(async { inbox.1.recv().await.map_or(Ev::Tick, Ev::Msg) })
            .or(async {
                match errs.as_mut() {
                    Some(e) => match e.next().await {
                        Some(Ok(line)) => Ev::Msg(Msg::Stderr(line)),
                        _ => std::future::pending().await,
                    },
                    None => std::future::pending().await,
                }
            })
            .or(async {
                match next_age {
                    Some(at) => {
                        Timer::at(at).await;
                        Ev::Tick
                    }
                    None => std::future::pending().await,
                }
            })
            .await;
        let mut changed = false;
        match ev {
            Ev::Line(None) => break,
            Ev::Line(Some(line)) => changed |= line_in(session, &line),
            Ev::Msg(Msg::Line(line)) => changed |= line_in(session, &line),
            Ev::Msg(Msg::Stderr(text)) => {
                let text = text.trim();
                if !text.is_empty() {
                    session.state.last_error = text.to_owned();
                    changed = true;
                }
            }
            Ev::Msg(Msg::ActionFailed) => {
                if session.state.last_error.is_empty() {
                    session.state.last_error = "The phone did not take that action".into();
                    changed = true;
                }
            }
            Ev::Msg(Msg::PairTimeout(generation)) => {
                if generation == session.pair_generation && session.state.advertising {
                    session.finish_pairing = true;
                }
            }
            Ev::Cmd(cmd) => changed |= handle(ctx, session, cmd, inbox),
            Ev::Tick => {}
        }
        if session.restart_timer {
            session.restart_timer = false;
            let (generation, tx) = (session.pair_generation, inbox.0.clone());
            ctx.spawn(async move {
                Timer::after(PAIR_WINDOW).await;
                let _ = tx.send(Msg::PairTimeout(generation)).await;
            });
        }
        if session.finish_pairing {
            session.finish_pairing = false;
            end_pairing(ctx, session);
            changed = true;
        }
        let in_focus = session.state.in_focus;
        session.age();
        changed |= in_focus != session.state.in_focus;
        next_age = (!session.arrivals.is_empty()).then(|| Instant::now() + FOCUS_TICK);
        if changed {
            publish(ctx, &session.state);
        }
        for d in session.mirror.drain(..) {
            ctx.publish(store::Diff::Notifications(d));
        }
    }
    child.status().await.ok().and_then(|s| s.code()).unwrap_or(-1)
}

fn line_in(session: &mut Session, line: &str) -> bool {
    let Some(event) = iphone::parse_event(line) else { return false };
    session.state.installed = true;
    session.event(event)
}

/// One panel or IPC verb. True when the state moved.
fn handle(ctx: &Ctx, session: &mut Session, cmd: Cmd, inbox: &(async_channel::Sender<Msg>, async_channel::Receiver<Msg>)) -> bool {
    let invoke = |session: &mut Session, entry: &Notification, positive: bool| {
        if !session.state.installed {
            return false;
        }
        if !iphone::is_actionable(Some(entry), session.state.session) {
            session.state.last_error = "The phone reconnected since this arrived, so it can no longer be acted on".into();
            return true;
        }
        let _ = panel_queue().0.try_send(invoke_argv(&entry.device_handle, entry.id, positive));
        false
    };
    match cmd {
        Cmd::Invoke { id, positive } => {
            let Some(entry) = session.state.recent.iter().find(|e| e.id == id).cloned() else { return false };
            invoke(session, &entry, positive)
        }
        Cmd::Dismiss(id) => {
            let Some(entry) = session.state.recent.iter().find(|e| e.id == id).cloned() else { return false };
            session.state.recent = iphone::remove_by_id(&session.state.recent, id);
            session.known.remove(&id);
            if !entry.negative_action.is_empty() {
                invoke(session, &entry, false);
            }
            true
        }
        Cmd::Clear => {
            for entry in session.state.recent.clone() {
                if !entry.negative_action.is_empty() {
                    invoke(session, &entry, false);
                }
            }
            session.known.clear();
            session.state.recent.clear();
            session.state.unread = 0;
            if session.state.installed {
                ctx.spawn(run_child(vec![BRIDGE.into(), "clear".into()], inbox.0.clone(), false));
            }
            true
        }
        Cmd::MarkRead => std::mem::take(&mut session.state.unread) != 0,
        Cmd::Pair => {
            if !session.state.installed || session.state.advertising {
                return false;
            }
            session.state.last_error.clear();
            session.state.pairing_code.clear();
            session.saw_unpaired = !session.state.bonded;
            let mut argv = vec![BRIDGE.to_owned(), "pair".into(), "--name".into(), hostname()];
            let status = iphone::Status {
                paired: session.state.bonded,
                connected: session.state.connected,
                linked: session.state.linked,
                address: session.state.bond_address,
                ..iphone::Status::default()
            };
            if iphone::forget_before_pair(Some(&status))
                && let Some(address) = status.address
            {
                argv.extend(["--forget".to_owned(), address.to_string()]);
            }
            ctx.spawn(run_child(argv, inbox.0.clone(), false));
            // Advertising from the press on, not from the bridge's line: a
            // second EnableAdvertising while the first is still settling
            // tears it down and fails the mode change with Busy.
            session.state.advertising = true;
            session.pair_generation += 1;
            session.restart_timer = true;
            true
        }
    }
}

/// ancs4linux renames the whole adapter to the pairing name and keeps it
/// discoverable until DisableAdvertising, so the session is ended here, and
/// the phone trusted or BlueZ asks the agent to authorize every profile on
/// reconnect with no agent left to accept.
fn end_pairing(ctx: &Ctx, session: &mut Session) {
    if !session.advertising_hci.is_empty() {
        let hci = std::mem::take(&mut session.advertising_hci);
        ctx.spawn(async move {
            let _ = crate::services::proc::command("busctl")
                .args(["call", "--system", "ancs4linux.Advertising", "/", "ancs4linux.Advertising", "DisableAdvertising", "s", &hci])
                .stdout(async_process::Stdio::null())
                .stderr(async_process::Stdio::null())
                .status()
                .await;
        });
    }
    if session.state.advertising
        && session.state.bonded
        && let Some(address) = session.state.bond_address
    {
        crate::services::devices::bluetooth::send(crate::services::devices::bluetooth::Cmd::Trust(address, true));
    }
    session.state.advertising = false;
    session.state.pairing_code.clear();
    session.saw_unpaired = false;
}

fn publish(ctx: &Ctx, state: &State) {
    ams::set_connected(state.connected);
    ctx.publish(store::Diff::Iphone(state.clone()));
}

/// The phone's notifications reach the centre with no cell or panel open,
/// so the bridge child lives as long as the shell.
pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let inbox = async_channel::unbounded::<Msg>();
    let worker_inbox = inbox.0.clone();
    ctx.spawn(async move {
        while let Ok(argv) = panel_queue().1.recv().await {
            run_child(argv, worker_inbox.clone(), true).await;
        }
    });
    ctx.spawn(async move {
        while let Ok(argv) = card_queue().1.recv().await {
            run_quiet(argv).await;
        }
    });
    let mut session = Session::default();
    let mut backoff = Backoff::new(BASE_BACKOFF, MAX_BACKOFF);
    session.state.installed = on_path(BRIDGE);
    publish(&ctx, &session.state);
    loop {
        if !crate::settings::IPHONE_ENABLE.get(&config::current()) {
            Timer::after(MAX_BACKOFF).await;
            continue;
        }
        let code = listen(&ctx, &mut session, &inbox).await;
        session.state = State {
            installed: code != MISSING,
            unread: session.state.unread,
            recent: std::mem::take(&mut session.state.recent),
            ..State::default()
        };
        publish(&ctx, &session.state);
        backoff.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(connected: bool, battery: f64) -> Event {
        iphone::parse_event(&format!(r#"{{"type":"status","observer":true,"connected":{connected},"deviceName":"Phone","battery":{battery}}}"#)).unwrap()
    }

    #[test]
    fn battery_reads_only_while_connected() {
        let mut s = Session::default();
        s.event(status(true, 80.0));
        assert_eq!(s.state.battery, Some(0.8));
        s.event(status(false, 80.0));
        assert_eq!(s.state.battery, None);
        s.event(status(true, -1.0));
        assert_eq!(s.state.battery, None);
    }

    #[test]
    fn a_notification_counts_once_and_lands_in_recent() {
        let mut s = Session::default();
        let n = iphone::parse_event(r#"{"type":"notification","id":7,"title":"t"}"#).unwrap();
        s.event(n.clone());
        s.event(n);
        assert_eq!(s.state.unread, 1);
        assert_eq!(s.state.recent.len(), 1);
        s.event(iphone::parse_event(r#"{"type":"dismiss","id":7}"#).unwrap());
        assert!(s.state.recent.is_empty());
    }

    #[test]
    fn pairing_ends_when_a_bond_appears_after_the_phone_was_seen_unpaired() {
        let mut s = Session::default();
        s.event(iphone::parse_event(r#"{"type":"status","observer":true,"paired":true,"address":"AA:BB:CC:DD:EE:01"}"#).unwrap());
        s.event(iphone::parse_event(r#"{"type":"advertising","hci":"hci0","name":"x"}"#).unwrap());
        s.event(iphone::parse_event(r#"{"type":"status","observer":true,"paired":true,"address":"AA:BB:CC:DD:EE:01"}"#).unwrap());
        assert!(!s.finish_pairing, "a bond that was already there is not the phone accepting");
        s.event(iphone::parse_event(r#"{"type":"status","observer":true,"paired":false,"address":""}"#).unwrap());
        s.event(iphone::parse_event(r#"{"type":"status","observer":true,"paired":true,"address":"AA:BB:CC:DD:EE:01"}"#).unwrap());
        assert!(s.finish_pairing);
    }

    #[test]
    fn history_keeps_the_last_record_per_id() {
        let mut s = Session::default();
        s.event(iphone::parse_event(r#"{"type":"history","items":[{"id":1,"title":"old","ts":10},{"id":2,"title":"b","ts":11},{"id":1,"title":"new","ts":12}]}"#).unwrap());
        let titles: Vec<&str> = s.state.recent.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["new", "b"]);
    }

    #[test]
    fn a_pair_run_failing_before_it_advertises_ends_pairing() {
        let mut s = Session::default();
        s.state.advertising = true;
        s.event(iphone::parse_event(r#"{"type":"error","message":"Busy"}"#).unwrap());
        assert!(!s.state.advertising);
        s.event(iphone::parse_event(r#"{"type":"advertising","hci":"hci0","name":"x"}"#).unwrap());
        s.event(iphone::parse_event(r#"{"type":"error","message":"later"}"#).unwrap());
        assert!(s.state.advertising, "an error once advertising is up leaves the window open");
    }

    #[test]
    fn a_forgotten_bond_clears() {
        let mut s = Session::default();
        s.event(iphone::parse_event(r#"{"type":"status","observer":true,"paired":true,"address":"aa:bb:cc:dd:ee:01"}"#).unwrap());
        s.event(iphone::parse_event(r#"{"type":"forgot","address":"aa:bb:cc:dd:ee:01"}"#).unwrap());
        assert!(!s.state.bonded && s.state.bond_address.is_none());
    }
}
