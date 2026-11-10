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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, SystemTime};

use fs_menu::frecency::Record;
use fs_types::Bind;
use fs_menu::base::BaseInputs;
use fs_menu::model::{entries_from_value, parse_jsonc};
use fs_menu::actions::Run;
use fs_menu::intent::Intent;
use fs_menu::node::{ActionEntry, DesktopEntry, Entries, Kind, Node, Provider, Tree};
use fs_menu::providers::{self, Listed, Peeked};

use super::{appicon, icons, proc};
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

/// How long after an ask the condition checks start: past the launcher's
/// entrance, whose springs settle in twice the spatial family's length.
fn conds_after() -> Duration {
    Duration::from_secs_f64(2.0 * fs_theme::tokens::motion_tokens(true).spatial / 1000.0)
}

/// The app cell's icon edge (`controlHeight * 2`), the size every app
/// icon is decoded at.
pub const ICON_EXTENT: u32 = 64;

/// A launcher row's icon edge (`LAUNCHER.row_icon`), decoded beside the
/// cell's so a row never scales one down while it draws.
pub const ROW_ICON_EXTENT: u32 = fs_theme::tokens::LAUNCHER.row_icon as u32;

/// One icon's path, then its file at the cell's edge and at the row's.
pub type Icon = (String, Option<Bitmap>, Option<Bitmap>);

#[derive(Default)]
pub struct State {
    base: Tree,
    sources: HashMap<Provider, Vec<Node>>,
    /// The base with every source attached under its provider node.
    pub tree: Tree,
    /// `when` results by node id; absent is unresolved, which hides.
    pub cond: HashMap<String, bool>,
    /// Shell `checked` results by node id.
    pub checked: HashMap<String, bool>,
    /// Decoded app icons by `iconSource` path, `None` where the file did
    /// not decode.
    pub icons: HashMap<String, Option<Bitmap>>,
    /// The same icons at [`ROW_ICON_EXTENT`].
    pub row_icons: HashMap<String, Option<Bitmap>>,
    /// The compositor's binds: `None` until it answered, `Err` when it
    /// failed.
    pub binds: Option<Result<Arc<[Bind]>, ()>>,
    pub emoji: Option<Arc<fs_menu::providers::EmojiIndex>>,
    pub exec: Exec,
    /// The folder → last listed, by path: its sorted entries, or why it
    /// could not be read.
    pub folder: Option<(String, Listing)>,
    /// What the preview pane last read off a file or folder, by path.
    pub peek: Option<(String, Peeked)>,
    /// Picked files and folders by path: whether each is a folder, `None`
    /// once it is gone. Absent is not looked at yet.
    pub files: HashMap<String, Option<bool>>,
}

/// A folder's sorted entries, or why it could not be read.
pub type Listing = Result<Arc<[Listed]>, String>;

