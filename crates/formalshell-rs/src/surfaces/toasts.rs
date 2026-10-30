//! The sonner depth stack. One overlay surface down the stack's own side,
//! the output's height, for as long as anything is popped up, so the
//! collapse and expand reflow moves cards inside a static window rather than
//! resizing a layer surface the compositor would animate against the shell's
//! own motion; the input region is the stack's own rect, and empty on a
//! covered output.
//!
//! Collapsed is the depth stack (the front card, two levels peeking behind
//! it as empty chrome at the front card's height), expanded the plain
//! column, both laid out by fs-info's `toast_stack`.
//!
//! Each group keeps its slot across draws, so a card retargets rather than
//! snaps: x, y, width and height ride the table's `restack` clock (y behind
//! its rank's share of `restack.stagger`), presence rides `arrive` in and
//! `restack` out. A bubble unfolds off its own top edge where it lands and
//! fades where it is. A group that left keeps its slot, its card and its
//! last place while it fades.
//!
//! Activity's live groups ride the same stack as cards of their own, keyed
//! `activity:<source>` ([`activity_groups`]): one per fspec operation or
//! app progress, updated in place, never timing out while it runs. A
//! notification carrying a `value` hint is its own live card and draws the
//! same ring inside its bubble instead.
//!
//! A two-finger horizontal swipe over a card carries it along x, sonner's
//! swipe: toward the stack's own edge it follows the fingers, the other way
//! it pulls against the rubber band, and the lift either sends it on off
//! that edge or springs it back.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use fs_info::notifications::{self as model, Group, Urgency};
use fs_info::toast_stack::{self, StackParams};
use fs_theme::theme::Theme;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use vello_cpu::kurbo::{Affine, Rect};

use crate::motion::{Animated, Curve, EFFECTS, Kind};
use crate::scene::{Bitmap, IRect, NodeId, Scene};
use crate::scroll::{self, Travel, Velocity};
use crate::services::notifications::now_ms;
use crate::store::Store;
use crate::surface::Surface;
use crate::surfaces::activity::{self, Verb};
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::ui::{self, El, Ink, Type, Ui, Variant, Weight, w};

const MAX_PEEK_LEVELS: usize = 2;
/// sonner's SWIPE_THRESHOLD: the travel past which a lift dismisses.
const SWIPE_DISTANCE: f64 = 45.0;
/// sonner's 0.11px/ms: a flick this fast dismisses however short it was.
const SWIPE_SPEED: f64 = 110.0;

/// A lift with the card `dx` off its place (the fingers' travel, not the
/// drawn stretch) at `speed` px/s, both along x: whether it goes. `dir` is
/// the stack's own edge, 1 right and -1 left.
fn swipe_dismisses(dx: f64, speed: f64, dir: f64) -> bool {
    dx * dir >= SWIPE_DISTANCE || speed * dir > SWIPE_SPEED
}

/// The drawn offset for `pull` px of finger travel: one for one toward the
/// stack's edge, the rubber band against it.
fn swipe_shown(pull: f64, dir: f64, width: f64) -> f64 {
    if pull * dir >= 0.0 { pull } else { scroll::rubber(pull, width) }
}

/// The swipe under way: the card it took at its first frame and the
/// fingers' travel since, in card pixels.
struct Swipe {
    key: String,
    pull: f64,
    velocity: Velocity,
}

/// How often the relative times ("2m ago") recompute, off their own clock.
pub const REL_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// What a click on a card asks for.
pub enum Act {
    None,
    /// The card's X: every member of its group.
    Dismiss(Vec<String>),
    Action(String, String),
    /// The body: the default action when there is one, else the sender's
    /// window, and the toast goes either way.
    Body(Vec<String>),
    /// An Activity card's row verb, by source.
    Activity(Verb, String),
}

const ACTIVITY: &str = "activity:";

/// The Activity groups that get a card now: every `toasts()` source, held
/// back under Do Not Disturb unless it failed.
pub fn activity_groups(store: &Store) -> Vec<fs_activity::Group<'_>> {
    let a = &store.activity.activity;
    let cards = a.toasts();
    let dnd = store.notifications.model.dnd;
    a.groups()
        .into_iter()
        .filter(|g| cards.contains(&g.source) && (!dnd || g.state() == fs_activity::State::Failed))
        .collect()
}

