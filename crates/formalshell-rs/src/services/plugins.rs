//! Command plugins (the spec's "User code" section). A plugin is a directory
//! under `~/.config/formalshell/plugins/<id>/` with a `manifest.json`
//! (fs-chrome's port of the eight-key manifest) whose `entry` names an
//! executable. The shell starts it with the directory as its working
//! directory and talks JSON lines:
//!
//! - stdout: one JSON object per line, `{"text", "icon", "tooltip", "class",
//!   "rows"}`. Each line replaces everything the last one said; keys it
//!   leaves out are empty. A line that is not an object is skipped. A line
//!   whose top level carries `actions` is a launcher plugin's answer to an
//!   `actions` event, naming the event's subject id in `subject`, and
//!   leaves what it shows alone.
//! - stdin: one JSON object per line, `{"event": "click", "button":
//!   "left"|"right"|"middle"}`, `{"event": "scroll", "direction":
//!   "up"|"down"}` and `{"event": "activate", "id": "<row id>"}` (a toggle
//!   row adds `"checked"`, the state it was asked to take, and a launcher
//!   action adds `"action"`). A launcher plugin is also sent `{"event":
//!   "query", "text"}` while its mode is open and `{"event": "actions",
//!   "subject": {"id", "kind", "path"}}` on Tab over a subject of a kind it
//!   declared.
//!
//! A bar or service plugin, and a panel or overlay with `keepLoaded`, runs
//! from the shell's start; any other panel or overlay runs while its card is
//! open, and a launcher plugin while the launcher is.
//!
//! A plugin that exits for any reason, or cannot be started, is a PLUGIN
//! ERROR cell and is started again on a doubling backoff. The shell does
//! nothing on a plugin's behalf while it prints nothing: no timer, no
//! polling, only a read parked on its stdout.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use async_io::Timer;
use fs_chrome::plugins::{self, Kind, Plugin, Resolved};
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, AsyncWriteExt, FutureExt, StreamExt};
use serde_json::{Value, json};

use super::info;
use super::proc;
use crate::runtime::Ctx;
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A run this long was healthy, so its next restart starts from the base.
const STABLE: Duration = Duration::from_secs(30);
/// How long a plugin let go of has to exit on its closed stdin before it is
/// killed.
const LET_GO: Duration = Duration::from_secs(1);

/// What a panel row is drawn as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowKind {
    /// Icon, text and a dim detail; activating it sends the event.
    #[default]
    Row,
    /// A button carrying the text; activating it sends the event.
    Button,
    /// Icon and text with a switch on the end.
    Toggle,
    /// A dim section label. Takes no id and no keyboard stop.
    Label,
}

impl RowKind {
    fn parse(s: &str) -> Option<RowKind> {
        match s {
            "" | "row" => Some(RowKind::Row),
            "button" => Some(RowKind::Button),
            "toggle" => Some(RowKind::Toggle),
            "label" => Some(RowKind::Label),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    pub id: String,
    pub text: String,
    pub icon: String,
    pub detail: String,
    pub checked: bool,
    /// A launcher row's own actions after its activate.
    pub actions: Vec<RowAction>,
}

/// One action a launcher plugin offers, on its own row or on a subject.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowAction {
    pub id: String,
    pub label: String,
    pub icon: String,
}

/// A launcher plugin's answer to an `actions` event: the subject it is
/// for, its actions, and when it came.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub at: Instant,
    pub subject: String,
    pub actions: Vec<RowAction>,
}

