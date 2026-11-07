// Portions from omarchy-spaces (MIT, Copyright 2026 Tornike Gomareli)

//! Model for the bar's workspace cell: which workspaces render, in what
//! order, and which windows each one shows.
//!
//! A workspace renders if it holds at least one window, is active/focused, or
//! is one of the persistent slots 1..n. Occupancy is counted from the windows
//! list by workspace id because the backend's workspace payload carries no
//! occupancy field of its own. Order is the backend's own per-output ordinal
//! `idx`; ids stay opaque strings, never parsed. Workspaces on `output_name`
//! win; when none match (compositor output name vs screen name mismatch)
//! every workspace is considered, grouped by output name so the fallback
//! stays deterministic.
//!
//! A persistent slot the compositor has no workspace for at all, on any
//! output, is a placeholder: `id` empty, `placeholder` true, its `idx` the
//! only thing it knows. Going there is a focus by idx, since there is no id
//! to hand over. One that exists on another output is that output's, and
//! stays off this bar.

use std::cmp::Ordering;
use std::collections::HashSet;

use fs_types::Rect;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub idx: i64,
    pub name: String,
    pub output: String,
    pub is_active: bool,
    pub is_focused: bool,
    pub is_urgent: bool,
    pub placeholder: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    pub id: String,
    pub workspace_id: String,
    pub app_id: String,
    pub initial_class: String,
    pub initial_title: String,
    pub pid: i64,
    pub is_focused: bool,
    pub is_urgent: bool,
    pub is_floating: bool,
    /// The window's box, absent when the backend has none (`rect: null`).
    pub rect: Option<Rect>,
}

fn slot_output(pool: &[&Workspace], output_name: &str, matched: bool) -> String {
    if matched || pool.is_empty() {
        return output_name.into();
    }
    pool.iter()
        .map(|w| w.output.as_str())
        .min()
        .expect("pool is not empty")
        .into()
}

/// The workspaces this bar draws, sorted by output then `idx`. `persistent`
/// is the count of slots 1..n kept even when empty; below 0 reads as 0.
pub fn visible_model(
    workspaces: &[Workspace],
    windows: &[Window],
    output_name: &str,
    persistent: i64,
) -> Vec<Workspace> {
    let on_output: Vec<&Workspace> = workspaces
        .iter()
        .filter(|w| w.output == output_name)
        .collect();
    let matched = !on_output.is_empty();
    let pool: Vec<&Workspace> = if matched {
        on_output
    } else {
        workspaces.iter().collect()
    };

    let occupied: HashSet<&str> = windows.iter().map(|w| w.workspace_id.as_str()).collect();
    let keep = persistent.max(0);
    let known: HashSet<i64> = workspaces.iter().map(|w| w.idx).collect();

    let mut out: Vec<Workspace> = pool
        .iter()
        .filter(|ws| {
            occupied.contains(ws.id.as_str())
                || ws.is_active
                || ws.is_focused
                || (ws.idx >= 1 && ws.idx <= keep)
        })
        .map(|ws| (*ws).clone())
        .collect();

    let slot_output = slot_output(&pool, output_name, matched);
    for n in 1..=keep {
        if known.contains(&n) {
            continue;
        }
        out.push(Workspace {
            idx: n,
            output: slot_output.clone(),
            placeholder: true,
            ..Workspace::default()
        });
    }

    out.sort_by(|a, b| a.output.cmp(&b.output).then(a.idx.cmp(&b.idx)));
    out
}

/// Screen order: left to right, then top to bottom. A window with no box goes
/// after every placed one, and ties keep the order the backend listed them in.
fn by_position(a: &(&Window, usize), b: &(&Window, usize)) -> Ordering {
    match (&a.0.rect, &b.0.rect) {
        (Some(ra), Some(rb)) => {
            if ra.x != rb.x {
                return ra.x.partial_cmp(&rb.x).unwrap_or(Ordering::Equal);
            }
            if ra.y != rb.y {
                return ra.y.partial_cmp(&rb.y).unwrap_or(Ordering::Equal);
            }
        }
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (None, None) => {}
    }
    a.1.cmp(&b.1)
}

