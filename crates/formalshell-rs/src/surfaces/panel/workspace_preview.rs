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
//! the panel table: with no workspace to show there is nothing to open.
//!
//! The thumbnails are the shell's window captures, which live on the App;
//! they reach the card through [`Shared`], and the card leaves what it laid
//! out there for the captures and for `workspaces status`.

mod strip;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fs_chrome::bar::workspaces::{PreviewLayout, preview_layout};
use fs_types::{Rect, Window, WindowId, WorkspaceId};

use super::{Effect, Out, Panel, View};
use strip::{Strip, Tile};
use crate::scene::Bitmap;
use crate::services::compositor;
use crate::store::Topic;
use crate::ui::el::Pic;
use crate::ui::{El, Event, Ink, On, Size, What, w};

/// A press on a window's thumbnail, and the thumbnail's cursor stop, by
/// the window's id.
enum Press {
    Focus(String),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Press::Focus(id) => write!(f, "win:{id}"),
        }
    }
}

/// The card's name in the panel host.
pub const ID: &str = "workspaces";

/// The appicon slice's key for a window's icon here.
pub fn icon_key(id: &WindowId) -> String {
    format!("preview:{id}")
}

/// What the card laid out last, for the status and the captures.
#[derive(Clone, Debug, Default)]
pub struct Laid {
    pub plan: Option<PreviewLayout>,
    pub view: (f64, f64),
    pub content: (f64, f64),
    pub scale: f64,
    pub inset: f64,
    /// Each tile's id, its box in the content and the window's own rect.
    pub tiles: Vec<(WindowId, (f64, f64, f64, f64), Option<Rect>)>,
}

#[derive(Default)]
pub struct Shared {
    pub thumbs: HashMap<WindowId, Bitmap>,
    pub laid: Laid,
    /// The miniature's scroll, and whether anything has moved it since it
    /// opened on the output's region.
    pub scroll: (f64, f64),
    pub scrolled: bool,
}

pub struct WorkspacePreview {
    pub workspace_id: WorkspaceId,
    label: String,
    pub from_pointer: bool,
    pub shared: Rc<RefCell<Shared>>,
}

impl WorkspacePreview {
    pub fn new(workspace_id: WorkspaceId, label: String, from_pointer: bool, shared: Rc<RefCell<Shared>>) -> Self {
        *shared.borrow_mut() = Shared::default();
        Self { workspace_id, label, from_pointer, shared }
    }

    fn windows<'a>(&self, v: &View<'a>) -> Vec<&'a Window> {
        v.store.compositor.snapshot.windows.iter().filter(|w| w.workspace_id == self.workspace_id).collect()
    }

    fn focus(&self, id: &str, fx: &mut Effect) {
        if !id.is_empty() {
            compositor::focus_window(&WindowId::new(id));
            fx.push(Out::Close);
        }
    }
}

impl Panel for WorkspacePreview {
    fn title(&self, _: &View) -> String {
        format!("Workspace {}", self.label)
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Compositor, Topic::AppIcon]
    }

    fn width(&self, v: &View) -> Option<f64> {
        Some(v.theme.space.popup_width_wide)
    }

    fn takes_keyboard(&self) -> bool {
        !self.from_pointer
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let n = self.windows(v).len();
        vec![w::caption(format!("{n} {}", if n == 1 { "window" } else { "windows" }))]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let windows = self.windows(v);
        let mini_w = (s.popup_width_wide - s.panel_padding * 2.0).max(0.0);
        let (aw, ah) = v.output;
        let mini_h = if aw > 0.0 { (mini_w * ah / aw).round() } else { (mini_w * 9.0 / 16.0).round() };
        let inset = s.xs;
        let (vw, vh) = (mini_w - inset * 2.0, mini_h - inset * 2.0);
        let area = (aw > 0.0 && ah > 0.0).then_some(Rect { x: 0.0, y: 0.0, width: aw, height: ah });
        let model: Vec<Window> = windows.iter().map(|w| (*w).clone()).collect();
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
                laid.tiles.push((p.id.clone(), rect, win.rect.map(Rect::from)));
                let edge = (v.theme.space.xl * 2.0).round() as u32;
                let icon = v.store.appicon.get(&icon_key(&p.id), edge, v.raster).and_then(|i| i.image);
                Some(Tile {
                    key: Press::Focus(p.id.to_string()).into(),
                    on: Press::Focus(p.id.to_string()).into(),
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

        let footer_win = v.cursor.and_then(|k| k.get::<Press>()).and_then(|Press::Focus(id)| windows.iter().find(|w| w.id == id.as_str()));
        let footer = match footer_win {
            Some(w) => w::caption(if w.title.is_empty() { w.app_id.clone() } else { w.title.clone() }).ink(Ink::Fg),
            None => w::caption("Click a window to jump to it"),
        };
        let mini = if tiles.is_empty() {
            w::panel_empty(s, "No windows")
        } else {
            El::custom(Strip { size: (mini_w, mini_h), inset, scroll, tiles, on: "mini".into() }).width(Size::Px(mini_w))
        };
        w::column(s.sm, vec![mini, footer.elide().mid()])
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let Some(Press::Focus(id)) = ev.on.get::<Press>() {
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

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        if let Some(Press::Focus(id)) = stop.get::<Press>() {
            self.focus(id, fx);
        }
    }
}
