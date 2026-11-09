//! Zen: the focused window floating and centred on its output, with the
//! sidebar folded to its rail and, under the scrolling layout, the rest of
//! the workspace's tape slid off the output. Leaving puts the window back
//! tiled in the column place it left (or at its old rect if it was
//! floating already) and lets the tape go.

use fs_types::{IRect, Window, WindowId, WorkspaceId};
use serde_json::json;

use super::App;
use crate::services::compositor::{self, Command};
use crate::surfaces::sidebar;

#[derive(Default)]
pub struct ZenState {
    on: Option<Zen>,
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
fn centred(x: i64, y: i64, w: i64, h: i64, margin: f64) -> IRect {
    let (wf, hf) = (w as f64, h as f64);
    let width = (wf * 0.6).max((wf - 2.0 * margin).min(1100.0));
    let height = hf - 2.0 * (hf * 0.06).max(margin);
    IRect::new(
        x + ((wf - width) / 2.0).round() as i64,
        y + ((hf - height) / 2.0).round() as i64,
        width.round() as i64,
        height.round() as i64,
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
        if self.zen.on.is_some() { self.zen_exit() } else { self.zen_enter() }
    }

    pub fn zen_enter(&mut self) -> Result<(), String> {
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

    pub fn zen_status(&self) -> serde_json::Value {
        match &self.zen.on {
            Some(z) => json!({"on": true, "windowId": z.id, "workspace": z.workspace, "before": z.before, "floating": z.floating.is_some()}),
            None => json!({"on": false}),
        }
    }

    /// The focused output's logical box, or the first one's.
    fn zen_output(&self) -> Option<(i64, i64, i64, i64)> {
        let focused = &self.store.compositor.snapshot.focused_output_name;
        let infos: Vec<_> = self.outputs.outputs().filter_map(|o| self.outputs.info(&o)).collect();
        let info = infos.iter().find(|i| i.name.as_deref() == Some(focused.as_str())).or(infos.first())?;
        let (x, y) = info.logical_position.unwrap_or((0, 0));
        let (w, h) = info.logical_size?;
        Some((x.into(), y.into(), w.into(), h.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_on_the_output() {
        assert_eq!(centred(0, 0, 2560, 1440, 12.0), IRect::new(512, 86, 1536, 1267));
        // A laptop's 1536 wide output keeps 1100.
        assert_eq!(centred(1920, 0, 1536, 864, 12.0), IRect::new(1920 + 218, 52, 1100, 760));
    }

    #[test]
    fn next_column_by_x() {
        let w = |id: &str, x: i64, floating: bool| Window {
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