pub enum Diff {
    Base(Tree),
    Source(Provider, Vec<Node>),
    Cond { id: String, when: bool, ok: bool },
    Icons(Vec<Icon>),
    Binds(Result<Arc<[Bind]>, ()>),
    Emoji(Arc<fs_menu::providers::EmojiIndex>),
    Exec(Exec),
    Folder(String, Listing),
    Peek(String, Peeked),
    Files(HashMap<String, Option<bool>>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Base(mut base) => {
                check_calls(base.nodes.values_mut());
                self.base = base;
                self.assemble();
            }
            Diff::Source(name, mut rows) => {
                check_calls(rows.iter_mut());
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
                for (path, cell, row) in list {
                    self.icons.insert(path.clone(), cell);
                    self.row_icons.insert(path, row);
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
            Diff::Folder(path, listing) => self.folder = Some((path, listing)),
            Diff::Peek(path, peeked) => self.peek = Some((path, peeked)),
            Diff::Files(files) => {
                let before = self.files.len();
                let changed = files.iter().any(|(p, k)| self.files.get(p) != Some(k));
                self.files.extend(files);
                if !changed && self.files.len() == before {
                    return false;
                }
            }
        }
        true
    }

    /// Providers.applyProviders over the base, one source at a time.
    fn assemble(&mut self) {
        let mut tree = self.base.clone();
        let targets: Vec<(String, Provider)> = tree
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

/// Every `@ipc:` call on `nodes` checked against what answers it
/// (`ipc::check`): one that fits takes its function's arguments, one that
/// does not is dropped, logged once with its reason, and a row left with
/// no action of its own, or holding a refused column action, goes dim.
/// Returns the refusals.
fn check_calls<'a>(nodes: impl Iterator<Item = &'a mut Node>) -> Vec<String> {
    static LOGGED: LazyLock<std::sync::Mutex<HashSet<String>>> = LazyLock::new(Default::default);
    let mut refused = Vec::new();
    let mut check = |id: &str, slot: &mut Option<Run>| {
        let Some(Run::Ipc(call)) = slot.as_ref() else { return true };
        match crate::ipc::check(call) {
            Ok(fit) => {
                *slot = Some(Run::Ipc(fit));
                true
            }
            Err(reason) => {
                refused.push(format!("{id}: refused {}: {reason}", Run::Ipc(call.clone())));
                *slot = None;
                false
            }
        }
    };
    for n in nodes {
        if !check(&n.id, &mut n.action) {
            n.dim = Some(true);
        }
        if !check(&n.id, &mut n.alternate) {
            n.alternate_label = None;
        }
        // A refused override left empty would fall back to the built-in
        // action of the same id, and one left out would hand Enter to the
        // next: it stays in the column running nothing, on a dim row.
        for a in &mut n.actions {
            if !check(&format!("{} {}", n.id, a.id), &mut a.action) {
                a.action = Some(Run::Note);
                n.dim = Some(true);
            }
        }
    }
    let mut logged = LOGGED.lock().expect("refusals");
    for line in refused.iter().filter(|line| logged.insert((*line).clone())) {
        eprintln!("Menu: {line}");
    }
    refused
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
    /// An applications directory changed under the shell.
    Rescan,
    /// The scale the launcher opens at (the bar's output's) moved: the
    /// apps' icons are decoded again for it.
    Scale(f64),
    /// The Files source again: the recent list and the user directories.
    Files,
    /// One folder's entries, for →.
    Folder(String),
    /// A file or folder's preview.
    Peek(String),
    /// Whether each picked path is still there, and a folder.
    Stat(Vec<String>),
    /// Icon files by absolute path, for rows that are no desktop entry (a
    /// catalog's package icon).
    Icons(Vec<String>),
}

/// Seeded with the warm-up asks as it is made, so an ask sent before `run`
/// starts queues behind them rather than being overwritten by them: the tree
/// and the apps exist by the first summon even with no settings.json and no
/// state.json yet.
static ASK: LazyLock<(async_channel::Sender<Ask>, async_channel::Receiver<Ask>)> = LazyLock::new(|| {
    let (tx, rx) = async_channel::unbounded();
    for a in [Ask::Base(BaseInputs::default()), Ask::Apps(Vec::new()), Ask::Emoji, Ask::Files] {
        let _ = tx.try_send(a);
    }
    (tx, rx)
});

/// Callable from any thread.
pub fn ask(a: Ask) {
    let _ = ASK.0.try_send(a);
}

/// The app subjects among state.json's picks, keyed by desktop id as the
/// apps source orders by.
pub fn launches(picks: &[Record]) -> Vec<Record> {
    picks.iter().filter_map(|r| Some(Record { id: r.id.strip_prefix("app:")?.to_owned(), ..*r })).collect()
}

/// state.json's abbreviations, empty before the first pin or learned pick.
pub fn abbreviation_book(store: &store::Store) -> fs_menu::abbrev::Book {
    store.state.data.abbreviations.clone().unwrap_or_default()
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (tx, rx) = (ASK.0.clone(), ASK.1.clone());
    ctx.spawn(watch_entries(tx.clone()));
    let mut scale = 1.0;
    let mut launches_seen: Vec<Record> = Vec::new();
    let scan = Arc::new(std::sync::Mutex::new(Scan::default()));
    // Set by a change under an applications directory: the next scan reads
    // the entries again even where no directory's mtime moved (an edit in
    // place, a subdirectory).
    let rescan = Arc::new(AtomicBool::new(false));
    // The pane's latest ask of each kind. The store holds one reply each, so
    // a slower reply for a row the cursor left is dropped rather than left
    // over the newer one.
    let (peek_latest, folder_latest) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    while let Ok(a) = rx.recv().await {
        match a {
            Ask::Base(inputs) => {
                if let Some(tree) = ctx.pool().run(move || base(&inputs)).await {
                    ctx.publish(store::Diff::Menu(Diff::Base(tree)));
                }
            }
            Ask::Rescan => {
                rescan.store(true, Ordering::Relaxed);
                let _ = tx.try_send(Ask::Apps(launches_seen.clone()));
            }
            Ask::Scale(k) => {
                if k != scale {
                    scale = k;
                    let _ = tx.try_send(Ask::Apps(launches_seen.clone()));
                }
            }
            Ask::Apps(launches) => {
                launches_seen = launches.clone();
                let (scan, rescan) = (scan.clone(), rescan.clone());
                let done = ctx.pool().run(move || scan.lock().unwrap().apps(&launches, rescan.swap(false, Ordering::Relaxed), scale)).await;
                if let Some((rows, icons, exec)) = done {
                    ctx.publish(store::Diff::Menu(Diff::Exec(exec)));
                    ctx.publish(store::Diff::Menu(Diff::Source(Provider::Apps, rows)));
                    if !icons.is_empty() {
                        ctx.publish(store::Diff::Menu(Diff::Icons(icons)));
                    }
                }
            }
            Ask::Icons(paths) => {
                let scan = scan.clone();
                let done = ctx.pool().run(move || scan.lock().unwrap().icons(&paths, scale)).await;
                if let Some(icons) = done.filter(|i| !i.is_empty()) {
                    ctx.publish(store::Diff::Menu(Diff::Icons(icons)));
                }
            }
            Ask::Conds(list) => {
                // The launcher opens on the last results; the shells that
                // check them again start once its card is up, not while it
                // is still coming in.
                let c = ctx.clone();
                ctx.spawn(async move {
                    async_io::Timer::after(conds_after()).await;
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
                if let Some(index) = ctx.pool().run(|| fs_menu::providers::EmojiIndex::new(fs_menu::providers::parse_emoji_dataset(fs_menu::base::EMOJI))).await {
                    ctx.publish(store::Diff::Menu(Diff::Emoji(Arc::new(index))));
                }
            }
            Ask::Files => {
                if let Some(rows) = ctx.pool().run(files).await {
                    ctx.publish(store::Diff::Menu(Diff::Source(Provider::Files, rows)));
                }
            }
            // The pane's asks answer the row under the cursor, so they never
            // wait behind a rescan or an icon decode already in this queue.
            Ask::Folder(path) => {
                let (c, latest) = (ctx.clone(), folder_latest.clone());
                let n = latest.fetch_add(1, Ordering::Relaxed) + 1;
                ctx.spawn(async move {
                    let p = path.clone();
                    if let Some(listing) = c.pool().run(move || list_folder(&p).map(Arc::from)).await
                        && latest.load(Ordering::Relaxed) == n
                    {
                        c.publish(store::Diff::Menu(Diff::Folder(path, listing)));
                    }
                });
            }
            Ask::Stat(paths) => {
                let stat = move || paths.into_iter().map(|p| {
                    let kind = std::fs::metadata(&p).ok().map(|m| m.is_dir());
                    (p, kind)
                }).collect();
                if let Some(files) = ctx.pool().run(stat).await {
                    ctx.publish(store::Diff::Menu(Diff::Files(files)));
                }
            }
            Ask::Peek(path) => {
                let (c, latest) = (ctx.clone(), peek_latest.clone());
                let n = latest.fetch_add(1, Ordering::Relaxed) + 1;
                ctx.spawn(async move {
                    let p = path.clone();
                    if let Some(peeked) = c.pool().run(move || peek(&p)).await
                        && latest.load(Ordering::Relaxed) == n
                    {
                        c.publish(store::Diff::Menu(Diff::Peek(path, peeked)));
                    }
                });
            }
        }
    }
}

/// default-menu.jsonc merged with the self-targeted fragments and the
/// custom power buttons, then the user's overlay over it.
fn base(inputs: &BaseInputs) -> Tree {
    let path = user_menu_path();
    let read = match std::fs::read_to_string(&path) {
        Ok(text) => parse_jsonc(&text).and_then(|v| entries_from_value(&v)).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Entries::default()),
        Err(e) => Err(e.to_string()),
    };
    let user = read.unwrap_or_else(|e| {
        eprintln!("Menu: ignoring {}: {e}", path.display());
        Entries::default()
    });
    base_over(inputs, &user)
}

fn base_over(inputs: &BaseInputs, user: &Entries) -> Tree {
    let defaults = fs_menu::base::defaults().unwrap_or_else(|e| {
        eprintln!("Menu: the shipped default-menu.jsonc is invalid: {e}");
        Entries::default()
    });
    fs_menu::base::base_tree(defaults, inputs, user)
}

pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn user_menu_path() -> PathBuf {
    crate::paths::config().join("formalshell").join("menu.jsonc")
}

/// The Files route's rows: the user directories that exist, then the
/// recent list's newest files that still do.
fn files() -> Vec<Node> {
    use fs_menu::providers::{Found, RECENT_FILES, recent_paths, user_dirs};
    let home = home();
    let found = |path: String| std::fs::metadata(&path).ok().map(|m| Found { path, dir: m.is_dir() });
    let dirs = std::fs::read_to_string(crate::paths::config().join("user-dirs.dirs")).unwrap_or_default();
    let places: Vec<Found> = user_dirs(&dirs, &home).into_iter().filter_map(found).filter(|f| f.dir).collect();
    let xbel = std::fs::read_to_string(crate::paths::data().join("recently-used.xbel")).unwrap_or_default();
    let recent: Vec<Found> = recent_paths(&xbel).into_iter().filter_map(found).take(RECENT_FILES).collect();
    providers::files_rows(&places, &recent, &home)
}

fn list_folder(path: &str) -> Result<Vec<Listed>, String> {
    read_folder(path).map(fs_menu::providers::sort_listing)
}

/// Every entry of `path`, unsorted and uncapped.
fn read_folder(path: &str) -> Result<Vec<Listed>, String> {
    let read = std::fs::read_dir(path).map_err(|e| format!("Cannot read this folder: {}", io_reason(&e)))?;
    Ok(read
        .flatten()
        .map(|e| Listed {
            name: e.file_name().to_string_lossy().into_owned(),
            // A link to a directory steps in like one.
            dir: std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir()),
        })
        .collect())
}

fn io_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "it no longer exists".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => e.to_string(),
    }
}