/// One stdout line.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Display(Display),
    /// The subject's id and its actions.
    Actions(String, Vec<RowAction>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Display {
    pub text: String,
    pub icon: String,
    pub tooltip: String,
    pub class: String,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Live(Display),
    Failed(String),
}

#[derive(Default)]
pub struct State {
    pub directory: String,
    pub loaded: bool,
    pub resolved: Resolved,
    /// Only plugins that have printed or failed: one that has not is
    /// starting, and shows nothing.
    pub runs: BTreeMap<String, Run>,
    /// Each launcher plugin's last `actions` answer.
    pub answers: BTreeMap<String, Answer>,
}

pub enum Diff {
    Scanned { directory: String, resolved: Resolved },
    Run(String, Option<Run>),
    Answer(String, Answer),
}

/// What a diff changed: the plugin list (the layout is resolved again) or
/// only what a plugin shows.
pub enum Change {
    List,
    Output,
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> Option<Change> {
        match diff {
            Diff::Scanned { directory, resolved } => {
                let same = self.loaded && self.directory == directory && self.resolved == resolved;
                self.runs.retain(|id, _| resolved.by_id(id).is_some());
                self.answers.retain(|id, _| resolved.by_id(id).is_some());
                (self.directory, self.resolved, self.loaded) = (directory, resolved, true);
                (!same).then_some(Change::List)
            }
            Diff::Run(id, run) => {
                let changed = match &run {
                    Some(run) => self.runs.get(&id) != Some(run),
                    None => self.runs.contains_key(&id),
                };
                match run {
                    Some(run) => self.runs.insert(id, run),
                    None => self.runs.remove(&id),
                };
                changed.then_some(Change::Output)
            }
            Diff::Answer(id, answer) => {
                self.answers.insert(id, answer);
                Some(Change::Output)
            }
        }
    }

    pub fn bar_plugins(&self) -> Vec<Plugin> {
        plugins::bar_plugins(&self.resolved.plugins)
    }

    /// The IPC `list` reply.
    pub fn list(&self) -> Value {
        Value::Array(
            self.resolved
                .plugins
                .iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "kind": p.kind.as_str(),
                        "entry": p.entry,
                        "dir": p.dir,
                        "name": p.name,
                        "region": p.region.map(|r| r.as_str()),
                        "keepLoaded": p.keep_loaded,
                        "width": p.width.map(|w| w.as_str()),
                        "mode": p.mode,
                        "subjects": p.subjects,
                        "entryUrl": p.entry_url,
                    })
                })
                .collect(),
        )
    }

    /// The IPC `status` reply. `errors` are the plugins failing now.
    pub fn status(&self) -> Value {
        let count = |kind: Kind| self.resolved.plugins.iter().filter(|p| p.kind == kind).count();
        let surfaces: Vec<String> = plugins::surface_plugins(&self.resolved.plugins).iter().map(plugins::surface_key).collect();
        let errors: Vec<Value> = self
            .runs
            .iter()
            .filter_map(|(id, run)| match run {
                Run::Failed(message) => Some(json!({"id": id, "message": message})),
                Run::Live(_) => None,
            })
            .collect();
        json!({
            "directory": self.directory,
            "loaded": self.loaded,
            "count": self.resolved.plugins.len(),
            "bar": count(Kind::Bar),
            "surface": count(Kind::Panel) + count(Kind::Overlay),
            "service": count(Kind::Service),
            "launcher": count(Kind::Launcher),
            "surfaces": surfaces,
            "errors": errors,
            "warnings": self.resolved.warnings,
        })
    }
}

