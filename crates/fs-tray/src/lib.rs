//! StatusNotifier watcher and host plus a dbusmenu client, on zbus and no particular executor.
//!
//! [`Tray::start`] returns a [`Tray`] handle (state and actions) and a [`TrayEvents`] pump the
//! caller polls on its own executor.

mod tray;
mod types;
mod watcher;

pub use tray::{Event, MenuEventKind, Tray, TrayEvents};
pub use types::{
    CheckState, IconSource, Item, MenuItem, Pixmap, Status, Toggle, Tooltip,
    closest_pixmap,
};