/// What the preview pane shows of `path`: a folder's names, a text file's
/// first lines out of its first [`PEEK_BYTES`](fs_menu::providers::PEEK_BYTES),
/// or that it is a picture or neither.
fn peek(path: &str) -> Peeked {
    use std::io::Read;

    use fs_menu::providers::{PEEK_BYTES, Peek, folder_peek, is_image, text_lines};
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return Peeked { size: None, modified: None, peek: Peek::Unreadable(format!("Cannot read it: {}", io_reason(&e))) },
    };
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
    if meta.is_dir() {
        let peek = match read_folder(path) {
            Ok(entries) => folder_peek(entries),
            Err(e) => Peek::Unreadable(e),
        };
        return Peeked { size: None, modified, peek };
    }
    let peek = if is_image(path) {
        Peek::Image
    } else {
        let mut head = Vec::new();
        match std::fs::File::open(path).and_then(|f| f.take(PEEK_BYTES as u64).read_to_end(&mut head)) {
            Ok(_) => text_lines(&head).map_or(Peek::Binary, Peek::Text),
            Err(e) => Peek::Unreadable(format!("Cannot read it: {}", io_reason(&e))),
        }
    };
    Peeked { size: Some(meta.len()), modified, peek }
}

/// The desktop entries and the icons already decoded, kept across asks.
#[derive(Default)]
struct Scan {
    stamp: Vec<Option<SystemTime>>,
    scanned: bool,
    entries: Vec<appicon::Launchable>,
    icon_of: HashMap<String, String>,
    decoded: HashSet<String>,
    /// The scale the icons were decoded for: the launcher's output's, the
    /// bar's.
    scale: f64,
}