fn text(o: &serde_json::Map<String, Value>, key: &str) -> String {
    o.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

/// An `actions` array's entries; one with no id is dropped.
fn row_actions(v: Option<&Value>) -> Vec<RowAction> {
    v.and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_object)
                .map(|a| RowAction { id: text(a, "id"), label: text(a, "label"), icon: text(a, "icon") })
                .filter(|a| !a.id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// One stdout line: what the plugin now shows, or an answer to `actions`.
pub fn parse_line(line: &str) -> Option<Line> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let object = value.as_object()?;
    if object.contains_key("actions") {
        return Some(Line::Actions(text(object, "subject"), row_actions(object.get("actions"))));
    }
    let rows = object
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_object)
                .filter_map(|r| {
                    Some(Row {
                        kind: RowKind::parse(&text(r, "type"))?,
                        id: text(r, "id"),
                        text: text(r, "text"),
                        icon: text(r, "icon"),
                        detail: text(r, "detail"),
                        checked: r.get("checked").and_then(Value::as_bool).unwrap_or(false),
                        actions: row_actions(r.get("actions")),
                    })
                })
                .filter(|r| r.kind == RowKind::Label || !r.id.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Some(Line::Display(Display { text: text(object, "text"), icon: text(object, "icon"), tooltip: text(object, "tooltip"), class: text(object, "class"), rows }))
}

pub fn click_event(button: &str) -> String {
    json!({"event": "click", "button": button}).to_string()
}

pub fn scroll_event(up: bool) -> String {
    json!({"event": "scroll", "direction": if up { "up" } else { "down" }}).to_string()
}

/// Sent when a row of the plugin's panel or overlay is activated. A toggle
/// row carries the state it was asked to take.
pub fn activate_event(id: &str, checked: Option<bool>) -> String {
    match checked {
        Some(checked) => json!({"event": "activate", "id": id, "checked": checked}),
        None => json!({"event": "activate", "id": id}),
    }
    .to_string()
}

/// A launcher plugin's action run on a row or subject `id`.
pub fn action_event(id: &str, action: &str) -> String {
    json!({"event": "activate", "id": id, "action": action}).to_string()
}

/// The field's words while a launcher plugin's mode is open.
pub fn query_event(text: &str) -> String {
    json!({"event": "query", "text": text}).to_string()
}

/// Tab over a subject of a kind a launcher plugin declared.
pub fn actions_event(id: &str, kind: &str, path: &str) -> String {
    json!({"event": "actions", "subject": {"id": id, "kind": kind, "path": path}}).to_string()
}

enum Cmd {
    Rescan,
    Event(String, String),
    /// A card opened (true) or closed (false) on a plugin.
    Shown(String, bool),
    /// The launcher opened or closed.
    Launcher(bool),
}

static CMDS: OnceLock<async_channel::Sender<Cmd>> = OnceLock::new();

/// Scan the directory again and restart every plugin (`plugins reload`).
pub fn reload() {
    if let Some(tx) = CMDS.get() {
        let _ = tx.try_send(Cmd::Rescan);
    }
}

/// One event line for a plugin, dropped while it is not running.
pub fn send(id: &str, line: String) {
    if let Some(tx) = CMDS.get() {
        let _ = tx.try_send(Cmd::Event(id.to_owned(), line));
    }
}

/// A panel or overlay card of plugin `id` opened or closed. A plugin that
/// is not kept loaded runs only while it is open.
pub fn shown(id: &str, open: bool) {
    if let Some(tx) = CMDS.get() {
        let _ = tx.try_send(Cmd::Shown(id.to_owned(), open));
    }
}

/// The launcher opened or closed: every launcher plugin runs while it is
/// open.
pub fn launcher(open: bool) {
    if let Some(tx) = CMDS.get() {
        let _ = tx.try_send(Cmd::Launcher(open));
    }
}

pub fn directory() -> PathBuf {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
    };
    base.join("formalshell").join("plugins")
}

