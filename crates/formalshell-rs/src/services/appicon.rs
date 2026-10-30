//! App icons for windows: which desktop entry a window
//! belongs to (`fs_system`'s class, process and title tiers), that entry's
//! icon through [`icons`], fitted to the size the cell draws. All
//! the filesystem and decode work runs on the blocking pool; the cell only
//! ever reads finished pixmaps off the store.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use fs_system::compositor::appicon::{self, DesktopEntry, ProcInfo};

use super::icons;
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

/// What the cell asks about one window.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub id: String,
    pub app_id: String,
    pub initial_class: String,
    pub initial_title: String,
    pub pid: i64,
}

/// One of a desktop entry's own `[Desktop Action]` groups.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// The group's key, `new-private-window` in `[Desktop Action new-private-window]`.
    pub key: String,
    pub name: String,
    pub command: Vec<String>,
}

/// The desktop entry a window resolved to, for what asks more of it than
/// an icon (the app menu's name and actions).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub command: Vec<String>,
    pub terminal: bool,
    pub actions: Vec<Action>,
}

/// One window's answer. `source` is empty when no entry or no theme icon
/// answered, which is the cell's cue for the generic window mark.
#[derive(Clone, Debug, Default)]
pub struct Icon {
    pub source: String,
    pub image: Option<Bitmap>,
    pub entry: Option<Entry>,
}

impl PartialEq for Icon {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.entry == other.entry
            && match (&self.image, &other.image) {
                (Some(a), Some(b)) => a.same_as(b),
                (None, None) => true,
                _ => false,
            }
    }
}

/// Icons by window and the pixel size they were made at, so a bar on an
/// output at another scale keeps its own.
#[derive(Default)]
pub struct State {
    by_window: HashMap<(String, u32), Icon>,
}

pub struct Diff(pub Vec<((String, u32), Icon)>);

impl State {
    pub fn apply(&mut self, Diff(icons): Diff) -> bool {
        let mut changed = false;
        for (key, icon) in icons {
            if self.by_window.get(&key) != Some(&icon) {
                self.by_window.insert(key, icon);
                changed = true;
            }
        }
        changed
    }

    /// The window's icon as [`probe`] made it for a `size` square at
    /// `scale`, else one made at another pixel size, drawn in the same box.
    pub fn get(&self, id: &str, size: u32, scale: f64) -> Option<Icon> {
        let px = crate::scene::pixels_at(f64::from(size), scale);
        let (density, icon) = match self.by_window.get(&(id.to_owned(), px)) {
            Some(hit) => (scale, hit),
            None => self.by_window.iter().find(|((i, _), _)| i == id).map(|((_, made), icon)| (f64::from(*made) / f64::from(size.max(1)), icon))?,
        };
        let mut icon = icon.clone();
        icon.image = icon.image.map(|b| b.at(density));
        Some(icon)
    }
}

static PROBE: OnceLock<async_channel::Sender<(u32, Vec<Query>)>> = OnceLock::new();

/// Resolves `queries` to icons drawn in a `size` square on a surface at
/// `scale`, which [`State::get`] reads back.
pub fn probe(size: u32, scale: f64, queries: Vec<Query>) {
    if let Some(tx) = PROBE.get() {
        let _ = tx.try_send((crate::scene::pixels_at(f64::from(size), scale), queries));
    }
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = PROBE.set(tx);
    let inner = Arc::new(Mutex::new(Inner::default()));
    while let Ok(first) = rx.recv().await {
        // Only the newest ask at each size matters when several queue up.
        let mut asks: Vec<(u32, Vec<Query>)> = vec![first];
        while let Ok((size, queries)) = rx.try_recv() {
            match asks.iter_mut().find(|(s, _)| *s == size) {
                Some(slot) => slot.1 = queries,
                None => asks.push((size, queries)),
            }
        }
        for (size, queries) in asks {
            let inner = inner.clone();
            let done = ctx.pool().run(move || inner.lock().unwrap().resolve(size, &queries)).await;
            if let Some(icons) = done {
                ctx.publish(store::Diff::AppIcon(Diff(icons)));
            }
        }
    }
}

#[derive(Default)]
struct Inner {
    entries: Vec<DesktopEntry>,
    extras: HashMap<String, Extras>,
    stamp: Vec<Option<SystemTime>>,
    scanned: bool,
    procs: HashMap<String, Vec<ProcInfo>>,
    asked: HashSet<i64>,
}

impl Inner {
    fn resolve(&mut self, px: u32, queries: &[Query]) -> Vec<((String, u32), Icon)> {
        self.rescan();
        self.read_procs(queries);
        queries.iter().map(|q| ((q.id.clone(), px), self.icon_for(px, q))).collect()
    }

