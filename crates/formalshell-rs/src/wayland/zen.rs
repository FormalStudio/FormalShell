//! Zen: the focused window floating and centred on its output, with the
//! sidebar folded to its rail while its workspace has the focus and, under
//! the scrolling layout, the rest of the workspace's tape slid off the
//! output. Zen follows the focus on its workspace: a tiled window taking it
//! (a focus bind, a click, the switcher, an activation, a new window, the
//! zen window closing) takes zen, the window before going back where it
//! was. A float taking the focus has it over the zen window. Leaving puts
//! the window back tiled where it left the tape, in its stack if it had one
//! (or at its old rect if it was floating already), and lets the tape go.
//!
//! Under a compositor that owns zen (Shoji) the shell only asks for it and
//! reads each workspace's zen window back, folding the sidebar while the
//! focused workspace holds one.

use std::time::{Duration, Instant};

use fs_types::{IRect, Window, WindowId, WorkspaceId};
use serde_json::json;

use super::App;
use crate::services::compositor::{self, Command, ZenKeys, ZenPlace};
use crate::surfaces::sidebar;

/// How long zen waits for the focus to reach the window it moved to before
/// it goes by the focus again.
const SETTLE: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct ZenState {
    on: Option<Zen>,
    /// While zen holds the focused workspace: whether the sidebar was open,
    /// so leaving opens it again.
    folded: Option<bool>,
    /// A workspace whose tape is still held after its zen window closed
    /// there; let go once it is focused, the only workspace a layout
    /// message reaches.
    release: Option<WorkspaceId>,
    /// The window zen just went to, until the focus is on it: on the way
    /// the focus passes over the window going back and the tape's.
    moving: Option<(WindowId, Instant)>,
}

struct Zen {
    id: WindowId,
    workspace: WorkspaceId,
    /// Its rect before, for a window that was floating already.
    floating: Option<IRect>,
    /// Where it goes back in a scrolling layout's tape.
    place: ZenPlace,
}

/// The zen window's rect on an output's logical box: 60% of its width but
/// never under 1100 where the output has the room, and its height less a
/// band above and below.
fn centred(x: i32, y: i32, w: i32, h: i32, margin: f64) -> IRect {
    let (wf, hf) = (f64::from(w), f64::from(h));
    let width = (wf * 0.6).max((wf - 2.0 * margin).min(1100.0));
    let height = hf - 2.0 * (hf * 0.06).max(margin);
    IRect::new(
        x + ((wf - width) / 2.0).round() as i32,
        y + ((hf - height) / 2.0).round() as i32,
        width.round() as i32,
        height.round() as i32,
    )
}

