//! The launcher's index (the menu tree, the live device sources, the `when`
//! and `checked` conditions and the keybinds), kept warm on the service
//! thread so an open only maps a surface over rows that already exist.
//!
//! The tree is a base (default-menu.jsonc, the self-targeted fragments, the
//! user's menu.jsonc and `menu.customPowerButtons`) and one row list per
//! provider source. Each source is recomputed alone and lands as its own
//! diff; the UI thread re-attaches it under its provider node. Desktop
//! entries are scanned, and their icons looked up and decoded, on the
//! blocking pool.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use fs_menu::frecency::Record;
use fs_menu::keybinds::Bind;
use fs_menu::model::{build_tree, entries_from_value, parse_jsonc};
use fs_menu::node::{DesktopEntry, Entries, Kind, Node, Tree};
use fs_menu::providers::{self, CustomPowerButton};
use serde_json::Value;

use super::{appicon, icons, proc};
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

const DEFAULT_MENU: &str = include_str!("../../../fs-menu/data/default-menu.jsonc");
const EMOJI: &str = include_str!("../../../fs-menu/data/emoji.json");
/// How long after an ask the condition checks start: past the launcher's
/// entrance, whose springs settle in about a second.
const CONDS_AFTER: Duration = Duration::from_millis(1000);

/// What fs-menu's self-targeted fragments call the shell by, which
/// [`ipc_command`] turns into the client installed beside this binary at the
/// moment an action runs.
pub const SELF: &str = "formalshell-ipc";

/// The app cell's icon edge (`controlHeight * 2`), the size every app
/// icon is decoded at.
pub const ICON_EXTENT: u32 = 64;

#[derive(Default)]
pub struct State {
    base: Tree,
    sources: HashMap<String, Vec<Node>>,
    /// The base with every source attached under its provider node.
    pub tree: Tree,
    /// `when` results by node id; absent is unresolved, which hides.
    pub cond: HashMap<String, bool>,
    /// Shell `checked` results by node id.
    pub checked: HashMap<String, bool>,
    /// Decoded app icons by `iconSource` path, `None` where the file did
    /// not decode.
    pub icons: HashMap<String, Option<Bitmap>>,
    /// The compositor's binds: `None` until it answered, `Err` when it
    /// failed.
    pub binds: Option<Result<Arc<[Bind]>, ()>>,
    pub emoji: Option<Arc<fs_menu::providers::EmojiIndex>>,
    /// Each desktop entry's `Exec` argv by entry id.
    pub exec: HashMap<String, Vec<String>>,
}

pub enum Diff {
    Base(Tree),
    Source(String, Vec<Node>),
    Cond { id: String, when: bool, ok: bool },
    Icons(Vec<(String, Option<Bitmap>)>),
    Binds(Result<Arc<[Bind]>, ()>),
    Emoji(Arc<fs_menu::providers::EmojiIndex>),
    Exec(HashMap<String, Vec<String>>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Base(base) => {
                self.base = base;
                self.assemble();
            }
            Diff::Source(name, rows) => {
                if self.sources.get(&name) == Some(&rows) {
                    return false;
                }
                self.sources.insert(name, rows);
                self.assemble();
            }
            Diff::Cond { id, when, ok } => {
                let map = if when { &mut self.cond } else { &mut self.checked };
                if map.get(&id) == Some(&ok) {
                    return false;
                }
                map.insert(id, ok);
            }
            Diff::Icons(list) => {
                for (path, bitmap) in list {
                    self.icons.insert(path, bitmap);
                }
            }
            Diff::Binds(reply) => self.binds = Some(reply),
            Diff::Emoji(index) => self.emoji = Some(index),
            Diff::Exec(exec) => {
                if self.exec == exec {
                    return false;
                }
                self.exec = exec;
            }
        }
        true
    }

    /// Providers.applyProviders over the base, one source at a time.
    fn assemble(&mut self) {
        let mut tree = self.base.clone();
        let targets: Vec<(String, String)> = tree
            .nodes
            .values()
            .filter(|n| n.kind == Kind::Provider)
            .filter_map(|n| n.provider.clone().map(|p| (n.id.clone(), p)))
            .collect();
        for (id, provider) in targets {
            if let Some(rows) = self.sources.get(&provider) {
                providers::attach_children(&mut tree, &id, rows.clone());
            }
        }
        self.tree = tree;
    }

    pub fn nodes(&self) -> &indexmap::IndexMap<String, Node> {
        &self.tree.nodes
    }
}

