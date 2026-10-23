//! A card hanging off a line, on any of the four edges. Under
//! the `join` emerge habit it is the joined shape:
//! Presence's emerge on the theme's `emerge` clock, the let-go on
//! `spatialFast`, Deform's springs, Shoulders' outline cut at the line, the
//! walls a side too close to the line's end runs out to (M57 D2) and the
//! bud a card hanging off another card's far edge is clamped into (M57 D3).
//! Under `popover` it is a plain card on its casts,
//! fading on the table's `emerge` clock over an `md` drop out of its cell;
//! under `slide` the same card slides in along the line from the side of
//! the output it rests on.
//!
//! Worked out for a top edge in the card's own coordinates, `u` along the
//! line and `v` away from the output's edge; [`Card::edge_map`] turns those
//! into the surface's, so every edge shares one implementation. Content is
//! never mirrored: it is placed at the mapped content rect.
//!
//! Two rects drive it, as on Drawer: `rest`, where the card settles (walls
//! and depth are decided off it, so a card mid-travel never walls and
//! unwalls itself), and `live`, where the frame is drawn this frame (the
//! handoff's travel, the size morph).

use std::time::Instant;

use fs_chrome::types::Edge;
use vello_cpu::kurbo::{Affine, Rect};

use crate::motion::{Animated, Curve, Deform, EMERGE, SPATIAL_FAST, SPATIAL_FAST_MS};
use crate::scene::{Cast, IRect, NodeId, Paint, Scene};
use crate::surfaces::shoulders::{self, Walls};
use fs_theme::color::Rgba;
use fs_theme::style::Radius;
use fs_theme::theme::Theme;
use serde_json::json;

use crate::services::theme::getter;

/// The deform amount for a popout.
const DEFORM_AMOUNT: f64 = 0.15;
/// Room past the card's resting rect for the overshoot, the deform's
/// stretch and the cast, on a surface sized to one card.
pub const SLACK: i32 = 48;
/// Past any output's edge, for the half-plane beyond the line.
const FAR: f64 = 1.0e5;
/// How far a `slide` card travels along its line, in logical pixels.
const SLIDE: f64 = 40.0;

/// What the card takes off the theme.
struct Look {
    fill: Rgba,
    border: Rgba,
    border_width: f64,
    radius: f64,
    chamfer: [f64; 4],
    casts: Vec<fs_theme::style::Cast<Rgba>>,
    /// Off the line, on the popover path: `popover` and `slide` both.
    popover: bool,
    /// The travel along the line a `slide` card makes, 0 for a popover.
    slide: f64,
    emerge_ms: f64,
    emerge_curve: Curve,
    drop: f64,
}

impl Look {
    fn new(theme: &Theme, role: &str, cast: bool) -> Self {
        let card = theme.box_style(role, None);
        let border = card.border.clone().unwrap_or(fs_theme::style::Line { color: Rgba::TRANSPARENT, width: 0.0 });
        let emerge = theme.habit("emerge").and_then(|v| v.as_str());
        let popover = matches!(emerge, Some("popover" | "slide"));
        let casts = if popover {
            card.casts.clone()
        } else if cast {
            let pantheon = Theme::resolve(
                getter(json!({ "theme": { "preset": "pantheon" } })),
                &fs_theme::palette::fallback(&theme.colors.mode),
            );
            pantheon.box_style("card", None).casts
        } else {
            Vec::new()
        };
        let motion = theme.motion();
        Self {
            fill: card.fill,
            border: border.color,
            border_width: border.width,
            radius: match card.radius {
                Radius::Px(px) => px,
                Radius::Pill => theme.radii.xl,
            },
            chamfer: card.chamfer,
            casts,
            popover,
            slide: if emerge == Some("slide") { SLIDE * theme.spacing_scale } else { 0.0 },
            emerge_ms: if theme.motion_enabled { motion.emerge } else { 0.0 },
            emerge_curve: Curve::from_table(&motion.emerge_curve),
            drop: theme.space.md,
        }
    }
}

