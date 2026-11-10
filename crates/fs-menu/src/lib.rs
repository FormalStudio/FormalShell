//! The launcher's pure logic: the tree model, fuzzy search, abbreviations,
//! cursor movement, pick frecency, the calculator, toggle conditions, route
//! icons, the providers that build each source's rows, clipboard history and
//! emoji detection, and window matching for app rows.
//!
//! Everything here is a function over plain data. Providers take their live
//! source as an argument and return nodes, so a caller can recompute one
//! source alone and re-attach it with `providers::attach_children`.

pub mod abbrev;
pub mod actions;
pub mod appgrid;
pub mod appmatch;
pub mod calc;
pub mod clipboard;
pub mod frecency;
pub mod icons;
pub mod intent;
pub mod keybinds;
pub mod model;
pub mod nav;
pub mod node;
pub mod providers;
pub mod search;
pub mod thumbnails;
pub mod toggles;