/// The fields of a window that pick an icon and a state. The title changes on
/// every keystroke in a terminal and the rect on every resize; carrying
/// either would republish the whole bar model for a change nothing on the
/// strip draws. Tooltips and the preview read those live by id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlotWindow {
    pub id: String,
    pub app_id: String,
    pub initial_class: String,
    pub initial_title: String,
    pub pid: i64,
    pub is_focused: bool,
    pub is_urgent: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlotWindows {
    pub windows: Vec<SlotWindow>,
    pub overflow: usize,
}

/// The windows one slot shows, in screen order, capped at `max_icons` (at
/// least 1) with the rest counted in `overflow`. The focused window is never
/// the one the cap drops: past the cap it takes the last shown place instead,
/// so the lit icon is always on the bar.
pub fn slot_windows(windows: &[Window], workspace_id: &str, max_icons: usize) -> SlotWindows {
    if workspace_id.is_empty() {
        return SlotWindows::default();
    }
    let mut indexed: Vec<(&Window, usize)> = windows
        .iter()
        .enumerate()
        .filter(|(_, w)| w.workspace_id == workspace_id)
        .map(|(at, w)| (w, at))
        .collect();
    indexed.sort_by(by_position);
    let cap = max_icons.max(1);
    let mut shown: Vec<(&Window, usize)> = indexed.iter().take(cap).copied().collect();
    if indexed.len() > cap
        && let Some(focused) = indexed[cap..].iter().find(|(w, _)| w.is_focused)
    {
        shown[cap - 1] = *focused;
    }
    SlotWindows {
        windows: shown
            .into_iter()
            .map(|(w, _)| SlotWindow {
                id: w.id.clone(),
                app_id: w.app_id.clone(),
                initial_class: w.initial_class.clone(),
                initial_title: w.initial_title.clone(),
                pid: w.pid,
                is_focused: w.is_focused,
                is_urgent: w.is_urgent,
            })
            .collect(),
        overflow: indexed.len().saturating_sub(cap),
    }
}