/// The card this one hangs off (a tray item's menu off the second bar):
/// its span along the shared line and its corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub along: f64,
    pub length: f64,
    pub radius: f64,
}

/// The line's ends, for the walls: the output's length along the line and
/// what each end gives up to a frame ring (0 on a bare edge), and the corner
/// the ring's two lines meet in.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Ends {
    pub along: f64,
    pub inset_start: f64,
    pub inset_end: f64,
    pub radius: f64,
}

/// A gap one card opens in a line: the edge it lies on, its start and
/// width along that line and the fillets' reach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Join {
    pub edge: Edge,
    pub x: f64,
    pub width: f64,
    pub reach: f64,
}

pub struct Card {
    pub scene: Scene,
    look: Look,
    /// The box style `look` was resolved from, for a theme change.
    role: &'static str,
    pub edge: Edge,
    rest: Rect,
    live: Rect,
    line_at: f64,
    open: bool,
    mapped: bool,
    /// The pose lands at once (Presence's `bypass`, a handoff's halves).
    bypass: bool,
    /// Off for a card being handed over: it publishes no gap.
    joined: bool,
    progress: Animated,
    attach: Animated,
    deform: Deform,
    deform_running: bool,
    last_tick: Option<Instant>,
    scale: f64,
    pub target: Option<Target>,
    pub ends: Ends,
    /// A child's gap in this card's far edge, `(start, end)` along the line.
    pub far_gap: Option<(f64, f64)>,
    /// The frame's whole opacity: 0 cuts a handed-over card outright.
    pub frame_alpha: f32,
    /// The deform amount: the pill is small and travels its whole
    /// height, so it asks for more than a popout does.
    pub deform_amount: f64,
    casts: NodeId,
    shape: NodeId,
    /// Every gap this card opens this frame: the line's, and a walled side's.
    pub joins: Vec<Join>,
    /// Where content goes this frame, in surface pixels, and how opaque.
    pub content: (IRect, f32),
    /// A popover's frame this frame, in surface pixels, and how opaque.
    pub popover_frame: (IRect, f32),
    /// The compositor draws a popover's look off `popover_frame`; the
    /// card's own surface paints none of it.
    pub compositor_look: bool,
    /// The band content may draw in, held to the bud while it is clamped.
    pub clip: IRect,
}

impl Card {
    /// A card `size` (along, depth) centred along the line on `anchor`,
    /// clamped a `screenPadding` in from either end, a `barMargin` off the
    /// line at `line_at`, on a surface sized to it; `length` is the
    /// output's length along the edge.
    #[allow(clippy::too_many_arguments)]
    pub fn new(theme: &Theme, edge: Edge, length: i32, line_at: f64, anchor: f64, size: (f64, f64), scale: f64, cast: bool) -> Self {
        let (w, h) = size;
        let pad = theme.space.screen_padding;
        let u = (anchor - w / 2.0).clamp(pad, (length as f64 - w - pad).max(pad)).round();
        let v = line_at + theme.space.bar_margin;
        let rest = Rect::new(u, v, u + w, v + h);
        let depth = rest.y1 as i32 + SLACK;
        Self::build(theme, "card", edge, (length, depth), line_at, rest, scale, cast, false)
    }