/// An action string as `sh -c` runs it: fs-menu's client prefix swapped for
/// the one installed beside this binary.
pub fn ipc_command(action: &str) -> String {
    let prefix = format!("{SELF} call ");
    match action.strip_prefix(&prefix) {
        Some(rest) => format!("{} call {rest}", ipc_client()),
        None => action.to_owned(),
    }
}

pub fn ipc_client() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("formalshell-ipc")))
        .filter(|p| p.exists())
        .map_or_else(|| "formalshell-ipc".to_owned(), |p| p.to_string_lossy().into_owned())
}

/// What the base tree is built from besides the files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BaseInputs {
    pub buttons: Value,
    /// The keyboard lights' effects, `None` while no keyboard was found.
    pub lights: Option<Vec<providers::LightEffect>>,
    /// The share subtree: LocalSend's state and what it can send.
    pub share: Option<ShareInputs>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShareInputs {
    pub installed: bool,
    pub peers: Vec<String>,
    pub items: Vec<fs_menu::clipboard::history::Entry>,
    pub receive: providers::ReceiveStatus,
}

pub enum Ask {
    /// An input of the base changed, or a refresh: the base again.
    Base(BaseInputs),
    /// The apps source again, rescanning only when a directory moved.
    Apps(Vec<Record>),
    /// Run these `when`/`checked` commands, `(id, command, is_when)`.
    Conds(Vec<(String, String, bool)>),
    Binds,
    Emoji,
    Spawn(String),
    /// An applications directory changed under the shell.
    Rescan,
}

static ASK: OnceLock<async_channel::Sender<Ask>> = OnceLock::new();

pub fn ask(a: Ask) {
    if let Some(tx) = ASK.get() {
        let _ = tx.try_send(a);
    }
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    // Warm before anything asks: the tree and the apps exist by the first
    // summon even with no settings.json and no state.json yet.
    let _ = tx.try_send(Ask::Base(BaseInputs::default()));
    let _ = tx.try_send(Ask::Apps(Vec::new()));
    let _ = tx.try_send(Ask::Emoji);
    ctx.spawn(watch_entries(tx.clone()));
    let _ = ASK.set(tx);
    let mut launches_seen: Vec<Record> = Vec::new();
    let scan = Arc::new(std::sync::Mutex::new(Scan::default()));
    while let Ok(a) = rx.recv().await {
        match a {
            Ask::Base(inputs) => {
                if let Some(tree) = ctx.pool().run(move || base(&inputs)).await {
                    ctx.publish(store::Diff::Menu(Diff::Base(tree)));
                }
            }
            Ask::Rescan => {
                let _ = ASK.get().map(|tx| tx.try_send(Ask::Apps(launches_seen.clone())));
            }
            Ask::Apps(launches) => {
                launches_seen = launches.clone();
                let scan = scan.clone();
                let done = ctx.pool().run(move || scan.lock().unwrap().apps(&launches)).await;
                if let Some((rows, icons, exec)) = done {
                    ctx.publish(store::Diff::Menu(Diff::Exec(exec)));
                    ctx.publish(store::Diff::Menu(Diff::Source("apps".into(), rows)));
                    if !icons.is_empty() {
                        ctx.publish(store::Diff::Menu(Diff::Icons(icons)));
                    }
                }
            }
            Ask::Conds(list) => {
                // The launcher opens on the last results; the shells that
                // check them again start once its card is up, not while it
                // is still coming in.
                let c = ctx.clone();
                ctx.spawn(async move {
                    async_io::Timer::after(CONDS_AFTER).await;
                    for (id, command, when) in list {
                        let c2 = c.clone();
                        c.spawn(async move {
                            let done = proc::capture(&proc::argv(&["sh", "-c", &command]), Duration::from_secs(10)).await;
                            c2.publish(store::Diff::Menu(Diff::Cond { id, when, ok: done.code == 0 }));
                        });
                    }
                });
            }
            Ask::Binds => {
                let c = ctx.clone();
                ctx.spawn(async move {
                    let reply = super::compositor::binds().await.map(Arc::<[Bind]>::from);
                    c.publish(store::Diff::Menu(Diff::Binds(reply)));
                });
            }
            Ask::Emoji => {
                if let Some(index) = ctx.pool().run(|| fs_menu::providers::EmojiIndex::new(fs_menu::providers::parse_emoji_dataset(EMOJI))).await {
                    ctx.publish(store::Diff::Menu(Diff::Emoji(Arc::new(index))));
                }
            }
            Ask::Spawn(command) => {
                super::compositor::spawn(&proc::argv(&["sh", "-c", &command]));
            }
        }
    }
}