    /// Entries are rescanned when any applications directory's mtime moved,
    /// which is what an install under a running shell does.
    fn rescan(&mut self) {
        let dirs = applications_dirs();
        let stamp: Vec<Option<SystemTime>> = dirs.iter().map(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok()).collect();
        if self.scanned && stamp == self.stamp {
            return;
        }
        (self.entries, self.extras) = scan_entries(&dirs);
        self.stamp = stamp;
        self.scanned = true;
    }

    /// `/proc` once per window pid that no entry claims by class.
    fn read_procs(&mut self, queries: &[Query]) {
        let mut wanted: Vec<String> = Vec::new();
        for q in queries {
            let win = window_of(q);
            if q.pid > 1 && !self.asked.contains(&q.pid) && appicon::by_class(Some(&win), &self.entries).is_none() {
                self.asked.insert(q.pid);
                wanted.push(q.pid.to_string());
            }
        }
        if wanted.is_empty() {
            return;
        }
        let argv = appicon::proc_command(&wanted);
        let Ok(out) = crate::services::proc::std_command(&argv[0]).args(&argv[1..]).output() else { return };
        self.procs.extend(appicon::parse_procs(&String::from_utf8_lossy(&out.stdout)));
    }

    fn icon_for(&mut self, size: u32, q: &Query) -> Icon {
        let win = window_of(q);
        let procs = self.procs.get(&q.pid.to_string()).map(Vec::as_slice);
        let Some(entry) = appicon::entry_for(Some(&win), &self.entries, procs) else { return Icon::default() };
        let extras = self.extras.get(&entry.id).cloned().unwrap_or_default();
        let info = Entry {
            id: entry.id.clone(),
            name: entry.name.clone(),
            command: entry.command.clone(),
            terminal: extras.terminal,
            actions: extras.actions,
        };
        let Some(path) = icons::lookup(&entry.icon, "", size) else { return Icon { entry: Some(info), ..Icon::default() } };
        let source = path.to_string_lossy().into_owned();
        let image = icons::load_at(&path, size, 1.0);
        Icon { source, image, entry: Some(info) }
    }
}

fn window_of(q: &Query) -> appicon::Window {
    appicon::Window { app_id: q.app_id.clone(), initial_class: q.initial_class.clone(), initial_title: q.initial_title.clone() }
}

pub fn applications_dirs() -> Vec<PathBuf> {
    icons::data_dirs().into_iter().map(|d| d.join("applications")).collect()
}

// ---- desktop entries --------------------------------------------------------

/// Every launchable entry, a higher-priority directory shadowing a lower one's
/// entry of the same id.
fn scan_entries(dirs: &[PathBuf]) -> (Vec<DesktopEntry>, HashMap<String, Extras>) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    let mut extras = HashMap::new();
    let desktops = current_desktops();
    for dir in dirs {
        let mut files = Vec::new();
        collect(dir, dir, &mut files);
        files.sort();
        for (id, path) in files {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some((entry, more)) = parse_entry(&id, &path, &desktops) {
                extras.insert(entry.id.clone(), more);
                out.push(entry);
            }
        }
    }
    (out, extras)
}

pub fn scan_launchable(dirs: &[PathBuf]) -> Vec<DesktopEntry> {
    scan_entries(dirs).0
}

/// One launchable entry as the launcher's apps source reads it.
pub struct Launchable {
    pub entry: DesktopEntry,
    pub path: PathBuf,
    pub actions: Vec<Action>,
}

pub fn scan_apps(dirs: &[PathBuf]) -> Vec<Launchable> {
    let (entries, mut extras) = scan_entries(dirs);
    entries
        .into_iter()
        .map(|entry| {
            let more = extras.remove(&entry.id).unwrap_or_default();
            Launchable { entry, path: more.path, actions: more.actions }
        })
        .collect()
}

/// What an entry carries past the fields `DesktopEntry` holds.
#[derive(Clone, Debug, Default)]
struct Extras {
    terminal: bool,
    actions: Vec<Action>,
    path: PathBuf,
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for item in read.flatten() {
        let path = item.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            let rel = path.strip_prefix(root).unwrap_or(&path).with_extension("");
            out.push((rel.to_string_lossy().replace('/', "-"), path));
        }
    }
}

fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').filter(|d| !d.is_empty()).map(str::to_owned).collect()
}

/// OnlyShowIn and NotShowIn against the session's desktops, the way GLib reads
/// them: an entry with OnlyShowIn and no desktop of ours in it stays hidden,
/// even when XDG_CURRENT_DESKTOP is unset.
fn shown_in(fields: &HashMap<&str, &str>, desktops: &[String]) -> bool {
    let names = |k: &str| fields.get(k).map(|v| v.split(';').any(|d| desktops.iter().any(|c| c == d)));
    names("OnlyShowIn").unwrap_or(true) && !names("NotShowIn").unwrap_or(false)
}

