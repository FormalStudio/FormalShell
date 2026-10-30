//! Launch feedback (Plan.md phase 6): a desktop entry the shell spawns is a
//! running Activity item from the spawn until a window of it maps, read off
//! the compositor's window list as it changes. One that maps none in
//! [`fs_activity::LAUNCH_MS`] reads as failed (the model's own fold), and a
//! window mapping later still takes that back, however much later: a launch
//! is watched until its window maps or its entry is launched again, so the
//! list holds one launch per entry at most.

use std::sync::Mutex;

use fs_menu::appmatch::{self, Window};
use fs_menu::node::DesktopEntry;

use super::activity::{self, Input};
use super::compositor;
use crate::store::Store;

struct Pending {
    entry: DesktopEntry,
    /// The windows the entry already had, which do not end the launch.
    before: Vec<String>,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

fn windows(store: &Store) -> Vec<Window> {
    store.compositor.snapshot.windows.iter().map(|w| Window { id: w.id.clone(), app_id: w.app_id.clone() }).collect()
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
    let before = appmatch::match_windows(Some(&entry), &windows(store));
    let title = if entry.name.is_empty() { entry.id.clone() } else { entry.name.clone() };
    if let Ok(mut pending) = PENDING.lock() {
        pending.retain(|p| p.entry.id != entry.id);
        pending.push(Pending { entry: entry.clone(), before });
    }
    activity::send(Input::Launch { app: entry.id, title });
}

/// The compositor's windows changed: a launch whose entry has a window it
/// did not have at the spawn is over.
pub fn windows_changed(store: &Store) {
    let Ok(mut pending) = PENDING.lock() else { return };
    if pending.is_empty() {
        return;
    }
    let windows = windows(store);
    pending.retain(|p| {
        let mapped = appmatch::match_windows(Some(&p.entry), &windows).iter().any(|id| !p.before.contains(id));
        if mapped {
            activity::send(Input::Launched(p.entry.id.clone()));
        }
        !mapped
    });
}
