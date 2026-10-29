// Portions from omarchy-spaces (MIT, Copyright 2026 Tornike Gomareli)

//! The Spaces cell's workspace preview:
//! a card off a workspace's chip, under a header naming the workspace and its
//! window count, holding a miniature of the output with each window at its
//! own place and full size, and a footer naming the window under the cursor.
//! Windows past the output's edge sit outside the viewport, which scrolls
//! over the union of the output and every window and opens on the output's
//! own region. Enter or a click focuses the window.
//!
//! Opened from the pointer it takes no keyboard (`takes_keyboard`), so the
//! bar keeps its hover; `workspaces peek` opens an ordinary panel. Not in
//! `PANELS`: with no workspace to show there is nothing to open.
//!
//! The thumbnails are the shell's window captures, which live on the App;
//! they reach the card through [`Shared`], and the card leaves what it laid
//! out there for the captures and for `workspaces status`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fs_chrome::bar::workspaces::{PreviewLayout, preview_layout};
use fs_chrome::types::Rect;

use super::{Effect, Panel, View};
use crate::scene::Bitmap;
use crate::services::compositor;
use crate::store::Topic;
use crate::surfaces::bar::cells::workspaces::window;
use crate::ui::el::{Pic, Tile};
use crate::ui::{El, Event, Ink, What, w};

/// The appicon slice's key for a window's icon here.
pub fn icon_key(id: &str) -> String {
    format!("preview:{id}")
}

/// What the card laid out last, for the status and the captures.
#[derive(Clone, Debug, Default)]
pub struct Laid {
    pub plan: Option<PreviewLayout>,
    pub area: (f64, f64),
    pub view: (f64, f64),
    pub content: (f64, f64),
    pub scale: f64,
    pub inset: f64,
    /// Each tile's id, its box in the content and the window's own rect.
    pub tiles: Vec<(String, (f64, f64, f64, f64), Option<Rect>)>,
}

#[derive(Default)]
pub struct Shared {
    pub thumbs: HashMap<String, Bitmap>,
    pub laid: Laid,
    /// The miniature's scroll, and whether anything has moved it since it
    /// opened on the output's region.
    pub scroll: (f64, f64),
    pub scrolled: bool,
}

pub struct WorkspacePreview {
    pub workspace_id: String,
    pub idx: i64,
    label: String,
    pub from_pointer: bool,
    pub shared: Rc<RefCell<Shared>>,
}

impl WorkspacePreview {
    pub fn new(workspace_id: String, idx: i64, label: String, from_pointer: bool, shared: Rc<RefCell<Shared>>) -> Self {
        *shared.borrow_mut() = Shared::default();
        Self { workspace_id, idx, label, from_pointer, shared }
    }

    fn windows<'a>(&self, v: &View<'a>) -> Vec<&'a compositor::Window> {
        v.store.compositor.snapshot.windows.iter().filter(|w| w.workspace_id == self.workspace_id).collect()
    }

    fn focus(&self, id: &str, fx: &mut Effect) {
        if !id.is_empty() {
            compositor::focus_window(id);
            fx.close = true;
        }
    }
}

