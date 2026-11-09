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
