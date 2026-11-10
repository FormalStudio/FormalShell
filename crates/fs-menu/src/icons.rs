//! Row icons for the launcher. A node's own `icon` is a raw glyph string,
//! which `Icon` cannot render: it resolves a NAME through the active set.
//! A route's `iconName` (the shipped menu's, a provider's) is that name, so
//! a row drawn from data still gets a named icon in whichever set
//! `theme.icons` selects.
//!
//! A node with no name draws its own glyph in the mono font, which keeps a
//! user-defined route, an emoji row (its icon IS the emoji) and a provider
//! row carrying its own glyph working unchanged.

use crate::node::{Kind, Node};

/// "" means "this row has no named icon": the caller draws the node's own
/// glyph instead.
pub fn icon_for(node: Option<&Node>) -> &str {
    node.map_or("", |n| n.icon_name.as_str())
}

/// The named icon a row draws when it has none of its own, by what the row
/// is. A row whose own icon is a raw glyph draws this instead: the icon set
/// is `theme.icons`' choice, and a codepoint from one set is a missing-glyph
/// box in another.
pub fn fallback_for(node: Option<&Node>) -> &'static str {
    match node.map(|n| &n.kind) {
        Some(Kind::App) => "app-window",
        Some(Kind::Submenu | Kind::Provider | Kind::Link) => "folder",
        Some(Kind::Action) => "terminal",
        Some(Kind::Window) => "app-window",
        Some(Kind::Folder) => "folder",
        Some(Kind::Package) => "package",
        Some(Kind::File) => {
            if node.is_some_and(|n| crate::providers::is_image(&n.path)) { "file-image" } else { "file" }
        }
        _ => "circle-help",
    }
}