impl Scan {
    fn apps(&mut self, launches: &[Record], rescan: bool, scale: f64) -> (Vec<Node>, Vec<Icon>, Exec) {
        let dirs = appicon::applications_dirs();
        let stamp: Vec<Option<SystemTime>> = dirs.iter().map(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok()).collect();
        if rescan || !self.scanned || stamp != self.stamp {
            self.entries = appicon::scan_apps(&dirs);
            self.stamp = stamp;
            self.scanned = true;
            // An icon a package links in after its entry: the name that
            // found no file is looked up again.
            self.icon_of.retain(|_, path| !path.is_empty());
        }
        let entries: Vec<DesktopEntry> = self
            .entries
            .iter()
            .map(|l| DesktopEntry {
                id: l.entry.id.clone(),
                name: l.entry.name.clone(),
                icon: l.entry.icon.clone(),
                generic_name: l.entry.generic_name.clone(),
                startup_class: l.entry.startup_class.clone(),
                path: l.path.to_string_lossy().into_owned(),
                actions: l.actions.iter().map(|a| (a.key.clone(), a.name.clone())).collect(),
            })
            .collect();
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
        let paths: Vec<String> = rows.iter().map(|r| r.icon_source.clone()).collect();
        let fresh = self.icons(&paths, scale);
        let mut exec = Exec::default();
        for l in &self.entries {
            exec.apps.insert(l.entry.id.clone(), l.entry.command.clone());
            for a in &l.actions {
                exec.actions.insert((l.entry.id.clone(), a.key.clone()), a.command.clone());
            }
        }
        (rows, fresh, exec)
    }

    /// Each path not decoded yet, at the cell's and the row's size, at the
    /// scale the apps were decoded at, else `wanted`.
    fn icons(&mut self, paths: &[String], wanted: f64) -> Vec<Icon> {
        let scale = if self.scale > 0.0 { self.scale } else { wanted };
        let mut fresh = Vec::new();
        for path in paths {
            if path.is_empty() || !self.decoded.insert(path.clone()) {
                continue;
            }
            let file = std::path::Path::new(path);
            fresh.push((path.clone(), icons::load_at(file, ICON_EXTENT, scale), icons::load_at(file, ROW_ICON_EXTENT, scale)));
        }
        fresh
    }
}

/// Each desktop entry's `Exec` argv by entry id, and each of its
/// `[Desktop Action]`s' by entry id and action key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exec {
    pub apps: HashMap<String, Vec<String>>,
    pub actions: HashMap<(String, String), Vec<String>>,
}

fn uwsm() -> bool {
    std::env::var_os("UWSM_FINALIZE_VARNAMES").is_some_and(|v| !v.is_empty())
}

/// Proc.appLaunch: under uwsm a plain entry goes by its id, so uwsm reads
/// its Exec line; everything else runs its own argv.
pub fn launch_argv(state: &State, id: &str) -> Option<Vec<String>> {
    let command = state.exec.apps.get(id).filter(|c| !c.is_empty())?;
    let plain = id.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if uwsm() && plain {
        return Some(proc::argv(&["uwsm", "app", "--", &format!("{id}.desktop")]));
    }
    Some(under_uwsm(command))
}

/// Desktop action `key` of entry `id`, run as its own argv.
pub fn action_argv(state: &State, id: &str, key: &str) -> Option<Vec<String>> {
    let command = state.exec.actions.get(&(id.to_owned(), key.to_owned())).filter(|c| !c.is_empty())?;
    Some(under_uwsm(command))
}

fn under_uwsm(command: &[String]) -> Vec<String> {
    if !uwsm() {
        return command.to_vec();
    }
    let mut argv = proc::argv(&["uwsm", "app", "--"]);
    argv.extend(command.iter().cloned());
    argv
}

/// Desktop entries installed or removed while the shell runs reach the
/// apps source without a summon: every applications directory and its
/// subdirectories watched, one that does not exist yet (`/usr/local/share`
/// before fspec's first app) from its nearest ancestor.
#[cfg(target_os = "linux")]
async fn watch_entries(tx: async_channel::Sender<Ask>) {
    let Ok(mut watch) = super::watch::Watch::dirs(appicon::applications_dirs()) else { return };
    loop {
        watch.changed().await;
        if tx.try_send(Ask::Rescan).is_err() {
            return;
        }
    }
}

#[cfg(not(target_os = "linux"))]
async fn watch_entries(_tx: async_channel::Sender<Ask>) {}


pub fn radio_station(s: &fs_media::radio::stations::Station) -> fs_menu::providers::RadioStation {
    fs_menu::providers::RadioStation { uuid: s.uuid.clone(), name: s.name.clone(), country: s.country.clone(), codec: s.codec.clone() }
}