/// One card's place in the pile: notifications by arrival, an Activity
/// group by when it started or ended, a failure ranked as critical.
#[derive(Clone)]
struct Rank {
    key: String,
    at: i64,
    urgency: Urgency,
}

impl model::Ranked for Rank {
    fn arrived_at(&self) -> i64 {
        self.at
    }

    fn urgency(&self) -> Urgency {
        self.urgency
    }
}

/// What a card draws and holds, whichever kind it is.
struct Card {
    el: El,
    critical: bool,
    members: Vec<String>,
}

struct Slot {
    nodes: Vec<NodeId>,
    ui: Ui,
    /// The card as it last drew, kept for the fade of a group that left.
    el: El,
    critical: bool,
    /// Where it is drawn this frame, for the pointer.
    rect: IRect,
    z: i64,
    members: Vec<String>,
    x: Animated,
    y: Animated,
    width: Animated,
    height: Animated,
    /// 0 off the stack, 1 shown: the card's opacity.
    presence: Animated,
    /// 0 for a card deeper than the last peek level, which keeps its place
    /// behind it unseen.
    visible: Animated,
    /// The card's words over the peek level's bare chrome.
    content: Animated,
    /// The bubble unfolding off its own top edge on `arrive`.
    reveal: Animated,
    /// The group is gone and the card is fading where it was.
    departing: bool,
    /// The card's own widgets still crossfading (a button's ink).
    ui_animating: bool,
    /// The swipe's offset along x: under the fingers, springing back to 0,
    /// or carrying on off the edge.
    dx: Animated,
}

impl Slot {
    fn moving(&self, now: Instant) -> bool {
        [&self.x, &self.y, &self.width, &self.height, &self.presence, &self.dx].iter().any(|a| a.running(now))
    }

    fn animating(&self, now: Instant) -> bool {
        self.moving(now) || self.content.running(now) || self.reveal.running(now) || self.visible.running(now) || self.ui_animating
    }

    /// Faded out and no longer anything to draw.
    fn gone(&self, now: Instant) -> bool {
        self.departing && !self.presence.running(now)
    }
}

/// One card's place this frame, worked out before any card paints so each
/// knows the cards in front of it.
struct Placed {
    key: String,
    fx: f64,
    fy: f64,
    fh: f64,
    matrix: Affine,
    /// The card as it lands on screen, scaled.
    drawn: IRect,
    alpha: f64,
    /// Off its place along x, so its edge covers nothing behind it.
    shifted: bool,
}

pub struct Toasts {
    pub surface: Surface,
    pub scene: Scene,
    slots: HashMap<String, Slot>,
    region: Option<(IRect, bool)>,
    stack: IRect,
    /// The pointer over the stack: it fans out and every toast in it holds.
    pub hovered: bool,
    pressed: Option<(String, String)>,
    /// Pictures fitted to the icon slot, per entry.
    fitted: HashMap<String, (f64, Option<Bitmap>)>,
    /// When the relative times were last recomputed.
    pub rel_at: Instant,
    /// The last draw ran with a clock still running.
    pub was_animating: bool,
    swipe: Option<Swipe>,
    /// The stack hangs off the right edge.
    right: bool,
    /// The side and width the layer was made with.
    pub column: (bool, u32),
    layer: Layer,
    /// The output the layer was made on.
    pub output: Option<WlOutput>,
}

/// Where the stack hangs: the bar's own edge cleared, `screenPadding` in.
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

