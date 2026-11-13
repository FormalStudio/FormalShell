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

use fs_types::{IRect, Window, WindowId, WorkspaceId};
use serde_json::json;

use super::App;
use crate::services::compositor::{self, Command, Snapshot, ZenKeys, ZenPlace};
use crate::surfaces::sidebar;

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
    /// The `seq` of the last zen write sent.
    seq: u64,
    /// The zen write not yet reported done. The focus moves it makes on
    /// the way (the window going back focused before the new one is) are
    /// not the user's, so zen goes by the focus again only after it.
    pending: Option<u64>,
    /// The config reloads seen: a reload drops the focus binds' zen binds.
    reloads: u64,
}

struct Zen {
    id: WindowId,
    workspace: WorkspaceId,
    /// Its rect before, for a window that was floating already.
    floating: Option<IRect>,
    /// Out of a scrolling layout's tape: where it goes back, kept up to
    /// date as the windows around it change.
    spot: Option<Spot>,
    /// Where the focus binds go from it, as last sent.
    keys: ZenKeys,
}

/// Where a window out of its tape for zen sits in it, by the windows
/// around it, so it keeps its place when some of them close or leave: its
/// column's windows above and below it, nearest first, and the windows of
/// the columns after its own, in tape order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Spot {
    above: Vec<WindowId>,
    below: Vec<WindowId>,
    after: Vec<WindowId>,
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

type Columns<'a> = Vec<Vec<(IRect, &'a WindowId)>>;

/// The tiled windows of `workspace` but `skip`, in columns told apart by x
/// and stacked by y.
fn columns<'a>(windows: &'a [Window], workspace: &WorkspaceId, skip: &WindowId) -> Columns<'a> {
    let mut tiled: Vec<(IRect, &WindowId)> = windows
        .iter()
        .filter(|w| w.workspace_id == *workspace && !w.is_floating && w.id != *skip)
        .filter_map(|w| Some((w.rect?, &w.id)))
        .collect();
    tiled.sort_by_key(|(r, _)| (r.x, r.y));
    let mut columns: Columns = Vec::new();
    for (rect, id) in tiled {
        match columns.last_mut() {
            Some(column) if column[0].0.x == rect.x => column.push((rect, id)),
            _ => columns.push(vec![(rect, id)]),
        }
    }
    columns
}

enum Located {
    /// Into column `.0` at row `.1`.
    Stack(usize, usize),
    /// A column of its own at `.0`.
    Column(usize),
}

/// Where `spot` puts its window in `columns`: in the column of a window
/// that was stacked with it, above the nearest one that was below it, or
/// failing that before the first column after it still there.
fn locate(columns: &Columns, spot: &Spot) -> Located {
    let column_of = |w: &WindowId| columns.iter().position(|c| c.iter().any(|(_, x)| *x == w));
    match spot.above.iter().chain(&spot.below).find_map(column_of) {
        Some(c) => {
            let row = spot.below.iter().find_map(|w| columns[c].iter().position(|(_, x)| x == &w)).unwrap_or(columns[c].len());
            Located::Stack(c, row)
        }
        None => Located::Column(spot.after.iter().find_map(column_of).unwrap_or(columns.len())),
    }
}

/// `spot` as the windows the compositor puts `id` back by.
fn place(windows: &[Window], id: &WindowId, spot: &Spot) -> ZenPlace {
    let Some(me) = windows.iter().find(|w| w.id == *id) else {
        return ZenPlace::Column { before: None };
    };
    let columns = columns(windows, &me.workspace_id, id);
    match locate(&columns, spot) {
        Located::Stack(c, row) => ZenPlace::Stack { mate: columns[c][0].1.clone(), below: columns[c].get(row).map(|(_, w)| (*w).clone()) },
        Located::Column(at) => ZenPlace::Column { before: columns.get(at).map(|c| c[0].1.clone()) },
    }
}