    /// A card on a surface `size` (along, across) at `rest`, all in the
    /// card's own coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn build(theme: &Theme, role: &'static str, edge: Edge, size: (i32, i32), line_at: f64, rest: Rect, scale: f64, cast: bool, clear: bool) -> Self {
        let look = Look::new(theme, role, cast);
        let (sw, sh) = if edge.is_vertical() { (size.1, size.0) } else { size };
        let mut scene = if clear { Scene::clear(sw, sh) } else { Scene::new(sw, sh) };
        let empty = || Paint::Shape { fill: None, strokes: Vec::new() };
        let casts = scene.add(IRect::default(), empty());
        let shape = scene.add(IRect::default(), empty());
        for id in [casts, shape] {
            scene.set_visible(id, false);
        }
        let curve = look.emerge_curve;
        Self {
            scene,
            look,
            role,
            edge,
            rest,
            live: rest,
            line_at,
            open: true,
            mapped: false,
            bypass: false,
            joined: true,
            progress: Animated::new(0.0, curve),
            attach: Animated::new(1.0, SPATIAL_FAST),
            deform: Deform::new(),
            deform_running: false,
            last_tick: None,
            scale,
            target: None,
            ends: Ends::default(),
            far_gap: None,
            frame_alpha: 1.0,
            deform_amount: DEFORM_AMOUNT,
            casts,
            shape,
            joins: Vec::new(),
            content: (IRect::default(), 0.0),
            popover_frame: (IRect::default(), 0.0),
            compositor_look: false,
            clip: IRect::default(),
        }
    }

    /// The card as a menu: the `menu` role's fill, border and radius.
    pub fn menu(&mut self, theme: &Theme, cast: bool) {
        self.role = "menu";
        self.look = Look::new(theme, "menu", cast);
    }

    /// The frame drawn in `color` rather than its role's border.
    pub fn outline(&mut self, color: Rgba) {
        self.look.border = color;
    }

    /// The look resolved again off a new theme, the clocks left running: a
    /// card open across a mode flip or a wallpaper change kept the old fill
    /// under the new ink.
    pub fn restyle(&mut self, theme: &Theme, cast: bool) {
        self.look = Look::new(theme, self.role, cast);
    }

    /// The same card at `size`, centred on `anchor` along a line `length`
    /// long and kept `pad` in from either end, landing at once. The surface
    /// must already be deep enough.
    pub fn resize(&mut self, now: Instant, anchor: f64, length: i32, pad: f64, size: (f64, f64)) {
        let (w, h) = size;
        let u = (anchor - w / 2.0).clamp(pad, (length as f64 - w - pad).max(pad)).round();
        let rest = Rect::new(u, self.rest.y0, u + w, self.rest.y0 + h);
        self.set_rect(rest, rest);
        self.tick(now);
    }

    /// The node every other node on this surface paints above.
    pub fn top_node(&self) -> NodeId {
        self.shape
    }

    /// The surface's extent away from its edge.
    pub fn depth(&self) -> i32 {
        if self.edge.is_vertical() { self.scene.size.w } else { self.scene.size.h }
    }

    /// The card's resting rect in surface pixels.
    pub fn rest_rect(&self) -> IRect {
        Scene::cover(self.rest, self.edge_map(), 0.0)
    }

    /// The frame's rect this frame, in surface pixels, before its slide.
    pub fn live_rect(&self) -> IRect {
        Scene::cover(self.live, self.edge_map(), 0.0)
    }

    pub fn rest(&self) -> Rect {
        self.rest
    }

    pub fn live(&self) -> Rect {
        self.live
    }

    /// From the card's own (u, v) to surface pixels.
    pub fn edge_map(&self) -> Affine {
        let d = self.depth() as f64;
        match self.edge {
            Edge::Top => Affine::IDENTITY,
            Edge::Bottom => Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, d]),
            Edge::Left => Affine::new([0.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
            Edge::Right => Affine::new([0.0, 1.0, -1.0, 0.0, d, 0.0]),
        }
    }

    /// Moves where the card settles and where it is drawn now.
    pub fn set_rect(&mut self, rest: Rect, live: Rect) {
        self.rest = rest;
        self.live = live;
    }

    pub fn set_bypass(&mut self, now: Instant, bypass: bool) {
        self.bypass = bypass;
        if bypass {
            self.progress.jump(if self.open { 1.0 } else { 0.0 });
            self.deform = Deform::new();
        }
        let _ = now;
    }

    pub fn set_joined(&mut self, joined: bool) {
        self.joined = joined;
    }

    /// Presence's `mapped`: the enter starts once the window is on screen.
    pub fn mapped(&mut self, now: Instant) {
        if !self.mapped {
            self.mapped = true;
            self.set_open(now, self.open);
        }
    }

    pub fn set_open(&mut self, now: Instant, open: bool) {
        self.open = open;
        let held = open && !self.mapped;
        let target = if open && !held { 1.0 } else { 0.0 };
        if self.bypass {
            self.progress.jump(target);
        } else {
            self.progress.set(now, target, self.look.emerge_ms * self.scale);
        }
        self.tick(now);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Presence's pose, held to 0..1 the way an opacity riding it must be.
    pub fn pose(&self, now: Instant) -> f64 {
        if self.bypass { if self.open { 1.0 } else { 0.0 } } else { self.progress.value(now).clamp(0.0, 1.0) }
    }

    /// Joint's `attach`: 1 while the card still hangs off its line.
    pub fn attach(&self, now: Instant) -> f64 {
        self.attach.value(now).clamp(0.0, 1.0)
    }

    fn shown(&self, now: Instant) -> bool {
        self.open || self.progress.value(now) > 0.0
    }

    /// Gone: closed with the pose back under its closed rest.
    pub fn finished(&self, now: Instant) -> bool {
        !self.shown(now) || (!self.open && !self.progress.running(now))
    }

    /// Presence's `settled`: open (or closed) and standing still.
    pub fn settled(&self, now: Instant) -> bool {
        self.bypass || (self.mapped && !self.progress.running(now))
    }

    pub fn animating(&self, now: Instant) -> bool {
        !self.mapped || self.progress.running(now) || self.attach.running(now) || !self.deform.at_rest
    }

    /// Presence's `contentOpacity`: the card lands before its text.
    fn content_alpha(&self, pose: f64) -> f32 {
        if self.look.popover { pose.clamp(0.0, 1.0) as f32 } else { ((pose - 0.3) / 0.7).clamp(0.0, 1.0) as f32 }
    }

    pub fn tick(&mut self, now: Instant) {
        if self.look.popover {
            self.tick_popover(now);
        } else {
            self.tick_join(now);
        }
    }

    /// A popover's casts, fill and border for a card at `card`, `alpha`
    /// opaque, onto `nodes` of `scene`.
    fn paint_popover(look: &Look, scene: &mut Scene, nodes: (NodeId, NodeId), card: Rect, alpha: f32, shown: bool) {
        let radius = look.radius;
        let layers: Vec<Cast> = look
            .casts
            .iter()
            .map(|c| Cast { x: c.x, y: c.y, blur: c.blur, spread: c.spread, color: c.color.with_alpha(c.color.a * alpha) })
            .collect();
        let reach = Self::cast_reach(look);
        let rr = |r: Rect, radius: f64| vello_cpu::kurbo::Shape::to_path(&vello_cpu::kurbo::RoundedRect::from_rect(r, radius.max(0.0)), 0.1);
        let size = scene.size;
        scene.update_with(
            nodes.0,
            Scene::cover(card, Affine::IDENTITY, reach).intersect(&size),
            Paint::Casts { rect: card, radius, layers, cutout: rr(card, radius) },
            shown && !look.casts.is_empty(),
            Affine::IDENTITY,
            None,
        );
        let fill = look.fill.with_alpha(look.fill.a * alpha);
        let border = look.border.with_alpha(look.border.a * alpha);
        let paint = if look.chamfer.iter().any(|c| *c > 0.0) {
            crate::ui::boxes::chamfer_paint(card, look.chamfer, fill, border, look.border_width)
        } else {
            let half = look.border_width / 2.0;
            let mut line = rr(card.inflate(-half, -half), radius - half);
            line.close_path();
            Paint::Shape { fill: Some((rr(card, radius), fill)), strokes: vec![(line, border, look.border_width)] }
        };
        scene.update_with(nodes.1, Scene::cover(card, Affine::IDENTITY, 2.0), paint, shown, Affine::IDENTITY, None);
    }

    /// How far past the card a popover's casts reach.
    fn cast_reach(look: &Look) -> f64 {
        let reach = look.casts.iter().map(|c| c.blur + c.spread.max(0.0) + c.x.abs().max(c.y.abs())).fold(0.0, f64::max);
        reach * 1.25 + 2.0
    }

    /// Whether the card buds as a popover, whose look the compositor can
    /// carry (`popover_look`).
    pub fn popover(&self) -> bool {
        self.look.popover
    }

    /// The popover's look painted fully opaque at `card` onto `nodes` of
    /// `scene`.
    pub fn popover_look(&self, scene: &mut Scene, nodes: (NodeId, NodeId), card: Rect) {
        Self::paint_popover(&self.look, scene, nodes, card, 1.0, true);
    }

    /// The room a popover's casts need around the card.
    pub fn popover_margin(&self) -> i32 {
        Self::cast_reach(&self.look).ceil() as i32
    }

    fn tick_popover(&mut self, now: Instant) {
        // A popover never hangs off its line. `attach` starts at 1 for the
        // joined habit, and the scrim's band over the bar fades by 1 - attach.
        self.attach.jump(0.0);
        let pose = if self.bypass { if self.open { 1.0 } else { 0.0 } } else { self.progress.value(now) };
        let alpha = pose.clamp(0.0, 1.0) as f32 * self.frame_alpha;
        let shown = self.shown(now) && alpha > 0.0;
        let map = self.edge_map();
        let offset = if self.look.slide > 0.0 {
            // In from the end of the line the card rests nearer.
            let along = if self.edge.is_vertical() { self.scene.size.h } else { self.scene.size.w } as f64;
            let side = if self.live.center().x > along / 2.0 { 1.0 } else { -1.0 };
            vello_cpu::kurbo::Vec2::new(side * (1.0 - pose) * self.look.slide, 0.0)
        } else {
            // Toward the line it dropped out of.
            vello_cpu::kurbo::Vec2::new(0.0, -(1.0 - pose) * self.look.drop)
        };
        let at = self.live + offset;
        let device = Scene::cover(at, map, 0.0);
        let card = Rect::new(device.x as f64, device.y as f64, device.right() as f64, device.bottom() as f64);
        // Painted by the compositor instead (`popover_look`): the card's own
        // surface carries nothing of it.
        let here = shown && !self.compositor_look;
        Self::paint_popover(&self.look, &mut self.scene, (self.casts, self.shape), card, alpha, here);
        self.popover_frame = (device, if shown { alpha } else { 0.0 });
        self.content = (device, if shown { self.content_alpha(pose) * self.frame_alpha } else { 0.0 });
        self.clip = self.scene.size;
        self.joins.clear();
    }

    fn tick_join(&mut self, now: Instant) {
        let dt = self.last_tick.map(|t| now.saturating_duration_since(t).as_secs_f64()).unwrap_or(0.0);
        self.last_tick = Some(now);

        let border = self.look.border_width;
        let radius = self.look.radius;
        let (rest, live) = (self.rest, self.live);
        let rest_extent = rest.height();
        let extent = live.height();
        let (along, length) = (live.x0, live.width());
        let depth = (rest.y0 - self.line_at).max(0.0) + border;
        let travel = rest_extent + depth;
        let release_at = 0.85 - 0.35 * (depth / rest_extent.max(1.0)).clamp(0.0, 1.0);
        let amount = self.deform_amount * rest_extent / (rest_extent + depth).max(1.0);

        let pose = if self.bypass { if self.open { 1.0 } else { 0.0 } } else { self.progress.value(now) };
        let shown = self.shown(now);
        let slide = (1.0 - pose) * travel;
        let shape_depth = (extent + depth - slide).max(0.0);
        let neck = (depth - slide).max(0.0);

        // The span the card may bud in, and whether it is too
        // tight to bud at all.
        let spanned = self.target.is_some_and(|t| t.length > 0.0);
        let ends = self.ends;
        let (span_start, span_end) = match self.target {
            Some(t) if spanned => (t.along + t.radius, t.along + t.length - t.radius),
            _ => (ends.inset_start, ends.along - ends.inset_end),
        };
        let span_length = (span_end - span_start).max(0.0);
        let span_tight = spanned && span_length < 4.0 * radius;
        let joinable = self.joined && !span_tight;

        let attach_target = if joinable && !(self.open && pose >= release_at) { 1.0 } else { 0.0 };
        if shown && !self.bypass {
            self.attach.set(now, attach_target, SPATIAL_FAST_MS * self.scale);
        } else {
            self.attach.jump(attach_target);
        }
        let a = self.attach.value(now).clamp(0.0, 1.0);
        let near_inset = neck * (1.0 - a);
        let reach = shoulders::fillet_radius(radius * (2.0 * a - 1.0), shape_depth - near_inset);

        let can_wall = joinable && self.target.is_none() && radius > 0.0 && ends.along > 0.0 && rest.width() > 0.0;
        let room_start = rest.x0 - ends.inset_start;
        let room_end = ends.along - ends.inset_end - rest.x1;
        let wall = |room: f64, inset: f64| {
            if can_wall && room < radius { room.max(0.0) + if inset > 0.0 { border } else { radius } } else { -1.0 }
        };
        let (wall_start, wall_end) = (wall(room_start, ends.inset_start), wall(room_end, ends.inset_end));
        let clampable = joinable && (spanned || ends.along > 0.0);
        let bud_start = if clampable && wall_start < 0.0 { (along + length).min(along.max(span_start + radius)) } else { along };
        let bud_end = if clampable && wall_end < 0.0 { bud_start.max((along + length).min(span_end - radius)) } else { along + length };
        let clamped = bud_start > along || bud_end < along + length;
        let clamped_along = along + (bud_start - along) * a;
        let clamped_length = (along + length + (bud_end - along - length) * a - clamped_along).max(0.0);
        let span_clip = (shown && span_tight && slide > 0.0).then_some((span_start, span_length));
        let bud_clip = (shown && !span_tight && a > 0.0 && clamped).then_some((clamped_along - reach, clamped_length + reach * 2.0));
        let along_pivot = if clamped && a > 0.0 {
            clamped_along + clamped_length / 2.0 - along
        } else if (wall_start >= 0.0) == (wall_end >= 0.0) {
            length / 2.0
        } else if wall_start >= 0.0 {
            -wall_start
        } else {
            length + wall_end
        };
        let pivot_inset = (depth - slide) * a;

        let frame_y = live.y0 - slide;
        let settled = self.settled(now);
        let running = !settled || !self.deform.at_rest;
        if running && !self.bypass {
            if !self.deform_running {
                self.deform.unsample();
            }
            self.deform.step(dt, live.x0, frame_y, extent, amount);
        }
        self.deform_running = running;

        let d = &self.deform;
        let (cx, cy) = (along_pivot, -pivot_inset);
        let matrix = Affine::new([d.m00, d.m01, d.m01, d.m11, 0.0, 0.0]);
        let frame = Affine::translate((live.x0, frame_y)) * Affine::translate((cx, cy)) * matrix * Affine::translate((-cx, -cy));

        let map = self.edge_map();
        let cut = rest.y0 - depth;
        let mut band_local = Rect::new(-FAR, cut.floor(), FAR, FAR);
        if let Some((s, l)) = span_clip {
            band_local.x0 = s;
            band_local.x1 = s + l;
        }
        let band = Scene::cover(band_local, map, 0.0).intersect(&self.scene.size);

        let walls = Walls { start: wall_start, end: wall_end, attach: a, radius: ends.radius };
        let over = shoulders::overhang(radius, wall_start, wall_end);
        let lift = shoulders::far_overhang(radius, wall_start, wall_end);
        let origin = clamped_along - over;
        let item_w = clamped_length + over * 2.0;
        let item_h = shape_depth + lift;
        let item = map * frame * Affine::translate((origin - along, slide - depth));
        let item_rect = Rect::new(0.0, 0.0, item_w, item_h);
        let visible = shown && shape_depth > 0.0;
        let far_gap = self.far_gap.map(|(s, e)| (s - origin, e - origin));
        let paths = shoulders::paths_with(item_w, item_h, radius, border, a, near_inset, walls, far_gap);
        let fa = self.frame_alpha;
        let fill = self.look.fill.with_alpha(self.look.fill.a * fa);
        let line = self.look.border.with_alpha(self.look.border.a * fa);
        let near = line.with_alpha(line.a * (1.0 - a) as f32);
        let bounds = Scene::cover(item_rect, item, 2.0).intersect(&band);
        let cutout = paths.fill.clone();
        self.scene.update_with(
            self.shape,
            bounds,
            Paint::Shape { fill: Some((paths.fill, fill)), strokes: vec![(paths.outer, line, border), (paths.near, near, border)] },
            visible && fa > 0.0,
            item,
            Some(band),
        );

        let cast_alpha = (1.0 - a) as f32 * fa;
        let layers: Vec<Cast> = self
            .look
            .casts
            .iter()
            .map(|c| Cast { x: c.x, y: c.y, blur: c.blur, spread: c.spread, color: c.color.with_alpha(c.color.a * cast_alpha) })
            .collect();
        let reach_out = layers.iter().map(|c| c.blur + c.spread.max(0.0) + c.y.abs()).fold(0.0, f64::max);
        let card = Rect::new(along - origin, near_inset, along - origin + length, shape_depth);
        self.scene.update_with(
            self.casts,
            Scene::cover(card, item, reach_out * 1.25 + 2.0).intersect(&band),
            Paint::Casts { rect: card, radius, layers, cutout },
            visible && cast_alpha > 0.0 && !self.look.casts.is_empty(),
            item,
            Some(band),
        );

        // Content rides the card's slide, unmirrored and undeformed.
        let at = Rect::new(live.x0, frame_y, live.x1, frame_y + extent);
        self.content = (Scene::cover(at, map, 0.0), if visible { self.content_alpha(pose) * fa } else { 0.0 });
        let content_cut = Rect::new(-FAR, self.line_at + border, FAR, FAR);
        let mut clip = Scene::cover(content_cut, map, 0.0).intersect(&self.scene.size).intersect(&band);
        if let Some((s, l)) = bud_clip {
            clip = clip.intersect(&Scene::cover(Rect::new(s, -FAR, s + l, FAR), map, 0.0));
        }
        self.clip = clip;

        self.joins.clear();
        if joinable && shown && a > 0.0 {
            let out_start = if wall_start >= 0.0 { wall_start * a } else { 0.0 };
            let out_end = if wall_end >= 0.0 { wall_end * a } else { 0.0 };
            let start = clamped_along - out_start;
            let len = clamped_length + out_start + out_end;
            self.joins.push(Join { edge: self.edge, x: start + len * (1.0 - a) / 2.0, width: len * a, reach });
            let (first, last) = if self.edge.is_vertical() { (Edge::Top, Edge::Bottom) } else { (Edge::Left, Edge::Right) };
            // A walled side's own line runs across the output; the gap sits
            // on it from the line down to the shape's far edge, in the
            // output's coordinates along that wall.
            let from = Scene::cover(Rect::new(0.0, cut, 0.0, cut + shape_depth), map, 0.0);
            let (wx, ww) = if self.edge.is_vertical() { (from.x as f64, from.w as f64) } else { (from.y as f64, from.h as f64) };
            for (k, w) in [(first, wall_start), (last, wall_end)] {
                if w >= 0.0 {
                    self.joins.push(Join { edge: k, x: wx, width: ww, reach });
                }
            }
        }
    }

    /// Where content goes once the card has settled on its live rect: an
    /// owner drawing on a [`crate::surfaces::modal::Layer`] lays out here,
    /// and the layer is moved by the difference.
    pub fn content_rest(&self) -> IRect {
        Scene::cover(self.live, self.edge_map(), 0.0)
    }

    /// The band past the line, where content may draw.
    pub fn content_clip(&self) -> IRect {
        if self.clip.is_empty() {
            let cut = self.line_at + self.look.border_width;
            return Scene::cover(Rect::new(-FAR, cut, FAR, FAR), self.edge_map(), 0.0).intersect(&self.scene.size);
        }
        self.clip
    }

    /// The card's own join along its line, as the bar takes it.
    pub fn join(&self) -> Option<(f64, f64, f64)> {
        self.joins.first().map(|j| (j.x, j.width, j.reach))
    }

    pub fn radius(&self) -> f64 {
        self.look.radius
    }

    pub fn border_width(&self) -> f64 {
        self.look.border_width
    }
}