/// RadioSearchProvider.rowsFor: the answer for `q` once it is in, a
/// searching row until then. Asking is idempotent; the service debounces.
pub fn radio_search_rows(store: &store::Store, q: &str) -> Vec<Node> {
    use fs_menu::providers as p;
    let q = q.trim();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    super::radio::send(super::radio::Cmd::Search(q.to_owned()));
    let radio = &store.media.radio;
    match &radio.search {
        Some((answered, found)) if answered == q => match found {
            None => vec![p::radio_failed_row()],
            Some(v) if v.is_empty() => vec![p::radio_no_results_row()],
            Some(v) => {
                let favs: std::collections::HashSet<String> = radio.favorites.iter().map(|s| s.uuid.clone()).collect();
                let list: Vec<_> = v.iter().take(50).map(radio_station).collect();
                p::radio_result_rows(&list, &favs)
            }
        },
        _ => vec![p::radio_searching_row()],
    }
}

pub fn wifi_network(r: &super::devices::network::Row) -> fs_menu::providers::WifiNetwork {
    fs_menu::providers::WifiNetwork {
        name: r.ssid.clone(),
        known: r.known,
        connected: r.connected,
        secured: r.secured,
        enterprise: r.enterprise,
        in_range: r.in_range,
        signal: r.signal.as_fraction(),
        security: r.security.clone(),
        addresses: r.addresses.clone(),
    }
}

/// A network stepped into: its details off the live scan, a note once it
/// is no longer listed.
pub fn network_rows(store: &store::Store, ssid: &str, q: &str) -> Vec<Node> {
    let Some(row) = store.network.row(ssid) else { return vec![Node::note("wifi.details.gone", "No longer listed")] };
    let q = q.trim().to_lowercase();
    fs_menu::providers::wifi_details(&wifi_network(row)).into_iter().filter(|n| q.is_empty() || n.label.to_lowercase().contains(&q)).collect()
}

/// The four device routes' rows, off the store.
pub fn device_sources(store: &store::Store) -> Vec<(Provider, Vec<Node>)> {
    use fs_menu::providers as p;
    let n = &store.network;
    let wifi = p::WifiState {
        has_device: n.wifi_device.is_some(),
        enabled: n.wifi_enabled,
        networks: n.rows.iter().map(wifi_network).collect(),
        action_ssid: n.action.as_ref().map(|(_, s)| s.clone()).unwrap_or_default(),
        action_kind: n.action.as_ref().map(|(k, _)| k.as_str().to_owned()).unwrap_or_default(),
        failure_ssid: n.failure.as_ref().map(|f| f.ssid.clone()).unwrap_or_default(),
        failure_text: n.failure.as_ref().map(|f| f.text.clone()).unwrap_or_default(),
    };
    let b = &store.bluetooth;
    let bluetooth = p::BluetoothState { available: b.powered.is_some(), enabled: b.powered == Some(true), devices: b.devices.clone() };
    let audio: Vec<p::AudioDevice> =
        store.audio.devices.iter().map(|(name, label, sink)| p::AudioDevice { name: name.clone(), label: label.clone(), is_sink: *sink }).collect();
    let r = &store.media.radio;
    let radio = p::RadioState { running: r.running(), favorites: r.favorites.iter().map(radio_station).collect() };
    vec![
        (Provider::Wifi, p::wifi_rows(&wifi)),
        (Provider::Bluetooth, p::bluetooth_rows(&bluetooth)),
        (Provider::Audio, p::audio_rows(&audio)),
        (Provider::Radio, p::radio_rows(&radio)),
    ]
}

/// Every launcher plugin with a mode, as `(route id, name)`.
pub fn plugin_modes(store: &store::Store) -> Vec<(String, String)> {
    fs_chrome::plugins::launcher_plugins(&store.plugins.resolved.plugins)
        .filter_map(|p| Some((fs_chrome::plugins::surface_key(p), p.mode.clone()?)))
        .collect()
}

/// Each plugin mode's rows off what its plugin last printed, or the one dim
/// row that says why there are none.
pub fn plugin_sources(store: &store::Store) -> Vec<(Provider, Vec<Node>)> {
    use super::plugins::Run as Printed;
    let state = &store.plugins;
    fs_chrome::plugins::launcher_plugins(&state.resolved.plugins)
        .filter(|p| p.mode.is_some())
        .map(|p| {
            let route = fs_chrome::plugins::surface_key(p);
            // Only the mode's own view shows why it has no rows.
            let note = |text: String| vec![Node { local_only: true, ..Node::note(format!("{route}.note"), text) }];
            let rows = match state.runs.get(&p.id) {
                None => note(format!("Loading {}", p.name)),
                Some(Printed::Failed(why)) => note(format!("{} failed: {why}", p.name)),
                Some(Printed::Live(d)) => {
                    let rows = plugin_rows(&p.id, &route, &d.rows);
                    if rows.is_empty() { note("No results".into()) } else { rows }
                }
            };
            (Provider::Plugin(route), rows)
        })
        .collect()
}

/// The verb that writes `line` to plugin `id`'s stdin.
pub fn plugin_send(id: &str, line: &str) -> Intent {
    Intent::PluginSend(id.to_owned(), line.to_owned())
}

