//! Hands the Super chords to a focused Mac window. Hyprland takes a bound
//! chord before any window sees it and a Mac app reads Super as Command,
//! so while a Mac window has the compositor's focus the session sits in the
//! `mac` submap: the default map less the Super and Super+Shift chords the
//! user has not reserved (`macApps.reservedBinds`). The submap is built by
//! the include (`docs/examples/hyprland/formalshell.lua`) out of the binds
//! it recorded, the first time a Mac window takes focus. With no window in
//! focus and a Mac app still frontmost (its last window closed), the session
//! sits in `macapp` instead, where the app's own key equivalents run its
//! menu items through the shell, Super+Q its Quit.

use std::sync::Mutex;

use fs_types::WindowId;

use super::appmenu;
use super::compositor::{self, Command};
use crate::store::Store;

struct Sent {
    /// The reserved list and the config generation the submap was built
    /// for; a reload drops every bind, the submap's with them.
    built: Option<(Vec<String>, u64)>,
    /// The Mac window the session was last asked into the submap over, none
    /// when it was asked out, and the config generation it was asked under.
    /// Unset until the first sync: the session outlives the shell, and one
    /// that exited over a Mac window left it in the submap.
    asked: Option<(Option<WindowId>, u64)>,
    /// The key equivalents the `macapp` submap was last built with for a
    /// frontmost Mac app with no window in focus, none once out of it, and
    /// the config generation.
    app: Option<(Option<Vec<String>>, u64)>,
}

static SENT: Mutex<Sent> = Mutex::new(Sent { built: None, asked: None, app: None });

/// The compositor or the settings changed.
pub fn sync(store: &Store) {
    if !compositor::ready() {
        return;
    }
    let h = &store.compositor;
    // The compositor's own focus, not the held one: with a shell surface
    // holding the keyboard the default map is the one in force.
    let mac = h.window(&h.snapshot.focused_window_id).filter(|w| store.appmenu.is_mac(w)).map(|w| w.id.clone());
    let mut sent = SENT.lock().unwrap();
    let want = (crate::settings::mac_reserved_binds(&store.config), h.config_reloads);
    if mac.is_some() && sent.built.as_ref() != Some(&want) {
        compositor::send(Command::MacKeys(want.0.clone()));
        sent.built = Some(want);
    }
    // Asked again per window: the include holds the submap over the one
    // window it was last told of.
    let ask = (mac, h.config_reloads);
    if sent.asked.as_ref() != Some(&ask) {
        compositor::send(Command::MacFocus(ask.0.is_some(), h.snapshot.focused_window_id.clone()));
        sent.asked = Some(ask);
    }
    // After MacFocus, which takes the session out of `macapp` over no window.
    let none_focused = h.window(&h.snapshot.focused_window_id).is_none();
    let menu = store.appmenu.menu.as_ref().filter(|m| none_focused && appmenu::front(store) == Some(m.pid));
    let app = (menu.map(appmenu::Menu::chords), h.config_reloads);
    if sent.app.as_ref() != Some(&app) {
        compositor::send(Command::MacApp(app.0.clone()));
        sent.app = Some(app);
    }
}

/// Whether the session was last asked into the `mac` submap, for `appmenu
/// status`.
pub fn inside() -> bool {
    SENT.lock().unwrap().asked.as_ref().is_some_and(|(window, _)| window.is_some())
}
