//! One open panel's window. A full-output layer surface on the
//! bar's edge that takes input everywhere while the panel is open, so a
//! click outside the card closes it, and none at all once it is closing or
//! handed over. Inside it, the card (`card::Card`) and its contents: the
//! header row, the seam under it and the body, which scrolls past the
//! output's room.
//!
//! The frame's size follows the content on `spatial`:
//! tracked while open, frozen on close, and armed off the surface being
//! mapped rather than its enter settling. A panel opened over another is
//! a handoff: this card is seeded on the outgoing one's rect and travels
//! to its own on `spatial`, its contents coming up on `effects` over the
//! same movement.

use std::time::Instant;

use fs_chrome::types::{Edge, Insets};
use fs_info::notifications::PositionSpec;
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use vello_cpu::kurbo::Rect;

use super::{Edit, Effect, Panel, View};
use crate::motion::{Animated, Kind as Clock, SPATIAL};
use crate::runtime::Runtime;
use crate::scene::{IRect, NodeId, Paint};
use crate::scroll::{Touchpad, Travel};
use crate::store::Store;
use crate::surface::Surface;
use crate::surfaces::bar::cell::{Button, Kit, Painter};
use crate::surfaces::card::{Card, Ends, Target};
use crate::ui::{self, El, Event, Ui, What, w};

/// Where a card hangs: the line it comes out of, its ends, and the cell
/// it is centred on.
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub edge: Edge,
    /// The output's (width, height).
    pub output: (f64, f64),
    /// The line, across from the output's edge.
    pub line_at: f64,
    pub ends: Ends,
    /// The far inset across, a frame ring's on the opposite edge.
    pub far_inset: f64,
    /// The opening cell's centre along the line, or none for the line's end.
    pub anchor: Option<f64>,
    /// A card this one hangs off rather than the bar.
    pub target: Option<Target>,
}

impl Place {
    fn along_len(&self) -> f64 {
        if self.edge.is_vertical() { self.output.1 } else { self.output.0 }
    }

    fn across_len(&self) -> f64 {
        if self.edge.is_vertical() { self.output.0 } else { self.output.1 }
    }
    /// What a card clears on each output edge: the line on its own edge,
    /// the far inset across from it and the ends' insets along it.
    pub fn insets(&self) -> Insets {
        let (start, end) = (self.ends.inset_start, self.ends.inset_end);
        let mut i = if self.edge.is_vertical() {
            Insets { top: start, bottom: end, left: self.far_inset, right: self.far_inset }
        } else {
            Insets { top: self.far_inset, bottom: self.far_inset, left: start, right: end }
        };
        match self.edge {
            Edge::Top => i.top = self.line_at,
            Edge::Bottom => i.bottom = self.line_at,
            Edge::Left => i.left = self.line_at,
            Edge::Right => i.right = self.line_at,
        }
        i
    }

    /// The corner a stack with no cell of its own (the toasts) takes:
    /// beside a side line, where every panel hangs, at the end the
    /// configured corner names; along a top or bottom line, the configured
    /// corner itself.
    pub fn corner(&self, spec: PositionSpec) -> PositionSpec {
        let right = match self.edge {
            Edge::Left => false,
            Edge::Right => true,
            _ => spec.right,
        };
        PositionSpec { left: !right, right, ..spec }
    }
}

/// The cursor: where it is, whether it shows, and whether the
/// keyboard put it there.
#[derive(Default)]
pub struct Cursor {
    pub index: usize,
    pub key: Option<String>,
    pub active: bool,
    pub keyed: bool,
    pub travels: bool,
}

impl Cursor {
    pub fn starting(key: Option<String>) -> Self {
        Self { key, ..Self::default() }
    }

    /// The stop under the cursor, while it shows.
    pub fn shown(&self) -> Option<&str> {
        self.key.as_deref().filter(|_| self.active)
    }

    /// Keeps the cursor on its stop through a relayout, or on the nearest
    /// one when its stop went.
    pub fn sync(&mut self, stops: &[ui::Stop]) {
        if stops.is_empty() {
            self.key = None;
            return;
        }
        if let Some(i) = self.key.as_ref().and_then(|k| stops.iter().position(|st| &st.key == k)) {
            self.index = i;
        } else {
            self.index = self.index.min(stops.len() - 1);
            self.key = Some(stops[self.index].key.clone());
        }
    }

    /// The stop a panel moved the cursor to by itself.
    pub fn take(&mut self, module: &mut dyn Panel, stops: &[ui::Stop]) {
        if let Some(k) = module.take_cursor() {
            if let Some(i) = stops.iter().position(|st| st.key == k) {
                self.index = i;
            }
            self.travels = self.active;
            self.key = Some(k);
        }
    }

    /// An arrow: steps the value on a stop that carries one, else walks.
    /// True when the cursor landed on another stop.
    fn walk(&mut self, module: &mut dyn Panel, stops: &[ui::Stop], dx: i32, dy: i32, fx: &mut Effect) -> bool {
        if module.steps() && self.active && dy == 0 && dx != 0 {
            if let Some(k) = self.key.clone() {
                module.step(&k, dx.signum(), fx);
            }
            self.keyed = true;
            return false;
        }
        if stops.is_empty() {
            return false;
        }
        let next = step(self.index, stops.len(), self.active, dx, dy, module.columns());
        self.travels = self.active && next != self.index;
        self.index = next;
        self.key = Some(stops[next].key.clone());
        self.active = true;
        self.keyed = true;
        true
    }
}