/// A plugin mode's rows: each activates its row, then its own actions; a
/// `label` row heads the rows under it.
pub fn plugin_rows(id: &str, route: &str, rows: &[super::plugins::Row]) -> Vec<Node> {
    use super::plugins::RowKind;
    let mut section = None;
    let mut out = Vec::new();
    for r in rows {
        if r.kind == RowKind::Label {
            section = Some(r.text.clone()).filter(|t| !t.is_empty());
            continue;
        }
        let toggle = r.kind == RowKind::Toggle;
        let activate = super::plugins::activate_event(&r.id, toggle.then_some(!r.checked));
        let mut n = Node::new(format!("{route}.{}", r.id), r.text.clone(), Kind::Action).with_action(plugin_send(id, &activate));
        n.icon = r.icon.clone();
        n.desc = if toggle { Some(if r.checked { "On" } else { "Off" }.to_owned()) } else { Some(r.detail.clone()).filter(|d| !d.is_empty()) };
        n.verb = toggle.then(|| "Toggle".to_owned());
        n.keep_open = toggle.then_some(true);
        n.section = section.clone();
        n.actions = r
            .actions
            .iter()
            .map(|a| {
                let label = if a.label.is_empty() { &a.id } else { &a.label };
                let run = plugin_send(id, &super::plugins::action_event(&r.id, &a.id));
                ActionEntry { icon: Some(a.icon.clone()).filter(|i| !i.is_empty()), ..ActionEntry::new(&a.id, label, run) }
            })
            .collect();
        out.push(n);
    }
    out
}

/// `menu.abbreviations` in settings.json, query to subject id.
pub fn configured_abbreviations(store: &store::Store) -> HashMap<String, String> {
    match store.config.get("menu.abbreviations") {
        Some(serde_json::Value::Object(map)) => {
            map.iter().filter_map(|(q, id)| Some((fs_menu::abbrev::key(q)?, id.as_str()?.to_owned()))).collect()
        }
        _ => Default::default(),
    }
}

/// Whether a subject id names something now: in the tree, or a file still
/// on disk, or an emoji in the dataset.
pub fn resolves(store: &store::Store, id: &str) -> bool {
    fs_menu::model::subject_node(store.menu.nodes(), id).is_some() || outside_subject(store, id).is_some()
}

/// A subject id naming no tree node, as a row: a file or folder still on
/// disk, an emoji in the dataset. Windows are left out: their ids do not
/// outlive the window.
pub fn outside_subject(store: &store::Store, id: &str) -> Option<Node> {
    use fs_menu::actions::SubjectId;
    match SubjectId::parse(id) {
        SubjectId::File(path) => {
            // Read off the service's stat of the picks, never the disk here.
            let dir = (*store.menu.files.get(&path)?)?;
            Some(providers::recent_subject(&path, dir, &std::env::var("HOME").unwrap_or_default()))
        }
        SubjectId::Emoji(ch) => store.menu.emoji.as_ref()?.row_for(&ch, crate::settings::CLIPBOARD_PASTE.get(&store.config)),
        SubjectId::App(_) | SubjectId::Node(_) => None,
    }
}

/// What typing `q` resolves to: the configured abbreviations over the
/// learned ones, a subject that no longer resolves dropped.
pub fn abbreviation_lookup(store: &store::Store, q: &str, now: f64) -> fs_menu::abbrev::Lookup {
    fs_menu::abbrev::lookup(&configured_abbreviations(store), &abbreviation_book(store), q, now, |id| resolves(store, id))
}

/// `subject` pinned under `q`, or forgotten from it.
pub fn set_abbreviation(store: &store::Store, q: &str, subject: &str, pin: bool) {
    let book = abbreviation_book(store);
    let book = if pin { fs_menu::abbrev::pin(&book, q, subject) } else { fs_menu::abbrev::forget(&book, q, subject) };
    super::state::set(vec![super::state::Field::Abbreviations(Some(book))]);
}

/// `subject` pinned under `new` and forgotten from `old`.
pub fn move_abbreviation(store: &store::Store, old: &str, new: &str, subject: &str) {
    let book = fs_menu::abbrev::forget(&abbreviation_book(store), old, subject);
    let book = fs_menu::abbrev::pin(&book, new, subject);
    super::state::set(vec![super::state::Field::Abbreviations(Some(book))]);
}

/// `id` pinned to the root's Recent section, or unpinned.
pub fn set_recent_pinned(store: &store::Store, id: &str, pin: bool) {
    let mut pinned: Vec<String> = store.state.data.recent_pinned.iter().filter(|p| *p != id).cloned().collect();
    if pin {
        pinned.push(id.to_owned());
    }
    super::state::set(vec![super::state::Field::RecentPinned(pinned)]);
}

