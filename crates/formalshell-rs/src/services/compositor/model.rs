//! The shapes every backend maps its compositor onto and the surfaces read.
//! Window and workspace ids are opaque strings minted by the backend that
//! reported them; only that backend ever takes one apart again.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub idx: i64,
    pub name: String,
    pub output: String,
    pub is_active: bool,
    pub is_focused: bool,
    pub is_urgent: bool,
    /// Its windows sit in a scrolling layout's columns, which reorder by
    /// moving a column rather than swapping two windows.
    pub columns: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub id: String,
    pub title: String,
    pub app_id: String,
    pub initial_class: String,
    pub initial_title: String,
    pub pid: i64,
    pub workspace_id: String,
    pub is_focused: bool,
    pub is_floating: bool,
    pub is_urgent: bool,
    pub rect: Option<Rect>,
    /// The compositor's ext-foreign-toplevel-list identifier, for a backend
    /// that reports one; thumbnails find the window's handle by it.
    #[serde(skip)]
    pub toplevel: String,
}

/// The compositor's outer gap on each side of an output, in logical
/// pixels: Hyprland's `general.gaps_out`, zero where a backend has none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gaps {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl Gaps {
    /// getoption's answer: a css-style `"top right bottom left"` (one to
    /// four numbers, as CSS fills them in) or a plain `int`.
    pub fn parse(text: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let nums: Vec<i32> = match (v.get("css").and_then(serde_json::Value::as_str), v.get("int").and_then(serde_json::Value::as_i64)) {
            (Some(css), _) => css.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?,
            (None, Some(i)) => vec![i32::try_from(i).ok()?],
            _ => return None,
        };
        let at = |i: usize| nums.get(i).copied();
        let top = at(0)?;
        let right = at(1).unwrap_or(top);
        let bottom = at(2).unwrap_or(top);
        Some(Self { top, right, bottom, left: at(3).unwrap_or(right) })
    }
}

/// Everything one refresh learns, before the backend's own bookkeeping
/// (window order, urgency) is laid over it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub available: bool,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub fullscreen_outputs: Vec<String>,
    /// The covered outputs with no tiled fullscreen window over them: the
    /// cover is a floating window, fullscreen or the size of the output.
    pub floating_outputs: Vec<String>,
    pub focused_window_id: String,
    pub focused_workspace_id: String,
    pub focused_output_name: String,
    /// Windows put away off every workspace (Hyprland's console special
    /// workspace while it is hidden, a minimized mango client).
    pub parked: Vec<String>,
    /// Windows on screen over the focused workspace though they belong to
    /// another (a shown special workspace, a second viewed mango tag).
    pub overlaid: Vec<String>,
}

/// A toplevel is appended when it is first heard of and never
/// reorders, so a window keeps its place across refreshes. `order` is the
/// ids seen so far, oldest first.
pub fn keep_order(order: &mut Vec<String>, windows: Vec<Window>) -> Vec<Window> {
    let live: HashSet<&str> = windows.iter().map(|w| w.id.as_str()).collect();
    order.retain(|id| live.contains(id.as_str()));
    for w in &windows {
        if !order.contains(&w.id) {
            order.push(w.id.clone());
        }
    }
    let mut by_id: HashMap<String, Window> = windows.into_iter().map(|w| (w.id.clone(), w)).collect();
    order.iter().filter_map(|id| by_id.remove(id)).collect()
}

/// focus.js `held`: the compositor's focused window, or across a gap where
/// it reports none (a shell surface took the keyboard) the last one it did,
/// while that window is still open on the focused workspace.
pub fn held_focus(focused: &str, remembered: &str, windows: &[Window], focused_workspace: &str) -> String {
    if !focused.is_empty() {
        return focused.to_owned();
    }
    if remembered.is_empty() {
        return String::new();
    }
    match windows.iter().find(|w| w.id == remembered) {
        Some(w) if focused_workspace.is_empty() || w.workspace_id == focused_workspace => remembered.to_owned(),
        _ => String::new(),
    }
}

/// Parked, or on another workspace than the focused one and not shown over
/// it. A window the backend has never heard of counts as parked, so a
/// console toggle asks for it to be brought here.
pub fn is_parked(s: &Snapshot, id: &str) -> bool {
    match s.windows.iter().find(|w| w.id == id) {
        None => true,
        Some(w) => {
            s.parked.contains(&w.id) || (w.workspace_id != s.focused_workspace_id && !s.overlaid.contains(&w.id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_read_as_css_fills_them_in() {
        let g = |t: &str| Gaps::parse(t);
        assert_eq!(g(r#"{"option":"general:gaps_out","css":"6 6 6 6","set":true}"#), Some(Gaps { top: 6, right: 6, bottom: 6, left: 6 }));
        assert_eq!(g(r#"{"css":"1 2 3 4"}"#), Some(Gaps { top: 1, right: 2, bottom: 3, left: 4 }));
        assert_eq!(g(r#"{"css":"5 9"}"#), Some(Gaps { top: 5, right: 9, bottom: 5, left: 9 }));
        assert_eq!(g(r#"{"int":20}"#), Some(Gaps { top: 20, right: 20, bottom: 20, left: 20 }));
        assert_eq!(g("no"), None);
    }

    #[test]
    fn order_holds_across_refreshes() {
        let w = |id: &str| Window { id: id.into(), ..Window::default() };
        let mut order = Vec::new();
        keep_order(&mut order, vec![w("b"), w("a")]);
        let next = keep_order(&mut order, vec![w("c"), w("a"), w("b")]);
        let ids: Vec<&str> = next.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["b", "a", "c"]);
        let next = keep_order(&mut order, vec![w("c"), w("a")]);
        assert_eq!(next.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
    }

    #[test]
    fn held_focus_follows_focus_js() {
        let windows = vec![Window { id: "a".into(), workspace_id: "1".into(), ..Window::default() }];
        assert_eq!(held_focus("b", "a", &windows, "1"), "b");
        assert_eq!(held_focus("", "a", &windows, "1"), "a");
        assert_eq!(held_focus("", "a", &windows, "2"), "");
        assert_eq!(held_focus("", "a", &windows, ""), "a");
        assert_eq!(held_focus("", "gone", &windows, "1"), "");
        assert_eq!(held_focus("", "", &windows, "1"), "");
    }

    #[test]
    fn parked_reads_the_snapshot() {
        let w = |id: &str, ws: &str| Window { id: id.into(), workspace_id: ws.into(), ..Window::default() };
        let mut s = Snapshot {
            windows: vec![w("here", "1"), w("away", "2"), w("over", "-98"), w("hidden", "1")],
            focused_workspace_id: "1".into(),
            parked: vec!["hidden".into()],
            overlaid: vec!["over".into()],
            ..Snapshot::default()
        };
        assert!(!is_parked(&s, "here"));
        assert!(is_parked(&s, "away"));
        assert!(!is_parked(&s, "over"));
        assert!(is_parked(&s, "hidden"));
        assert!(is_parked(&s, "unknown"));
        s.overlaid.clear();
        assert!(is_parked(&s, "over"));
    }
}