fn disabled() -> Vec<String> {
    info::settings()
        .get("plugins.disabled")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn publish(ctx: &Ctx, diff: Diff) {
    ctx.publish(store::Diff::Plugins(diff));
}

/// Why a run ended, for the cell's tooltip.
fn ended(status: std::io::Result<std::process::ExitStatus>) -> String {
    use std::os::unix::process::ExitStatusExt;
    match status {
        Ok(s) => match (s.code(), s.signal()) {
            (Some(code), _) => format!("exited with status {code}"),
            (None, Some(signal)) => format!("killed by signal {signal}"),
            _ => "exited".to_owned(),
        },
        Err(e) => format!("lost: {e}"),
    }
}

enum Wake {
    Line(Option<std::io::Result<String>>),
    Event(Option<String>),
}

/// One run of the plugin: its stdout lines become what it shows, queued
/// events go to its stdin. Answers why it ended, or nothing when the shell
/// let go of it.
async fn session(ctx: &Ctx, plugin: &Plugin, events: &async_channel::Receiver<String>) -> Option<String> {
    let dir = PathBuf::from(&plugin.dir);
    let child = crate::services::proc::command(dir.join(&plugin.entry))
        .current_dir(&dir)
        .stdin(async_process::Stdio::piped())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Some(format!("cannot start: {e}")),
    };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return Some("no pipes".to_owned()) };
    let mut lines = BufReader::new(stdout).lines();
    loop {
        let wake = async { Wake::Line(lines.next().await) }.or(async { Wake::Event(events.recv().await.ok()) }).await;
        match wake {
            // Let go of, what it prints is no longer for anything.
            Wake::Line(Some(Ok(_))) if events.is_closed() => {}
            Wake::Line(Some(Ok(line))) => match parse_line(&line) {
                Some(Line::Display(display)) => publish(ctx, Diff::Run(plugin.id.clone(), Some(Run::Live(display)))),
                Some(Line::Actions(subject, actions)) => {
                    publish(ctx, Diff::Answer(plugin.id.clone(), Answer { at: Instant::now(), subject, actions }));
                }
                None => eprintln!("plugin {}: skipped a line that is not a JSON object", plugin.id),
            },
            Wake::Line(_) => return Some(ended(child.status().await)),
            Wake::Event(Some(line)) => {
                // A plugin that closed its stdin may still be printing.
                let _ = stdin.write_all(format!("{line}\n").as_bytes()).await;
            }
            Wake::Event(None) => {
                // Let go of: the events written last still have to be read,
                // so its stdin closes and it gets a moment to exit on that.
                drop(stdin);
                let _ = child.status().or(async {
                    Timer::after(LET_GO).await;
                    Err(std::io::ErrorKind::TimedOut.into())
                })
                .await;
                return None;
            }
        }
    }
}

async fn host(ctx: Ctx, plugin: Plugin, events: async_channel::Receiver<String>) {
    let mut backoff = BASE_BACKOFF;
    loop {
        let started = Instant::now();
        let Some(reason) = session(&ctx, &plugin, &events).await else { return };
        // One let go of that ended on its own is neither failed nor started
        // again: what it showed went when it was let go of.
        if events.is_closed() {
            return;
        }
        eprintln!("plugin {}: {reason}, restarting in {}s", plugin.id, backoff.as_secs());
        publish(&ctx, Diff::Run(plugin.id.clone(), Some(Run::Failed(reason))));
        Timer::after(backoff).await;
        if events.is_closed() {
            return;
        }
        // Whatever was clicked while it was down is not for the next run.
        while events.try_recv().is_ok() {}
        backoff = if started.elapsed() >= STABLE { BASE_BACKOFF } else { (backoff * 2).min(MAX_BACKOFF) };
    }
}

/// Plugins that run from the start: bar cells and services always, a panel
/// or overlay only when its manifest keeps it loaded (the others start when
/// their card opens), a launcher plugin never (it starts with the launcher).
fn starts(plugin: &Plugin) -> bool {
    match plugin.kind {
        Kind::Bar | Kind::Service => true,
        Kind::Panel | Kind::Overlay => plugin.keep_loaded == Some(true),
        Kind::Launcher => false,
    }
}

/// Each running host's event sender, its stop sender, and a receiver on the
/// same events for dropping the oldest when a stuck plugin fills them.
type Hosts = HashMap<String, (async_channel::Sender<String>, async_channel::Sender<()>, async_channel::Receiver<String>)>;

/// Hosts let go of while they still owe their plugin the events queued
/// before: each runs until its events run out, its stop sender kept here so
/// a rescan still ends it.
type Draining = Vec<async_channel::Sender<()>>;

fn start(ctx: &Ctx, hosts: &mut Hosts, plugin: &Plugin) {
    if hosts.contains_key(&plugin.id) {
        return;
    }
    let (events_tx, events_rx) = async_channel::bounded(64);
    let (stop_tx, stop_rx) = async_channel::bounded::<()>(1);
    let events_rx_oldest = events_rx.clone();
    let task = host(ctx.clone(), plugin.clone(), events_rx);
    ctx.spawn(async move {
        task.or(async {
            let _ = stop_rx.recv().await;
        })
        .await
    });
    hosts.insert(plugin.id.clone(), (events_tx, stop_tx, events_rx_oldest));
}

