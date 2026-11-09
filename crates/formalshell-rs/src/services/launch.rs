//! Launch feedback (Plan.md phase 6): a desktop entry the shell spawns is a
//! running Activity item from the spawn until a window of it maps, read off
//! the compositor's window list as it changes. One that maps none in
//! [`fs_activity::LAUNCH_MS`] reads as failed (the model's own fold), and a
//! window mapping later still takes that back, however much later: a launch
//! is watched until its window maps or its entry is launched again, so the
//! list holds one launch per entry at most.

use std::sync::OnceLock;

use fs_menu::appmatch;
use fs_menu::node::DesktopEntry;
use fs_system::monitor::gpu::{self, OffloadTools};
use fs_types::WindowId;

use super::activity::{self, Input};
use super::compositor;
use crate::runtime::{Ctx, Publisher};
use crate::store::{self, Store};

pub struct Pending {
    entry: DesktopEntry,
    /// The windows the entry already had, which do not end the launch.
    before: Vec<WindowId>,
}

/// The launches being watched, one per entry.
#[derive(Default)]
pub struct State {
    pending: Vec<Pending>,
}

pub enum Diff {
    Spawned(Pending),
    /// These entries' launches mapped a window.
    Landed(Vec<String>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Spawned(p) => {
                self.pending.retain(|q| q.entry.id != p.entry.id);
                self.pending.push(p);
                true
            }
            Diff::Landed(ids) => {
                self.pending.retain(|p| !ids.contains(&p.entry.id));
                !ids.is_empty()
            }
        }
    }
}

/// Spawns run on the UI thread from surfaces holding the store read-only,
/// so a launch reaches its slice through the channel every service uses.
static PUBLISHER: OnceLock<Publisher> = OnceLock::new();

pub fn start(ctx: &Ctx) {
    let _ = PUBLISHER.set(ctx.publisher().clone());
}

/// The desktop entry `id` names in the launcher's tree.
fn entry(store: &Store, id: &str) -> Option<DesktopEntry> {
    store.menu.tree.nodes.values().find_map(|n| n.entry.as_ref().filter(|e| e.id == id)).cloned()
}

/// Spawns `argv`, which starts desktop entry `id`, its launch shown until a
/// new window of it maps. An id the launcher's tree does not hold has no
/// window to wait on, so its spawn shows nothing.
pub fn spawn(store: &Store, id: &str, argv: &[String]) {
    compositor::spawn(argv);
    let Some(entry) = entry(store, id) else { return };
    let before = appmatch::match_windows(Some(&entry), &store.compositor.snapshot.windows);
    let title = if entry.name.is_empty() { entry.id.clone() } else { entry.name.clone() };
    let app = entry.id.clone();
    if let Some(p) = PUBLISHER.get() {
        p.publish(store::Diff::Launch(Diff::Spawned(Pending { entry, before })));
    }
    activity::send(Input::Launch { app, title });
}

/// The compositor's windows changed: a launch whose entry has a window it
/// did not have at the spawn is over.
pub fn windows_changed(store: &Store) -> Option<store::Diff> {
    let windows = &store.compositor.snapshot.windows;
    let landed: Vec<String> = store
        .launches
        .pending
        .iter()
        .filter(|p| appmatch::match_windows(Some(&p.entry), windows).iter().any(|id| !p.before.contains(id)))
        .map(|p| p.entry.id.clone())
        .collect();
    for id in &landed {
        activity::send(Input::Launched(id.clone()));
    }
    (!landed.is_empty()).then_some(store::Diff::Launch(Diff::Landed(landed)))
}

/// What [`on_gpu`] started.
pub struct OnGpu {
    pub card: String,
    pub name: String,
    pub argv: Vec<String>,
    /// The entry asked for a terminal and `console.command` names none.
    pub bare_terminal: bool,
}

struct DesktopFile {
    exec: String,
    path: String,
    terminal: bool,
}

/// `DesktopEntries.byId`: the first `<id>.desktop` on the data dirs. The
/// raw `Exec` is kept, since the offload wrapper runs it through `sh -c`.
fn desktop_file(id: &str) -> Option<DesktopFile> {
    let text = crate::paths::data_dirs().iter().find_map(|d| std::fs::read_to_string(d.join("applications").join(format!("{id}.desktop"))).ok())?;
    let mut in_entry = false;
    let (mut exec, mut path, mut terminal) = (None, None, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Exec" if exec.is_none() => exec = Some(v.trim().to_owned()),
                "Path" if path.is_none() => path = Some(v.trim().to_owned()),
                "Terminal" => terminal = v.trim() == "true",
                _ => {}
            }
        }
    }
    Some(DesktopFile { exec: exec.unwrap_or_default(), path: path.unwrap_or_default(), terminal })
}

fn shq(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Spawns entry `id` on GPU `card`, or the first discrete one when `card` is
/// empty, with the offload argv built by hand since a desktop entry's own
/// launch carries no environment.
pub fn on_gpu(store: &Store, id: &str, card: &str) -> Result<OnGpu, String> {
    let Some(file) = desktop_file(id) else { return Err(format!("no desktop entry '{id}'")) };
    let m = &store.monitor;
    let target = if card.is_empty() {
        m.cards.iter().find(|c| c.discrete).ok_or("no discrete GPU on this machine")?
    } else {
        m.cards.iter().find(|c| c.record.card.card == card).ok_or_else(|| format!("no GPU card '{card}'"))?
    };
    let mut exec = file.exec;
    if !file.path.is_empty() {
        exec = format!("cd {} && {exec}", shq(&file.path));
    }
    let tools = OffloadTools { nvidia_offload: m.tools.0, prime_run: m.tools.1 };
    let mut argv = gpu::offload_argv(&exec, Some(&target.record.card), tools);
    let mut bare = true;
    if file.terminal {
        let term = crate::settings::console_command_set(&store.config).unwrap_or_default();
        if !term.is_empty() {
            argv = term.into_iter().chain(argv).collect();
            bare = false;
        }
    }
    spawn(store, id, &argv);
    Ok(OnGpu { card: target.record.card.card.clone(), name: target.name.clone(), argv, bare_terminal: file.terminal && bare })
}