/// default-menu.jsonc merged with the self-targeted fragments and the
/// custom power buttons, then the user's overlay over it.
fn base(inputs: &BaseInputs) -> Tree {
    let mut merged: Entries = parse_jsonc(DEFAULT_MENU).ok().and_then(|v| entries_from_value(&v).ok()).unwrap_or_default();
    for (k, v) in providers::capture_entries(SELF) {
        merged.insert(k, v);
    }
    if let Some(effects) = &inputs.lights {
        for (k, v) in providers::lights_entries(true, effects) {
            merged.insert(k, v);
        }
    }
    if let Some(sh) = &inputs.share {
        let peers: Vec<providers::Peer> = sh.peers.iter().map(|n| providers::Peer { name: n.clone() }).collect();
        for (k, v) in providers::share_peer_entries(sh.installed, &peers, &sh.items, Some(&sh.receive)) {
            merged.insert(k, v);
        }
    }
    for (k, v) in providers::custom_power_button_entries(&power_buttons(&inputs.buttons)) {
        merged.insert(k, v);
    }
    let user = user_menu_path();
    let user = std::fs::read_to_string(&user)
        .ok()
        .and_then(|t| match parse_jsonc(&t) {
            Ok(v) => entries_from_value(&v).ok(),
            Err(e) => {
                eprintln!("Menu: failed to parse menu.jsonc: {e}");
                None
            }
        })
        .unwrap_or_default();
    let mut tree = build_tree(&merged, &user);
    let panels = providers::panels_provider(SELF);
    if tree.nodes.contains_key("panels") {
        providers::attach_children(&mut tree, "panels", panels);
    }
    tree
}