/// Where tiled `id` sits in its workspace's tape once `back`, the window
/// zen leaves (floating now), is back at its place: the place `id` goes
/// back to in turn, and where each focus move goes from `id`. Columns are
/// told apart by x, stacked windows by y; a move to a column lands on its
/// window nearest `id`'s middle.
fn slot(windows: &[Window], id: &WindowId, back: Option<(&WindowId, &ZenPlace)>) -> (ZenPlace, ZenKeys) {
    let last = ZenPlace::Column { before: None };
    let Some(me) = windows.iter().find(|w| w.id == *id && !w.is_floating) else {
        return (last, ZenKeys::default());
    };
    let mut tiled: Vec<(IRect, &WindowId)> = windows
        .iter()
        .filter(|w| w.workspace_id == me.workspace_id && !w.is_floating && back.is_none_or(|(b, _)| w.id != *b))
        .filter_map(|w| Some((w.rect?, &w.id)))
        .collect();
    tiled.sort_by_key(|(r, _)| (r.x, r.y));
    let mut columns: Vec<Vec<(IRect, &WindowId)>> = Vec::new();
    for (rect, id) in tiled {
        match columns.last_mut() {
            Some(column) if column[0].0.x == rect.x => column.push((rect, id)),
            _ => columns.push(vec![(rect, id)]),
        }
    }
    if let Some((back, place)) = back {
        let column_of = |w: &WindowId| columns.iter().position(|c| c.iter().any(|(_, x)| *x == w));
        let stack = match place {
            ZenPlace::Stack { mate, below } => column_of(mate).map(|c| (c, below)),
            ZenPlace::Column { .. } => None,
        };
        match (stack, place) {
            (Some((c, below)), _) => {
                let row = below.as_ref().and_then(|b| columns[c].iter().position(|(_, x)| *x == b)).unwrap_or(columns[c].len());
                let rect = columns[c].get(row).or(columns[c].last()).map_or_else(IRect::default, |(r, _)| *r);
                columns[c].insert(row, (rect, back));
            }
            (None, ZenPlace::Column { before: Some(before) }) => {
                let at = column_of(before).unwrap_or(columns.len());
                columns.insert(at, vec![(IRect::default(), back)]);
            }
            (None, _) => columns.push(vec![(IRect::default(), back)]),
        }
    }
    let Some((column, row)) = columns.iter().enumerate().find_map(|(c, col)| col.iter().position(|(_, w)| *w == id).map(|r| (c, r))) else {
        return (last, ZenKeys::default());
    };
    let mates = &columns[column];
    let middle = |r: &IRect| r.y + r.h / 2;
    let centre = middle(&mates[row].0);
    let nearest = |c: usize| columns[c].iter().min_by_key(|(r, _)| (middle(r) - centre).abs()).map(|(_, w)| (*w).clone());
    let n = columns.len();
    let place = if mates.len() > 1 {
        ZenPlace::Stack { mate: mates[usize::from(row == 0)].1.clone(), below: mates.get(row + 1).map(|(_, w)| (*w).clone()) }
    } else {
        ZenPlace::Column { before: columns.get(column + 1).map(|c| c[0].1.clone()) }
    };
    let keys = ZenKeys {
        left: (n > 1).then(|| nearest((column + n - 1) % n)).flatten(),
        right: (n > 1).then(|| nearest((column + 1) % n)).flatten(),
        up: row.checked_sub(1).map(|r| mates[r].1.clone()),
        down: mates.get(row + 1).map(|(_, w)| (*w).clone()),
    };
    (place, keys)
}

impl App {
    pub fn zen_toggle(&mut self) -> Result<(), String> {
        if compositor::kind() == compositor::Kind::Shoji {
            compositor::send(Command::ZenMode { on: None });
            return Ok(());
        }
        if self.zen.on.is_some() { self.zen_exit() } else { self.zen_enter() }
    }

    /// The compositor's zen window on the focused workspace.
    fn compositor_zen(&self) -> Option<(&WindowId, &WorkspaceId)> {
        let snap = &self.store.compositor.snapshot;
        snap.workspaces.iter().find(|w| w.id == snap.focused_workspace_id).and_then(|w| Some((w.zen.as_ref()?, &w.id)))
    }

    pub fn zen_enter(&mut self) -> Result<(), String> {
        if compositor::kind() == compositor::Kind::Shoji {
            if self.compositor_zen().is_some() {
                return Err("zen is on".into());
            }
            if self.store.compositor.snapshot.focused_window_id.is_empty() {
                return Err("no focused window".into());
            }
            compositor::send(Command::ZenMode { on: Some(true) });
            return Ok(());
        }
        if self.zen.on.is_some() {
            return Err("zen is on".into());
        }
        let snap = &self.store.compositor.snapshot;
        let Some(win) = snap.windows.iter().find(|w| w.id == snap.focused_window_id) else {
            return Err("no focused window".into());
        };
        let Some((ox, oy, ow, oh)) = self.zen_output() else {
            return Err("no output".into());
        };
        let columns = snap.workspaces.iter().any(|w| w.id == win.workspace_id && w.columns);
        let (place, keys) = if columns { slot(&snap.windows, &win.id, None) } else { (ZenPlace::Column { before: None }, ZenKeys::default()) };
        let zen = Zen {
            id: win.id.clone(),
            workspace: win.workspace_id.clone(),
            floating: if win.is_floating { win.rect } else { None },
            place,
        };
        let rect = centred(ox, oy, ow, oh, self.store.theme.theme.space.md);
        compositor::send(Command::Zen { id: zen.id.clone(), rect, held: false, keys });
        self.zen.release = None;
        self.zen.moving = Some((zen.id.clone(), Instant::now()));
        self.zen.on = Some(zen);
        self.zen_fold();
        Ok(())
    }

