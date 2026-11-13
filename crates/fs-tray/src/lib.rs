//! StatusNotifier watcher and host, a dbusmenu client and the appmenu registrar, on zbus and no
//! particular executor.
//!
//! [`Tray::start`] returns a [`Tray`] handle (state and actions) and a [`TrayEvents`] pump the
//! caller polls on its own executor; [`AppMenus::start`] does the same for app menus.

mod appmenu;
mod dbusmenu;
mod registrar;
mod tray;
mod types;
mod watcher;

pub use appmenu::{AppMenuEvent, AppMenuEvents, AppMenus, Held};
pub use dbusmenu::{Address, MenuEventKind};
pub use registrar::{BarState, REGISTRAR_NAME, REGISTRAR_PATH, Registrar, Registration, Registry};
pub use tray::{Event, Tray, TrayEvents};
pub use types::{
    CheckState, IconSource, Item, MenuItem, Pixmap, Status, Toggle, Tooltip,
    closest_pixmap,
};