/// A full-output black scrim on the pose a modal drawer rides,
/// here on its own `emerge` clock with no card in front of it. It rasters
/// nothing: its surface is one black pixel the compositor scales and fades.
pub struct Scrim {
    open: bool,
    mapped: bool,
    pose: Animated,
    scale: f64,
    /// The `scrim` role's fill alpha over its plain black.
    alpha: f64,
}

impl Scrim {
    pub fn new(theme: &Theme, scale: f64) -> Self {
        let alpha = f64::from(theme.box_style("scrim", None).fill.a);
        Self { open: true, mapped: false, pose: Animated::new(0.0, crate::motion::SPATIAL), scale, alpha }
    }

    pub fn mapped(&mut self, now: Instant) {
        if !self.mapped {
            self.mapped = true;
            self.set_open(now, self.open);
        }
    }

    pub fn set_open(&mut self, now: Instant, open: bool) {
        self.open = open;
        let target = if open && self.mapped { 1.0 } else { 0.0 };
        self.pose.set(now, target, EMERGE.0 * self.scale);
    }

    pub fn animating(&self, now: Instant) -> bool {
        !self.mapped || self.pose.running(now)
    }

    pub fn finished(&self, now: Instant) -> bool {
        !self.open && !self.pose.running(now)
    }

    /// The `scrim` role's fill alpha, which its pixel carries.
    pub fn tone(&self) -> f64 {
        self.alpha
    }