/// What a key did to a panel, for whoever holds its card.
#[derive(Debug, PartialEq)]
pub enum Driven {
    /// Escape the panel did not take: the card's to close.
    Escape,
    /// The cursor walked onto another stop: the card scrolls to it.
    Walked,
    Done,
}

/// One key into a panel outside a text field: the cursor model every card
/// holding a panel shares, the host's and the launcher's.
pub fn drive(module: &mut dyn Panel, cursor: &mut Cursor, stops: &[ui::Stop], name: Key, fx: &mut Effect) -> Driven {
    let stop = cursor.shown().map(str::to_owned);
    let mut walked = false;
    match name {
        Key::Escape if module.escape() => {}
        Key::Escape => return Driven::Escape,
        Key::Tab(d) => {
            if let Some(k) = module.tab(cursor.key.as_deref(), d) {
                if let Some(i) = stops.iter().position(|st| st.key == k) {
                    cursor.index = i;
                }
                cursor.travels = cursor.active;
                cursor.key = Some(k);
            }
            cursor.active = true;
            cursor.keyed = true;
        }
        Key::Move(dx, dy) => walked = cursor.walk(module, stops, dx, dy, fx),
        Key::Back => module.back(fx),
        Key::Edit(..) => {}
        Key::Activate => {
            if let Some(k) = stop.filter(|_| !stops.is_empty()) {
                module.activate(&k, fx);
            }
        }
        Key::Text(t) => match t.as_str() {
            "j" => walked = cursor.walk(module, stops, 0, 1, fx),
            "k" => walked = cursor.walk(module, stops, 0, -1, fx),
            "l" => walked = cursor.walk(module, stops, 1, 0, fx),
            "h" => walked = cursor.walk(module, stops, -1, 0, fx),
            "x" => {
                if let Some(k) = stop {
                    module.delete(&k, fx);
                }
            }
            _ => module.key(stop.as_deref(), &t, fx),
        },
    }
    if let Some(k) = cursor.shown().map(str::to_owned) {
        module.reached(&k, fx);
    }
    cursor.take(module, stops);
    if walked { Driven::Walked } else { Driven::Done }
}

struct Handoff {
    from: Rect,
    travel: Animated,
    alpha: Animated,
    started: bool,
}

/// What an input asked of the shell outside the panel.
#[derive(Debug, PartialEq)]
pub enum Out {
    None,
    Close,
    /// Close, then open the launcher on this route.
    Summon(&'static str),
    /// Open this panel in the open one's place.
    Panel(&'static str),
    /// Close, then call this IPC target's function on the shell itself.
    Call(&'static str, &'static str),
    /// A tray item's menu off this card, centred on `anchor` along the line
    /// (the item's own button) or at the line's end.
    Menu(String, Option<f64>),
}

impl Effect<'_> {
    pub fn out(&mut self) -> Out {
        if let Some(id) = self.menu.take() {
            return Out::Menu(id, None);
        }
        match (self.summon, self.panel, self.call, self.close) {
            (Some(route), ..) => Out::Summon(route),
            (None, Some(name), ..) => Out::Panel(name),
            (None, None, Some((target, function)), _) => Out::Call(target, function),
            (None, None, None, true) => Out::Close,
            (None, None, None, false) => Out::None,
        }
    }
}

pub struct Host {
    pub module: Box<dyn Panel>,
    pub card: Card,
    pub surface: Surface,
    head: Ui,
    body: Ui,
    halo_nodes: Vec<NodeId>,
    rule_nodes: Vec<NodeId>,
    pub place: Place,
    open: bool,
    /// Cut outright: the card that took this one's place is on top of it.
    pub handed: bool,
    morph: Option<(Animated, Animated)>,
    morph_armed: bool,
    cursor: Cursor,
    halo: [Animated; 4],
    halo_shown: bool,
    scroll: Animated,
    /// A touchpad's pull past either end, drawn over `scroll`, and the
    /// coast after a flick, which moves `scroll` frame by frame.
    touch: Touchpad,
    content_h: f64,
    viewport: IRect,
    handoff: Option<Handoff>,
    /// The keyboard's prime: Exclusive until this moment, then OnDemand.
    pub prime_until: Option<Instant>,
    pressed: Option<String>,
    dragging: Option<(String, IRect)>,
    pub wake: Option<Instant>,
    scale: f64,
    /// The input region last set, so it is sent only on a change.
    region: Option<(IRect, Vec<IRect>)>,
    /// The frame's height, the most the output leaves it and whether the
    /// content was cut to that, as of the last layout.
    pub fit: std::cell::Cell<(f64, f64, bool)>,
}

const PRIME_MS: u64 = 75;

impl Host {
    pub fn new(module: Box<dyn Panel>, place: Place, theme: &Theme, surface: Surface, scale: f64, cast: bool) -> Self {
        let size = (place.along_len() as i32, place.across_len() as i32);
        let rest = Rect::new(0.0, place.line_at, 0.0, place.line_at);
        let mut card = Card::build(theme, "card", place.edge, size, place.line_at, rest, scale, cast, true);
        card.ends = place.ends;
        card.target = place.target;
        let top = card.top_node();
        let halo = card.scene.add_after(Some(top), IRect::default(), Paint::Rect { fill: Rgba::TRANSPARENT, radius: 0.0 });
        card.scene.set_visible(halo, false);
        let rule = card.scene.add_after(Some(halo), IRect::default(), Paint::Rect { fill: Rgba::TRANSPARENT, radius: 0.0 });
        card.scene.set_visible(rule, false);
        let mut module = module;
        module.opened();
        let start = module.cursor_start();
        let keyboard = module.takes_keyboard();
        Self {
            module,
            card,
            surface,
            head: Ui::new(Some(rule)),
            body: Ui::new(Some(rule)),
            halo_nodes: vec![halo],
            rule_nodes: vec![rule],
            place,
            open: true,
            handed: false,
            morph: None,
            morph_armed: false,
            cursor: Cursor::starting(start),
            halo: std::array::from_fn(|_| Animated::new(0.0, SPATIAL)),
            halo_shown: false,
            scroll: Animated::new(0.0, Clock::SpatialFast.curve()),
            touch: Touchpad::new(),
            content_h: 0.0,
            viewport: IRect::default(),
            handoff: None,
            prime_until: keyboard.then(|| Instant::now() + std::time::Duration::from_millis(PRIME_MS)),
            pressed: None,
            dragging: None,
            wake: None,
            scale,
            region: None,
            fit: std::cell::Cell::new((0.0, 0.0, false)),
        }
    }