/// The tape of `id`'s workspace with `back`, a window out of it for zen,
/// back at its spot: where `id` sits in it, and where each focus move goes
/// from `id`. A move to a column lands on its window nearest `id`'s
/// middle. `id` is tiled, or `back` itself.
fn slot(windows: &[Window], id: &WindowId, back: Option<(&WindowId, &Spot)>) -> (Option<Spot>, ZenKeys) {
    let Some(me) = windows.iter().find(|w| w.id == *id && (!w.is_floating || back.is_some_and(|(b, _)| b == id))) else {
        return (None, ZenKeys::default());
    };
    let nobody = WindowId::default();
    let mut columns = columns(windows, &me.workspace_id, back.map_or(&nobody, |(b, _)| b));
    if let Some((b, spot)) = back {
        // Where it is drawn, centred in zen: the nearest window of a column
        // beside it goes by its middle, in a stack of its own or not.
        let rect = windows.iter().find(|w| w.id == *b).and_then(|w| w.rect).unwrap_or_default();
        match locate(&columns, spot) {
            Located::Stack(c, row) => columns[c].insert(row, (rect, b)),
            Located::Column(at) => columns.insert(at, vec![(rect, b)]),
        }
    }
    let Some((column, row)) = columns.iter().enumerate().find_map(|(c, col)| col.iter().position(|(_, w)| *w == id).map(|r| (c, r))) else {
        return (None, ZenKeys::default());
    };
    let mates = &columns[column];
    let middle = |r: &IRect| r.y + r.h / 2;
    let centre = middle(&mates[row].0);
    let nearest = |c: usize| columns[c].iter().min_by_key(|(r, _)| (middle(r) - centre).abs()).map(|(_, w)| (*w).clone());
    let n = columns.len();
    let ids = |cells: &[(IRect, &WindowId)]| cells.iter().map(|(_, w)| (*w).clone()).collect::<Vec<_>>();
    let spot = Spot {
        above: ids(&mates[..row]).into_iter().rev().collect(),
        below: ids(&mates[row + 1..]),
        after: columns[column + 1..].iter().flat_map(|c| ids(c)).collect(),
    };
    let keys = ZenKeys {
        left: (n > 1).then(|| nearest((column + n - 1) % n)).flatten(),
        right: (n > 1).then(|| nearest((column + 1) % n)).flatten(),
        up: row.checked_sub(1).map(|r| mates[r].1.clone()),
        down: mates.get(row + 1).map(|(_, w)| (*w).clone()),
    };
    (Some(spot), keys)
}