/// Elementary's bubble: the picture in a
/// `controlHeight` slot beside the words, a semibold summary, and the close
/// button only while the pointer is over the bubble.
fn bubble(theme: &Theme, g: &Group, now: i64, picture: Option<Bitmap>, hovered: bool, progress: Option<&fs_activity::Item>) -> El {
    let s = &theme.space;
    let e = &g.entry;
    let critical = e.urgency == Urgency::Critical;
    let rel = model::rel_time(now, e.arrived_at);
    let meta = if g.count > 1 { format!("{rel}  x{}", g.count) } else { rel };
    let slot = s.control_height;
    let mark = match picture.filter(|_| !critical) {
        Some(b) => w::picture(Some(b), slot),
        None => w::row(
            0.0,
            vec![w::icon(if critical { "triangle-alert" } else { "bell" })
                .size(Type::Heading)
                .ink(if critical { Ink::Destructive } else { Ink::Muted })],
        )
        .width(ui::Size::Px(slot))
        .centred(),
    };
    let mark = if e.source == "iphone" { mark.badge("smartphone") } else { mark };
    let mut head = vec![w::section_label(s, &e.app_name, None, false), w::caption(meta).mono().fill()];
    // A bare caption-sized glyph rather than an icon button: the button is
    // `controlHeight` tall, so the header grew by it on hover.
    head.push(if hovered {
        w::icon("x").size(Type::Caption).ink(Ink::Muted).width(ui::Size::Px(slot)).on("close")
    } else {
        w::space(slot)
    });
    let mut words = vec![w::row(s.icon_gap, head), w::para(e.summary.clone(), Type::Body, Weight::Semibold, Ink::Fg, 2)];
    let body = model::sanitize_body(&e.body, &e.app_name, &e.app_icon);
    if !body.is_empty() {
        words.push(w::para(body, Type::BodySmall, Weight::Normal, Ink::Muted, 2));
    }
    if let Some(item) = progress {
        words.push(w::row(s.sm, vec![w::ring(activity::ring(item)), w::caption(item.progress.words().unwrap_or_default()).mono().ink(Ink::Muted)]));
    }
    let actions = model::button_actions(e);
    if !actions.is_empty() {
        let buttons = actions.iter().map(|a| w::button(a.label.clone()).variant(Variant::Outline).on(format!("action:{}", a.key))).collect();
        words.push(w::row(s.sm, buttons).pad(0.0, s.row_gap, 0.0, 0.0));
    }
    w::row(s.sm, vec![mark, w::column(s.row_gap, words).fill()]).top().on("body")
}

fn card_width(theme: &Theme) -> f64 {
    theme.space.popup_width_bubble
}

/// The widest cast a card throws, which moves with the theme alone.
pub fn column_cast(theme: &Theme) -> f64 {
    ["rest", "critical"]
        .iter()
        .map(|k| fs_theme::style::geometry::cast_pad(&theme.box_style("notification", Some(k)).casts))
        .fold(0.0, f64::max)
}

/// How far the stack's layer reaches in from its own edge: the edge's
/// inset, `screenPadding`, the card, and room past it for the swipe's
/// rubber band (`scroll::rubber`, 40px at most) and the card's cast.
pub fn column_width(theme: &Theme, cast: f64, insets: &Insets, right: bool) -> u32 {
    let inset = if right { insets.right } else { insets.left };
    (inset + theme.space.screen_padding * 2.0 + card_width(theme) + 40.0 + cast).ceil() as u32
}

impl Toasts {
    pub fn new(surface: Surface, size: (i32, i32), column: (bool, u32), layer: Layer, output: Option<WlOutput>) -> Self {
        Self {
            layer,
            output,
            column,
            surface,
            scene: Scene::clear(size.0, size.1),
            slots: HashMap::new(),
            region: None,
            stack: IRect::default(),
            hovered: false,
            pressed: None,
            fitted: HashMap::new(),
            rel_at: Instant::now(),
            was_animating: false,
            swipe: None,
            right: true,
        }
    }

    pub fn expanded(&self, store: &Store) -> bool {
        self.hovered || store.notifications.stack_expanded
    }