    /// The card's look off a new theme.
    pub fn restyle(&mut self, theme: &Theme, cast: bool) {
        self.card.restyle(theme, cast);
    }

    pub fn id(&self) -> &'static str {
        self.module.id()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// A text field in the body holds the keyboard.
    pub fn editing(&self) -> bool {
        self.open && self.module.editing()
    }

    /// Gone from the screen: closed and landed, or cut by a handoff.
    pub fn finished(&self, now: Instant) -> bool {
        self.handed || (self.surface.mapped && self.card.finished(now))
    }

    pub fn close(&mut self, now: Instant) {
        if !self.open {
            return;
        }
        self.open = false;
        self.module.closed();
        self.cursor.active = false;
        self.handoff = None;
        self.card.set_bypass(now, false);
        self.card.set_open(now, false);
        self.set_keyboard(KeyboardInteractivity::None);
    }

    /// The outgoing half of a handoff: closed for input, held open on
    /// screen and no longer joined, until the incoming card starts moving.
    pub fn hand_over(&mut self) {
        self.open = false;
        self.module.closed();
        self.cursor.active = false;
        self.card.set_joined(false);
        self.set_keyboard(KeyboardInteractivity::None);
    }

    /// The incoming half, seeded on `from` (the outgoing card's live rect).
    pub fn take_over(&mut self, from: Rect, now: Instant) {
        self.handoff = Some(Handoff {
            from,
            travel: Animated::new(0.0, SPATIAL),
            alpha: Animated::new(0.0, Clock::Effects.curve()),
            started: false,
        });
        self.card.set_bypass(now, true);
    }

    /// The surface is up: an enter starts, and a handoff starts travelling.
    /// True when the outgoing card has to go now.
    pub fn mapped(&mut self, theme: &Theme, now: Instant) -> bool {
        self.card.mapped(now);
        let Some(h) = &mut self.handoff else { return false };
        if h.started {
            return false;
        }
        h.started = true;
        h.travel.set(now, 1.0, Clock::Spatial.ms(theme) * self.scale);
        h.alpha.set(now, 1.0, Clock::Effects.ms(theme) * self.scale);
        true
    }

    fn set_keyboard(&mut self, k: KeyboardInteractivity) {
        self.surface.layer.set_keyboard_interactivity(k);
        self.surface.layer.commit();
    }

    /// The prime's end: the keyboard stays, the pointer is the
    /// compositor's to route again.
    pub fn primed(&mut self) {
        self.prime_until = None;
        if self.open {
            self.set_keyboard(KeyboardInteractivity::OnDemand);
        }
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.card.animating(now)
            || self.morph.as_ref().is_some_and(|(w, h)| w.running(now) || h.running(now))
            || self.handoff.as_ref().is_some_and(|h| !h.started || h.travel.running(now) || h.alpha.running(now))
            || self.halo.iter().any(|a| a.running(now))
            || self.scroll.running(now)
            || self.touch.running(now)
    }

