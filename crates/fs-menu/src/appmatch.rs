//! Which running windows belong to a desktop entry, so activating an app row
//! focuses the app you already have open instead of spawning a second copy.
//!
//! Only two entry fields are read: `startup_class` ("initial class or app id
//! the app intends to use") and `id` (the .desktop basename, which equals the
//! app_id for the large majority of Linux apps). Each is tried exactly, then
//! case-folded.
//!
//! There is deliberately no third fuzzy tier (no reverse-DNS tail matching,
//! no substring). Electron and wrapper-launched apps often report an app_id
//! unrelated to their .desktop basename, and chasing that buys a worse
//! failure: a miss falls through to the caller's spawn path, while a fuzzy hit
//! focuses the wrong app.
//!
//! Window ids are opaque: `app_id`s are compared, ids are copied into the
//! result verbatim.

use fs_types::{Window, WindowId};

use crate::node::{DesktopEntry, Node};

/// One tier's comparison across every window, in window order. An empty
/// `app_id` never matches, so a window the compositor has not classified
/// cannot be claimed by an entry with an empty `startup_class`.
fn collect(needle: &str, windows: &[Window], fold: bool) -> Vec<WindowId> {
    let want = if fold { needle.to_lowercase() } else { needle.to_string() };
    windows
        .iter()
        .filter(|w| !w.app_id.is_empty())
        .filter(|w| if fold { w.app_id.to_lowercase() == want } else { w.app_id == want })
        .map(|w| w.id.clone())
        .collect()
}

/// Window ids in window order, or empty when nothing matched.
///
/// The first non-empty tier wins outright, never a union: a union would mix a
/// precise `startup_class` hit with a coincidental id hit and then cycle
/// between two different apps.
pub fn match_windows(entry: Option<&DesktopEntry>, windows: &[Window]) -> Vec<WindowId> {
    let Some(entry) = entry else { return Vec::new() };
    let mut tiers: Vec<&str> = Vec::new();
    if !entry.startup_class.is_empty() {
        tiers.push(&entry.startup_class);
    }
    if !entry.id.is_empty() {
        tiers.push(&entry.id);
    }
    for tier in tiers {
        let exact = collect(tier, windows, false);
        if !exact.is_empty() {
            return exact;
        }
        let folded = collect(tier, windows, true);
        if !folded.is_empty() {
            return folded;
        }
    }
    Vec::new()
}

/// The window to focus next: the one AFTER `focused_window_id` in `matches`,
/// wrapping, so repeat-activating an app row cycles its open instances. Focus
/// outside `matches` (another app, or nothing focused) starts the cycle at
/// the first instance. None when nothing matched.
pub fn next_window(matches: &[WindowId], focused_window_id: &WindowId) -> Option<WindowId> {
    let at = matches.iter().position(|m| m == focused_window_id).map_or(0, |at| at + 1);
    matches.get(at % matches.len().max(1)).cloned()
}

/// Marks the app rows that would focus rather than launch with the "FOCUS"
/// `desc`. Returns new rows, so the tree nodes stay the pure data they are.
pub fn decorate_app_rows(rows: &[Node], windows: &[Window]) -> Vec<Node> {
    rows.iter()
        .map(|row| {
            let mut copy = row.clone();
            if !match_windows(row.entry.as_ref(), windows).is_empty() {
                copy.desc = Some("FOCUS".to_string());
            }
            copy
        })
        .collect()
}