/// A plugin that runs only while something of it is open, opened or closed.
/// Closing lets the host write what was queued first (the activate a row
/// sent as the launcher closed over it), then close the plugin's stdin.
fn show(ctx: &Ctx, hosts: &mut Hosts, draining: &mut Draining, open: &mut HashSet<String>, resolved: &Resolved, id: String, on: bool) {
    if on {
        open.insert(id.clone());
    } else {
        open.remove(&id);
    }
    let Some(plugin) = resolved.by_id(&id).filter(|p| !starts(p)) else { return };
    if on {
        start(ctx, hosts, plugin);
    } else if let Some((events, stop, _)) = hosts.remove(&id) {
        drop(events);
        draining.retain(|s| !s.is_closed());
        draining.push(stop);
        // What it last printed goes with it.
        publish(ctx, Diff::Run(id, None));
    }
}

async fn rescan(ctx: &Ctx, hosts: &mut Hosts, open: &HashSet<String>) -> Resolved {
    let dir = directory();
    let done = proc::capture(&plugins::scan_command(&dir.to_string_lossy()), Duration::from_secs(10)).await;
    let resolved = plugins::resolve(Some(&done.stdout), &disabled());
    for warning in &resolved.warnings {
        eprintln!("plugins: {warning}");
    }
    // Dropping a stop sender ends its host, which kills the child.
    hosts.clear();
    for plugin in resolved.plugins.iter().filter(|p| starts(p) || open.contains(&p.id)) {
        start(ctx, hosts, plugin);
    }
    publish(ctx, Diff::Scanned { directory: dir.to_string_lossy().into_owned(), resolved: resolved.clone() });
    resolved
}