    fn view<'a>(&'a self, store: &'a Store, theme: &'a Theme) -> View<'a> {
        View { store, theme, output: self.place.output, cursor: self.cursor.key.as_deref().filter(|_| self.cursor.active), hovered: self.hovered_stop() }
    }

    /// The stop under the pointer: the hit's own, else the smallest row
    /// round it that has one.
    fn hovered_stop(&self) -> Option<String> {
        let path = self.body.hover.as_ref()?;
        let hit = self.body.hits.iter().find(|h| &h.path == path)?;
        if hit.stop.is_some() {
            return hit.stop.clone();
        }
        let r = hit.rect;
        self.body
            .hits
            .iter()
            .filter(|h| h.stop.is_some() && h.rect.x <= r.x && h.rect.y <= r.y && h.rect.right() >= r.right() && h.rect.bottom() >= r.bottom())
            .min_by_key(|h| h.rect.w * h.rect.h)
            .and_then(|h| h.stop.clone())
    }

    /// The frame's target size off the content, before the morph.
    fn target_size(&self, theme: &Theme, kit: &mut Kit, store: &Store) -> (f64, f64, f64) {
        let s = &theme.space;
        let v = self.view(store, theme);
        let vertical = self.place.edge.is_vertical();
        let p = &self.place;
        let width = self.module.width(&v);
        let body = self.module.body(&v);
        let content_w = (width - s.panel_row_inset * 2.0).max(0.0);
        let content_h = ui::measure(&body, content_w, theme, kit).1;
        let (head, gap) = self.header_metrics(theme);
        let max_frame = if vertical {
            (p.output.1 - p.ends.inset_start - p.ends.inset_end - s.screen_padding * 2.0).max(0.0)
        } else {
            (p.across_len() - p.line_at - p.far_inset - s.bar_margin - s.screen_padding).max(0.0)
        };
        let ends = s.panel_gap + self.foot(theme);
        let max_content = (max_frame - ends - head - gap).max(0.0);
        let height = ends + head + gap + content_h.min(max_content);
        self.fit.set((height, max_frame, content_h > max_content));
        (width, height, content_h)
    }

    /// The header row and the gap under it, the hairline in its middle
    /// (specs/2026-10-30-panel-language.md §3).
    fn header_metrics(&self, theme: &Theme) -> (f64, f64) {
        let s = &theme.space;
        if self.module.header() { (s.panel_row, s.panel_gap * 2.0 + theme.border_width) } else { (0.0, 0.0) }
    }

    /// The room under the body: under a header, the same `panelInset` the
    /// title keeps from the card's edge; a headerless card (the tiles) keeps
    /// its rows' own inset on all four sides.
    fn foot(&self, theme: &Theme) -> f64 {
        let s = &theme.space;
        if self.module.header() { s.panel_inset } else { s.panel_gap }
    }

    /// Lays the frame out for this frame and draws everything on it.
    pub fn layout(&mut self, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        let (tw, th, content_h) = self.target_size(theme, kit, store);
        self.content_h = content_h;
        let spatial = Clock::Spatial.ms(theme) * self.scale;
        let morph = self.morph.get_or_insert_with(|| (Animated::new(tw, SPATIAL), Animated::new(th, SPATIAL)));
        if self.open {
            let armed = self.surface.mapped && self.handoff.is_none() && self.morph_armed;
            for (a, t) in [(&mut morph.0, tw), (&mut morph.1, th)] {
                if armed { a.set(now, t, spatial) } else { a.jump(t) }
            }
            self.morph_armed = true;
        }
        let (fw, fh) = (morph.0.value(now), morph.1.value(now));
        let s = &theme.space;
        let p = &self.place;
        let vertical = p.edge.is_vertical();
        let (along_size, across_size) = if vertical { (fh, fw) } else { (fw, fh) };
        let near = p.ends.inset_start + s.screen_padding;
        let far = p.along_len() - p.ends.inset_end - along_size - s.screen_padding;
        let u = match p.anchor {
            Some(a) => a - along_size / 2.0,
            None => far,
        };
        let u = u.min(far.max(near)).max(near).round();
        let v = p.line_at + s.bar_margin;
        let rest = Rect::new(u, v, u + along_size, v + across_size);
        let live = match &self.handoff {
            Some(h) => {
                let t = h.travel.value(now);
                let f = h.from;
                Rect::new(f.x0 + (rest.x0 - f.x0) * t, f.y0 + (rest.y0 - f.y0) * t, f.x1 + (rest.x1 - f.x1) * t, f.y1 + (rest.y1 - f.y1) * t)
            }
            None => rest,
        };
        if self.handoff.as_ref().is_some_and(|h| h.started && !h.travel.running(now)) {
            self.handoff = None;
            self.card.set_bypass(now, false);
        }
        let moved = (rest, live) != (self.card.rest(), self.card.live());
        self.card.set_rect(rest, live);
        if moved && !self.card.animating(now) || !self.surface.mapped {
            self.card.tick(now);
        }
        self.draw(store, theme, kit, now);
    }

    /// Input everywhere while open (a click outside closes) but over the
    /// boxes the bar takes the pointer over (`holes`: its strip, or the
    /// sidebar's cards), over the card's resting rect alone for a card that
    /// takes no keyboard, and nowhere once closing. The strip stays the
    /// bar's: a click on another cell opens that cell's panel, and one
    /// landing as this surface goes away is not lost with it.
    pub fn sync_region(&mut self, compositor: &smithay_client_toolkit::compositor::CompositorState, holes: &[IRect]) {
        let (want, holes) = match (self.open, self.module.takes_keyboard()) {
            (false, _) => (IRect::default(), Vec::new()),
            (true, true) => (self.card.scene.size, holes.to_vec()),
            (true, false) => (self.card.rest_rect(), Vec::new()),
        };
        if self.region.as_ref().is_some_and(|(w, h)| *w == want && *h == holes) {
            return;
        }
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor) {
            if !want.is_empty() {
                region.add(want.x, want.y, want.w, want.h);
            }
            for hole in &holes {
                region.subtract(hole.x, hole.y, hole.w, hole.h);
            }
            self.surface.layer.set_input_region(Some(region.wl_region()));
        }
        self.region = Some((want, holes));
    }

    fn header(&self, v: &View, s: &fs_theme::tokens::Space) -> El {
        let mut parts = vec![w::label(self.module.title(v)).size(ui::Type::Display).weight(ui::Weight::Semibold).elide(), w::space(s.panel_gap)];
        let actions = self.module.actions(v);
        if !actions.is_empty() {
            parts.push(w::row(s.panel_gap, actions));
        }
        if self.module.closable() {
            parts.push(w::icon_button("x").tip("Close").on("close").key("close"));
        }
        // The row takes the header's whole height so the title and its
        // buttons centre in it: the card's edge to the title then matches
        // the title to the hairline.
        w::row(0.0, parts).fill().height(s.panel_row)
    }

    fn draw(&mut self, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        let (frame, card_alpha) = self.card.content;
        let alpha = card_alpha * self.handoff.as_ref().map_or(1.0, |h| h.alpha.value(now).clamp(0.0, 1.0) as f32);
        let clip = self.card.content_clip();
        let s = theme.space.clone();
        // The body's rows start `panelRowInset` in, their text at
        // `panelInset`; the header's title sits on that text column and its
        // last button's glyph ends on it.
        let pad = s.panel_row_inset;
        let vpad = s.panel_gap;
        let (head_h, head_gap) = self.header_metrics(theme);
        let foot = self.foot(theme);
        let fx = frame.x as f64;
        let fy = frame.y as f64;
        let fw = frame.w as f64;
        let inner_w = (fw - pad * 2.0).max(0.0);
        let head_end = s.panel_inset - (s.control_height - s.panel_mark) / 2.0;
        let ring = theme.ring_width();

        self.wake = None;
        let v = View { store, theme, output: self.place.output, cursor: self.cursor.key.as_deref().filter(|_| self.cursor.active), hovered: self.hovered_stop() };
        let head = if self.module.header() { Some(self.header(&v, &s)) } else { None };
        let body = self.module.body(&v);
        let module_wake = self.module.wake(&v);

        let scene = &mut self.card.scene;
        if let Some(head) = head {
            self.head.halo_owned = false;
            let r = Rect::new(fx + s.panel_inset, fy + vpad, fx + fw - head_end, fy + vpad + head_h);
            let d = self.head.draw(&head, r, Some(clip), alpha, theme, kit, scene, now);
            self.wake = if d.animating { Some(now) } else { d.wake };
            let mut p = Painter::new(scene, &mut self.rule_nodes, Some(clip));
            let (sx, sw) = ((fx + s.panel_inset).round() as i32, (fw - s.panel_inset * 2.0).max(0.0).round() as i32);
            let seam = IRect::new(sx, (fy + vpad + head_h + s.panel_gap).round() as i32, sw, theme.border_width.round().max(1.0) as i32);
            let sep = theme.box_style("separator", None).fill;
            p.rect(seam, sep.with_alpha(sep.a * alpha), 0.0);
            p.finish();
        } else {
            self.head.hide(scene);
            let p = Painter::new(scene, &mut self.rule_nodes, Some(clip));
            p.finish();
        }

        let top = fy + vpad + head_h + head_gap;
        let viewport_h = (frame.h as f64 - vpad - foot - head_h - head_gap).max(0.0);
        self.viewport = IRect::new((fx + pad).round() as i32, top.round() as i32, inner_w.round() as i32, viewport_h.round() as i32);
        let max_scroll = (self.content_h - viewport_h).max(0.0);
        if self.scroll.target() > max_scroll {
            self.scroll.jump(max_scroll);
        }
        if let Some(coast) = self.touch.tick(now, max_scroll) {
            self.scroll.jump(coast);
        }
        let scroll = self.scroll.value(now) + self.touch.value(now);
        let vp = self.viewport;
        let r_ = ring.ceil() as i32;
        let body_clip = IRect::new(vp.x - r_, vp.y - r_, vp.w + r_ * 2, vp.h + r_ * 2).intersect(&clip);
        self.body.halo_owned = true;
        self.body.motion_scale = self.scale;
        self.head.motion_scale = self.scale;
        self.body.cursor = self.cursor.key.clone().filter(|_| self.cursor.active);
        self.body.ring = self.cursor.keyed;
        let origin = (fx + pad, top - scroll);
        let d = self.body.draw(&body, Rect::new(origin.0, origin.1, origin.0 + inner_w, origin.1 + self.content_h), Some(body_clip), alpha, theme, kit, scene, now);
        if let Some(w) = d.wake {
            self.wake = Some(self.wake.map_or(w, |x: Instant| x.min(w)));
        }
        self.sync_cursor();

        // One halo for the whole list (M53 D4), travelling on a key step,
        // with the cursor's fill riding it.
        let stop = self.cursor.key.as_ref().filter(|_| self.cursor.active && self.cursor.keyed).and_then(|k| self.body.stops.iter().find(|s| &s.key == k)).cloned();
        let cr = theme.cursor_ring();
        let fast = Clock::SpatialFast.ms(theme) * self.scale;
        match stop {
            Some(st) => {
                let local = [st.rect.x as f64 - origin.0, st.rect.y as f64 - origin.1, st.rect.w as f64, st.rect.h as f64];
                for (a, t) in self.halo.iter_mut().zip(local) {
                    if self.halo_shown && self.cursor.travels { a.set(now, t, fast) } else { a.jump(t) }
                }
                self.halo_shown = true;
                let [x, y, w, h] = [0, 1, 2, 3].map(|i| self.halo[i].value(now));
                let sp = cr.spread;
                let row = Rect::new(origin.0 + x, origin.1 + y, origin.0 + x + w, origin.1 + y + h);
                let rect = row.inflate(sp, sp);
                let fill = theme.box_style("cursor", None).fill;
                let mut p = Painter::new(&mut self.card.scene, &mut self.halo_nodes, Some(body_clip));
                p.rect(ui::irect(rect), cr.color.with_alpha(cr.color.a * alpha), (st.radius + sp) as f32);
                p.rect(ui::irect(row), fill.with_alpha(fill.a * alpha), st.radius as f32);
                p.finish();
            }
            None => {
                self.halo_shown = false;
                let p = Painter::new(&mut self.card.scene, &mut self.halo_nodes, None);
                p.finish();
            }
        }
        self.cursor.travels = false;
        if let Some(w) = module_wake {
            self.wake = Some(self.wake.map_or(w, |x: Instant| x.min(w)));
        }
        if d.animating {
            self.wake = Some(now);
        }
    }

    /// Keeps the cursor on its row by identity when the rows renumber.
    fn sync_cursor(&mut self) {
        self.cursor.sync(&self.body.stops);
    }

    /// Whether the content asks for frames on its own (a marquee, a fade).
    pub fn content_animating(&self, now: Instant) -> bool {
        self.wake.is_some_and(|w| w <= now)
    }

    fn effect<'a>(store: &'a Store, runtime: Option<&'a Runtime>) -> Effect<'a> {
        Effect::new(store, runtime)
    }

    /// cursor.js `follow`: the smallest scroll that shows the cursor row.
    fn follow(&mut self) {
        let Some(st) = self.body.stops.get(self.cursor.index) else { return };
        let now = Instant::now();
        let scroll = self.scroll.value(now);
        let top = st.rect.y as f64 - self.viewport.y as f64 + scroll;
        let (h, view) = (st.rect.h as f64, self.viewport.h as f64);
        let pad = 0.0;
        let mut next = scroll;
        if top - pad < next {
            next = top - pad;
        } else if top + h + pad > next + view {
            next = top + h + pad - view;
        }
        let next = next.clamp(0.0, (self.content_h - view).max(0.0));
        if (next - scroll).abs() > 0.5 {
            self.scroll.set(now, next, crate::motion::SPATIAL_FAST_MS * self.scale);
        }
    }

    /// The clipboard's text into the field holding the keyboard.
    pub fn paste(&mut self, text: String, store: &Store, runtime: Option<&Runtime>) -> Out {
        if !self.open || !self.module.editing() {
            return Out::None;
        }
        let mut fx = Self::effect(store, runtime);
        self.module.edit(Edit::Paste(text), &mut fx);
        fx.out()
    }

    /// One key.
    pub fn key(&mut self, name: Key, store: &Store, runtime: Option<&Runtime>) -> Out {
        if !self.open {
            return Out::None;
        }
        let mut fx = Self::effect(store, runtime);
        if self.module.editing() {
            let edit = match name {
                Key::Escape => Edit::Cancel,
                Key::Tab(_) => Edit::Tab,
                Key::Activate => Edit::Submit,
                Key::Edit(k, mods, t) => Edit::Key(k, mods, t),
                Key::Back | Key::Text(_) | Key::Move(..) => return Out::None,
            };
            self.module.edit(edit, &mut fx);
            return fx.out();
        }
        match drive(self.module.as_mut(), &mut self.cursor, &self.body.stops, name, &mut fx) {
            Driven::Escape => return Out::Close,
            Driven::Walked => {
                self.touch.settle();
                self.follow();
            }
            Driven::Done => {}
        }
        fx.out()
    }

    fn take_module_cursor(&mut self) {
        self.cursor.take(self.module.as_mut(), &self.body.stops);
    }

    fn hit_at(&self, x: f64, y: f64) -> Option<(bool, ui::Hit)> {
        let in_body = self.viewport.intersects(&IRect::new(x.floor() as i32, y.floor() as i32, 1, 1));
        if in_body && let Some(h) = self.body.hit(x, y) {
            return Some((true, h.clone()));
        }
        self.head.hit(x, y).map(|h| (false, h.clone()))
    }

    /// The pointer moved, or left (`None`). True when what is under it
    /// changed.
    pub fn pointer(&mut self, at: Option<(f64, f64)>, store: &Store, runtime: Option<&Runtime>) -> bool {
        if let (Some((path, rect)), Some((x, _))) = (self.dragging.clone(), at) {
            if let Some(on) = self.body.hits.iter().chain(self.head.hits.iter()).find(|h| h.path == path).and_then(|h| h.on.clone()) {
                let f = ((x - rect.x as f64) / rect.w.max(1) as f64).clamp(0.0, 1.0);
                let mut fx = Self::effect(store, runtime);
                self.module.event(&Event { on, what: What::Fraction(f) }, &mut fx);
            }
        }
        let hit = at.and_then(|(x, y)| self.hit_at(x, y));
        let (body_hover, head_hover) = match &hit {
            Some((true, h)) => (Some(h.path.clone()), None),
            Some((false, h)) => (None, Some(h.path.clone())),
            None => (None, None),
        };
        let changed = self.body.hover != body_hover || self.head.hover != head_hover;
        let before = self.hovered_stop();
        self.body.hover = body_hover;
        self.head.hover = head_hover;
        let after = self.hovered_stop();
        if after != before {
            self.module.pointed(after.as_deref());
        }
        // A row the pointer reaches takes the cursor, without the ring.
        if let Some((true, h)) = &hit
            && let Some(stop) = &h.stop
            && (self.cursor.key.as_ref() != Some(stop) || !self.cursor.active)
        {
            self.cursor.key = Some(stop.clone());
            self.cursor.active = true;
            self.cursor.keyed = false;
        }
        let on_card = at.is_some_and(|(x, y)| {
            let r = self.card.live_rect();
            x >= r.x as f64 && x < r.right() as f64 && y >= r.y as f64 && y < r.bottom() as f64
        });
        if !on_card && self.cursor.active && !self.cursor.keyed {
            self.cursor.active = false;
        }
        changed
    }

    /// Whether the pointer is over something that answers a click.
    pub fn hand(&self) -> bool {
        let on = |ui: &Ui| ui.hover.as_ref().and_then(|p| ui.hits.iter().find(|h| &h.path == p)).is_some_and(|h| h.on.is_some());
        on(&self.body) || on(&self.head)
    }

    /// The tooltip the hit under the pointer carries, with its rect.
    pub fn tooltip(&self) -> Option<(String, IRect)> {
        let find = |ui: &Ui| ui.hover.as_ref().and_then(|p| ui.hits.iter().find(|h| &h.path == p)).and_then(|h| h.tip.clone().map(|t| (t, h.rect)));
        find(&self.body).or_else(|| find(&self.head))
    }

    pub fn press(&mut self, x: f64, y: f64, store: &Store, runtime: Option<&Runtime>) {
        let hit = self.hit_at(x, y);
        self.pressed = hit.as_ref().map(|(_, h)| h.path.clone());
        self.body.pressed = self.pressed.clone();
        self.head.pressed = self.pressed.clone();
        if let Some((_, h)) = hit
            && h.what == ui::HitWhat::Track
        {
            self.dragging = Some((h.path.clone(), h.rect));
            if let Some(e) = Ui::event(&h, x) {
                let mut fx = Self::effect(store, runtime);
                self.module.event(&e, &mut fx);
            }
        }
    }

    pub fn release(&mut self, x: f64, y: f64, button: Button, store: &Store, runtime: Option<&Runtime>) -> Out {
        let pressed = self.pressed.take();
        self.body.pressed = None;
        self.head.pressed = None;
        if self.dragging.take().is_some() {
            return Out::None;
        }
        let hit = self.hit_at(x, y);
        let r = self.card.live_rect();
        let on_card = x >= r.x as f64 && x < r.right() as f64 && y >= r.y as f64 && y < r.bottom() as f64;
        if !on_card && pressed.is_none() {
            return Out::Close;
        }
        let Some((_, h)) = hit.filter(|(_, h)| Some(&h.path) == pressed.as_ref()) else { return Out::None };
        let Some(mut e) = Ui::event(&h, x) else { return Out::None };
        if e.on == "close" {
            return Out::Close;
        }
        // Only the tray tells the buttons apart; every other panel takes any
        // button as a click.
        if e.what == What::Click && self.module.id() == super::tray::ID {
            e.what = match button {
                Button::Right => What::Right,
                Button::Middle => What::Middle,
                Button::Left => What::Click,
            };
        }
        let mut fx = Self::effect(store, runtime);
        self.module.event(&e, &mut fx);
        self.take_module_cursor();
        let r = h.rect;
        let along = if self.place.edge.is_vertical() { r.y as f64 + r.h as f64 / 2.0 } else { r.x as f64 + r.w as f64 / 2.0 };
        match fx.out() {
            Out::Menu(id, None) => Out::Menu(id, Some(along)),
            out => out,
        }
    }

    /// A scroll of `dx`, `dy` wheel notches: a scroll region under the
    /// pointer takes both axes (a pane the host moves a control height a
    /// notch, as WheelHandler's angleDelta over 120 does), a track under it
    /// steps on a vertical one, and anything else scrolls the body
    /// vertically by `travel`, the same frame read as a wheel's or a
    /// touchpad's.
    #[allow(clippy::too_many_arguments)]
    pub fn scroll(&mut self, x: f64, y: f64, dx: f64, dy: f64, travel: Travel, store: &Store, runtime: Option<&Runtime>) {
        let now = Instant::now();
        if let Travel::Lift(time) = travel {
            let max = (self.content_h - self.viewport.h as f64).max(0.0);
            self.touch.release(now, time, self.scroll.target(), max, Clock::SpatialFast.ms(&store.theme.theme) * self.scale, self.scale);
            return;
        }
        let point = IRect::new(x.floor() as i32, y.floor() as i32, 1, 1);
        if self.viewport.intersects(&point)
            && let Some(h) = self.body.scroll_hit(x, y).cloned()
            && let Some(on) = h.on.clone()
        {
            self.body.scroll(&h.path, -dy * store.theme.theme.space.control_height);
            let mut fx = Self::effect(store, runtime);
            self.module.event(&Event { on, what: What::Scroll(dx, dy) }, &mut fx);
            return;
        }
        if dy == 0.0 {
            return;
        }
        let up = dy < 0.0;
        if let Some((_, h)) = self.hit_at(x, y)
            && h.what == ui::HitWhat::Track
            && let Some(on) = h.on.clone()
        {
            let mut fx = Self::effect(store, runtime);
            self.module.event(&Event { on, what: What::Wheel(if up { 1 } else { -1 }) }, &mut fx);
            return;
        }
        let view = self.viewport.h as f64;
        let max = (self.content_h - view).max(0.0);
        if max <= 0.0 {
            return;
        }
        if let Travel::Pixels { px, finger, time } = travel {
            let base = self.scroll.target();
            let next = if finger {
                self.touch.drag(now, time, base, max, px, view)
            } else {
                self.touch.settle();
                (base + px).clamp(0.0, max)
            };
            self.scroll.jump(next);
            return;
        }
        self.touch.settle();
        let step = 32.0;
        let base = self.scroll.target();
        let next = (base + if up { -step } else { step }).clamp(0.0, max);
        self.scroll.set(now, next, crate::motion::SPATIAL_FAST_MS * self.scale);
    }
}