fn parse_entry(id: &str, path: &Path, desktops: &[String]) -> Option<(DesktopEntry, Extras)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut section = String::new();
    let mut fields: HashMap<&str, &str> = HashMap::new();
    let mut groups: Vec<(String, HashMap<&str, &str>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.to_owned();
            if let Some(name) = line.strip_prefix("[Desktop Action ").and_then(|n| n.strip_suffix(']')) {
                groups.push((name.to_owned(), HashMap::new()));
            }
        } else if !line.starts_with('#')
            && let Some((k, v)) = line.split_once('=')
        {
            if section == "[Desktop Entry]" {
                fields.entry(k.trim()).or_insert_with(|| v.trim());
            } else if section.starts_with("[Desktop Action ")
                && let Some((_, g)) = groups.last_mut()
            {
                g.entry(k.trim()).or_insert_with(|| v.trim());
            }
        }
    }
    let flag = |k: &str| fields.get(k) == Some(&"true");
    if fields.get("Type").is_some_and(|t| *t != "Application") || flag("NoDisplay") || flag("Hidden") || !shown_in(&fields, desktops) {
        return None;
    }
    let text = |k: &str| fields.get(k).copied().unwrap_or_default().to_owned();
    let declared: Vec<&str> = fields.get("Actions").copied().unwrap_or_default().split(';').filter(|a| !a.is_empty()).collect();
    let actions = declared
        .iter()
        .filter_map(|name| {
            let g = &groups.iter().find(|(n, _)| n == name)?.1;
            Some(Action {
                key: (*name).to_owned(),
                name: g.get("Name")?.to_string(),
                command: split_exec(g.get("Exec").copied().unwrap_or_default()),
            })
        })
        .collect();
    let entry = DesktopEntry {
        id: id.to_owned(),
        name: text("Name"),
        startup_class: text("StartupWMClass"),
        icon: text("Icon"),
        command: split_exec(fields.get("Exec").copied().unwrap_or_default()),
        generic_name: text("GenericName"),
    };
    Some((entry, Extras { terminal: flag("Terminal"), actions, path: path.to_owned() }))
}

/// An `Exec` line as an argv: quotes and backslashes honoured, field codes
/// (`%f`, `%U`, ...) dropped, `%%` a percent.
fn split_exec(exec: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut live = false;
    let mut quote: Option<char> = None;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                live = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    live = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if live || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                    live = false;
                }
            }
            (None, '%') => match chars.next() {
                Some('%') => {
                    cur.push('%');
                    live = true;
                }
                _ => {}
            },
            (None, c) => {
                cur.push(c);
                live = true;
            }
        }
    }
    if live || !cur.is_empty() {
        args.push(cur);
    }
    args.retain(|a| !a.is_empty());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(px: u16) -> Icon {
        Icon { image: Some(Bitmap::from_rgba(px, px, vec![0; usize::from(px) * usize::from(px) * 4])), ..Icon::default() }
    }

    #[test]
    fn each_scale_reads_the_icon_made_for_it() {
        let mut state = State::default();
        state.apply(Diff(vec![(("w".into(), 20), icon(20)), (("w".into(), 16), icon(16))]));
        let at = |scale| state.get("w", 16, scale).and_then(|i| i.image).map(|b| (b.pixmap.width(), b.density));
        assert_eq!(at(1.25), Some((20, 1.25)));
        assert_eq!(at(1.0), Some((16, 1.0)));
    }

    #[test]
    fn a_scale_not_made_yet_borrows_another_at_its_own_size() {
        let mut state = State::default();
        state.apply(Diff(vec![(("w".into(), 20), icon(20))]));
        let b = state.get("w", 16, 1.0).and_then(|i| i.image).unwrap();
        assert_eq!(b.logical(), (16.0, 16.0));
        assert!(state.get("other", 16, 1.0).is_none());
    }

    #[test]
    fn show_in_follows_the_current_desktop() {
        let shown = |only: Option<&str>, not: Option<&str>, current: &[&str]| {
            let fields: HashMap<&str, &str> = only.map(|v| ("OnlyShowIn", v)).into_iter().chain(not.map(|v| ("NotShowIn", v))).collect();
            shown_in(&fields, &current.iter().map(|d| d.to_string()).collect::<Vec<_>>())
        };
        assert!(shown(None, None, &["Hyprland"]));
        assert!(!shown(Some("KDE;"), None, &["Hyprland"]));
        assert!(shown(Some("GNOME;KDE;"), None, &["ubuntu", "KDE"]));
        assert!(!shown(Some("KDE;"), None, &[]));
        assert!(!shown(None, Some("Hyprland;"), &["Hyprland"]));
        assert!(shown(None, Some("KDE;"), &["Hyprland"]));
    }

    #[test]
    fn exec_lines() {
        assert_eq!(split_exec("foot --app-id=x %U"), ["foot", "--app-id=x"]);
        assert_eq!(split_exec(r#"env "A B"=1 /usr/bin/app %%x"#), ["env", "A B=1", "/usr/bin/app", "%x"]);
        assert!(split_exec("").is_empty());
    }
}