    pub fn draw(&mut self, store: &Store, theme: &Theme, kit: &mut Kit, insets: &Insets, scale: f64, now: Instant) {
        let s = &theme.space;
        let n = &store.notifications;
        let spec = model::position_spec(store.config.str("notifications.position"));
        self.right = spec.right;
        let groups = model::group_entries(&n.model.popups);
        let live = activity_groups(store);
        let mut ranks: Vec<Rank> =
            groups.iter().map(|g| Rank { key: model::group_key(g), at: g.arrived_at, urgency: g.urgency }).collect();
        ranks.extend(live.iter().map(|g| {
            let lead = g.lead();
            let failed = g.state() == fs_activity::State::Failed;
            Rank { key: format!("{ACTIVITY}{}", g.source), at: lead.ended_ms.unwrap_or(lead.started_ms), urgency: if failed { Urgency::Critical } else { Urgency::Normal } }
        }));
        let stack_order: Vec<String> = model::stack_order(&ranks).into_iter().map(|r| r.key).collect();
        let by_key: HashSet<String> = stack_order.iter().cloned().collect();
        // The column fans the pile out in its own order, front card against
        // the anchored edge, so no card trades places on the way: the column
        // lays out top-down, which puts a bottom stack's front card last.
        let mut column = stack_order.clone();
        if !spec.top {
            column.reverse();
        }

        let motion = theme.motion();
        let restack = (motion.restack * scale, Curve::from_table(&motion.restack_curve));
        let arrive = (motion.arrive * scale, Curve::from_table(&motion.arrive_curve));
        let effects = motion.families.effects * scale;

        // A group that left fades where it was; one back before its fade
        // ended takes its slot again.
        for (k, slot) in self.slots.iter_mut() {
            let live = by_key.contains(k);
            if !live && !slot.departing {
                slot.departing = true;
                slot.presence.set_on(now, 0.0, restack.0, restack.1);
            } else if live && slot.departing {
                slot.departing = false;
                slot.presence.set_on(now, 1.0, arrive.0, arrive.1);
            }
        }
        let gone: Vec<String> = self.slots.iter().filter(|(_, sl)| sl.gone(now)).map(|(k, _)| k.clone()).collect();
        for k in gone {
            if let Some(mut slot) = self.slots.remove(&k) {
                for id in slot.nodes.drain(..) {
                    self.scene.remove(id);
                }
                slot.ui.hide(&mut self.scene);
            }
        }

        let width = card_width(theme);
        let clock = now_ms();
        let size = theme.font_size.heading.round() as u32;
        let scale = crate::scene::raster_scale();
        self.fitted.retain(|id, (k, _)| n.icons.contains_key(id) && *k == scale);
        let mut cards: HashMap<String, Card> = HashMap::new();
        for g in &groups {
            let k = model::group_key(g);
            let pic = n.icons.get(&g.id).and_then(|raw| {
                self.fitted.entry(g.id.clone()).or_insert_with(|| (scale, raw.bitmap(size, scale))).1.clone()
            });
            let hovered = self.slots.get(k.as_str()).is_some_and(|sl| sl.ui.hover.is_some());
            let progress = store.activity.activity.get(&fs_activity::notification_id(g.id.parse().unwrap_or(u32::MAX)));
            let el = bubble(theme, g, clock, pic, hovered, progress);
            cards.insert(k, Card { el, critical: g.urgency == Urgency::Critical, members: g.member_ids.clone() });
        }
        for g in &live {
            let el = activity::row(theme, store, g, true);
            let critical = g.state() == fs_activity::State::Failed;
            cards.insert(format!("{ACTIVITY}{}", g.source), Card { el, critical, members: Vec::new() });
        }
        let mut heights = HashMap::new();
        for (k, c) in &cards {
            let inner = ui::measure(&c.el, width - s.panel_padding * 2.0, theme, kit).1;
            heights.insert(k.clone(), inner + s.panel_padding * 2.0);
        }
        let layout = toast_stack::layout(&StackParams {
            frame_width: width,
            peek_inset: s.md,
            peek_offset: s.panel_padding,
            max_peek_levels: MAX_PEEK_LEVELS,
            gap: s.panel_padding,
            top: spec.top,
            heights: heights.clone(),
            collapsed: stack_order.iter().cloned().map(Some).collect(),
            expanded: column.iter().cloned().map(Some).collect(),
        });
        let expanded = self.expanded(store);
        let (sw, sh) = (self.scene.size.w as f64, self.scene.size.h as f64);
        let max_h = (sh - insets.top - insets.bottom - s.screen_padding * 2.0).max(0.0);
        let stack_h = max_h.min(if expanded { layout.expanded_height } else { layout.collapsed_height });
        let x0 = if spec.left { insets.left + s.screen_padding } else { sw - insets.right - s.screen_padding - width };
        let y0 = if spec.top { insets.top + s.screen_padding } else { sh - insets.bottom - s.screen_padding - stack_h };
        self.stack = if by_key.is_empty() {
            IRect::default()
        } else {
            IRect::new(x0.round() as i32, y0.round() as i32, width.round() as i32, stack_h.round() as i32)
        };

        // The restack's window shared out over the cards still live, each
        // waiting its own rank's share.
        let stagger = (motion.restack_stagger * scale / by_key.len().max(1) as f64).round();
        for k in &stack_order {
            let Some(geom) = layout.by_key.get(k) else { continue };
            let Some(g) = (if expanded { geom.expanded.or(geom.collapsed) } else { geom.collapsed }) else { continue };
            let h = g.height;
            let (gx, gy) = (x0 + g.x, y0 + g.y);
            let content = if g.content_visible { 1.0 } else { 0.0 };
            let slot = self.slots.entry(k.clone()).or_insert_with(|| {
                let mut reveal = Animated::new(0.0, arrive.1);
                reveal.set(now, 1.0, arrive.0);
                let mut presence = Animated::new(0.0, arrive.1);
                presence.set(now, 1.0, arrive.0);
                Slot {
                    nodes: Vec::new(),
                    ui: Ui::new(None),
                    el: cards[k].el.clone(),
                    critical: false,
                    rect: IRect::default(),
                    z: g.z,
                    members: Vec::new(),
                    x: Animated::new(gx, restack.1),
                    y: Animated::new(gy, restack.1),
                    width: Animated::new(g.width, restack.1),
                    height: Animated::new(h, restack.1),
                    presence,
                    visible: Animated::new(1.0, EFFECTS),
                    content: Animated::new(content, EFFECTS),
                    reveal,
                    departing: false,
                    ui_animating: false,
                    dx: Animated::new(0.0, Kind::Spatial.curve()),
                }
            });
            slot.x.set(now, gx, restack.0);
            slot.y.set_after(now, gy, restack.0, stagger * g.rank as f64);
            slot.width.set(now, g.width, restack.0);
            // A card still arriving takes its content in the frame it is
            // measured in, so nothing grows and slides at once.
            if slot.presence.running(now) {
                slot.height.jump(h);
            } else {
                slot.height.set(now, h, restack.0);
            }
            slot.content.set(now, content, effects);
            slot.visible.set(now, if g.visible { 1.0 } else { 0.0 }, effects);
            slot.z = g.z;
            slot.el = cards[k].el.clone();
            slot.critical = cards[k].critical;
            slot.members = cards[k].members.clone();
        }

        // Back to front, so the front card's chrome covers the peeks.
        let mut order: Vec<(i64, String)> = self.slots.iter().map(|(k, sl)| (sl.z, k.clone())).collect();
        order.sort();
        let placed: Vec<Placed> = order
            .iter()
            .map(|(_, k)| {
                let slot = &self.slots[k];
                let presence = slot.presence.value(now);
                let w = slot.width.value(now);
                let fh = slot.height.value(now);
                // A bubble unfolds where it lands; one swiped away leaves
                // along x alone.
                let dx = slot.dx.value(now);
                // Every card is drawn at the column's full width and scaled
                // to its level's, words and all, about the edge its sliver
                // shows from, so a card behind coming forward grows as one
                // piece rather than its box widening under words already
                // full width.
                let fx = slot.x.value(now) + (w - width) / 2.0 + dx;
                let fy = slot.y.value(now);
                let sc = (w / width).clamp(0.0, 1.0);
                let pivot = (fx + width / 2.0, if spec.top { fy + fh } else { fy });
                let (sw, sh) = (width * sc, fh * sc);
                let sy = if spec.top { fy + fh - sh } else { fy };
                let swiped = (1.0 - dx.abs() / width).clamp(0.0, 1.0);
                let alpha = presence.clamp(0.0, 1.0) * slot.visible.value(now).clamp(0.0, 1.0) * swiped;
                Placed {
                    key: k.clone(),
                    fx,
                    fy,
                    fh,
                    matrix: Affine::translate(pivot) * Affine::scale(sc) * Affine::translate((-pivot.0, -pivot.1)),
                    drawn: IRect::new((pivot.0 - sw / 2.0).round() as i32, sy.round() as i32, sw.round() as i32, sh.round() as i32),
                    alpha,
                    shifted: dx != 0.0,
                }
            })
            .collect();

        let mut anchor: Option<NodeId> = None;
        for (i, at) in placed.iter().enumerate() {
            // A card stops at the edge of the settled card in front of it,
            // so nothing behind ever shows through that card's edges and
            // antialiased border: the pile reads as solid cards. One still
            // fading in or out covers nothing yet.
            let solid = placed[i + 1..].iter().filter(|f| f.alpha >= 0.999 && !f.shifted && !f.drawn.is_empty());
            let cut = if spec.top { solid.map(|f| f.drawn.bottom()).max() } else { solid.map(|f| f.drawn.y).min() };
            let slot = self.slots.get_mut(&at.key).expect("ordered from the map");
            slot.rect = at.drawn;
            let b = theme.box_style("notification", Some(if slot.critical { "critical" } else { "rest" }));
            let (fx, fy, fh) = (at.fx, at.fy, at.fh);
            let rect = IRect::new(fx.round() as i32, fy.round() as i32, width.round() as i32, fh.round() as i32);
            let alpha = at.alpha as f32;
            let radius = theme.box_radius(&b, rect.h as f64);
            let shown = IRect::new(rect.x, rect.y, rect.w, (rect.h as f64 * slot.reveal.value(now).clamp(0.0, 1.0)).round() as i32);
            let limit = |r: IRect| match cut {
                Some(c) if spec.top => {
                    let top = r.y.max(c);
                    IRect::new(r.x, top, r.w, (r.bottom() - top).max(0))
                }
                Some(c) => IRect::new(r.x, r.y, r.w, (r.h.min(c - r.y)).max(0)),
                None => r,
            };
            // The box opens out with the unfold, so its cast opens with it
            // rather than being cut to the card's own rect.
            let reach = fs_theme::style::geometry::cast_pad(&b.casts).ceil() as i32;
            let around = limit(IRect::new(rect.x - reach, rect.y - reach, rect.w + reach * 2, shown.h + reach * 2));
            for id in &slot.nodes {
                self.scene.set_transform(*id, Affine::IDENTITY);
            }
            let mut p = Painter::new(&mut self.scene, &mut slot.nodes, Some(around)).after(anchor);
            ui::boxes::paint(&mut p, shown, &b, radius, alpha, 0.0);
            p.finish();
            for id in &slot.nodes {
                let own = self.scene.transform_of(*id);
                self.scene.set_transform(*id, at.matrix * own);
            }
            anchor = slot.nodes.last().copied().or(anchor);
            let content = slot.content.value(now).clamp(0.0, 1.0) as f32 * alpha;
            if content > 0.0 {
                slot.ui.motion_scale = 1.0;
                let pad = s.panel_padding;
                let inner = Rect::new(fx + pad, fy + pad, fx + width - pad, fy + fh - pad);
                // The words go right over their own box, never over the
                // card in front of it, and the next card stacks over them.
                slot.ui.anchor = anchor;
                slot.ui.set_transform(&mut self.scene, Affine::IDENTITY);
                let drawn = slot.ui.draw(&slot.el, inner, Some(limit(shown)), content, theme, kit, &mut self.scene, now);
                slot.ui.set_transform(&mut self.scene, at.matrix);
                slot.ui_animating = drawn.animating;
                anchor = drawn.last.or(anchor);
            } else {
                slot.ui_animating = false;
                slot.ui.hide(&mut self.scene);
            }
        }
    }

