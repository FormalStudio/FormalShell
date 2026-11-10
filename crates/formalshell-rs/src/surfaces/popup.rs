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
use crate::surfaces::card_window::CardWindow;
use crate::ui::{Drawn, El, Ui};

pub struct Popup {
    pub win: CardWindow,
    ui: Ui,
    /// Nodes the owner paints itself, above the card and under its content.
    extra: Vec<NodeId>,
}

impl Popup {
    pub fn new(surface: Surface, card: Card) -> Self {
        let ui = Ui::new(Some(card.top_node()));
        Self { win: CardWindow::new(card, surface), ui, extra: Vec::new() }
    }

    pub fn is_open(&self) -> bool {
        self.win.card.is_open()
    }

    pub fn set_open(&mut self, now: Instant, open: bool) {
        self.win.card.set_open(now, open);
    }

    /// Takes input over the card's resting rect alone, or none while it is
    /// not wanted.
    pub fn sync_region(&mut self, compositor: &smithay_client_toolkit::compositor::CompositorState, input: bool) {
        let want = if input { self.win.card.rest_rect() } else { IRect::default() };
        if self.win.sync_region(compositor, want, &[]) {
            use smithay_client_toolkit::shell::WaylandSurface;
            self.win.surface.layer.commit();
        }
    }

    /// The card's content rect, a panel's padding in.
    pub fn inner(&self, theme: &Theme) -> Rect {
        let frame = self.win.card.content.0;
        let pad = theme.space.panel_padding;
        Rect::new(frame.x as f64 + pad, frame.y as f64 + pad, (frame.x + frame.w) as f64 - pad, (frame.y + frame.h) as f64 - pad)
    }

    pub fn draw(&mut self, root: &El, theme: &Theme, kit: &mut Kit, now: Instant) -> Drawn {
        let inner = self.inner(theme);
        self.draw_in(root, inner, theme, kit, now)
    }

    /// Lays `root` at `rect`'s width from its top-left.
    pub fn draw_in(&mut self, root: &El, rect: Rect, theme: &Theme, kit: &mut Kit, now: Instant) -> Drawn {
        let card = &mut self.win.card;
        let (_, alpha) = card.content;
        let clip = card.content_clip();
        self.ui.draw(root, rect, Some(clip), alpha, theme, kit, &mut card.scene, now)
    }

    /// A painter for what the owner draws itself, and the content's opacity.
    pub fn painter(&mut self) -> (Painter<'_>, f32) {
        let card = &mut self.win.card;
        let alpha = card.content.1;
        let clip = card.content_clip();
        let top = card.top_node();
        (Painter::new(&mut card.scene, &mut self.extra, Some(clip)).after(Some(top)), alpha)
    }

    /// Every control's `on` and rect, in the surface's own pixels.
    pub fn controls(&self) -> Vec<(String, IRect)> {
        self.ui.hits.iter().filter_map(|h| Some((h.on.clone()?.into(), h.rect))).collect()
    }

    /// The `on` of the control under a point, in the surface's own pixels.
    pub fn hit(&self, x: f64, y: f64) -> Option<crate::ui::On> {
        self.ui.hit(x, y).and_then(|h| h.on.clone())
    }
}