    pub fn zen_exit(&mut self) -> Result<(), String> {
        if compositor::kind() == compositor::Kind::Shoji {
            if self.compositor_zen().is_none() {
                return Err("zen is off".into());
            }
            compositor::send(Command::ZenMode { on: Some(false) });
            return Ok(());
        }
        let Some(zen) = self.zen.on.take() else {
            return Err("zen is off".into());
        };
        self.zen.moving = None;
        if self.store.compositor.window(&zen.id).is_some() {
            compositor::send(Command::Unzen { id: zen.id, floating: zen.floating, place: zen.place });
        } else {
            self.zen.release = Some(zen.workspace);
            self.zen_release();
        }
        self.zen_fold();
        Ok(())
    }

    pub fn zen_changed(&mut self) {
        if compositor::kind() != compositor::Kind::Shoji {
            self.zen_follow();
            self.zen_release();
        }
        self.zen_fold();
    }

    /// Zen goes with the focus to a tiled window of its workspace, and to
    /// the one the focus lands on when the zen window closes; with nothing
    /// tiled taking it then, zen ends.
    fn zen_follow(&mut self) {
        let Some(zen) = &self.zen.on else {
            return;
        };
        let snap = &self.store.compositor.snapshot;
        let focused = snap.windows.iter().find(|w| w.id == snap.focused_window_id);
        if let Some((to, since)) = &self.zen.moving {
            if focused.is_none_or(|w| w.id != *to) && since.elapsed() < SETTLE {
                return;
            }
            self.zen.moving = None;
        }
        let gone = self.store.compositor.window(&zen.id).is_none();
        let next = focused.filter(|w| w.id != zen.id && !w.is_floating && w.workspace_id == zen.workspace).map(|w| w.id.clone());
        match next {
            Some(to) => self.zen_move(to, gone),
            None if gone => {
                let _ = self.zen_exit();
            }
            None => {}
        }
    }

    /// Zen from its window to `to` on the same workspace, the tape held
    /// throughout; the window before goes back where it was, unless it is
    /// `gone`.
    fn zen_move(&mut self, to: WindowId, gone: bool) {
        let Some((ox, oy, ow, oh)) = self.zen_output() else {
            return;
        };
        let Some(zen) = self.zen.on.take() else {
            return;
        };
        let rect = centred(ox, oy, ow, oh, self.store.theme.theme.space.md);
        let back = (!gone && zen.floating.is_none()).then_some((&zen.id, &zen.place));
        let (place, keys) = slot(&self.store.compositor.snapshot.windows, &to, back);
        if gone {
            compositor::send(Command::Zen { id: to.clone(), rect, held: true, keys });
        } else {
            compositor::send(Command::ZenSwitch { from: zen.id, floating: zen.floating, place: zen.place, to: to.clone(), rect, keys });
        }
        self.zen.moving = Some((to.clone(), Instant::now()));
        self.zen.on = Some(Zen { id: to, workspace: zen.workspace, floating: None, place });
    }

    fn zen_release(&mut self) {
        if self.zen.release.as_ref() == Some(&self.store.compositor.snapshot.focused_workspace_id) {
            self.zen.release = None;
            compositor::send(Command::ZenRelease);
        }
    }

    /// Whether zen holds the focused workspace.
    fn zen_here(&self) -> bool {
        if compositor::kind() == compositor::Kind::Shoji {
            return self.compositor_zen().is_some();
        }
        let focused = &self.store.compositor.snapshot.focused_workspace_id;
        self.zen.on.as_ref().is_some_and(|z| z.workspace == *focused)
    }