/// What a slot is called: its ordinal, always. A Hyprland `default_name` is
/// config (often a glyph and a word) and the chip already says what is on the
/// workspace with its icons.
pub fn label(ws: &Workspace) -> String {
    ws.idx.to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotOpts {
    pub persistent: i64,
    pub max_icons: usize,
}

impl Default for SlotOpts {
    fn default() -> Self {
        SlotOpts {
            persistent: 0,
            max_icons: 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub id: String,
    pub idx: i64,
    pub name: String,
    pub label: String,
    pub output: String,
    pub is_active: bool,
    pub is_focused: bool,
    /// On screen on this output (focused or merely visible): the slot that
    /// opens wide. `is_focused` is where the pill sits.
    pub current: bool,
    pub is_urgent: bool,
    pub placeholder: bool,
    pub windows: Vec<SlotWindow>,
    pub overflow: usize,
}

/// Every slot this bar draws, windows included.
pub fn slots(
    workspaces: &[Workspace],
    windows: &[Window],
    output_name: &str,
    opts: SlotOpts,
) -> Vec<Slot> {
    visible_model(workspaces, windows, output_name, opts.persistent)
        .into_iter()
        .map(|ws| {
            let list = slot_windows(windows, &ws.id, opts.max_icons);
            let urgent = ws.is_urgent || list.windows.iter().any(|w| w.is_urgent);
            Slot {
                label: label(&ws),
                current: ws.is_active || ws.is_focused,
                is_urgent: urgent,
                windows: list.windows,
                overflow: list.overflow,
                id: ws.id,
                idx: ws.idx,
                name: ws.name,
                output: ws.output,
                is_active: ws.is_active,
                is_focused: ws.is_focused,
                placeholder: ws.placeholder,
            }
        })
        .collect()
}

/// Whether a slot shows its icons. `mode` is `workspaces.showApps`: `all`
/// (every occupied slot), `active` (the current slot only) or `hover` (the
/// current slot, and any other while the pointer is on it). An unknown mode
/// reads as `hover`.
pub fn shows_apps(mode: &str, current: bool, hovered: bool) -> bool {
    match mode {
        "all" => true,
        "active" => current,
        _ => current || hovered,
    }
}

/// The slot a wheel notch lands on, wrapping at either end; the first slot
/// when none is focused here. `None` on an empty list.
pub fn step_index(count: usize, focused_index: i64, delta: i64) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let count = count as i64;
    if focused_index < 0 || focused_index >= count {
        return Some(0);
    }
    let step = if delta > 0 { 1 } else { -1 };
    Some(((focused_index + step + count) % count) as usize)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub floating: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewLayout {
    pub windows: Vec<Placed>,
    pub bounds: Size,
    pub home: Point,
}

/// Where each window sits in a miniature drawn at `width` x `height` per
/// `area` (the output's logical box), at its real place and full size. The
/// miniature scrolls over a strip that is the union of the output and every
/// window, so a window past the output's edge (a scrolling layout parks them
/// there) keeps its real position instead of being clamped into the frame.
/// Positions are in the strip's own space, whose origin is the union's top
/// left; `home` is where the output's own box sits in it, which is what is on
/// screen now. Floating windows come last so they draw on top, as they do on
/// screen. If any window has no box, none of them has a place that means
/// anything, and the lot are laid out as an even grid instead.
pub fn preview_layout(
    windows: &[Window],
    area: Option<Rect>,
    width: f64,
    height: f64,
    min_size: f64,
) -> PreviewLayout {
    let min = if min_size > 0.0 { min_size } else { 1.0 };
    let n = windows.len();
    let area = area.filter(|a| a.width > 0.0 && a.height > 0.0);
    let known = area.is_some() && n > 0 && windows.iter().all(|w| w.rect.is_some());
    let (mut left, mut top, mut right, mut bottom) = (0.0_f64, 0.0_f64, width, height);
    let mut home = Point { x: 0.0, y: 0.0 };
    let mut placed: Vec<Placed> = Vec::new();

    if let (true, Some(area)) = (known, area) {
        let sx = width / area.width;
        let sy = height / area.height;
        let raw: Vec<Placed> = windows
            .iter()
            .map(|w| {
                let r = w.rect.expect("every rect is present");
                Placed {
                    id: w.id.clone(),
                    x: (r.x - area.x) * sx,
                    y: (r.y - area.y) * sy,
                    width: min.max(r.width * sx),
                    height: min.max(r.height * sy),
                    floating: w.is_floating,
                }
            })
            .collect();
        for p in &raw {
            left = left.min(p.x);
            top = top.min(p.y);
            right = right.max(p.x + p.width);
            bottom = bottom.max(p.y + p.height);
        }
        home = Point { x: -left, y: -top };
        placed = raw
            .into_iter()
            .map(|p| Placed {
                x: p.x - left,
                y: p.y - top,
                ..p
            })
            .collect();
    } else if n > 0 {
        let cols = ((n as f64).sqrt().ceil() as usize).max(1);
        let rows = n.div_ceil(cols).max(1);
        let cw = width / cols as f64;
        let ch = height / rows as f64;
        for (j, w) in windows.iter().enumerate() {
            placed.push(Placed {
                id: w.id.clone(),
                x: (j % cols) as f64 * cw,
                y: (j / cols) as f64 * ch,
                width: cw,
                height: ch,
                floating: false,
            });
        }
    }

    placed.sort_by_key(|p| p.floating);
    PreviewLayout {
        windows: placed,
        bounds: Size {
            width: right - left,
            height: bottom - top,
        },
        home,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Flags {
        active: bool,
        focused: bool,
        urgent: bool,
        floating: bool,
        app_id: &'static str,
    }

    fn ws(id: &str, idx: i64, output: &str, flags: Flags) -> Workspace {
        Workspace {
            id: id.into(),
            idx,
            output: output.into(),
            is_active: flags.active,
            is_focused: flags.focused,
            ..Workspace::default()
        }
    }

    fn plain(id: &str, idx: i64, output: &str) -> Workspace {
        ws(id, idx, output, Flags::default())
    }

    fn lit(id: &str, idx: i64, output: &str) -> Workspace {
        ws(
            id,
            idx,
            output,
            Flags {
                active: true,
                focused: true,
                ..Flags::default()
            },
        )
    }

    fn active(id: &str, idx: i64, output: &str) -> Workspace {
        ws(
            id,
            idx,
            output,
            Flags {
                active: true,
                ..Flags::default()
            },
        )
    }

    fn win(id: &str, workspace_id: &str) -> Window {
        Window {
            id: id.into(),
            workspace_id: workspace_id.into(),
            ..Window::default()
        }
    }

    fn ids(model: &[Workspace]) -> String {
        model
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn rwin(id: &str, workspace_id: &str, at: Option<(f64, f64)>, flags: Flags) -> Window {
        Window {
            id: id.into(),
            workspace_id: workspace_id.into(),
            app_id: flags.app_id.into(),
            is_focused: flags.focused,
            is_urgent: flags.urgent,
            is_floating: flags.floating,
            rect: at.map(|(x, y)| Rect::new(x, y, 100.0, 100.0)),
            ..Window::default()
        }
    }

    fn ids_of(list: &SlotWindows) -> String {
        list.windows
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    #[test]
    fn sorts_by_idx_not_input_order() {
        let model = visible_model(
            &[
                plain("30", 3, "eDP-1"),
                plain("10", 1, "eDP-1"),
                plain("20", 2, "eDP-1"),
            ],
            &[win("a", "10"), win("b", "20"), win("c", "30")],
            "eDP-1",
            0,
        );
        assert_eq!(ids(&model), "10,20,30");
    }

    #[test]
    fn id_is_never_the_sort_key() {
        // ids are opaque: descending ids with ascending idx must order by idx
        let model = visible_model(
            &[plain("9", 2, "eDP-1"), plain("100", 1, "eDP-1")],
            &[win("a", "9"), win("b", "100")],
            "eDP-1",
            0,
        );
        assert_eq!(ids(&model), "100,9");
    }

    #[test]
    fn hides_empty_inactive_workspaces() {
        let model = visible_model(
            &[
                lit("1", 1, "eDP-1"),
                plain("2", 2, "eDP-1"),
                plain("3", 3, "eDP-1"),
                plain("4", 4, "eDP-1"),
            ],
            &[win("a", "1"), win("b", "3")],
            "eDP-1",
            0,
        );
        assert_eq!(ids(&model), "1,3");
    }

    #[test]
    fn keeps_focused_workspace_without_windows() {
        let model = visible_model(
            &[plain("1", 1, "eDP-1"), lit("2", 2, "eDP-1")],
            &[win("a", "1")],
            "eDP-1",
            0,
        );
        assert_eq!(ids(&model), "1,2");
    }

    #[test]
    fn keeps_active_but_unfocused_workspace() {
        // multi-output: active on its output, focus elsewhere
        let model = visible_model(
            &[active("1", 1, "HDMI-A-1"), plain("2", 2, "HDMI-A-1")],
            &[],
            "HDMI-A-1",
            0,
        );
        assert_eq!(ids(&model), "1");
    }

    #[test]
    fn filters_to_named_output() {
        let model = visible_model(
            &[active("1", 1, "eDP-1"), active("2", 1, "HDMI-A-1")],
            &[],
            "eDP-1",
            0,
        );
        assert_eq!(ids(&model), "1");
    }

    #[test]
    fn falls_back_to_all_outputs_when_none_match() {
        let model = visible_model(
            &[active("2", 1, "HDMI-A-1"), active("1", 1, "DP-1")],
            &[],
            "winit",
            0,
        );
        assert_eq!(ids(&model), "1,2");
    }

    #[test]
    fn fallback_groups_by_output_then_idx() {
        let model = visible_model(
            &[
                plain("b2", 2, "HDMI-A-1"),
                plain("a2", 2, "DP-1"),
                plain("b1", 1, "HDMI-A-1"),
                plain("a1", 1, "DP-1"),
            ],
            &[
                win("w", "a1"),
                win("x", "a2"),
                win("y", "b1"),
                win("z", "b2"),
            ],
            "winit",
            0,
        );
        assert_eq!(ids(&model), "a1,a2,b1,b2");
    }

    #[test]
    fn occupancy_matches_by_workspace_id_string() {
        let model = visible_model(
            &[lit("7", 1, "eDP-1"), plain("8", 2, "eDP-1")],
            &[win("a", "8")],
            "eDP-1",
            0,
        );
        assert_eq!(ids(&model), "7,8");
    }

    #[test]
    fn empty_windows_shows_only_active() {
        let model = visible_model(
            &[lit("1", 1, "eDP-1"), plain("2", 2, "eDP-1")],
            &[],
            "eDP-1",
            0,
        );
        assert_eq!(ids(&model), "1");
    }

    #[test]
    fn persistent_fills_missing_slots_as_placeholders() {
        let model = visible_model(&[lit("7", 2, "eDP-1")], &[], "eDP-1", 3);
        let idxs: Vec<_> = model.iter().map(|w| w.idx.to_string()).collect();
        assert_eq!(idxs.join(","), "1,2,3");
        assert_eq!(model[0].id, "");
        assert!(model[0].placeholder);
        assert_eq!(model[1].id, "7");
        assert!(!model[1].placeholder);
    }

    #[test]
    fn persistent_keeps_existing_empty_workspace() {
        let model = visible_model(
            &[
                lit("1", 1, "eDP-1"),
                plain("2", 2, "eDP-1"),
                plain("4", 4, "eDP-1"),
            ],
            &[],
            "eDP-1",
            2,
        );
        assert_eq!(ids(&model), "1,2");
    }

    #[test]
    fn persistent_leaves_another_outputs_workspace_alone() {
        let model = visible_model(
            &[lit("1", 1, "eDP-1"), active("2", 2, "HDMI-A-1")],
            &[],
            "eDP-1",
            2,
        );
        assert_eq!(ids(&model), "1");
    }

    #[test]
    fn persistent_placeholder_takes_the_first_output_when_none_match() {
        let model = visible_model(
            &[active("2", 2, "HDMI-A-1"), active("1", 5, "DP-1")],
            &[],
            "winit",
            3,
        );
        let placeholder = model
            .iter()
            .find(|w| w.placeholder)
            .expect("slot 1 is missing everywhere");
        assert_eq!((placeholder.idx, placeholder.output.as_str()), (1, "DP-1"));
    }

    #[test]
    fn slot_windows_in_screen_order() {
        let list = slot_windows(
            &[
                rwin("b", "1", Some((500.0, 0.0)), Flags::default()),
                rwin("x", "2", Some((0.0, 0.0)), Flags::default()),
                rwin("c", "1", Some((0.0, 500.0)), Flags::default()),
                rwin("a", "1", Some((0.0, 0.0)), Flags::default()),
                rwin("n", "1", None, Flags::default()),
            ],
            "1",
            8,
        );
        assert_eq!(ids_of(&list), "a,c,b,n");
        assert_eq!(list.overflow, 0);
    }

    #[test]
    fn slot_windows_cap_keeps_focused() {
        let list = slot_windows(
            &[
                rwin("a", "1", Some((0.0, 0.0)), Flags::default()),
                rwin("b", "1", Some((100.0, 0.0)), Flags::default()),
                rwin("c", "1", Some((200.0, 0.0)), Flags::default()),
                rwin(
                    "d",
                    "1",
                    Some((300.0, 0.0)),
                    Flags {
                        focused: true,
                        ..Flags::default()
                    },
                ),
            ],
            "1",
            2,
        );
        assert_eq!(ids_of(&list), "a,d");
        assert_eq!(list.overflow, 2);
    }

    // The carried type has no `title` or `rect` fields at all.
    #[test]
    fn slot_windows_drop_title_and_rect() {
        let list = slot_windows(
            &[rwin(
                "a",
                "1",
                Some((0.0, 0.0)),
                Flags {
                    app_id: "foot",
                    ..Flags::default()
                },
            )],
            "1",
            8,
        );
        assert_eq!(list.windows[0].app_id, "foot");
    }

    #[test]
    fn placeholder_slot_has_no_windows() {
        let list = slot_windows(&[rwin("a", "", Some((0.0, 0.0)), Flags::default())], "", 8);
        assert_eq!(list.windows.len(), 0);
    }

    #[test]
    fn slots_label_urgent_and_current() {
        let mut named = plain("3", 3, "eDP-1");
        named.name = "web".into();
        let model = slots(
            &[lit("1", 1, "eDP-1"), named],
            &[rwin(
                "a",
                "3",
                Some((0.0, 0.0)),
                Flags {
                    urgent: true,
                    ..Flags::default()
                },
            )],
            "eDP-1",
            SlotOpts {
                persistent: 0,
                max_icons: 8,
            },
        );
        assert_eq!(model.len(), 2);
        assert_eq!(model[0].label, "1");
        assert!(model[0].current);
        assert!(!model[0].is_urgent);
        assert_eq!(model[1].label, "3");
        assert_eq!(model[1].name, "web");
        assert!(!model[1].current);
        assert!(model[1].is_urgent);
        assert_eq!(model[1].windows.len(), 1);
    }

    #[test]
    fn shows_apps_modes() {
        assert!(shows_apps("all", false, false));
        assert!(!shows_apps("active", false, true));
        assert!(shows_apps("active", true, false));
        assert!(shows_apps("hover", false, true));
        assert!(!shows_apps("hover", false, false));
        assert!(shows_apps("bogus", true, false));
    }

    #[test]
    fn step_index_wraps() {
        assert_eq!(step_index(3, 2, 1), Some(0));
        assert_eq!(step_index(3, 0, -1), Some(2));
        assert_eq!(step_index(3, -1, 1), Some(0));
        assert_eq!(step_index(0, 0, 1), None);
    }

    fn preview_window(id: &str, rect: Option<Rect>, floating: bool) -> Window {
        Window {
            id: id.into(),
            rect,
            is_floating: floating,
            ..Window::default()
        }
    }

    #[test]
    fn preview_layout_keeps_windows_past_the_edge() {
        let area = Rect::new(1000.0, 0.0, 1000.0, 500.0);
        let layout = preview_layout(
            &[
                preview_window("a", Some(Rect::new(1000.0, 0.0, 500.0, 500.0)), false),
                preview_window("b", Some(Rect::new(2100.0, 100.0, 500.0, 400.0)), false),
            ],
            Some(area),
            200.0,
            100.0,
            4.0,
        );
        let placed = &layout.windows;
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].x, 0.0);
        assert_eq!(placed[0].width, 100.0);
        assert_eq!(placed[0].height, 100.0);
        assert_eq!(placed[1].x, 220.0);
        assert_eq!(placed[1].y, 20.0);
        assert_eq!(placed[1].width, 100.0);
        assert_eq!(placed[1].height, 80.0);
        assert_eq!(layout.bounds.width, 320.0);
        assert_eq!(layout.bounds.height, 100.0);
        assert_eq!(layout.home, Point { x: 0.0, y: 0.0 });
    }

    #[test]
    fn preview_layout_left_and_above_move_the_output_home() {
        let area = Rect::new(0.0, 0.0, 1000.0, 500.0);
        let layout = preview_layout(
            &[
                preview_window("l", Some(Rect::new(-600.0, -250.0, 500.0, 500.0)), false),
                preview_window("o", Some(Rect::new(0.0, 0.0, 1000.0, 500.0)), false),
            ],
            Some(area),
            200.0,
            100.0,
            4.0,
        );
        assert_eq!(layout.home, Point { x: 120.0, y: 50.0 });
        assert_eq!(layout.windows[0].x, 0.0);
        assert_eq!(layout.windows[0].y, 0.0);
        assert_eq!(layout.windows[1].x, layout.home.x);
        assert_eq!(layout.windows[1].y, layout.home.y);
        assert_eq!(layout.bounds.width, 320.0);
        assert_eq!(layout.bounds.height, 150.0);
    }

    #[test]
    fn preview_layout_bounds_are_the_output_when_windows_fit() {
        let layout = preview_layout(
            &[preview_window(
                "a",
                Some(Rect::new(10.0, 10.0, 400.0, 300.0)),
                false,
            )],
            Some(Rect::new(0.0, 0.0, 1000.0, 500.0)),
            200.0,
            100.0,
            4.0,
        );
        assert_eq!(
            layout.bounds,
            Size {
                width: 200.0,
                height: 100.0
            }
        );
        assert_eq!(layout.home, Point { x: 0.0, y: 0.0 });
    }

    #[test]
    fn preview_layout_floating_draws_last() {
        let placed = preview_layout(
            &[
                preview_window("f", Some(Rect::new(0.0, 0.0, 10.0, 10.0)), true),
                preview_window("t", Some(Rect::new(0.0, 0.0, 100.0, 100.0)), false),
            ],
            Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
            100.0,
            100.0,
            4.0,
        )
        .windows;
        let order: Vec<_> = placed.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order.join(","), "t,f");
    }

    #[test]
    fn preview_layout_grid_without_rects() {
        let placed = preview_layout(
            &[
                preview_window("a", None, false),
                preview_window("b", None, false),
                preview_window("c", None, false),
            ],
            Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
            200.0,
            100.0,
            4.0,
        )
        .windows;
        assert_eq!(placed.len(), 3);
        assert_eq!(placed[0].width, 100.0);
        assert_eq!(placed[0].height, 50.0);
        assert_eq!(placed[2].x, 0.0);
        assert_eq!(placed[2].y, 50.0);
    }
}