/// cursor.js `move`: the first key only reveals the cursor where it sits;
/// above one column the stops are a grid, vertical steps a whole row and
/// horizontal stops at the ends of its own.
pub fn step(index: usize, count: usize, active: bool, dx: i32, dy: i32, columns: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let clamp = |i: i64| i.clamp(0, count as i64 - 1) as usize;
    let index = index as i64;
    if !active {
        return clamp(index);
    }
    let cols = columns.max(1) as i64;
    if dy != 0 {
        clamp(index + dy as i64 * cols)
    } else if cols == 1 {
        clamp(index + dx as i64)
    } else {
        let start = (clamp(index) as i64 / cols) * cols;
        clamp((index + dx as i64).clamp(start, start + cols - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::{Edge, Ends, Place, step};
    use fs_info::notifications::position_spec;

    fn place(edge: Edge) -> Place {
        let ends = Ends { along: 1080.0, inset_start: 6.0, inset_end: 6.0, radius: 0.0 };
        Place { edge, output: (1920.0, 1080.0), line_at: 160.0, ends, far_inset: 6.0, anchor: None, target: None }
    }

    #[test]
    fn a_stack_clears_the_line_and_sits_beside_a_side_one() {
        let i = place(Edge::Left).insets();
        assert_eq!((i.left, i.right, i.top, i.bottom), (160.0, 6.0, 6.0, 6.0));
        let i = place(Edge::Top).insets();
        assert_eq!((i.top, i.bottom, i.left, i.right), (160.0, 6.0, 6.0, 6.0));
        let spec = position_spec(Some("bottom-right"));
        let c = place(Edge::Left).corner(spec);
        assert!(c.left && !c.right && c.bottom);
        assert!(place(Edge::Right).corner(position_spec(Some("top-left"))).right);
        assert_eq!(place(Edge::Top).corner(spec), spec);
    }

    #[test]
    fn the_first_key_reveals_and_the_rest_walk() {
        assert_eq!(step(0, 3, false, 0, 1, 1), 0);
        assert_eq!(step(0, 3, true, 0, 1, 1), 1);
        assert_eq!(step(2, 3, true, 0, 1, 1), 2);
        assert_eq!(step(0, 1, true, 0, 1, 1), 0);
        assert_eq!(step(5, 3, false, 0, 1, 1), 2);
    }

    #[test]
    fn a_grid_steps_weeks_and_stops_at_its_row() {
        assert_eq!(step(3, 14, true, 0, 1, 7), 10);
        assert_eq!(step(6, 14, true, 1, 0, 7), 6);
        assert_eq!(step(7, 14, true, -1, 0, 7), 7);
    }
}

/// What a key press means to a panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Escape,
    Tab(i32),
    Move(i32, i32),
    Activate,
    Back,
    Text(String),
    /// A key into the field that holds the keyboard, with what it types.
    Edit(fs_menu::field::Key, fs_menu::nav::Modifiers, Option<String>),
}