fn power_buttons(v: &Value) -> Vec<CustomPowerButton> {
    let Value::Array(items) = v else { return Vec::new() };
    items
        .iter()
        .filter_map(|b| {
            let s = |k: &str| b.get(k).and_then(Value::as_str).map(str::to_owned);
            Some(CustomPowerButton {
                label: s("label")?,
                icon: s("icon"),
                command: s("command")?,
                confirm: b.get("confirm").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

fn user_menu_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    config.join("formalshell").join("menu.jsonc")
}

/// The desktop entries and the icons already decoded, kept across asks.
#[derive(Default)]
struct Scan {
    stamp: Vec<Option<SystemTime>>,
    scanned: bool,
    entries: Vec<fs_system::compositor::appicon::DesktopEntry>,
    icon_of: HashMap<String, String>,
    decoded: HashSet<String>,
    /// The scale the icons were decoded for: the launcher's output's, the
    /// bar's.
    scale: f64,
}

impl Scan {
    fn apps(&mut self, launches: &[Record]) -> (Vec<Node>, Vec<(String, Option<Bitmap>)>, HashMap<String, Vec<String>>) {
        let dirs = appicon::applications_dirs();
        let stamp: Vec<Option<SystemTime>> = dirs.iter().map(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok()).collect();
        if !self.scanned || stamp != self.stamp {
            self.entries = appicon::scan_launchable(&dirs);
            self.stamp = stamp;
            self.scanned = true;
        }
        let entries: Vec<DesktopEntry> = self
            .entries
            .iter()
            .map(|e| DesktopEntry {
                id: e.id.clone(),
                name: e.name.clone(),
                icon: e.icon.clone(),
                generic_name: e.generic_name.clone(),
                startup_class: e.startup_class.clone(),
            })
            .collect();
        let scale = crate::surface::primary_scale();
        if scale != self.scale {
            self.scale = scale;
            self.icon_of.clear();
            self.decoded.clear();
        }
        let px = crate::scene::pixels_at(f64::from(ICON_EXTENT), scale);
        let icon_of = &mut self.icon_of;
        let resolve = |name: &str| -> String {
            icon_of
                .entry(name.to_owned())
                .or_insert_with(|| icons::lookup(name, "", px).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
                .clone()
        };
        let cell = std::cell::RefCell::new(resolve);
        let rows = providers::apps_provider(&entries, Some(&|n: &str| (cell.borrow_mut())(n)), launches, None);
        let mut fresh = Vec::new();
        for row in &rows {
            let path = &row.icon_source;
            if path.is_empty() || !self.decoded.insert(path.clone()) {
                continue;
            }
            let bitmap = icons::load_at(std::path::Path::new(path), ICON_EXTENT, scale);
            fresh.push((path.clone(), bitmap));
        }
        let exec = self.entries.iter().map(|e| (e.id.clone(), e.command.clone())).collect();
        (rows, fresh, exec)
    }
}

/// Proc.appLaunch: under uwsm a plain entry goes by its id, so uwsm reads
/// its Exec line; everything else runs its own argv.
pub fn launch_argv(state: &State, id: &str) -> Option<Vec<String>> {
    let command = state.exec.get(id).filter(|c| !c.is_empty())?;
    if std::env::var_os("UWSM_FINALIZE_VARNAMES").is_some_and(|v| !v.is_empty()) {
        let plain = id.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
            && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
        if plain {
            return Some(proc::argv(&["uwsm", "app", "--", &format!("{id}.desktop")]));
        }
        let mut argv = proc::argv(&["uwsm", "app", "--"]);
        argv.extend(command.iter().cloned());
        return Some(argv);
    }
    Some(command.clone())
}

/// Desktop entries installed or removed while the shell runs reach the
/// apps source without a summon: an inotify watch on every applications
/// directory, settled a moment so a package's burst of files is one scan.
#[cfg(target_os = "linux")]
async fn watch_entries(tx: async_channel::Sender<Ask>) {
    use std::os::fd::AsFd;

    use inotify::{Inotify, WatchMask};

    let Ok(mut inotify) = Inotify::init() else { return };
    let mask = WatchMask::CREATE | WatchMask::DELETE | WatchMask::MOVED_TO | WatchMask::MOVED_FROM | WatchMask::CLOSE_WRITE;
    for dir in appicon::applications_dirs() {
        let _ = inotify.watches().add(&dir, mask);
    }
    let Ok(fd) = inotify.as_fd().try_clone_to_owned() else { return };
    let Ok(ready) = async_io::Async::new(fd) else { return };
    let mut buffer = [0u8; 4096];
    loop {
        if ready.readable().await.is_err() {
            return;
        }
        async_io::Timer::after(Duration::from_millis(200)).await;
        while inotify.read_events(&mut buffer).is_ok_and(|e| e.count() > 0) {}
        let _ = tx.try_send(Ask::Rescan);
    }
}

#[cfg(not(target_os = "linux"))]
async fn watch_entries(_tx: async_channel::Sender<Ask>) {}