/// One pick of `node` into the picks ledger, and the query it was
/// picked under taught to the learned abbreviations, unless the query
/// already names it exactly or the subject never ranks in the tree (an
/// emoji, a folder's entry). An emoji row also moves its own grid ledger,
/// which orders the emoji grid and nothing else.
pub fn record_pick(store: &store::Store, node: &Node, query: &str, now: f64) {
    use fs_menu::actions::SubjectId;
    use super::state::{self, Field};
    let subject = fs_menu::actions::subject_id(node);
    if let SubjectId::Emoji(_) = subject {
        let next = fs_menu::frecency::record(&store.state.data.emoji_uses, &node.id, now, None);
        state::set(vec![Field::EmojiUses(next)]);
    }
    let id = subject.to_string();
    let file = match &subject {
        SubjectId::File(path) => Some(path.as_str()),
        _ => None,
    };
    if file.is_none() && !resolves(store, &id) {
        return;
    }
    if let Some(path) = file {
        ask(Ask::Stat(vec![path.to_owned()]));
    }
    // Only an uninstalled app, once the apps scan has landed, and a
    // deleted file are pruned: a provider's rows (a network out of
    // range, a stopped player) come back, and before the scan no app
    // resolves at all.
    let apps_known = !store.menu.exec.apps.is_empty();
    let gone = |id: &str| match SubjectId::parse(id) {
        SubjectId::File(path) => store.menu.files.get(&path) == Some(&None),
        SubjectId::App(_) => apps_known && !resolves(store, id),
        SubjectId::Emoji(_) | SubjectId::Node(_) => false,
    };
    let held: Vec<Record> = store.state.data.picks.iter().filter(|r| !gone(&r.id)).cloned().collect();
    let mut fields = vec![Field::Picks(fs_menu::frecency::record(&held, &id, now, None))];
    if !fs_menu::search::is_exact(node, query) && fs_menu::abbrev::key(query).is_some() && store.menu.nodes().contains_key(&node.id) {
        let book = fs_menu::abbrev::learn(&abbreviation_book(store), query, &id, now);
        fields.push(Field::Abbreviations(Some(book)));
    }
    state::set(fields);
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use fs_devices::bluetooth::Device as BluetoothDevice;
    use fs_menu::actions::{IpcCall, Subject, actions_for};
    use fs_menu::base::ShareInputs;
    use fs_menu::clipboard::entry::{Entry as ClipEntry, EntryKind};
    use fs_menu::providers::*;
    use serde_json::json;

    use super::*;

    fn inputs() -> BaseInputs {
        BaseInputs {
            buttons: json!([{"label": "Hibernate", "command": "systemctl hibernate"}, {"label": "Sleep", "command": "@ipc:power.suspend"}]),
            lights: Some(vec![LightEffect { id: "static".into(), label: "Static".into() }]),
            ipad: true,
            share: Some(ShareInputs {
                installed: true,
                peers: vec!["phone".into()],
                items: vec![ClipEntry { id: "c1".into(), kind: EntryKind::Text, text: Some("hi".into()), ..ClipEntry::default() }],
                receive: ReceiveStatus { enabled: true, receiving: false, alias: "me".into(), dir: "/d".into() },
            }),
            plugin_modes: Vec::new(),
        }
    }

    /// Every row a source can publish, with each verb its rows mint.
    fn sources() -> Vec<Node> {
        let net = |name: &str, known: bool, connected: bool| WifiNetwork { name: name.into(), secured: true, in_range: true, known, connected, signal: 0.5, ..WifiNetwork::default() };
        let device = |address: &str, connected: bool| BluetoothDevice { address: address.parse().unwrap(), name: address.into(), paired: true, connected, ..BluetoothDevice::default() };
        let station = |uuid: &str| RadioStation { uuid: uuid.into(), name: uuid.into(), ..RadioStation::default() };
        let clip = vec![
            ClipEntry { id: "t".into(), kind: EntryKind::Text, text: Some("a:b".into()), ..ClipEntry::default() },
            ClipEntry { id: "i".into(), kind: EntryKind::Image, path: Some("/tmp/a.png".into()), ..ClipEntry::default() },
        ];
        let package = |id: &str, installed: bool| Package { id: id.into(), name: id.into(), source: "zypper".into(), installed, installable: true, run: true, ..Package::default() };
        let mut rows = wifi_rows(&WifiState { has_device: true, enabled: true, networks: vec![net("Home:5G", true, true), net("Cafe", false, false)], ..WifiState::default() });
        rows.extend(wifi_rows(&WifiState { has_device: true, ..WifiState::default() }));
        rows.extend(bluetooth_rows(&BluetoothState { available: true, enabled: true, devices: vec![device("AA:BB:CC:DD:EE:01", true), device("AA:BB:CC:DD:EE:02", false)] }));
        rows.extend(bluetooth_rows(&BluetoothState { available: true, ..BluetoothState::default() }));
        rows.extend(audio_rows(&[AudioDevice { name: "sink".into(), label: "S".into(), is_sink: true }, AudioDevice { name: "src".into(), label: "M".into(), is_sink: false }]));
        rows.extend(radio_rows(&RadioState { running: true, favorites: vec![station("u1")] }));
        rows.extend(radio_result_rows(&[station("u1"), station("u2")], &HashSet::from(["u1".to_owned()])));
        rows.extend(clipboard_provider(&clip, ClipMode::Copy, false));
        rows.extend(clipboard_provider(&clip, ClipMode::Share, false));
        rows.extend(clipssh_rows(&[ClipsshAlias { name: "box".into(), target: "k@box".into() }]));
        rows.extend(package_rows(&[package("zypper:foot", true), package("flatpak:org.example.App", false)]));
        rows.extend(tray_provider(&[TrayItem { id: "org.kde.Item:1".into(), title: "Item".into(), tooltip_title: String::new() }]));
        rows.extend(apps_provider(&[DesktopEntry { id: "foot".into(), name: "Foot".into(), actions: vec![("new".into(), "New".into())], ..DesktopEntry::default() }], None, &[], None));
        rows
    }

    #[test]
    fn every_shipped_call_is_answered() {
        let mut tree = base_over(&inputs(), &Entries::new());
        let mut rows = sources();
        let targets: HashSet<String> = rows
            .iter()
            .flat_map(|n| [&n.action, &n.alternate].into_iter().chain(n.actions.iter().map(|a| &a.action)))
            .filter_map(|run| match run {
                Some(Run::Ipc(call)) => Some(call.target.clone()),
                Some(Run::Intent(intent)) => Some(intent.call().target),
                _ => None,
            })
            .collect();
        for t in ["wifi", "bluetooth", "audio", "radio", "clipboard", "clipssh", "package", "panel", "tray"] {
            assert!(targets.contains(t), "no {t} call among {targets:?}");
        }
        let refused: Vec<String> = check_calls(tree.nodes.values_mut()).into_iter().chain(check_calls(rows.iter_mut())).collect();
        assert_eq!(refused, Vec::<String>::new());
        let tray = rows.iter().find(|n| n.id.starts_with("tray.")).unwrap();
        assert_eq!(tray.action, Some(Run::Ipc(IpcCall::new("tray", "activate").arg("org.kde.Item:1"))), "a colon in the last argument stays in it");
    }

    #[test]
    fn every_builtin_call_is_answered() {
        let app = apps_provider(&[DesktopEntry { id: "foot".into(), name: "Foot".into(), actions: vec![("new".into(), "New".into())], ..DesktopEntry::default() }], None, &[], None).remove(0);
        let window = Node { path: "0xabc".into(), ..Node::new("w", "Window", Kind::Window) };
        let image = Node { path: "/w/a.png".into(), ..Node::new("img", "a", Kind::Image) };
        let submenu = Node::new("system", "System", Kind::Submenu);
        let (mut calls, mut intents) = (Vec::new(), 0);
        for node in [&app, &window, &image, &submenu] {
            for (pinned, abbrev) in [(false, None), (true, Some(("fx", fs_menu::abbrev::Source::Learned)))] {
                let s = Subject { discrete_gpu: true, pinned, abbrev, removable: Some("flatpak:x"), single: true, ..Subject::of(node) };
                for a in actions_for(&s) {
                    match a.run {
                        Run::Ipc(call) => calls.push(call),
                        Run::Intent(_) => intents += 1,
                        _ => {}
                    }
                }
            }
        }
        assert!(calls.len() + intents > 15);
        for call in calls {
            assert!(crate::ipc::check(&call).is_ok(), "{call}: {:?}", crate::ipc::check(&call));
        }
    }

    #[test]
    fn a_user_call_nothing_answers_is_refused_with_its_reason() {
        let user = entries_from_value(&json!({
            "mine": {"label": "Mine", "action": "@ipc:nope.verb"},
            "short": {"label": "Short", "action": "@ipc:wallpaper.setFor:dark"},
            "typed": {"label": "Typed", "action": "@ipc:debug.motionScale:fast"},
            "alt": {"label": "Alt", "action": "@ipc:caffeinate.toggle", "alternate": "@ipc:caffeinate.nap", "alternateLabel": "Nap"},
            "col": {"label": "Col", "action": "foot -e htop", "actions": [{"id": "x", "label": "X", "action": "@ipc:panel.shut:audio"}]},
            "fine": {"label": "Fine", "action": "@ipc:panel.open:audio:extra"},
        }))
        .unwrap();
        let mut tree = base_over(&BaseInputs::default(), &user);
        let refused = check_calls(tree.nodes.values_mut());
        let node = |id: &str| &tree.nodes[id];
        assert_eq!(refused, [
            "mine: refused @ipc:nope.verb: no IPC target \"nope\"",
            "short: refused @ipc:wallpaper.setFor:dark: function setFor(mode: string, path: string): string takes 2 arguments, given 1",
            "typed: refused @ipc:debug.motionScale:fast: debug.motionScale: percent \"fast\" is not int",
            "alt: refused @ipc:caffeinate.nap: target caffeinate has no function \"nap\"",
            "col x: refused @ipc:panel.shut:audio: target panel has no function \"shut\"",
        ]);
        assert_eq!((node("mine").action.clone(), node("mine").dim), (None, Some(true)));
        assert_eq!((node("alt").alternate.clone(), node("alt").alternate_label.clone(), node("alt").dim), (None, None, None));
        assert_eq!((node("col").actions[0].action.clone(), node("col").dim), (Some(Run::Note), Some(true)));
        assert_eq!(node("col").action, Some(Run::Shell("foot -e htop".into())));
        assert_eq!(node("fine").action, Some(Run::Ipc(IpcCall::new("panel", "open").arg("audio:extra"))));
    }

    #[test]
    fn a_refused_override_of_a_builtin_action_runs_nothing() {
        let user = entries_from_value(&json!({
            "mine": {"label": "Mine", "kind": "action", "action": "foot -e htop", "actions": [{"id": "run", "action": "@ipc:nope.verb"}]},
        }))
        .unwrap();
        let mut tree = base_over(&BaseInputs::default(), &user);
        assert_eq!(check_calls(tree.nodes.values_mut()).len(), 1);
        let s = Subject::of(&tree.nodes["mine"]);
        let run = actions_for(&s).into_iter().find(|a| a.id == "run").map(|a| a.run);
        assert_eq!(run, Some(Run::Note));
        assert_eq!((fs_menu::actions::first_action(&s), fs_menu::actions::second_action(&s)), (None, None));
    }
}