enum Next {
    Cmd(Option<Cmd>),
    Settings,
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = CMDS.set(tx);
    let settings = info::changed();
    // The disabled list is part of the scan, so wait for settings.json.
    if !info::configured() {
        let _ = settings.recv().await;
    }
    let mut hosts = Hosts::new();
    let mut draining = Draining::new();
    let mut off = disabled();
    let mut open = HashSet::new();
    let mut resolved = rescan(&ctx, &mut hosts, &open).await;
    loop {
        let next = async { Next::Cmd(rx.recv().await.ok()) }.or(async {
            let _ = settings.recv().await;
            Next::Settings
        });
        match next.await {
            Next::Cmd(None) => return,
            Next::Cmd(Some(Cmd::Rescan)) => {
                draining.clear();
                resolved = rescan(&ctx, &mut hosts, &open).await;
            }
            Next::Cmd(Some(Cmd::Shown(id, on))) => show(&ctx, &mut hosts, &mut draining, &mut open, &resolved, id, on),
            Next::Cmd(Some(Cmd::Launcher(on))) => {
                let ids: Vec<String> = plugins::launcher_plugins(&resolved.plugins).map(|p| p.id.clone()).collect();
                for id in ids {
                    show(&ctx, &mut hosts, &mut draining, &mut open, &resolved, id, on);
                }
            }
            Next::Cmd(Some(Cmd::Event(id, line))) => {
                // The newest event wins a full queue: a query typed while
                // the plugin is stuck is the one it has to answer.
                if let Some((events, _, oldest)) = hosts.get(&id)
                    && let Err(async_channel::TrySendError::Full(line)) = events.try_send(line)
                {
                    let _ = oldest.try_recv();
                    let _ = events.try_send(line);
                }
            }
            Next::Settings => {
                let now = disabled();
                if now != off {
                    off = now;
                    draining.clear();
                    resolved = rescan(&ctx, &mut hosts, &open).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(line: &str) -> Display {
        match parse_line(line) {
            Some(Line::Display(d)) => d,
            other => panic!("{line} is not a display: {other:?}"),
        }
    }

    #[test]
    fn a_line_is_the_whole_display() {
        let d = display(r#"{"text":"42","icon":"cpu","class":"warning","tooltip":"t","rows":[{"id":"a","text":"A"},{"text":"no id"},7]}"#);
        assert_eq!((d.text.as_str(), d.icon.as_str(), d.class.as_str(), d.tooltip.as_str()), ("42", "cpu", "warning", "t"));
        assert_eq!(d.rows, [Row { id: "a".into(), text: "A".into(), ..Row::default() }]);
        assert_eq!(display("{}"), Display::default());
        let d = display(r#"{"rows":[{"type":"toggle","id":"t","text":"T","checked":true},{"type":"label","text":"L"},{"type":"nope","id":"x"},{"type":"button","text":"no id"}]}"#);
        let kinds: Vec<_> = d.rows.iter().map(|r| (r.kind, r.checked)).collect();
        assert_eq!(kinds, [(RowKind::Toggle, true), (RowKind::Label, false)]);
        assert_eq!(display(r#"{"text": 3}"#).text, "");
    }

    #[test]
    fn a_launcher_row_carries_its_actions_and_an_actions_line_is_an_answer() {
        let d = display(r#"{"rows":[{"id":"n1","text":"Note","actions":[{"id":"pin","label":"Pin","icon":"pin"},{"label":"no id"}]}]}"#);
        assert_eq!(d.rows[0].actions, [RowAction { id: "pin".into(), label: "Pin".into(), icon: "pin".into() }]);
        let line = parse_line(r#"{"subject":"files.ab","actions":[{"id":"share","label":"Share"}],"rows":[]}"#);
        assert_eq!(line, Some(Line::Actions("files.ab".into(), vec![RowAction { id: "share".into(), label: "Share".into(), icon: String::new() }])));
        assert_eq!(parse_line(r#"{"actions":7}"#), Some(Line::Actions(String::new(), Vec::new())));
    }

    #[test]
    fn lines_that_are_not_objects_are_skipped() {
        for line in ["", "nope", "[1]", "3", "\"x\""] {
            assert_eq!(parse_line(line), None, "{line}");
        }
    }

    #[test]
    fn events_are_json_lines() {
        let v: Value = serde_json::from_str(&click_event("left")).unwrap();
        assert_eq!((v["event"].as_str(), v["button"].as_str()), (Some("click"), Some("left")));
        let v: Value = serde_json::from_str(&scroll_event(true)).unwrap();
        assert_eq!((v["event"].as_str(), v["direction"].as_str()), (Some("scroll"), Some("up")));
        let v: Value = serde_json::from_str(&activate_event("r1", None)).unwrap();
        assert_eq!((v["event"].as_str(), v["id"].as_str(), v.get("checked")), (Some("activate"), Some("r1"), None));
        let v: Value = serde_json::from_str(&activate_event("t1", Some(true))).unwrap();
        assert_eq!(v["checked"], json!(true));
        let v: Value = serde_json::from_str(&action_event("files.ab", "share")).unwrap();
        assert_eq!(v, json!({"event": "activate", "id": "files.ab", "action": "share"}));
        let v: Value = serde_json::from_str(&query_event("fo")).unwrap();
        assert_eq!(v, json!({"event": "query", "text": "fo"}));
        let v: Value = serde_json::from_str(&actions_event("files.ab", "file", "/x")).unwrap();
        assert_eq!(v, json!({"event": "actions", "subject": {"id": "files.ab", "kind": "file", "path": "/x"}}));
    }

    #[test]
    fn a_failed_run_is_an_error_until_it_prints_again() {
        let mut s = State::default();
        assert!(s.apply(Diff::Run("a".into(), Some(Run::Failed("exited with status 1".into())))).is_some());
        assert_eq!(s.status()["errors"][0]["id"], json!("a"));
        s.apply(Diff::Run("a".into(), Some(Run::Live(Display::default()))));
        assert_eq!(s.status()["errors"], json!([]));
    }
}