impl Panel for WorkspacePreview {
    fn id(&self) -> &'static str {
        "workspaces"
    }

    fn title(&self, _: &View) -> String {
        format!("Workspace {}", self.label)
    }

    fn icon(&self, _: &View) -> String {
        "layout-grid".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Compositor, Topic::AppIcon]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_wide
    }

    fn takes_keyboard(&self) -> bool {
        !self.from_pointer
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let n = self.windows(v).len();
        vec![w::section_label(&v.theme.space, &format!("{n} {}", if n == 1 { "window" } else { "windows" }), None, false)]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let windows = self.windows(v);
        let mini_w = (self.width(v) - s.panel_padding * 2.0).max(0.0);
        let (aw, ah) = v.output;
        let mini_h = if aw > 0.0 { (mini_w * ah / aw).round() } else { (mini_w * 9.0 / 16.0).round() };
        let inset = s.xs;
        let (vw, vh) = (mini_w - inset * 2.0, mini_h - inset * 2.0);
        let area = (aw > 0.0 && ah > 0.0).then_some(Rect { x: 0.0, y: 0.0, width: aw, height: ah });
        let model: Vec<_> = windows.iter().map(|w| window(w)).collect();
        let plan = preview_layout(&model, area, vw, vh, s.control_height);
        let content = (vw.max(plan.bounds.width), vh.max(plan.bounds.height));
        let mut shared = self.shared.borrow_mut();
        if !shared.scrolled {
            shared.scroll = (plan.home.x, plan.home.y);
        }
        shared.scroll = (shared.scroll.0.clamp(0.0, (content.0 - vw).max(0.0)), shared.scroll.1.clamp(0.0, (content.1 - vh).max(0.0)));
        let pad = s.xxs;
        let mut laid = Laid {
            plan: Some(plan.clone()),
            area: (aw, ah),
            view: (vw, vh),
            content,
            scale: if aw > 0.0 { vw / aw } else { 0.0 },
            inset: pad,
            tiles: Vec::new(),
        };
        let tiles: Vec<Tile> = plan
            .windows
            .iter()
            .filter_map(|p| {
                let win = windows.iter().find(|w| w.id == p.id)?;
                let rect = (p.x + pad, p.y + pad, (p.width - pad * 2.0).max(0.0), (p.height - pad * 2.0).max(0.0));
                laid.tiles.push((p.id.clone(), rect, model.iter().find(|m| m.id == p.id).and_then(|m| m.rect)));
                let edge = (v.theme.space.xl * 2.0).round() as u32;
                let icon = v.store.appicon.get(&icon_key(&p.id), edge, crate::scene::raster_scale()).and_then(|i| i.image);
                Some(Tile {
                    key: format!("win:{}", p.id),
                    on: format!("focus:{}", p.id),
                    rect,
                    picture: Pic(shared.thumbs.get(&p.id).cloned()),
                    icon: Pic(icon),
                    title: win.title.clone(),
                    selected: win.is_focused,
                    badge: true,
                    raised: p.floating,
                })
            })
            .collect();
        let scroll = shared.scroll;
        shared.laid = laid;
        drop(shared);

        let footer_win = v.cursor.and_then(|k| k.strip_prefix("win:")).and_then(|id| windows.iter().find(|w| w.id == id));
        let footer = match footer_win {
            Some(w) => w::caption(if w.title.is_empty() { w.app_id.clone() } else { w.title.clone() }).ink(Ink::Fg),
            None => w::caption("Click a window to jump to it"),
        };
        let mini = if tiles.is_empty() {
            w::section_label(s, "No windows", None, false).centred()
        } else {
            w::strip((mini_w, mini_h), inset, content, scroll, tiles).on("mini")
        };
        w::column(s.sm, vec![mini, footer.elide().mid()])
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let Some(id) = ev.on.strip_prefix("focus:") {
            self.focus(id, fx);
            return;
        }
        if ev.on == "mini"
            && let What::Scroll(dx, dy) = ev.what
        {
            // Qt turns the rig's continuous axis into 72 px a notch where a wheel's
            // 120 angle units give one control height; the card steps that far.
            let step = fx.store.theme.theme.space.control_height * 2.25;
            let mut shared = self.shared.borrow_mut();
            let laid = &shared.laid;
            let wide = laid.content.0 > laid.view.0;
            let tall = laid.content.1 > laid.view.1;
            if !wide && !tall {
                return;
            }
            // A vertical notch moves the strip along whichever axis
            // overflows; a sideways one keeps its own axis.
            let (dx, dy) = if wide && !tall && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
            let max = ((laid.content.0 - laid.view.0).max(0.0), (laid.content.1 - laid.view.1).max(0.0));
            let next = ((shared.scroll.0 + dx * step).clamp(0.0, max.0), (shared.scroll.1 + dy * step).clamp(0.0, max.1));
            shared.scroll = next;
            shared.scrolled = true;
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(id) = stop.strip_prefix("win:") {
            self.focus(id, fx);
        }
    }
}