    /// The sidebar on its rail while zen holds the focused workspace, back
    /// where it was once it does not.
    fn zen_fold(&mut self) {
        match (self.zen_here(), self.zen.folded) {
            (true, None) => {
                let unfold = live!(self).bar.sidebar().is_some() && !self.store.state.data.sidebar_collapsed;
                if unfold {
                    sidebar::set_collapsed(true);
                }
                self.zen.folded = Some(unfold);
            }
            (false, Some(unfold)) => {
                if unfold {
                    sidebar::set_collapsed(false);
                }
                self.zen.folded = None;
            }
            _ => {}
        }
    }

    pub fn zen_status(&self) -> serde_json::Value {
        if compositor::kind() == compositor::Kind::Shoji {
            return match self.compositor_zen() {
                Some((id, ws)) => json!({"on": true, "windowId": id, "workspace": ws}),
                None => json!({"on": false}),
            };
        }
        match &self.zen.on {
            Some(z) => {
                let (before, mate) = match &z.place {
                    ZenPlace::Column { before } => (before.as_ref(), None),
                    ZenPlace::Stack { mate, .. } => (None, Some(mate)),
                };
                json!({"on": true, "windowId": z.id, "workspace": z.workspace, "before": before, "mate": mate, "floating": z.floating.is_some()})
            }
            None => json!({"on": false}),
        }
    }