/// Zen's writes, worked out from the compositor's snapshots alone.
impl ZenState {
    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.pending = Some(self.seq);
        self.seq
    }

    /// Zen on the focused window, centred at `rect`.
    fn enter(&mut self, snap: &Snapshot, rect: Option<IRect>) -> Result<Vec<Command>, String> {
        if self.on.is_some() {
            return Err("zen is on".into());
        }
        let Some(win) = snap.windows.iter().find(|w| w.id == snap.focused_window_id) else {
            return Err("no focused window".into());
        };
        let rect = rect.ok_or("no output")?;
        let tape = !win.is_floating && snap.workspaces.iter().any(|w| w.id == win.workspace_id && w.columns);
        let (spot, keys) = if tape { slot(&snap.windows, &win.id, None) } else { (None, ZenKeys::default()) };
        let id = win.id.clone();
        self.on = Some(Zen {
            id: id.clone(),
            workspace: win.workspace_id.clone(),
            floating: if win.is_floating { win.rect } else { None },
            spot,
            keys: keys.clone(),
        });
        self.release = None;
        let seq = self.next_seq();
        Ok(vec![Command::Zen { id, rect, held: false, keys, seq }])
    }

    fn exit(&mut self, snap: &Snapshot) -> Result<Vec<Command>, String> {
        let Some(zen) = self.on.take() else {
            return Err("zen is off".into());
        };
        self.pending = None;
        if snap.windows.iter().any(|w| w.id == zen.id) {
            let place = zen.spot.as_ref().map(|spot| place(&snap.windows, &zen.id, spot));
            Ok(vec![Command::Unzen { id: zen.id, floating: zen.floating, place }])
        } else {
            self.release = Some(zen.workspace);
            Ok(self.released(snap))
        }
    }

    /// A new snapshot, `reloads` the config reloads so far.
    fn changed(&mut self, snap: &Snapshot, reloads: u64, rect: Option<IRect>) -> Vec<Command> {
        let mut out = self.follow(snap, rect, reloads);
        out.extend(self.released(snap));
        out
    }

    /// Zen goes with the focus to a tiled window of its workspace, and to
    /// the one the focus lands on when the zen window closes; with nothing
    /// tiled taking it then, zen ends. Staying put, it keeps its place and
    /// its keys up to date.
    fn follow(&mut self, snap: &Snapshot, rect: Option<IRect>, reloads: u64) -> Vec<Command> {
        let Some(zen) = &self.on else {
            self.reloads = reloads;
            return Vec::new();
        };
        // A reload while a write is pending stays unseen until it is done:
        // it may have dropped the binds that write put in.
        if let Some(seq) = self.pending {
            if snap.zen_done < seq {
                return Vec::new();
            }
            self.pending = None;
        }
        let reloaded = std::mem::replace(&mut self.reloads, reloads) != reloads;
        let gone = !snap.windows.iter().any(|w| w.id == zen.id);
        let next = snap
            .windows
            .iter()
            .find(|w| w.id == snap.focused_window_id && w.id != zen.id && !w.is_floating && w.workspace_id == zen.workspace)
            .map(|w| w.id.clone());
        match next {
            Some(to) => self.switch(snap, to, gone, rect),
            None if gone => self.exit(snap).unwrap_or_default(),
            None => self.keep(snap, reloaded),
        }
    }

    /// Zen from its window to `to` on the same workspace, the tape held
    /// throughout; the window before goes back where it was, unless it is
    /// `gone`.
    fn switch(&mut self, snap: &Snapshot, to: WindowId, gone: bool, rect: Option<IRect>) -> Vec<Command> {
        let Some(rect) = rect else {
            return Vec::new();
        };
        let Some(zen) = self.on.take() else {
            return Vec::new();
        };
        let back = zen.spot.as_ref().filter(|_| !gone).map(|spot| (&zen.id, spot));
        let (spot, keys) = slot(&snap.windows, &to, back);
        let seq = self.next_seq();
        let command = if gone {
            Command::Zen { id: to.clone(), rect, held: true, keys: keys.clone(), seq }
        } else {
            let place = zen.spot.as_ref().map(|spot| place(&snap.windows, &zen.id, spot));
            Command::ZenSwitch { from: zen.id.clone(), floating: zen.floating, place, to: to.clone(), rect, keys: keys.clone(), seq }
        };
        self.on = Some(Zen { id: to, workspace: zen.workspace, floating: None, spot, keys });
        vec![command]
    }

    /// The zen window staying: its spot and keys follow the windows around
    /// it closing, leaving or moving, and the keys go again after a reload.
    fn keep(&mut self, snap: &Snapshot, reloaded: bool) -> Vec<Command> {
        let Some(zen) = self.on.as_mut() else {
            return Vec::new();
        };
        let Some(spot) = &zen.spot else {
            return Vec::new();
        };
        let (spot, keys) = slot(&snap.windows, &zen.id, Some((&zen.id, spot)));
        if let Some(spot) = spot {
            zen.spot = Some(spot);
        }
        if keys == zen.keys && !reloaded {
            return Vec::new();
        }
        zen.keys = keys.clone();
        vec![Command::ZenKeys { id: zen.id.clone(), keys }]
    }

    fn released(&mut self, snap: &Snapshot) -> Vec<Command> {
        if self.release.as_ref() == Some(&snap.focused_workspace_id) {
            self.release = None;
            return vec![Command::ZenRelease];
        }
        Vec::new()
    }
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
        let rect = self.zen_rect();
        let commands = self.zen.enter(&self.store.compositor.snapshot, rect)?;
        commands.into_iter().for_each(compositor::send);
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
        let commands = self.zen.exit(&self.store.compositor.snapshot)?;
        commands.into_iter().for_each(compositor::send);
        self.zen_fold();
        Ok(())
    }

    pub fn zen_changed(&mut self) {
        if compositor::kind() != compositor::Kind::Shoji {
            let rect = self.zen_rect();
            let compositor = &self.store.compositor;
            self.zen.changed(&compositor.snapshot, compositor.config_reloads, rect).into_iter().for_each(compositor::send);
        }
        self.zen_fold();
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
                let place = z.spot.as_ref().map(|spot| place(&self.store.compositor.snapshot.windows, &z.id, spot));
                let (before, mate) = match &place {
                    Some(ZenPlace::Column { before }) => (before.as_ref(), None),
                    Some(ZenPlace::Stack { mate, .. }) => (None, Some(mate)),
                    None => (None, None),
                };
                json!({"on": true, "windowId": z.id, "workspace": z.workspace, "before": before, "mate": mate, "floating": z.floating.is_some()})
            }
            None => json!({"on": false}),
        }
    }

    /// Where the zen window goes: centred on the focused output.
    fn zen_rect(&self) -> Option<IRect> {
        let (x, y, w, h) = self.zen_output()?;
        Some(centred(x, y, w, h, self.store.theme.theme.space.md))
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
    use fs_types::Workspace;

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

    fn ids(list: &[&str]) -> Vec<WindowId> {
        list.iter().map(|s| WindowId::from(*s)).collect()
    }

    fn spot(above: &[&str], below: &[&str], after: &[&str]) -> Spot {
        Spot { above: ids(above), below: ids(below), after: ids(after) }
    }

    #[test]
    fn a_window_goes_back_before_the_column_after_it_and_the_walk_wraps() {
        let windows = vec![window("a", 0, 0, 800, false), window("b", 500, 0, 800, false), window("c", 900, 0, 800, false), window("d", 600, 0, 800, true)];
        let at = |id: &str| slot(&windows, &id.into(), None);
        assert_eq!(at("a"), (Some(spot(&[], &[], &["b", "c"])), keys(Some("c"), Some("b"), None, None)));
        assert_eq!(at("b"), (Some(spot(&[], &[], &["c"])), keys(Some("a"), Some("c"), None, None)));
        assert_eq!(at("c"), (Some(spot(&[], &[], &[])), keys(Some("b"), Some("a"), None, None)));
        assert_eq!(at("d"), (None, ZenKeys::default()), "a float has no place in the tape");
    }

    #[test]
    fn the_window_leaving_zen_is_a_column_again_before_its_own_next() {
        // b is in zen, floating, and goes back before c: from a, b is next
        // on the right and a's column goes back before b.
        let windows = vec![window("a", -1800, 0, 800, false), window("c", -1200, 0, 800, false), window("b", 200, 50, 700, true)];
        let b = WindowId::from("b");
        let before_c = spot(&[], &[], &["c"]);
        assert_eq!(slot(&windows, &"a".into(), Some((&b, &before_c))), (Some(spot(&[], &[], &["b", "c"])), keys(Some("c"), Some("b"), None, None)));
        assert_eq!(slot(&windows, &"c".into(), Some((&b, &before_c))), (Some(spot(&[], &[], &[])), keys(Some("b"), Some("a"), None, None)));
        assert_eq!(place(&windows, &b, &before_c), ZenPlace::Column { before: Some("c".into()) });
        // With none after it, b goes back last.
        let last = spot(&[], &[], &[]);
        assert_eq!(slot(&windows, &"c".into(), Some((&b, &last))), (Some(spot(&[], &[], &["b"])), keys(Some("a"), Some("b"), None, None)));
        assert_eq!(place(&windows, &b, &last), ZenPlace::Column { before: None });
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
        assert_eq!(at("top"), (Some(spot(&[], &["bottom"], &["c"])), keys(Some("a"), Some("c"), None, Some("bottom"))));
        assert_eq!(at("bottom"), (Some(spot(&["top"], &[], &["c"])), keys(Some("a"), Some("c"), Some("top"), None)));
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
        let under_top = spot(&["top"], &[], &[]);
        assert_eq!(slot(&windows, &"top".into(), Some((&bottom, &under_top))).1, keys(Some("a"), Some("a"), None, Some("bottom")));
        assert_eq!(place(&windows, &bottom, &under_top), ZenPlace::Stack { mate: "top".into(), below: None });
        // Out of a stack of three: mid goes back between top and low.
        let windows = vec![window("top", 0, 0, 260, false), window("low", 0, 540, 260, false), window("mid", 200, 50, 700, true)];
        let mid = WindowId::from("mid");
        let between = spot(&["top"], &["low"], &[]);
        assert_eq!(slot(&windows, &"low".into(), Some((&mid, &between))), (Some(spot(&["mid", "top"], &[], &[])), keys(None, None, Some("mid"), None)));
        assert_eq!(place(&windows, &mid, &between), ZenPlace::Stack { mate: "top".into(), below: Some("low".into()) });
        // With its stack gone, it comes back before the column after it.
        let windows = vec![window("a", 0, 0, 800, false), window("b", 200, 50, 700, true)];
        let b = WindowId::from("b");
        let gone = spot(&["gone"], &[], &["a"]);
        assert_eq!(slot(&windows, &"a".into(), Some((&b, &gone))).1, keys(Some("b"), Some("b"), None, None));
        assert_eq!(place(&windows, &b, &gone), ZenPlace::Column { before: Some("a".into()) });
    }

    fn snap(windows: Vec<Window>, focused: &str, done: u64) -> Snapshot {
        Snapshot {
            workspaces: vec![
                Workspace { id: "1".into(), columns: true, ..Workspace::default() },
                Workspace { id: "2".into(), columns: true, ..Workspace::default() },
            ],
            windows,
            focused_window_id: focused.into(),
            focused_workspace_id: "1".into(),
            zen_done: done,
            ..Snapshot::default()
        }
    }

    /// Columns at 0, 500, 900 and 1300, the ones `zen` names floating.
    fn tape(names: &[&str], zen: &str) -> Vec<Window> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| if *n == zen { window(n, 300, 50, 700, true) } else { window(n, [0, 500, 900, 1300][i], 0, 800, false) })
            .collect()
    }

    const RECT: Option<IRect> = Some(IRect { x: 300, y: 50, w: 1100, h: 700 });

    fn shown(commands: &[Command]) -> Vec<&'static str> {
        commands
            .iter()
            .map(|c| match c {
                Command::Zen { .. } => "Zen",
                Command::ZenSwitch { .. } => "ZenSwitch",
                Command::ZenKeys { .. } => "ZenKeys",
                Command::Unzen { .. } => "Unzen",
                Command::ZenRelease => "ZenRelease",
                _ => "other",
            })
            .collect()
    }

    fn switches(commands: &[Command]) -> usize {
        commands.iter().filter(|c| matches!(c, Command::ZenSwitch { .. })).count()
    }

    #[test]
    fn a_switch_goes_by_the_focus_only_once_the_compositor_has_carried_it_out() {
        let mut zen = ZenState::default();
        let sent = zen.enter(&snap(tape(&["a", "b", "c"], ""), "a", 0), RECT).unwrap();
        assert!(matches!(&sent[..], [Command::Zen { seq: 1, .. }]));
        assert!(zen.changed(&snap(tape(&["a", "b", "c"], "a"), "a", 1), 0, RECT).is_empty(), "zen on a, carried out");

        let mut sent = zen.changed(&snap(tape(&["a", "b", "c"], "a"), "b", 1), 0, RECT);
        assert!(matches!(&sent[..], [Command::ZenSwitch { seq: 2, .. }]), "{:?}", shown(&sent));
        // Still b focused before the switch runs, then a focused on the
        // way as it goes back into the tape: neither is the user's.
        sent.extend(zen.changed(&snap(tape(&["a", "b", "c"], "a"), "b", 1), 0, RECT));
        sent.extend(zen.changed(&snap(tape(&["a", "b", "c"], ""), "a", 1), 0, RECT));
        sent.extend(zen.changed(&snap(tape(&["a", "b", "c"], ""), "b", 1), 0, RECT));
        sent.extend(zen.changed(&snap(tape(&["a", "b", "c"], "b"), "b", 2), 0, RECT));
        assert_eq!(switches(&sent), 1, "{:?}", shown(&sent));
        assert_eq!(sent.len(), 1, "{:?}", shown(&sent));
        assert_eq!(zen.on.as_ref().map(|z| z.id.clone()), Some("b".into()));
        // Done, the focus is the user's again.
        assert_eq!(switches(&zen.changed(&snap(tape(&["a", "b", "c"], "b"), "c", 2), 0, RECT)), 1);
    }

    #[test]
    fn a_target_closing_before_the_switch_is_carried_out_ends_zen_on_the_report() {
        let mut zen = ZenState::default();
        zen.enter(&snap(tape(&["a", "b", "c"], ""), "a", 0), RECT).unwrap();
        zen.changed(&snap(tape(&["a", "b", "c"], "a"), "a", 1), 0, RECT);
        zen.changed(&snap(tape(&["a", "b", "c"], "a"), "b", 1), 0, RECT);
        let no_b = |done| snap(tape(&["a", "c"], ""), "", done);
        assert!(zen.changed(&no_b(1), 0, RECT).is_empty(), "b closed before the switch ran");
        // The report of the switch is all it takes: no further event.
        let sent = zen.changed(&no_b(2), 0, RECT);
        assert!(matches!(&sent[..], [Command::ZenRelease]), "{:?}", shown(&sent));
        assert!(zen.on.is_none());
        // With a tile focused instead, zen goes on to it.
        let mut zen = ZenState::default();
        zen.enter(&snap(tape(&["a", "b", "c"], ""), "a", 0), RECT).unwrap();
        zen.changed(&snap(tape(&["a", "b", "c"], "a"), "b", 1), 0, RECT);
        let sent = zen.changed(&snap(tape(&["a", "c"], ""), "c", 2), 0, RECT);
        assert!(matches!(&sent[..], [Command::Zen { held: true, seq: 3, .. }]), "{:?}", shown(&sent));
    }

    #[test]
    fn a_reload_before_a_switch_is_reported_done_sends_the_keys_once_it_is() {
        let mut zen = ZenState::default();
        zen.enter(&snap(tape(&["a", "b", "c"], ""), "a", 0), RECT).unwrap();
        zen.changed(&snap(tape(&["a", "b", "c"], "a"), "a", 1), 0, RECT);
        zen.changed(&snap(tape(&["a", "b", "c"], "a"), "b", 1), 0, RECT);
        // The switch has put b's keys in; the reload drops them before the
        // snapshot that reports the switch done.
        assert!(zen.changed(&snap(tape(&["a", "b", "c"], "b"), "b", 1), 1, RECT).is_empty());
        let sent = zen.changed(&snap(tape(&["a", "b", "c"], "b"), "b", 2), 1, RECT);
        assert!(matches!(&sent[..], [Command::ZenKeys { id, keys: k }] if id.as_str() == "b" && *k == keys(Some("a"), Some("c"), None, None)), "{:?}", shown(&sent));
        assert!(zen.changed(&snap(tape(&["a", "b", "c"], "b"), "b", 2), 1, RECT).is_empty(), "sent once");
    }

    #[test]
    fn a_column_beside_the_zen_window_lands_on_its_window_nearest_the_middle() {
        // b full height with top, mid and low stacked left of it: H goes
        // to mid, on entering and once b floats centred.
        let stack = |b: Window| vec![window("top", 0, 0, 260, false), window("mid", 0, 270, 260, false), window("low", 0, 540, 260, false), b];
        let mut zen = ZenState::default();
        let sent = zen.enter(&snap(stack(window("b", 500, 0, 800, false)), "b", 0), RECT).unwrap();
        assert!(matches!(&sent[..], [Command::Zen { keys: k, .. }] if k.left.as_ref().is_some_and(|l| l.as_str() == "mid")), "{:?}", shown(&sent));
        let sent = zen.changed(&snap(stack(window("b", 300, 50, 700, true)), "b", 1), 0, RECT);
        assert!(sent.is_empty(), "b's keys stay as they were: {:?}", shown(&sent));
        assert_eq!(zen.on.as_ref().and_then(|z| z.keys.left.clone()), Some("mid".into()));
    }

    #[test]
    fn a_stacked_zen_window_goes_by_its_own_middle_too() {
        // b between a and c, with top, mid and low stacked left of them: H
        // goes to mid, on entering and once b floats centred.
        let tape = |b: Window| {
            vec![
                window("top", 0, 0, 260, false),
                window("mid", 0, 270, 260, false),
                window("low", 0, 540, 260, false),
                window("a", 500, 0, 260, false),
                b,
                window("c", 500, 540, 260, false),
            ]
        };
        let mut zen = ZenState::default();
        let sent = zen.enter(&snap(tape(window("b", 500, 270, 260, false)), "b", 0), RECT).unwrap();
        assert!(matches!(&sent[..], [Command::Zen { keys: k, .. }] if k.left.as_ref().is_some_and(|l| l.as_str() == "mid")), "{:?}", shown(&sent));
        let sent = zen.changed(&snap(tape(window("b", 300, 50, 700, true)), "b", 1), 0, RECT);
        assert!(sent.is_empty(), "b's keys stay as they were: {:?}", shown(&sent));
        assert_eq!(zen.on.as_ref().and_then(|z| z.keys.left.clone()), Some("mid".into()));
    }

    #[test]
    fn the_keys_and_the_way_back_follow_the_neighbours_while_zen_stays() {
        let mut zen = ZenState::default();
        zen.enter(&snap(tape(&["a", "b", "c", "d"], ""), "b", 0), RECT).unwrap();
        assert!(zen.changed(&snap(tape(&["a", "b", "c", "d"], "b"), "b", 1), 0, RECT).is_empty());

        // c closes: right goes to d, and b goes back before d.
        let sent = zen.changed(&snap(tape(&["a", "b", "d"], "b"), "b", 1), 0, RECT);
        assert!(matches!(&sent[..], [Command::ZenKeys { id, keys: k }] if id.as_str() == "b" && *k == keys(Some("a"), Some("d"), None, None)), "{:?}", shown(&sent));
        let back = |zen: &ZenState, windows: &[Window]| zen.on.as_ref().and_then(|z| Some(place(windows, &z.id, z.spot.as_ref()?)));
        assert_eq!(back(&zen, &tape(&["a", "b", "d"], "b")), Some(ZenPlace::Column { before: Some("d".into()) }));
        // d moves to workspace 2: the walk wraps to a both ways.
        let mut windows = tape(&["a", "b", "d"], "b");
        windows[2].workspace_id = "2".into();
        let sent = zen.changed(&snap(windows.clone(), "b", 1), 0, RECT);
        assert!(matches!(&sent[..], [Command::ZenKeys { keys: k, .. }] if *k == keys(Some("a"), Some("a"), None, None)), "{:?}", shown(&sent));
        assert!(zen.changed(&snap(windows.clone(), "b", 1), 0, RECT).is_empty(), "nothing new, nothing sent");
        // A reload drops the binds: the same keys go again, no handover.
        let sent = zen.changed(&snap(windows.clone(), "b", 1), 1, RECT);
        assert!(matches!(&sent[..], [Command::ZenKeys { keys: k, .. }] if *k == keys(Some("a"), Some("a"), None, None)), "{:?}", shown(&sent));
        assert_eq!(zen.on.as_ref().map(|z| z.id.clone()), Some("b".into()));
        let sent = zen.exit(&snap(windows, "b", 1)).unwrap();
        assert!(matches!(&sent[..], [Command::Unzen { place: Some(ZenPlace::Column { before: None }), .. }]), "{:?}", shown(&sent));
    }
}
