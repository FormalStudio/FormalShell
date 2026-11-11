//! Hands the Super chords to a focused Mac window. Hyprland takes a bound
//! chord before any window sees it and a Mac app reads Super as Command,
//! so while a Mac window has the compositor's focus the session sits in the
//! `mac` submap: the default map less the Super and Super+Shift chords the
//! user has not reserved (`macApps.reservedBinds`). The submap is built by
//! the include (`docs/examples/hyprland/formalshell.lua`) out of the binds
//! it recorded; nothing is sent in a session that never focuses a Mac
//! window.

use std::sync::Mutex;

use super::compositor::{self, Command};
use crate::store::Store;

#[derive(Default)]
struct Sent {
    /// The reserved list and the config generation the submap was built
    /// for; a reload drops every bind, the submap's with them.
    built: Option<(Vec<String>, u64)>,
    inside: bool,
}

static SENT: Mutex<Sent> = Mutex::new(Sent { built: None, inside: false });

/// The compositor or the settings changed.
pub fn sync(store: &Store) {
    if !compositor::ready() {
        return;
    }
    let h = &store.compositor;
    // The compositor's own focus, not the held one: with a shell surface
    // holding the keyboard the default map is the one in force.
    let mac = h.window(&h.snapshot.focused_window_id).is_some_and(|w| w.is_mac());
    let mut sent = SENT.lock().unwrap();
    let want = (crate::settings::mac_reserved_binds(&store.config), h.config_reloads);
    let stale = sent.built.as_ref() != Some(&want);
    if mac && stale {
        compositor::send(Command::MacKeys(want.0.clone()));
        sent.built = Some(want);
        // A reload left the session in the default map.
        sent.inside = false;
    }
    if mac != sent.inside && (mac || !stale) {
        compositor::send(Command::MacFocus(mac));
        sent.inside = mac;
    } else if !mac && stale {
        sent.inside = false;
    }
}

/// Whether the session was last put in the `mac` submap, for `appmenu
/// status`.
pub fn inside() -> bool {
    SENT.lock().unwrap().inside
}
