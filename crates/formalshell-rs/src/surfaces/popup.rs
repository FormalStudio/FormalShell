//! A small card on its own layer surface that buds off an edge's line, comes
//! out on the card's clock and is gone, with its surface, once it has let
//! go: the OSD pill and the headset connect card. The owner states the
//! content as an `El` tree each frame; this places it on the card's content
//! rect inside a panel's padding, the same way a panel host does.

use std::time::Instant;

use fs_theme::theme::Theme;
use vello_cpu::kurbo::Rect;

use crate::scene::{IRect, NodeId};
use crate::surface::Surface;
use crate::paint::{Kit, Painter};
use crate::surfaces::card::Card;
use crate::ui::{Drawn, El, Ui};

pub struct Popup {
    pub surface: Surface,
    pub card: Card,
    ui: Ui,
    /// Nodes the owner paints itself, above the card and under its content.
    extra: Vec<NodeId>,
    region: Option<IRect>,
}

impl Popup {
    pub fn new(mut surface: Surface, card: Card) -> Self {
        surface.wait_map = true;
        let ui = Ui::new(Some(card.top_node()));
        Self { surface, card, ui, extra: Vec::new(), region: None }
    }

    pub fn is_open(&self) -> bool {
        self.card.is_open()
    }

    pub fn set_open(&mut self, now: Instant, open: bool) {
        self.card.set_open(now, open);
    }

    /// Gone from the screen: closed and landed.
    pub fn finished(&self, now: Instant) -> bool {
        self.surface.mapped && self.card.finished(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.card.animating(now)
    }

    /// The frame callback: the first one maps the card, the rest tick it.
    pub fn frame(&mut self, now: Instant) {
        let s = &mut self.surface;
        (s.frame_pending, s.callbacks) = (false, s.callbacks + 1);
        if s.mapped {
            self.card.tick(now);
        } else {
            s.mapped = true;
            self.card.mapped(now);
        }
    }

    pub fn configure(&mut self) {
        let size = self.card.scene.size;
        self.surface.configure(size.w, size.h);
    }

    /// Takes input over the card's resting rect alone, or none while it is
    /// not wanted; sent only on a change.
    pub fn sync_region(&mut self, compositor: &smithay_client_toolkit::compositor::CompositorState, input: bool) {
        let want = if input { self.card.rest_rect() } else { IRect::default() };
        if self.region == Some(want) {
            return;
        }
        self.region = Some(want);
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor) {
            if !want.is_empty() {
                region.add(want.x, want.y, want.w, want.h);
            }
            use smithay_client_toolkit::shell::WaylandSurface;
            self.surface.layer.set_input_region(Some(region.wl_region()));
            self.surface.layer.commit();
        }
    }

    /// The card's content rect, a panel's padding in.
    pub fn inner(&self, theme: &Theme) -> Rect {
        let frame = self.card.content.0;
        let pad = theme.space.panel_padding;
        Rect::new(frame.x as f64 + pad, frame.y as f64 + pad, (frame.x + frame.w) as f64 - pad, (frame.y + frame.h) as f64 - pad)
    }

    /// Lays `root` on the card's content rect.
    pub fn draw(&mut self, root: &El, theme: &Theme, kit: &mut Kit, now: Instant) -> Drawn {
        let inner = self.inner(theme);
        self.draw_in(root, inner, theme, kit, now)
    }

    /// Lays `root` at `rect`'s width from its top-left.
    pub fn draw_in(&mut self, root: &El, rect: Rect, theme: &Theme, kit: &mut Kit, now: Instant) -> Drawn {
        let (_, alpha) = self.card.content;
        let clip = self.card.content_clip();
        self.ui.draw(root, rect, Some(clip), alpha, theme, kit, &mut self.card.scene, now)
    }

    /// A painter for what the owner draws itself, and the content's opacity.
    pub fn painter(&mut self) -> (Painter<'_>, f32) {
        let alpha = self.card.content.1;
        let clip = self.card.content_clip();
        let top = self.card.top_node();
        (Painter::new(&mut self.card.scene, &mut self.extra, Some(clip)).after(Some(top)), alpha)
    }

    /// Every control's `on` and rect, in the surface's own pixels.
    pub fn controls(&self) -> Vec<(String, IRect)> {
        self.ui.hits.iter().filter_map(|h| Some((h.on.clone()?, h.rect))).collect()
    }

    /// The `on` of the control under a point, in the surface's own pixels.
    pub fn hit(&self, x: f64, y: f64) -> Option<String> {
        self.ui.hit(x, y).and_then(|h| h.on.clone())
    }

    pub fn present(&mut self, animating: bool, qh: &smithay_client_toolkit::reexports::client::QueueHandle<crate::wayland::App>) {
        self.surface.present(&mut self.card.scene, animating, qh);
    }
}


