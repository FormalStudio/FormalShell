//! Zen: the focused window floating and centred on its output, with the
//! sidebar folded to its rail and, under the scrolling layout, the rest of
//! the workspace's tape slid off the output. Leaving puts the window back
//! tiled in the column place it left (or at its old rect if it was
//! floating already) and lets the tape go.
//!
//! Under a compositor that owns zen (Shoji) the shell only asks for it and
//! reads each workspace's zen window back, folding the sidebar while the
//! focused workspace holds one.

use fs_types::{IRect, Window, WindowId, WorkspaceId};
use serde_json::json;

use super::App;
use crate::services::compositor::{self, Command};
use crate::surfaces::sidebar;

#[derive(Default)]
pub struct ZenState {
    on: Option<Zen>,
    /// While the compositor's zen holds the focused workspace: whether the
    /// sidebar was open, so leaving opens it again.
    folded: Option<bool>,
    /// A workspace whose tape is still held after its zen window closed
    /// there; let go once it is focused, the only workspace a layout
    /// message reaches.
    release: Option<WorkspaceId>,
}

struct Zen {
    id: WindowId,
    workspace: WorkspaceId,
    /// Its rect before, for a window that was floating already.
    floating: Option<IRect>,
    /// A window of the column it sat before, under a scrolling layout.
    before: Option<WindowId>,
    /// The sidebar was open, so leaving opens it again.
    unfold: bool,
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

/// The column after `id`'s on its workspace, as one of its windows: the
/// tiled windows told apart by their x on the tape.
fn next_column(windows: &[Window], id: &WindowId, workspace: &WorkspaceId) -> Option<WindowId> {
    let x = windows.iter().find(|w| w.id == *id)?.rect?.x;
    windows
        .iter()
        .filter(|w| w.workspace_id == *workspace && !w.is_floating && w.id != *id)
        .filter_map(|w| Some((w.rect?.x, &w.id)))
        .filter(|(wx, _)| *wx > x)
        .min_by_key(|(wx, _)| *wx)
        .map(|(_, id)| id.clone())
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
        let zen = Zen {
            id: win.id.clone(),
            workspace: win.workspace_id.clone(),
            floating: if win.is_floating { win.rect } else { None },
            before: if columns && !win.is_floating { next_column(&snap.windows, &win.id, &win.workspace_id) } else { None },
            unfold: live!(self).bar.sidebar().is_some() && !self.store.state.data.sidebar_collapsed,
        };
        let rect = centred(ox, oy, ow, oh, self.store.theme.theme.space.md);
        compositor::send(Command::Zen { id: zen.id.clone(), rect });
        if zen.unfold {
            sidebar::set_collapsed(true);
        }
        self.zen.release = None;
        self.zen.on = Some(zen);
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
        if self.store.compositor.window(&zen.id).is_some() {
            compositor::send(Command::Unzen { id: zen.id, floating: zen.floating, before: zen.before });
        } else {
            self.zen.release = Some(zen.workspace);
            self.zen_release();
        }
        if zen.unfold {
            sidebar::set_collapsed(false);
        }
        Ok(())
    }

    /// The zen window closing ends zen, the sidebar opening again.
    pub fn zen_changed(&mut self) {
        if compositor::kind() == compositor::Kind::Shoji {
            self.zen_fold();
            return;
        }
        if self.zen.on.as_ref().is_some_and(|z| self.store.compositor.window(&z.id).is_none()) {
            let _ = self.zen_exit();
        }
        self.zen_release();
    }

    fn zen_release(&mut self) {
        if self.zen.release.as_ref() == Some(&self.store.compositor.snapshot.focused_workspace_id) {
            self.zen.release = None;
            compositor::send(Command::ZenRelease);
        }
    }

    /// The sidebar on its rail while the compositor holds the focused
    /// workspace in zen, back where it was once it lets go.
    fn zen_fold(&mut self) {
        match (self.compositor_zen().is_some(), self.zen.folded) {
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
            Some(z) => json!({"on": true, "windowId": z.id, "workspace": z.workspace, "before": z.before, "floating": z.floating.is_some()}),
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

    #[test]
    fn next_column_by_x() {
        let w = |id: &str, x: i32, floating: bool| Window {
            id: id.into(),
            workspace_id: "1".into(),
            is_floating: floating,
            rect: Some(IRect::new(x, 0, 100, 100)),
            ..Window::default()
        };
        let windows = vec![w("a", 0, false), w("b", 500, false), w("c", 900, false), w("d", 600, true)];
        let next = |id: &str| next_column(&windows, &id.into(), &"1".into());
        assert_eq!(next("a"), Some("b".into()));
        assert_eq!(next("c"), None);
    }
}
