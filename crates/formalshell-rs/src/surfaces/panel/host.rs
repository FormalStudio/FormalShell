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

use fs_chrome::types::Edge;
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
use crate::surfaces::bar::cell::{Kit, Painter};
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
}

/// The cursor: where it is, whether it shows, and whether the
/// keyboard put it there.
#[derive(Default)]
struct Cursor {
    index: usize,
    key: Option<String>,
    active: bool,
    keyed: bool,
    travels: bool,
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
}

impl Effect<'_> {
    fn out(&self) -> Out {
        match (self.summon, self.close) {
            (Some(route), _) => Out::Summon(route),
            (None, true) => Out::Close,
            (None, false) => Out::None,
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
    /// The cursor's leading bar, over the rows rather than under them.
    bar_nodes: Vec<NodeId>,
    rule_nodes: Vec<NodeId>,
    /// The console header's title, status strip and id strip.
    console_nodes: Vec<NodeId>,
    /// The status strip's line and when it last changed, for its typing.
    status_typed: (u64, Instant),
    /// When the enter began, which the rows' rise and the grain run off.
    opened: Option<Instant>,
    /// When a handoff's body started coming in (`swap: glitch`).
    swapped: Option<Instant>,
    grain: ui::Grain,
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
    region: Option<(IRect, IRect)>,
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
        if let Some(c) = console_outline(module.as_ref(), theme) {
            card.outline(c);
        }
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
            bar_nodes: Vec::new(),
            rule_nodes: vec![rule],
            console_nodes: Vec::new(),
            status_typed: (0, Instant::now()),
            opened: None,
            swapped: None,
            grain: ui::Grain::default(),
            place,
            open: true,
            handed: false,
            morph: None,
            morph_armed: false,
            cursor: Cursor { key: start, ..Cursor::default() },
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

    /// The card's look off a new theme, its console outline included.
    pub fn restyle(&mut self, theme: &Theme, cast: bool) {
        self.card.restyle(theme, cast);
        if let Some(c) = console_outline(self.module.as_ref(), theme) {
            self.card.outline(c);
        }
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
        if self.opened.is_none() {
            self.opened = Some(now);
            if self.handoff.is_none() {
                self.body.arrive = Some(now);
            }
        }
        let Some(h) = &mut self.handoff else { return false };
        if h.started {
            return false;
        }
        h.started = true;
        self.swapped = theme.motion().catalogue.swap.is_some().then_some(now);
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
        let strip = self.strip(theme);
        let vertical = self.place.edge.is_vertical();
        let p = &self.place;
        // The strip widens the card, short of a panel already as wide as
        // the output lets it be (the gallery).
        let room = if vertical {
            p.across_len() - p.line_at - p.far_inset - s.bar_margin - s.screen_padding
        } else {
            p.along_len() - p.ends.inset_start - p.ends.inset_end - s.screen_padding * 2.0
        };
        let width = self.module.width(&v);
        let width = if strip > 0.0 { (width + strip).min(room.max(width)) } else { width };
        let body = self.module.body(&v);
        let content_w = (width - strip - s.panel_padding * 2.0).max(0.0);
        let content_h = ui::measure(&body, content_w, theme, kit).1;
        let (head, gap) = self.header_metrics(theme, kit);
        let max_frame = if vertical {
            (p.output.1 - p.ends.inset_start - p.ends.inset_end - s.screen_padding * 2.0).max(0.0)
        } else {
            (p.across_len() - p.line_at - p.far_inset - s.bar_margin - s.screen_padding).max(0.0)
        };
        let max_content = (max_frame - s.panel_padding * 2.0 - head - gap).max(0.0);
        let height = s.panel_padding * 2.0 + head + gap + content_h.min(max_content);
        self.fit.set((height, max_frame, content_h > max_content));
        (width, height, content_h)
    }

    fn header_metrics(&self, theme: &Theme, kit: &mut Kit) -> (f64, f64) {
        if !self.module.header() {
            (0.0, 0.0)
        } else if console(self.module.as_ref(), theme) {
            let c = ConsoleType::new(theme, kit);
            (c.head_height(theme), theme.space.panel_padding)
        } else {
            (theme.space.control_height, theme.space.panel_padding * 2.0 + theme.border_width)
        }
    }

    /// The id strip down the card's leading side, 0 off a console table.
    fn strip(&self, theme: &Theme) -> f64 {
        if console(self.module.as_ref(), theme) { theme.metric("panel.strip", "width").max(0.0) } else { 0.0 }
    }

    /// Lays the frame out for this frame and draws everything on it.
    pub fn layout(&mut self, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        let accented;
        let theme = if console(self.module.as_ref(), theme) {
            accented = theme.accented(self.module.accent().role());
            &accented
        } else {
            theme
        };
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
    /// bar's `strip`, over the card's resting rect alone for a card that
    /// takes no keyboard, and nowhere once closing. The strip stays the
    /// bar's: a click on another cell opens that cell's panel, and one
    /// landing as this surface goes away is not lost with it.
    pub fn sync_region(&mut self, compositor: &smithay_client_toolkit::compositor::CompositorState, strip: IRect) {
        let (want, hole) = match (self.open, self.module.takes_keyboard()) {
            (false, _) => (IRect::default(), IRect::default()),
            (true, true) => (self.card.scene.size, strip),
            (true, false) => (self.card.rest_rect(), IRect::default()),
        };
        if self.region == Some((want, hole)) {
            return;
        }
        self.region = Some((want, hole));
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor) {
            if !want.is_empty() {
                region.add(want.x, want.y, want.w, want.h);
            }
            if !hole.is_empty() {
                region.subtract(hole.x, hole.y, hole.w, hole.h);
            }
            self.surface.layer.set_input_region(Some(region.wl_region()));
        }
    }

    fn header(&self, v: &View, s: &fs_theme::tokens::Space) -> El {
        if console(self.module.as_ref(), v.theme) {
            return self.console_header(v, s);
        }
        let mut parts = Vec::new();
        let glyph = self.module.icon(v);
        if !glyph.is_empty() {
            parts.push(w::icon(glyph).size(ui::Type::Subtitle));
            parts.push(w::space(s.icon_gap));
        }
        parts.push(w::label(self.module.title(v)).size(ui::Type::Subtitle).weight(ui::Weight::Semibold).elide());
        parts.push(w::space(s.icon_gap));
        let actions = self.module.actions(v);
        if !actions.is_empty() {
            parts.push(w::row(s.xs, actions));
        }
        if self.module.closable() {
            parts.push(w::icon_button("x").tip("Close").on("close").key("close"));
        }
        w::row(0.0, parts).fill()
    }

    /// The console header's controls: the panel's own actions and the back
    /// chip, Escape's keycap and "Back", trailing the title painted beside
    /// them.
    fn console_header(&self, v: &View, s: &fs_theme::tokens::Space) -> El {
        w::row(0.0, vec![w::spacer(), self.console_controls(v, s)]).fill()
    }

    fn console_controls(&self, v: &View, s: &fs_theme::tokens::Space) -> El {
        let mut parts = Vec::new();
        let actions = self.module.actions(v);
        if !actions.is_empty() {
            parts.push(w::row(s.xs, actions));
            parts.push(w::space(s.xs));
        }
        if self.module.closable() {
            let back = w::row(s.xs, vec![w::keycap("Esc"), w::label("BACK").size(ui::Type::Caption).weight(ui::Weight::Semibold)]);
            parts.push(
                w::cell(back)
                    .cell_state(|c| {
                        c.chip = true;
                        c.small = true;
                    })
                    .interactive()
                    .tip("Close")
                    .on("close")
                    .key("close"),
            );
        }
        w::row(0.0, parts)
    }

    /// The console's own paint: the title and `// subtitle` ahead of the
    /// header's controls, the status strip under them and the id strip down
    /// the leading side.
    #[allow(clippy::too_many_arguments)]
    fn paint_console(module: &dyn Panel, nodes: &mut Vec<NodeId>, scene: &mut crate::scene::Scene, v: &View, kit: &mut Kit, frame: IRect, head: Rect, room: f64, clip: IRect, alpha: f32, typed: (&mut (u64, Instant), f64, Instant)) -> bool {
        let theme = v.theme;
        let s = &theme.space;
        let c = ConsoleType::new(theme, kit);
        let a = |c: Rgba| c.with_alpha(c.a * alpha);
        let title_box = theme.box_style("panel.title", None);
        let sub_box = theme.box_style("panel.subtitle", None);
        let strip_box = theme.box_style("panel.strip", None);
        let status_box = theme.box_style("panel.status", None);
        let top_h = c.top_height(theme);

        let title = fit(kit, &module.title(v).to_uppercase(), c.title, room);
        let sub = kit.shape(&format!("// {}", module.subtitle(v).to_uppercase()), c.subtitle);
        // The subtitle goes before the title is cut.
        let sub_shown = (title.width + sub.width) as f64 + s.sm <= room;
        let id = module.id().to_uppercase();
        // The status line types in on open and whenever it changes
        // (`reveal: type`), placed and fitted by its whole width.
        let line = format!("{id} :: {}", module.status(v));
        let (mark, scale, now) = typed;
        let tok = line_token(&line);
        if mark.0 != tok {
            *mark = (tok, now);
        }
        let reveal = theme.motion().catalogue.reveal.map(|ms| crate::effects::progress(crate::effects::since(mark.1, now), ms * scale));
        let shown = reveal.map_or(line.as_str(), |p| crate::effects::prefix(&line, crate::effects::typed(line.chars().count(), p)));
        let whole = kit.shape(&line, c.status).width;
        let left = kit.shape(shown, c.status);
        let date = v.store.clock.now.format("( %Y / %m / %d )").to_string();
        let right = kit.shape(&date, c.status);
        let tag = kit.shape(&module.id().to_lowercase(), c.strip);

        let mut p = Painter::new(scene, nodes, Some(clip));
        let baseline = head.y0 + (top_h + (title.ascent - title.descent) as f64) / 2.0;
        let tx = head.x0.round() as i32;
        let ty = (baseline - title.ascent as f64).round() as i32;
        p.text(&title, (tx, ty), a(title_box.ink), &[]);
        let sx = tx + title.width + s.sm.round() as i32;
        let sy = (baseline - sub.ascent as f64).round() as i32;
        let glow: Vec<_> = sub_box.ink_shadow.iter().map(|g| fs_theme::style::Glow { color: a(g.color), ..g.clone() }).collect();
        if sub_shown {
            p.text(&sub, (sx, sy), a(sub_box.ink), &glow);
        }

        let sr = Rect::new(head.x0, head.y0 + top_h + s.sm, head.x1, head.y1);
        let radius = theme.box_radius(&status_box, sr.height());
        ui::boxes::paint(&mut p, ui::irect(sr), &status_box, radius, alpha, 0.0);
        let ly = (sr.y0 + (sr.height() - left.line_height() as f64) / 2.0).round() as i32;
        let inset = s.md;
        let room = (sr.width() - inset * 3.0 - right.width as f64).max(0.0);
        if (whole as f64) <= room {
            p.text(&left, ((sr.x0 + inset).round() as i32, ly), a(status_box.ink), &[]);
        }
        p.text(&right, ((sr.x1 - inset).round() as i32 - right.width, ly), a(status_box.ink), &[]);

        let strip_w = theme.metric("panel.strip", "width");
        if strip_w > 0.0 {
            let line = strip_box.border.clone().unwrap_or(fs_theme::style::Line { color: Rgba::TRANSPARENT, width: 0.0 });
            let lw = line.width.round().max(1.0) as i32;
            let x = frame.x + strip_w.round() as i32 - lw;
            p.rect(IRect::new(x, frame.y, lw, frame.h), a(line.color), 0.0);
            let (w, h) = tag.box_size();
            let lh = tag.line_height() as f64;
            let across = frame.x as f64 + ((strip_w - lh) / 2.0).round();
            let start = frame.y as f64 + s.panel_padding + tag.width as f64;
            // Read bottom to top, ending a panel padding under the top edge.
            let rot = vello_cpu::kurbo::Affine::new([0.0, -1.0, 1.0, 0.0, across, start]);
            let full = rot * vello_cpu::kurbo::Affine::translate((-crate::text::PAD as f64, -crate::text::PAD as f64));
            let bounds = crate::scene::Scene::cover(Rect::new(0.0, 0.0, w as f64, h as f64), full, 1.0);
            let transform = full * vello_cpu::kurbo::Affine::translate((-bounds.x as f64, -bounds.y as f64));
            p.text_in(&tag, bounds, transform, Some(clip), a(strip_box.ink), &[]);
        }
        p.finish();
        reveal.is_some_and(|p| p < 1.0)
    }

    fn draw(&mut self, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        let (frame, card_alpha) = self.card.content;
        let alpha = card_alpha * self.handoff.as_ref().map_or(1.0, |h| h.alpha.value(now).clamp(0.0, 1.0) as f32);
        let clip = self.card.clip;
        let s = theme.space.clone();
        let pad = s.panel_padding;
        let (head_h, head_gap) = self.header_metrics(theme, kit);
        let strip = self.strip(theme);
        let console = console(self.module.as_ref(), theme) && self.module.header();
        let fx = frame.x as f64 + strip;
        let fy = frame.y as f64;
        let fw = frame.w as f64 - strip;
        let inner_w = (fw - pad * 2.0).max(0.0);
        let ring = theme.ring_width();

        self.wake = None;
        let v = View { store, theme, output: self.place.output, cursor: self.cursor.key.as_deref().filter(|_| self.cursor.active), hovered: self.hovered_stop() };
        let head = if self.module.header() { Some(self.header(&v, &s)) } else { None };
        let room = if console { (inner_w - ui::measure(&self.console_controls(&v, &s), inner_w, theme, kit).0 - s.md).max(0.0) } else { 0.0 };
        let body = self.module.body(&v);
        let module_wake = self.module.wake(&v);

        let scene = &mut self.card.scene;
        if let Some(head) = head {
            self.head.halo_owned = false;
            let r = Rect::new(fx + pad, fy + pad, fx + pad + inner_w, fy + pad + head_h);
            let controls = if console { Rect::new(r.x0, r.y0, r.x1, r.y0 + ConsoleType::new(theme, kit).top_height(theme)) } else { r };
            let d = self.head.draw(&head, controls, Some(clip), alpha, theme, kit, scene, now);
            self.wake = if d.animating { Some(now) } else { d.wake };
            let mut p = Painter::new(scene, &mut self.rule_nodes, Some(clip));
            if !console {
                let seam = IRect::new(frame.x, (fy + pad + head_h + pad).round() as i32, frame.w, theme.border_width.round().max(1.0) as i32);
                let sep = theme.box_style("separator", None).fill;
                p.rect(seam, sep.with_alpha(sep.a * alpha), 0.0);
            }
            p.finish();
            if console {
                let typing = Self::paint_console(self.module.as_ref(), &mut self.console_nodes, scene, &v, kit, frame, r, room, clip, alpha, (&mut self.status_typed, self.scale, now));
                if typing {
                    self.wake = Some(now);
                }
            } else {
                Painter::new(scene, &mut self.console_nodes, None).finish();
            }
        } else {
            self.head.hide(scene);
            let p = Painter::new(scene, &mut self.rule_nodes, Some(clip));
            p.finish();
            Painter::new(scene, &mut self.console_nodes, None).finish();
        }

        let top = fy + pad + head_h + head_gap;
        let viewport_h = (frame.h as f64 - pad * 2.0 - head_h - head_gap).max(0.0);
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
        // A handed-over card's body comes in from ahead (`swap: glitch`);
        // the outgoing card is cut, so there is no way out to draw.
        let glitch = theme.motion().catalogue.swap.zip(self.swapped).map(|(g, at)| {
            let t = crate::effects::since(at, now) / self.scale.max(1e-6);
            (crate::effects::glitch_in(&g, t), crate::effects::glitching(&g, t + g.out))
        });
        let ((gdx, ga), glitching) = glitch.unwrap_or(((0.0, 1.0), false));
        if !glitching {
            self.swapped = None;
        }
        let origin = (fx + pad, top - scroll);
        let d = self.body.draw(&body, Rect::new(origin.0 + gdx, origin.1, origin.0 + gdx + inner_w, origin.1 + self.content_h), Some(body_clip), alpha * ga, theme, kit, scene, now);
        if let Some(w) = d.wake {
            self.wake = Some(self.wake.map_or(w, |x: Instant| x.min(w)));
        }
        self.sync_cursor();

        // One halo for the whole list (M53 D4), travelling on a key step,
        // with the cursor's fill and leading bar riding it.
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
                let mut p = Painter::new(&mut self.card.scene, &mut self.bar_nodes, Some(body_clip));
                if let Some(bar) = theme.cursor_bar() {
                    let r = ui::irect(row);
                    p.rect(IRect::new(r.x, r.y, (bar.width.round() as i32).min(r.w), r.h), bar.color.with_alpha(bar.color.a * alpha), 0.0);
                }
                p.finish();
            }
            None => {
                self.halo_shown = false;
                let mut p = Painter::new(&mut self.card.scene, &mut self.halo_nodes, None);
                p.finish();
                let p = Painter::new(&mut self.card.scene, &mut self.bar_nodes, None);
                p.finish();
            }
        }
        self.cursor.travels = false;
        if self.grain.paint(&mut self.card.scene, frame, clip, theme, self.opened, self.scale, now) || glitching {
            self.wake = Some(now);
        }
        if let Some(w) = module_wake {
            self.wake = Some(self.wake.map_or(w, |x: Instant| x.min(w)));
        }
        if d.animating {
            self.wake = Some(now);
        }
    }

    /// Keeps the cursor on its row by identity when the rows renumber.
    fn sync_cursor(&mut self) {
        let count = self.body.stops.len();
        if count == 0 {
            self.cursor.key = None;
            return;
        }
        if let Some(i) = self.cursor.key.as_ref().and_then(|k| self.body.stop_index(k)) {
            self.cursor.index = i;
        } else {
            self.cursor.index = self.cursor.index.min(count - 1);
            self.cursor.key = Some(self.body.stops[self.cursor.index].key.clone());
        }
    }

    /// Whether the content asks for frames on its own (a marquee, a fade).
    pub fn content_animating(&self, now: Instant) -> bool {
        self.wake.is_some_and(|w| w <= now)
    }

    fn effect<'a>(store: &'a Store, runtime: Option<&'a Runtime>) -> Effect<'a> {
        Effect { store, runtime, close: false, summon: None }
    }

    /// Move the cursor.
    fn move_cursor(&mut self, dx: i32, dy: i32, fx: &mut Effect) {
        let count = self.body.stops.len();
        if self.module.steps() && self.cursor.active && dy == 0 && dx != 0 {
            if let Some(k) = self.cursor.key.clone() {
                self.module.step(&k, dx.signum(), fx);
            }
            self.cursor.keyed = true;
            return;
        }
        if count == 0 {
            return;
        }
        let next = step(self.cursor.index, count, self.cursor.active, dx, dy, self.module.columns());
        self.cursor.travels = self.cursor.active && next != self.cursor.index;
        self.cursor.index = next;
        self.cursor.key = Some(self.body.stops[next].key.clone());
        self.cursor.active = true;
        self.cursor.keyed = true;
        self.touch.settle();
        self.follow();
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
        let stop = self.cursor.key.clone().filter(|_| self.cursor.active);
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
        match name {
            Key::Escape if self.module.escape() => {}
            Key::Escape => return Out::Close,
            Key::Tab(d) => {
                if let Some(k) = self.module.tab(self.cursor.key.as_deref(), d) {
                    if let Some(i) = self.body.stop_index(&k) {
                        self.cursor.index = i;
                    }
                    self.cursor.travels = self.cursor.active;
                    self.cursor.key = Some(k);
                }
                self.cursor.active = true;
                self.cursor.keyed = true;
            }
            Key::Move(dx, dy) => self.move_cursor(dx, dy, &mut fx),
            Key::Back | Key::Edit(..) => {}
            Key::Activate => {
                if let Some(k) = stop.filter(|_| !self.body.stops.is_empty()) {
                    self.body.used = Some((k.clone(), Instant::now()));
                    self.module.activate(&k, &mut fx);
                }
            }
            Key::Text(t) => match t.as_str() {
                "j" => self.move_cursor(0, 1, &mut fx),
                "k" => self.move_cursor(0, -1, &mut fx),
                "l" => self.move_cursor(1, 0, &mut fx),
                "h" => self.move_cursor(-1, 0, &mut fx),
                "x" => {
                    if let Some(k) = stop {
                        self.module.delete(&k, &mut fx);
                    }
                }
                _ => self.module.key(stop.as_deref(), &t, &mut fx),
            },
        }
        if let Some(k) = self.cursor.key.clone().filter(|_| self.cursor.active) {
            self.module.reached(&k, &mut fx);
        }
        self.take_module_cursor();
        fx.out()
    }

    /// The stop a panel moved the cursor to by itself.
    fn take_module_cursor(&mut self) {
        if let Some(k) = self.module.take_cursor() {
            if let Some(i) = self.body.stop_index(&k) {
                self.cursor.index = i;
            }
            self.cursor.travels = self.cursor.active;
            self.cursor.key = Some(k);
        }
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
        self.body.hover = body_hover;
        self.head.hover = head_hover;
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

    pub fn release(&mut self, x: f64, y: f64, store: &Store, runtime: Option<&Runtime>) -> Out {
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
        let Some(e) = Ui::event(&h, x) else { return Out::None };
        if e.on == "close" {
            return Out::Close;
        }
        if e.what == What::Click && let Some(k) = h.stop.clone() {
            self.body.used = Some((k, Instant::now()));
        }
        let mut fx = Self::effect(store, runtime);
        self.module.event(&e, &mut fx);
        self.take_module_cursor();
        fx.out()
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
    use super::step;

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

/// Whether `module` draws the console header: a table whose panels take
/// the console grammar, on a panel with a header at all.
fn console(module: &dyn Panel, theme: &Theme) -> bool {
    module.header() && theme.habit_str("panel") == "console"
}

/// The card's outline in the panel's accent, on a console table.
fn console_outline(module: &dyn Panel, theme: &Theme) -> Option<Rgba> {
    console(module, theme).then(|| theme.accented(module.accent().role()).box_style("panel.strip", None).border.map_or(Rgba::TRANSPARENT, |l| l.color))
}

/// The console header's type: the title wide and heavy, the subtitle
/// condensed, the status and the id strip in the values face.
struct ConsoleType {
    title: crate::text::TextStyle,
    subtitle: crate::text::TextStyle,
    status: crate::text::TextStyle,
    strip: crate::text::TextStyle,
    title_h: f64,
    status_h: f64,
}

impl ConsoleType {
    fn new(theme: &Theme, kit: &mut Kit) -> Self {
        use crate::surfaces::bar::cell::face;
        use crate::text::TextStyle;
        let f = &theme.font_size;
        let axes = |role: &str, size: f64, family| {
            let wdth = theme.metric(role, "wdth") as f32;
            let wght = theme.metric(role, "wght") as f32;
            TextStyle {
                family,
                size: size as f32,
                weight: if wght > 0.0 { wght } else { 700.0 },
                tracking: 0.0,
                wdth: if wdth > 0.0 { wdth } else { TextStyle::NORMAL_WIDTH },
                wght: (wght > 0.0).then_some(wght),
            }
        };
        // The reference's 24px and 19px over a 15px body.
        let title = axes("panel.title", (f.base_size * 1.6).round(), face(theme.faces.display));
        let subtitle = axes("panel.subtitle", (f.base_size * 1.25).round(), face(theme.faces.display));
        let mono = |size: f64| TextStyle { family: face(theme.faces.values), size: size as f32, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
        let status = mono(f.body_small);
        let strip = TextStyle { tracking: (f.caption * 0.1).round() as f32, ..mono(f.caption) };
        let title_h = kit.shape("Ag", title).line_height() as f64;
        let status_h = kit.shape("Ag", status).line_height() as f64;
        Self { title, subtitle, status, strip, title_h, status_h }
    }

    fn top_height(&self, theme: &Theme) -> f64 {
        self.title_h.max(theme.space.control_height)
    }

    fn head_height(&self, theme: &Theme) -> f64 {
        self.top_height(theme) + theme.space.sm + self.status_h + theme.space.xxs * 2.0
    }
}

fn line_token(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish() | 1
}

/// `text` shaped whole, or cut to `room` with an ellipsis.
fn fit(kit: &mut Kit, text: &str, style: crate::text::TextStyle, room: f64) -> crate::text::ShapedText {
    let whole = kit.shape(text, style);
    if whole.width as f64 <= room {
        return whole;
    }
    let chars: Vec<char> = text.chars().collect();
    for n in (0..chars.len()).rev() {
        let cut: String = chars[..n].iter().collect::<String>().trim_end().to_owned() + "\u{2026}";
        let shaped = kit.shape(&cut, style);
        if shaped.width as f64 <= room || n == 0 {
            return shaped;
        }
    }
    whole
}