    /// The focused output's logical box, or the first one's.
    fn zen_output(&self) -> Option<(i32, i32, i32, i32)> {
        let focused = &self.store.compositor.snapshot.focused_output_name;
        let infos: Vec<_> = self.outputs.outputs().filter_map(|o| self.outputs.info(&o)).collect();
        let info = infos.iter().find(|i| i.name.as_deref() == Some(focused.as_str())).or(infos.first())?;
        let (x, y) = info.logical_position.unwrap_or((0, 0));
        let (w, h) = info.logical_size?;
        Some((x, y, w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_on_the_output() {
        for (x, y, w, h) in [(0, 0, 2560, 1440), (1920, 0, 1536, 864), (-1280, 200, 1280, 720)] {
            let r = centred(x, y, w, h, 12.0);
            assert!((2 * r.x + r.w - (2 * x + w)).abs() <= 1, "{r:?} off centre horizontally on {w}x{h}");
            assert!((2 * r.y + r.h - (2 * y + h)).abs() <= 1, "{r:?} off centre vertically on {w}x{h}");
            assert!(r.x > x && r.y > y && r.right() < x + w && r.bottom() < y + h, "{r:?} touches an edge of {w}x{h}");
            assert!(r.w * 2 > w, "{r:?} is a sliver of {w}x{h}");
        }
    }

    fn window(id: &str, x: i32, y: i32, h: i32, floating: bool) -> Window {
        Window {
            id: id.into(),
            workspace_id: "1".into(),
            is_floating: floating,
            rect: Some(IRect::new(x, y, 100, h)),
            ..Window::default()
        }
    }

    fn keys(l: Option<&str>, r: Option<&str>, u: Option<&str>, d: Option<&str>) -> ZenKeys {
        let id = |s: Option<&str>| s.map(WindowId::from);
        ZenKeys { left: id(l), right: id(r), up: id(u), down: id(d) }
    }

    fn column(before: Option<&str>) -> ZenPlace {
        ZenPlace::Column { before: before.map(WindowId::from) }
    }

    fn stack(mate: &str, below: Option<&str>) -> ZenPlace {
        ZenPlace::Stack { mate: mate.into(), below: below.map(WindowId::from) }
    }

    #[test]
    fn a_window_goes_back_before_the_column_after_it_and_the_walk_wraps() {
        let windows = vec![window("a", 0, 0, 800, false), window("b", 500, 0, 800, false), window("c", 900, 0, 800, false), window("d", 600, 0, 800, true)];
        let at = |id: &str| slot(&windows, &id.into(), None);
        assert_eq!(at("a"), (column(Some("b")), keys(Some("c"), Some("b"), None, None)));
        assert_eq!(at("b"), (column(Some("c")), keys(Some("a"), Some("c"), None, None)));
        assert_eq!(at("c"), (column(None), keys(Some("b"), Some("a"), None, None)));
        assert_eq!(at("d"), (column(None), ZenKeys::default()), "a float has no place in the tape");
    }

    #[test]
    fn the_window_leaving_zen_is_a_column_again_before_its_own_next() {
        // b is in zen, floating, and goes back before c: from a, b is next
        // on the right and a's column goes back before b.
        let windows = vec![window("a", -1800, 0, 800, false), window("c", -1200, 0, 800, false), window("b", 200, 50, 700, true)];
        let b = WindowId::from("b");
        let before_c = column(Some("c"));
        assert_eq!(slot(&windows, &"a".into(), Some((&b, &before_c))), (column(Some("b")), keys(Some("c"), Some("b"), None, None)));
        assert_eq!(slot(&windows, &"c".into(), Some((&b, &before_c))), (column(None), keys(Some("b"), Some("a"), None, None)));
        // With none after it, b goes back last.
        let last = column(None);
        assert_eq!(slot(&windows, &"c".into(), Some((&b, &last))), (column(Some("b")), keys(Some("a"), Some("b"), None, None)));
    }

    #[test]
    fn a_stack_walks_up_and_down_and_a_column_lands_on_its_nearest_window() {
        let windows = vec![
            window("a", 0, 0, 800, false),
            window("top", 500, 0, 390, false),
            window("bottom", 500, 410, 390, false),
            window("c", 900, 0, 700, false),
        ];
        let at = |id: &str| slot(&windows, &id.into(), None);
        assert_eq!(at("top"), (stack("bottom", Some("bottom")), keys(Some("a"), Some("c"), None, Some("bottom"))));
        assert_eq!(at("bottom"), (stack("top", None), keys(Some("a"), Some("c"), Some("top"), None)));
        assert_eq!(at("c").1.left, Some("top".into()), "c's middle is nearer the top one's");
        let windows = vec![window("a", 0, 0, 800, false), window("b", 0, 0, 800, false)];
        assert_eq!(slot(&windows, &"a".into(), None).1, keys(None, None, None, Some("b")), "one column walks no side");
    }

    #[test]
    fn a_stacked_window_leaving_zen_goes_back_into_its_stack() {
        // bottom is in zen, out of its column for now; zen going up to top
        // puts it back under top, so the walk down from top reaches it.
        let windows = vec![window("a", -1800, 0, 800, false), window("top", -1200, 0, 800, false), window("bottom", 200, 50, 700, true)];
        let bottom = WindowId::from("bottom");
        let under_top = stack("top", None);
        assert_eq!(slot(&windows, &"top".into(), Some((&bottom, &under_top))), (stack("bottom", Some("bottom")), keys(Some("a"), Some("a"), None, Some("bottom"))));
        // Over a window of three: mid goes back between top and low.
        let windows = vec![window("top", 0, 0, 260, false), window("low", 0, 540, 260, false), window("mid", 200, 50, 700, true)];
        let mid = WindowId::from("mid");
        let over_low = stack("top", Some("low"));
        assert_eq!(slot(&windows, &"low".into(), Some((&mid, &over_low))), (stack("top", None), keys(None, None, Some("mid"), None)));
        // With its stack gone, it comes back as the last column.
        let windows = vec![window("a", 0, 0, 800, false), window("b", 200, 50, 700, true)];
        let b = WindowId::from("b");
        let under_gone = stack("gone", None);
        assert_eq!(slot(&windows, &"a".into(), Some((&b, &under_gone))), (column(Some("b")), keys(Some("b"), Some("b"), None, None)));
    }
}
