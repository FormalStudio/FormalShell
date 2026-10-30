//! Providers populate the launcher tree and its routes. Each source is a pure
//! function over plain data, so a caller can recompute one source alone (the
//! apps list, the clipboard rows, the Wi-Fi scan) and re-attach just that
//! subtree with `attach_children`, instead of rebuilding the whole tree.
//!
//! Provider functions return ready-made `Node` fragments (or, for the routes
//! that merge into the declared tree, `Entries`) rather than JSONC entries,
//! because they bypass `build_tree`'s kind inference: an app has no `action`
//! string, it has a desktop entry to execute, which respects `.desktop` field
//! codes and `Exec` quoting that re-running it through `sh -c` would mangle.

mod apps;
mod clipboard;
mod clipssh;
mod devices;
mod emoji;
mod share;
mod system;
mod wallpaper;

pub use apps::*;
pub use clipboard::*;
pub use clipssh::*;
pub use devices::*;
pub use emoji::*;
pub use share::*;
pub use system::*;
pub use wallpaper::*;