    /// The stack's rect as the input region, or nothing while `passive`.
    /// Whether it or `passive` changed, which the next commit has to carry.
    pub fn sync_region(&mut self, compositor: &smithay_client_toolkit::compositor::CompositorState, passive: bool) -> bool {
        let want = if passive { IRect::default() } else { self.stack };
        if self.region == Some((want, passive)) {
            return false;
        }
        self.region = Some((want, passive));
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor) {
            if !want.is_empty() {
                region.add(want.x, want.y, want.w, want.h);
            }
            self.surface.layer.set_input_region(Some(region.wl_region()));
        }
        true
    }

    /// Moves the column to `layer`. Whether it changed, which the next
    /// commit has to carry.
    pub fn sync_layer(&mut self, layer: Layer) -> bool {
        if self.layer == layer {
            return false;
        }
        self.layer = layer;
        self.surface.layer.set_layer(layer);
        true
    }

    fn contains(r: IRect, x: f64, y: f64) -> bool {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
    }

    /// The card under a point, front first, and what in it.
    fn hit(&self, x: f64, y: f64) -> Option<(String, String)> {
        let mut over: Vec<(&String, &Slot)> =
            self.slots.iter().filter(|(_, s)| !s.departing && Self::contains(s.rect, x, y)).collect();
        over.sort_by_key(|(_, s)| std::cmp::Reverse(s.z));
        let (k, slot) = over.into_iter().next()?;
        let on = slot.ui.hit(x, y).and_then(|h| h.on.clone()).unwrap_or_else(|| "body".into());
        Some((k.clone(), on))
    }

    /// True when what the pointer is over changed.
    pub fn pointer(&mut self, at: Option<(f64, f64)>) -> bool {
        let inside = at.is_some_and(|(x, y)| Self::contains(self.stack, x, y));
        let mut moved = inside != self.hovered;
        self.hovered = inside;
        for slot in self.slots.values_mut() {
            let hover = at.and_then(|(x, y)| slot.ui.hit(x, y)).map(|h| h.path.clone());
            if slot.ui.hover != hover {
                slot.ui.hover = hover;
                moved = true;
            }
        }
        moved
    }

    /// Members of every group on screen, which hold while the stack is
    /// expanded.
    pub fn members(&self) -> HashSet<String> {
        self.slots.values().filter(|s| !s.departing).flat_map(|s| s.members.iter().cloned()).collect()
    }

    /// Any card on the stack, a group that left included until its fade
    /// has run.
    pub fn holds_cards(&self) -> bool {
        !self.slots.is_empty()
    }

    pub fn press(&mut self, x: f64, y: f64) {
        self.pressed = self.hit(x, y);
    }

    pub fn release(&mut self, x: f64, y: f64) -> Act {
        let Some(pressed) = self.pressed.take() else { return Act::None };
        if self.hit(x, y).as_ref() != Some(&pressed) {
            return Act::None;
        }
        let (key, on) = pressed;
        let Some(slot) = self.slots.get(&key) else { return Act::None };
        if let Some(source) = key.strip_prefix(ACTIVITY) {
            return match Verb::parse(&on) {
                Some((verb, _)) => Act::Activity(verb, source.to_owned()),
                None if on == "body" => Act::Activity(Verb::Open, source.to_owned()),
                None => Act::None,
            };
        }
        let members = slot.members.clone();
        match on.as_str() {
            "close" => Act::Dismiss(members),
            "body" => Act::Body(members),
            a => match a.strip_prefix("action:") {
                Some(k) => Act::Action(members.last().cloned().unwrap_or_default(), k.to_owned()),
                None => Act::None,
            },
        }
    }

    /// One axis frame over the stack: `along` is its horizontal travel,
    /// `lift` the event time of a finger's axis_stop on either axis. Only a
    /// finger's horizontal travel moves a card, and a lift sends it on or
    /// back over `ms` (the spatial clock, motion scale applied). Returns
    /// whether anything moved, and the group a lift sends away.
    pub fn swipe(&mut self, x: f64, y: f64, along: Travel, lift: Option<u32>, ms: f64, now: Instant) -> (bool, Act) {
        let dir = if self.right { 1.0 } else { -1.0 };
        let Travel::Pixels { px, finger: true, time } = along else {
            return match lift {
                Some(time) => self.lift(time, dir, ms, now),
                None => (false, Act::None),
            };
        };
        if self.swipe.is_none() {
            let Some((key, _)) = self.hit(x, y) else { return (false, Act::None) };
            // Fingers back on a card mid spring take it from where it is.
            let at = self.slots.get(&key).map_or(0.0, |s| s.dx.value(now));
            let pull = if at * dir > 0.0 { at } else { 0.0 };
            self.swipe = Some(Swipe { key, pull, velocity: Velocity::default() });
        }
        let Some(sw) = &mut self.swipe else { return (false, Act::None) };
        let Some(slot) = self.slots.get_mut(&sw.key).filter(|s| !s.departing) else {
            self.swipe = None;
            return (false, Act::None);
        };
        // The card follows a scroll area's content under the same values.
        sw.pull -= px;
        sw.velocity.push(time, -px);
        slot.dx.jump(swipe_shown(sw.pull, dir, slot.width.value(now)));
        (true, Act::None)
    }

    fn lift(&mut self, time: u32, dir: f64, ms: f64, now: Instant) -> (bool, Act) {
        let Some(sw) = self.swipe.take() else { return (false, Act::None) };
        let Some(slot) = self.slots.get_mut(&sw.key).filter(|s| !s.departing) else { return (false, Act::None) };
        if swipe_dismisses(sw.pull, sw.velocity.at_lift(time), dir) {
            slot.dx.set(now, dir * slot.width.value(now), ms);
            if let Some(source) = sw.key.strip_prefix(ACTIVITY) {
                return (true, Act::Activity(Verb::Dismiss, source.to_owned()));
            }
            return (true, Act::Dismiss(slot.members.clone()));
        }
        slot.dx.set(now, 0.0, ms);
        (true, Act::None)
    }

    pub fn animating(&self, now: Instant) -> bool {
        !self.surface.mapped || self.slots.values().any(|s| s.animating(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::theme::getter;
    use crate::ui::el::Kind;
    use serde_json::json;

    fn group(source: &str, body: &str) -> Group {
        let entry = model::Entry {
            source: source.into(),
            app_name: "Messages".into(),
            summary: "hello".into(),
            body: body.into(),
            ..Default::default()
        };
        Group { entry, count: 1, member_ids: Vec::new() }
    }

    fn walk(el: &El, f: &mut dyn FnMut(&El)) {
        f(el);
        if let Kind::Column { children, .. } | Kind::Row { children, .. } = &el.kind {
            children.iter().for_each(|c| walk(c, f));
        }
    }

    fn theme() -> Theme {
        Theme::resolve(getter(json!({})), &fs_theme::palette::fallback("dark"))
    }

    #[test]
    fn a_card_mirrored_off_the_phone_carries_the_source_mark() {
        let theme = theme();
        for (source, marked) in [("iphone", true), ("", false)] {
            let mut badges = 0;
            walk(&bubble(&theme, &group(source, ""), 0, None, false, None), &mut |e| badges += usize::from(e.badge.is_some()));
            assert_eq!(badges, usize::from(marked), "source {source:?}");
        }
    }

    #[test]
    fn the_column_holds_the_card_its_cast_and_the_rubber_band() {
        let theme = theme();
        let insets = Insets { top: 0.0, bottom: 0.0, left: 12.0, right: 30.0 };
        for right in [true, false] {
            let inset = if right { insets.right } else { insets.left };
            let column = f64::from(column_width(&theme, column_cast(&theme), &insets, right));
            let card = card_width(&theme);
            let cast = fs_theme::style::geometry::cast_pad(&theme.box_style("notification", Some("rest")).casts);
            let inner = column - inset - theme.space.screen_padding - card;
            assert!(inner >= scroll::rubber(-1e9, card).abs() + cast, "{inner}px past the card's inner edge");
        }
    }

    #[test]
    fn a_lift_dismisses_past_the_threshold_or_on_a_flick_toward_the_edge() {
        for dir in [1.0, -1.0] {
            assert!(swipe_dismisses(45.0 * dir, 0.0, dir));
            assert!(!swipe_dismisses(44.0 * dir, 0.0, dir));
            assert!(swipe_dismisses(10.0 * dir, 111.0 * dir, dir));
            assert!(!swipe_dismisses(10.0 * dir, 100.0 * dir, dir));
            // The wrong way never goes, however far or fast.
            assert!(!swipe_dismisses(-200.0 * dir, -900.0 * dir, dir));
        }
    }

    #[test]
    fn the_card_follows_toward_its_edge_and_resists_the_other_way() {
        assert_eq!(swipe_shown(60.0, 1.0, 360.0), 60.0);
        assert_eq!(swipe_shown(-60.0, -1.0, 360.0), -60.0);
        let back = swipe_shown(-60.0, 1.0, 360.0);
        assert!(back < 0.0 && back > -40.0, "{back}");
        assert!(swipe_shown(-600.0, 1.0, 360.0) > -40.0);
    }

    #[test]
    fn a_bubble_keeps_its_height_when_the_close_button_shows() {
        let theme = theme();
        let mut kit = Kit::new(&theme);
        let g = group("", "some body");
        let h = |hovered, kit: &mut Kit| ui::measure(&bubble(&theme, &g, 0, None, hovered, None), 300.0, &theme, kit).1;
        assert_eq!(h(false, &mut kit), h(true, &mut kit));
    }

    #[test]
    fn the_summary_and_the_body_wrap_to_two_lines() {
        let theme = theme();
        let mut wrapped = Vec::new();
        walk(&bubble(&theme, &group("", "some body"), 0, None, false, None), &mut |e| {
            if let Kind::Para { lines, .. } = &e.kind {
                wrapped.push(*lines);
            }
        });
        assert_eq!(wrapped, vec![2, 2]);
    }
}