    /// How far in the scrim is on this frame, over its own alpha.
    pub fn fade(&self, now: Instant) -> f64 {
        // The spatial pose overshoots both ends; an opacity may not.
        self.pose.value(now).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::resolve(getter(json!({})), &fs_theme::palette::fallback("dark"))
    }

    /// The same card on every edge rests a `barMargin` off the line, centred
    /// on its anchor along it, inside the surface.
    #[test]
    fn rests_off_the_line_on_every_edge() {
        let t = theme();
        let margin = t.space.bar_margin as i32;
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let card = Card::new(&t, edge, 1080, 40.0, 500.0, (200.0, 60.0), 1.0, false);
            let r = card.rest_rect();
            let depth = card.depth();
            let (along, off) = match edge {
                Edge::Top => (r.x, r.y),
                Edge::Bottom => (r.x, depth - r.bottom()),
                Edge::Left => (r.y, r.x),
                Edge::Right => (r.y, depth - r.right()),
            };
            assert_eq!((along, off), (400, 40 + margin), "{edge:?} {r:?}");
            assert!(card.scene.size.intersect(&r) == r, "{edge:?} outside its surface");
            let clip = card.content_clip();
            assert!(clip.intersect(&r) == r, "{edge:?} content clipped off its own rest");
        }
    }

    /// Under `emerge: slide` a card opens from 40px along its line, toward
    /// the end of the line it rests nearer, and lands on its rest.
    #[test]
    fn a_slide_card_comes_in_along_its_line() {
        let t = Theme::resolve(getter(json!({ "theme": { "preset": "cartridge" } })), &fs_theme::palette::fallback("dark"));
        let travel = SLIDE * t.spacing_scale;
        for (anchor, side) in [(1600.0, 1.0), (300.0, -1.0)] {
            let now = Instant::now();
            let mut card = Card::new(&t, Edge::Top, 1920, 40.0, anchor, (200.0, 60.0), 1.0, false);
            card.mapped(now);
            let rest = card.rest_rect();
            let (frame, _) = card.popover_frame;
            assert_eq!(frame.y, rest.y);
            assert!((f64::from(frame.x - rest.x) - side * travel).abs() <= 1.0, "{anchor}: {frame:?} against {rest:?}");
            card.tick(now + std::time::Duration::from_secs(2));
            assert_eq!(card.popover_frame.0, rest);
        }
    }
}
