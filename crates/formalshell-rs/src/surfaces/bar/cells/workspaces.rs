//! Spaces: one chip per workspace on this output, each its
//! ordinal followed by the icons of the windows on it, the focused chip
//! under one pill that travels (Bar/workspaces.js picks the chips and what
//! each lists). T3 Code's agent state rides its window's icon as a badge:
//! a spinner while one works, a pulse while one waits on the user, a check
//! once one is done. The hover preview waits for the thumbnails (R8).

use std::collections::HashMap;
use std::time::Instant;

use fs_chrome::bar::workspaces::{self, Slot, SlotOpts};
use fs_theme::color::Rgba;
use fs_theme::tokens::WEIGHTS;
use serde_json::{Value, json};
use vello_cpu::kurbo::Affine;

use crate::motion::{Animated, EFFECTS, EFFECTS_SLOW, EMPHASIZED, PULSE_MS, breathe};
use crate::scene::IRect;
use crate::services::appicon::{self, Icon};
use crate::services::agents;
use crate::services::compositor::{Window, Workspace};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Custom, Env, Ink, Kit, Look, Painter, View};
use crate::text::{Family, TextStyle};

/// `workspaces.persistent`'s default: slots 1..n show even when empty.
const PERSISTENT: i64 = 5;
const MAX_ICONS: i64 = 8;
const SHOW_APPS: &str = "all";

fn badge_glyph(state: &str) -> &'static str {
    match state {
        "working" => "loader-circle",
        "blocked" => "circle-alert",
        _ => "circle-check",
    }
}

fn badge_ink(state: &str, look: &Look) -> Rgba {
    match state {
        "blocked" => look.destructive,
        "done" => look.primary,
        _ => look.foreground,
    }
}

/// The focused workspace's fill. Its two edges chase the same chip, the
/// leading pair on `emphasized` and the trailing pair on twice that clock,
/// so a switch stretches one shape across both chips before it lands.
struct Pill {
    lead: (Animated, Animated),
    trail: (Animated, Animated),
    /// 1 while a chip on this output holds focus; focus on another output
    /// fades it out where it stood.
    fade: Animated,
    placed: bool,
}

impl Pill {
    fn new() -> Self {
        let at = |v| Animated::new(v, EMPHASIZED);
        Self { lead: (at(0.0), at(0.0)), trail: (at(0.0), at(0.0)), fade: Animated::new(0.0, EFFECTS), placed: false }
    }

    fn span(&self, now: Instant) -> (f64, f64) {
        let from = self.lead.0.value(now).min(self.trail.0.value(now));
        let to = self.lead.1.value(now).max(self.trail.1.value(now));
        (from, to)
    }

    fn running(&self, now: Instant) -> bool {
        [&self.lead.0, &self.lead.1, &self.trail.0, &self.trail.1, &self.fade].iter().any(|a| a.running(now))
    }

    fn jump(&mut self, start: f64, end: f64) {
        self.lead.0.jump(start);
        self.trail.0.jump(start);
        self.lead.1.jump(end);
        self.trail.1.jump(end);
    }

    fn travel(&mut self, now: Instant, start: f64, end: f64, clock: f64) {
        self.lead.0.set(now, start, clock);
        self.lead.1.set(now, end, clock);
        self.trail.0.set(now, start, clock * 2.0);
        self.trail.1.set(now, end, clock * 2.0);
    }
}

/// What one chip measured to, along the strip.
#[derive(Clone, Debug, Default)]
struct Chip {
    start: f64,
    extent: f64,
    label_along: f64,
    label_drawn: bool,
    shows_apps: bool,
    icons: usize,
    overflow_along: f64,
}

/// An icon's own clocks: its step back on an unfocused window, and its
/// arrival.
struct IconFx {
    dim: Animated,
    arrive: Animated,
}

/// A chip label's ink on its way to the one its state now asks for
/// (`Behavior on color { CAnim {} }`).
struct LabelInk {
    from: Rgba,
    to: Rgba,
    t: Animated,
}

pub struct Workspaces {
    output: String,
    slots: Vec<Slot>,
    show_apps: String,
    agents: bool,
    /// Wheel travel since the last step, in notches.
    wheel_sum: f64,
    agent_state: HashMap<String, String>,
    icons: HashMap<String, Icon>,
    queries: Vec<appicon::Query>,
    probed: Option<(u32, Vec<appicon::Query>)>,
    chips: Vec<Chip>,
    icon_size: f64,
    pad_x: f64,
    xs: f64,
    vertical: bool,
    motion: bool,
    pill: Pill,
    fx: HashMap<String, IconFx>,
    /// The cell's own box as last drawn, in the bar window's coordinates.
    origin: IRect,
    hover_chip: Option<usize>,
    hover_icon: Option<(usize, usize)>,
    /// When each chip last turned urgent, for its one pulse.
    urgent: HashMap<i64, Instant>,
    label_ink: HashMap<i64, LabelInk>,
    was_urgent: HashMap<i64, bool>,
    clock: Instant,
    /// `debug r0Spinner`: a working badge on the first chip whether or not
    /// an agent works.
    forced_spinner: bool,
}

impl Default for Workspaces {
    fn default() -> Self {
        Self {
            output: String::new(),
            slots: Vec::new(),
            show_apps: SHOW_APPS.into(),
            agents: true,
            wheel_sum: 0.0,
            agent_state: HashMap::new(),
            icons: HashMap::new(),
            queries: Vec::new(),
            probed: None,
            chips: Vec::new(),
            icon_size: 0.0,
            pad_x: 0.0,
            xs: 0.0,
            vertical: false,
            motion: true,
            pill: Pill::new(),
            fx: HashMap::new(),
            origin: IRect::default(),
            hover_chip: None,
            hover_icon: None,
            urgent: HashMap::new(),
            label_ink: HashMap::new(),
            was_urgent: HashMap::new(),
            clock: Instant::now(),
            forced_spinner: false,
        }
    }
}

fn workspace(w: &Workspace) -> workspaces::Workspace {
    workspaces::Workspace {
        id: w.id.clone(),
        idx: w.idx,
        name: w.name.clone(),
        output: w.output.clone(),
        is_active: w.is_active,
        is_focused: w.is_focused,
        is_urgent: w.is_urgent,
        placeholder: false,
    }
}

pub(crate) fn window(w: &Window) -> workspaces::Window {
    workspaces::Window {
        id: w.id.clone(),
        workspace_id: w.workspace_id.clone(),
        app_id: w.app_id.clone(),
        initial_class: w.initial_class.clone(),
        initial_title: w.initial_title.clone(),
        pid: w.pid,
        is_focused: w.is_focused,
        is_urgent: w.is_urgent,
        is_floating: w.is_floating,
        rect: w.rect.map(|r| fs_chrome::types::Rect {
            x: r.x as f64,
            y: r.y as f64,
            width: r.width as f64,
            height: r.height as f64,
        }),
    }
}

/// Rounded, clamped, the default for a non-number.
fn int(env: &Env, key: &str, low: i64, high: i64, default: i64) -> i64 {
    env.store.config.f64(key).filter(|n| n.is_finite()).map_or(default, |n| (n.round() as i64).clamp(low, high))
}

fn label_style(look: &Look, band: bool) -> TextStyle {
    look.label(if band { WEIGHTS.semibold } else { WEIGHTS.medium } as f32)
}

fn caption_style(look: &Look) -> TextStyle {
    TextStyle { family: look.mono, size: look.caption, weight: WEIGHTS.medium as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
}

impl Workspaces {
    fn waiting(&self, slot: &Slot) -> bool {
        self.agents && !slot.current && slot.windows.iter().any(|w| self.agent_state.get(&w.id).is_some_and(|s| s == "blocked"))
    }

    fn agent(&self, window_id: &str) -> &str {
        if self.agents && !window_id.is_empty() { self.agent_state.get(window_id).map_or("", String::as_str) } else { "" }
    }

    fn focused_index(&self) -> i64 {
        self.slots.iter().position(|s| s.is_focused).map_or(-1, |i| i as i64)
    }

    /// A chip with an id focuses it, a persistent
    /// placeholder focuses by ordinal; the one already focused stays.
    fn go(&self, i: usize) -> Action {
        let s = &self.slots[i];
        if s.current && s.is_focused {
            return Action::None;
        }
        if s.id.is_empty() { Action::WorkspaceAt(s.idx, self.output.clone()) } else { Action::Workspace(s.id.clone()) }
    }

    /// Where each part of a chip starts along it: the label, every icon and
    /// the overflow count.
    fn parts(&self, chip: &Chip) -> (f64, Vec<f64>, Option<f64>) {
        let mut at = self.pad_x;
        let label = at;
        if chip.label_drawn {
            at += chip.label_along + if chip.icons > 0 { self.xs } else { 0.0 };
        }
        let mut icons = Vec::with_capacity(chip.icons);
        for _ in 0..chip.icons {
            icons.push(at);
            at += self.icon_size + self.xs;
        }
        (label, icons, (chip.overflow_along > 0.0).then_some(at))
    }

    fn icon_rect(&self, chip: &Chip, along: f64, rect: IRect) -> IRect {
        let size = self.icon_size.round() as i32;
        let at = (chip.start + along).round() as i32;
        if self.vertical {
            IRect::new(rect.x + (rect.w - size) / 2, rect.y + at, size, size)
        } else {
            IRect::new(rect.x + at, rect.y + (rect.h - size) / 2, size, size)
        }
    }

    fn chip_rect(&self, chip: &Chip, rect: IRect) -> IRect {
        let (start, extent) = (chip.start.round() as i32, chip.extent.round() as i32);
        if self.vertical { IRect::new(rect.x, rect.y + start, rect.w, extent) } else { IRect::new(rect.x + start, rect.y, extent, rect.h) }
    }

    fn chip_at(&self, along: f64) -> Option<usize> {
        self.chips.iter().position(|c| along >= c.start && along < c.start + c.extent)
    }

    /// The icon under a point along the cell: its box grown by half the gap
    /// each way, as the pointer area is.
    fn icon_at(&self, chip: usize, along: f64) -> Option<usize> {
        let c = self.chips.get(chip).filter(|c| c.shows_apps)?;
        let (_, icons, _) = self.parts(c);
        icons.iter().position(|a| {
            let start = c.start + a;
            along >= start - self.xs / 2.0 && along < start + self.icon_size + self.xs / 2.0
        })
    }

    /// The chip label's colour this frame, gliding to `target` whenever the
    /// chip's state changes it.
    fn label_color(&mut self, idx: i64, target: Rgba, now: Instant, look: &Look, scale: f64) -> Rgba {
        let fx = self.label_ink.entry(idx).or_insert_with(|| LabelInk { from: target, to: target, t: Animated::new(1.0, EFFECTS_SLOW) });
        if fx.to != target {
            fx.from = fx.from.mix(fx.to, fx.t.value(now) as f32);
            fx.to = target;
            fx.t.jump(0.0);
            if look.motion { fx.t.set(now, 1.0, look.effects_slow * scale) } else { fx.t.jump(1.0) }
        }
        fx.from.mix(fx.to, fx.t.value(now).clamp(0.0, 1.0) as f32)
    }

    /// An icon's clocks, born at its resting dim and fading in from nothing.
    fn fx_of(&mut self, id: &str, dim: f64, now: Instant, look: &Look, scale: f64) -> &mut IconFx {
        self.fx.entry(id.to_owned()).or_insert_with(|| {
            let mut arrive = Animated::new(0.0, EFFECTS);
            if look.motion { arrive.set(now, 1.0, look.effects * scale) } else { arrive.jump(1.0) }
            IconFx { dim: Animated::new(dim, EFFECTS), arrive }
        })
    }

    /// The agent badge on the corner of `anchor`, on its own nodes so a frame
    /// of a spinner or a pulse damages that rect alone.
    fn badge(&self, kit: &mut Kit, p: &mut Painter, anchor: IRect, state: &str, now: Instant, alpha: f32) {
        let look = kit.look.clone();
        let size = look.caption as i32;
        let xxs = look.xxs as i32;
        let badge = IRect::new(anchor.right() + xxs - size, anchor.y - xxs, size, size);
        let pulse = if state == "blocked" { breathe(self.clock, now) } else { 1.0 };
        let a = alpha * pulse;
        p.rect(badge, look.background.with_alpha(look.background.a * a), size as f32 / 2.0);
        let g = fs_theme::icons::glyph(&look.icon_set, badge_glyph(state));
        let shaped = kit.shape(g.text, TextStyle { family: Family::Named(g.family), size: look.caption, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
        let (bw, bh) = shaped.box_size();
        let at = (badge.x + (size - bw) / 2, badge.y + (size - bh) / 2);
        let centre = (badge.x as f64 + size as f64 / 2.0, badge.y as f64 + size as f64 / 2.0);
        let turn = if state == "working" && look.motion {
            let elapsed = now.saturating_duration_since(self.clock).as_secs_f64() * 1000.0;
            (elapsed / PULSE_MS).fract() * std::f64::consts::TAU
        } else {
            0.0
        };
        let spin = Affine::translate(centre) * Affine::rotate(turn) * Affine::translate((-centre.0, -centre.1));
        let shift = Affine::translate(((at.0 - badge.x) as f64, (at.1 - badge.y) as f64));
        let ink = badge_ink(state, &look);
        p.text_in(&shaped, badge, spin * shift, p.clip, ink.with_alpha(ink.a * a), &[]);
    }
}

impl Cell for Workspaces {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Compositor, Topic::Config, Topic::Agents, Topic::AppIcon]
    }

    fn read(&mut self, env: &Env) -> bool {
        let persistent = int(env, "workspaces.persistent", 0, 10, PERSISTENT);
        let max_icons = int(env, "workspaces.maxIcons", 1, 20, MAX_ICONS) as usize;
        let show_apps = env.store.config.str("workspaces.showApps").unwrap_or(SHOW_APPS).to_owned();
        let agents = env.store.config.bool("workspaces.agents") != Some(false);
        let c = &env.store.compositor.snapshot;
        let rows: Vec<workspaces::Workspace> = c.workspaces.iter().map(workspace).collect();
        let windows: Vec<workspaces::Window> = c.windows.iter().map(window).collect();
        let slots = workspaces::slots(&rows, &windows, env.output, SlotOpts { persistent, max_icons });

        let ids: Vec<&str> = slots.iter().flat_map(|s| s.windows.iter().map(|w| w.id.as_str())).collect();
        let scale = crate::scene::raster_scale();
        let agents_now: HashMap<String, String> = ids
            .iter()
            .filter_map(|id| env.store.agents.by_window.get(*id).map(|s| ((*id).to_owned(), s.clone())))
            .collect();
        let icons_now: HashMap<String, Icon> =
            ids.iter().filter_map(|id| env.store.appicon.get(id, self.icon_size.round() as u32, scale).map(|i| ((*id).to_owned(), i))).collect();
        let queries: Vec<appicon::Query> = slots
            .iter()
            .flat_map(|s| &s.windows)
            .map(|w| appicon::Query {
                id: w.id.clone(),
                app_id: w.app_id.clone(),
                initial_class: w.initial_class.clone(),
                initial_title: w.initial_title.clone(),
                pid: w.pid,
            })
            .collect();
        let watched = if agents {
            agents::Watch {
                windows: c
                    .windows
                    .iter()
                    .filter(|w| fs_info::agents::is_app(&w.app_id, &w.initial_class))
                    .map(|w| w.id.clone())
                    .collect(),
                hosts: env.store.config.get("workspaces.agentHosts").and_then(Value::as_array).map_or_else(Vec::new, |hosts| {
                    hosts.iter().filter_map(Value::as_str).map(str::to_owned).collect()
                }),
            }
        } else {
            agents::Watch::default()
        };
        agents::watch(watched);

        let vertical = env.edge.is_vertical();
        let changed = slots != self.slots
            || vertical != self.vertical
            || show_apps != self.show_apps
            || agents != self.agents
            || agents_now != self.agent_state
            || icons_now != self.icons
            || env.output != self.output;
        if slots != self.slots {
            let now = Instant::now();
            for s in &slots {
                if s.is_urgent && !self.was_urgent.get(&s.idx).copied().unwrap_or(false) {
                    self.urgent.insert(s.idx, now);
                }
            }
            self.was_urgent = slots.iter().map(|s| (s.idx, s.is_urgent)).collect();
        }
        self.output = env.output.to_owned();
        self.slots = slots;
        self.vertical = vertical;
        self.show_apps = show_apps;
        self.agents = agents;
        self.agent_state = agents_now;
        self.icons = icons_now;
        self.queries = queries;
        changed
    }

    fn view(&self, _: &Look) -> View {
        View::new(Vec::new(), 0.0)
    }

    fn click(&mut self, button: Button, at: (f64, f64), _: &Env) -> Action {
        if button != Button::Left {
            return Action::None;
        }
        let along = if self.vertical { at.1 } else { at.0 };
        let Some(i) = self.chip_at(along) else { return Action::None };
        if let Some(n) = self.icon_at(i, along) {
            let id = &self.slots[i].windows[n].id;
            if !id.is_empty() {
                return Action::Window(id.clone());
            }
        }
        self.go(i)
    }

    /// One notch steps one workspace, wrapping.
    fn wheel(&mut self, up: bool, _: &Env) -> Action {
        match workspaces::step_index(self.slots.len(), self.focused_index(), if up { -1 } else { 1 }) {
            Some(next) => self.go(next),
            None => Action::None,
        }
    }

    /// A touchpad reports a notch as many small deltas, so they sum to a
    /// notch before the cell steps.
    fn wheel_by(&mut self, vertical: f64, sideways: f64, env: &Env) -> Action {
        let delta = if vertical != 0.0 { vertical } else { sideways };
        if delta == 0.0 {
            return Action::None;
        }
        self.wheel_sum += delta;
        if self.wheel_sum.abs() < 1.0 {
            return Action::None;
        }
        let up = self.wheel_sum < 0.0;
        self.wheel_sum = 0.0;
        self.wheel(up, env)
    }

    fn custom(&mut self) -> Option<&mut dyn Custom> {
        Some(self)
    }

    /// `workspaces status`: the chips as resolved, with each icon's agent
    /// state and every chip's settled length, in the bar window's own
    /// coordinates.
    fn status(&self) -> Option<Value> {
        let rect = |r: IRect| json!({"x": r.x, "y": r.y, "width": r.w, "height": r.h});
        let slots: Vec<Value> = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let chip = self.chips.get(i).cloned().unwrap_or_default();
                let (_, at, _) = self.parts(&chip);
                let icons: Vec<Value> = s
                    .windows
                    .iter()
                    .enumerate()
                    .map(|(n, w)| {
                        let drawn = chip.shows_apps && n < at.len();
                        let r = if drawn { self.icon_rect(&chip, at[n], self.origin) } else { IRect::default() };
                        json!({
                            "id": w.id,
                            "appId": w.app_id,
                            "focused": w.is_focused,
                            "icon": self.icons.get(&w.id).is_some_and(|i| !i.source.is_empty()),
                            "agent": self.agent(&w.id),
                            "rect": rect(r),
                        })
                    })
                    .collect();
                json!({
                    "id": s.id,
                    "idx": s.idx,
                    "label": s.label,
                    "active": s.current,
                    "focused": s.is_focused,
                    "urgent": s.is_urgent,
                    "placeholder": s.placeholder,
                    "appsShown": chip.shows_apps,
                    "extent": crate::surfaces::bar::number(chip.extent),
                    "rect": rect(self.chip_rect(&chip, self.origin)),
                    "overflow": s.overflow,
                    "icons": icons,
                })
            })
            .collect();
        Some(json!({"output": self.output, "slots": slots}))
    }
}

impl Custom for Workspaces {
    fn measure(&mut self, kit: &mut Kit, vertical: bool, band: bool) -> f64 {
        let look = kit.look.clone();
        let now = Instant::now();
        self.vertical = vertical;
        self.pad_x = look.pad_x;
        self.xs = look.xs;
        self.motion = look.motion;
        let style = label_style(&look, band);
        self.icon_size = kit.shape("0", style).line_height() as f64;
        let (size, scale) = (self.icon_size.round() as u32, crate::scene::raster_scale());
        let px = crate::scene::pixels_at(f64::from(size), scale);
        if self.probed.as_ref().is_none_or(|(s, q)| *s != px || *q != self.queries) {
            self.probed = Some((px, self.queries.clone()));
            appicon::probe(size, scale, self.queries.clone());
        }

        self.chips.clear();
        let mut at = 0.0;
        let mut focused = None;
        for (i, slot) in self.slots.iter().enumerate() {
            let shaped = kit.shape(&slot.label, style);
            let (w, h) = (shaped.width as f64, shaped.line_height() as f64);
            let label_drawn = !(vertical && w > look.content_across() + 0.5);
            let label_along = if vertical { h } else { w };
            let shows = !slot.windows.is_empty() && workspaces::shows_apps(&self.show_apps, slot.current, self.hover_chip == Some(i));
            let icons = if shows { slot.windows.len() } else { 0 };
            let overflow_along = if shows && slot.overflow > 0 {
                let o = kit.shape(&format!("+{}", slot.overflow), caption_style(&look));
                if vertical { o.line_height() as f64 } else { o.width as f64 }
            } else {
                0.0
            };
            let items = icons + usize::from(overflow_along > 0.0);
            let mut content = if label_drawn { label_along } else { 0.0 };
            if items > 0 {
                content += if label_drawn { look.xs } else { 0.0 };
                content += icons as f64 * self.icon_size + overflow_along + (items - 1) as f64 * look.xs;
            }
            let extent = content + look.pad_x * 2.0;
            self.chips.push(Chip { start: at, extent, label_along, label_drawn, shows_apps: shows, icons, overflow_along });
            if slot.is_focused {
                focused = Some((at, at + extent));
            }
            at += extent + look.sm;
        }

        let clock = look.emphasized * kit.motion_scale;
        match focused {
            Some((start, end)) => {
                if self.pill.placed && look.motion && self.pill.fade.value(now) > 0.0 {
                    self.pill.travel(now, start, end, clock);
                } else {
                    self.pill.jump(start, end);
                }
                self.pill.placed = true;
                if look.motion {
                    self.pill.fade.set(now, 1.0, look.effects * kit.motion_scale);
                } else {
                    self.pill.fade.jump(1.0);
                }
            }
            None => {
                if look.motion {
                    self.pill.fade.set(now, 0.0, look.effects * kit.motion_scale);
                } else {
                    self.pill.fade.jump(0.0);
                }
            }
        }
        (at - look.sm).max(0.0)
    }

    fn draw(&mut self, kit: &mut Kit, p: &mut Painter, rect: IRect, ink: &Ink, now: Instant) {
        let look = kit.look.clone();
        self.origin = rect;
        let style = label_style(&look, ink.band);

        let fade = self.pill.fade.value(now).clamp(0.0, 1.0) as f32;
        if self.pill.placed && fade > 0.0 {
            let (from, to) = self.pill.span(now);
            let r = if self.vertical {
                IRect::new(rect.x, rect.y + from.round() as i32, rect.w, (to - from).round() as i32)
            } else {
                IRect::new(rect.x + from.round() as i32, rect.y, (to - from).round() as i32, rect.h)
            };
            match &look.cursor {
                // The console's cursor: a faint fill and the block on its
                // leading edge, the label keeping its own ink.
                Some((fill, bar)) => {
                    p.rect(r, ink.of(fill.with_alpha(fill.a * fade)), look.radius);
                    let w = (bar.width.round() as i32).min(r.w.min(r.h));
                    let block = if self.vertical { IRect::new(r.x, r.y, r.w, w) } else { IRect::new(r.x, r.y, w, r.h) };
                    p.rect(block, ink.of(bar.color.with_alpha(bar.color.a * fade)), 0.0);
                }
                None => p.rect(r, ink.of(look.active_fill.with_alpha(look.active_fill.a * fade)), look.radius),
            }
        }

        for i in 0..self.slots.len().min(self.chips.len()) {
            let chip = self.chips[i].clone();
            let slot = self.slots[i].clone();
            let cr = self.chip_rect(&chip, rect);
            let hovered = self.hover_chip == Some(i);
            if let Some(wash) = look.hover_wash.filter(|_| hovered && !slot.is_focused) {
                p.rect(cr, ink.of(wash), look.radius);
            }
            let waiting = self.waiting(&slot);
            let color = if slot.is_urgent || waiting {
                look.destructive
            } else if slot.is_focused && look.cursor.is_none() {
                look.on_active
            } else if slot.is_focused || slot.current || hovered {
                ink.fg
            } else {
                ink.dim
            };
            let color = self.label_color(slot.idx, color, now, &look, kit.motion_scale);
            let (label_at, icon_at, overflow_at) = self.parts(&chip);

            let mut opacity = if waiting { breathe(self.clock, now) } else { 1.0 };
            if let Some(since) = self.urgent.get(&slot.idx).copied() {
                let t = now.saturating_duration_since(since).as_secs_f64() * 1000.0;
                let (down, up) = (look.effects, look.effects_slow);
                let o = if !look.motion || t >= down + up {
                    1.0
                } else if t < down {
                    1.0 - 0.7 * EFFECTS.ease(t / down)
                } else {
                    0.3 + 0.7 * EFFECTS.ease((t - down) / up)
                };
                opacity *= o as f32;
            }

            if chip.label_drawn {
                let shaped = kit.shape(&slot.label, style);
                let lh = shaped.line_height();
                let (x, y) = if self.vertical {
                    (cr.x + (cr.w - shaped.width) / 2, cr.y + label_at.round() as i32)
                } else {
                    (cr.x + label_at.round() as i32, cr.y + (cr.h - lh) / 2)
                };
                let glow: &[_] = if slot.is_focused { &[] } else { &ink.glow };
                p.text(&shaped, (x, y), color.with_alpha(color.a * ink.alpha * opacity), glow);
            }

            for (n, w) in slot.windows.iter().enumerate().take(chip.icons) {
                let Some(along) = icon_at.get(n).copied() else { break };
                let r = self.icon_rect(&chip, along, rect);
                let focused_here = slot.is_focused && w.is_focused;
                let hovered_icon = self.hover_icon == Some((i, n));
                let scale = kit.motion_scale;
                let dim = if slot.is_focused && !focused_here && !hovered_icon { 0.5 } else { 1.0 };
                let fx = self.fx_of(&w.id, dim, now, &look, scale);
                if look.motion { fx.dim.set(now, dim, look.effects * scale) } else { fx.dim.jump(dim) }
                let a = (fx.arrive.value(now) * fx.dim.value(now)) as f32 * ink.alpha;
                match self.icons.get(&w.id).and_then(|i| i.image.clone()) {
                    Some(image) => p.image(&image, (r.x, r.y), a),
                    None => {
                        let g = fs_theme::icons::glyph(&look.icon_set, "app-window");
                        let shaped = kit.shape(g.text, TextStyle { family: Family::Named(g.family), size: look.body, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
                        let (bw, bh) = (shaped.width, shaped.line_height());
                        let at = (r.x + (r.w - bw) / 2, r.y + (r.h - bh) / 2);
                        p.text(&shaped, at, color.with_alpha(color.a * a), &[]);
                    }
                }
                let state = self.agent(&w.id);
                if !state.is_empty() {
                    self.badge(kit, p, r, state, now, ink.alpha);
                }
            }
            if let (Some(along), true) = (overflow_at, chip.overflow_along > 0.0) {
                let shaped = kit.shape(&format!("+{}", slot.overflow), caption_style(&look));
                let at = (chip.start + along).round() as i32;
                let (x, y) = if self.vertical {
                    (rect.x + (rect.w - shaped.width) / 2, rect.y + at)
                } else {
                    (rect.x + at, rect.y + (rect.h - shaped.line_height()) / 2)
                };
                p.text(&shaped, (x, y), color.with_alpha(color.a * ink.alpha * opacity), &[]);
            }
        }

        if self.forced_spinner {
            if let Some(chip) = self.chips.first() {
                let cr = self.chip_rect(chip, rect);
                let anchor = IRect::new(cr.right() - look.caption as i32, cr.y + look.xxs as i32 * 2, 0, 0);
                self.badge(kit, p, anchor, "working", now, ink.alpha);
            }
        }
    }

    fn animating(&self, now: Instant) -> bool {
        let badges = self.slots.iter().take(self.chips.len()).enumerate().any(|(i, s)| {
            let chip = &self.chips[i];
            s.windows.iter().take(chip.icons).any(|w| match self.agent(&w.id) {
                "working" => self.motion,
                "blocked" => true,
                _ => false,
            })
        });
        let waiting = self.slots.iter().any(|s| self.waiting(s));
        let urgent = self.urgent.values().any(|t| now.saturating_duration_since(*t).as_secs_f64() < 2.0);
        let fx = self.fx.values().any(|f| f.dim.running(now) || f.arrive.running(now));
        let ink = self.label_ink.values().any(|l| l.t.running(now));
        self.forced_spinner || badges || waiting || urgent || fx || ink || self.pill.running(now)
    }

    fn spinner(&mut self, on: bool, now: Instant) {
        self.forced_spinner = on;
        self.clock = now;
    }

    fn own_hover(&self) -> bool {
        true
    }

    fn pointer(&mut self, at: Option<(f64, f64)>) -> (bool, bool) {
        let (chip, icon) = match at {
            Some(p) => {
                let along = if self.vertical { p.1 } else { p.0 };
                let chip = self.chip_at(along);
                (chip, chip.and_then(|c| self.icon_at(c, along).map(|n| (c, n))))
            }
            None => (None, None),
        };
        let moved = chip != self.hover_chip || icon != self.hover_icon;
        let relayout = chip != self.hover_chip && self.show_apps != "all" && self.show_apps != "active";
        self.hover_chip = chip;
        self.hover_icon = icon;
        (moved, relayout)
    }
}

#[cfg(test)]
mod tests {
    use fs_chrome::bar::workspaces::SlotWindow;
    use fs_chrome::types::Edge;

    use super::*;
    use crate::store::Store;

    fn slot(idx: i64, id: &str, focused: bool, windows: &[&str]) -> Slot {
        Slot {
            id: id.into(),
            idx,
            name: idx.to_string(),
            label: idx.to_string(),
            output: "o".into(),
            is_active: focused,
            is_focused: focused,
            current: focused,
            is_urgent: false,
            placeholder: id.is_empty(),
            windows: windows.iter().map(|w| SlotWindow { id: (*w).into(), ..SlotWindow::default() }).collect(),
            overflow: 0,
        }
    }

    /// Three chips, 40 wide with 4 between them, the middle one carrying two
    /// 10 px icons at xs 2 and pad 6.
    fn cell() -> Workspaces {
        let mut c = Workspaces { icon_size: 10.0, pad_x: 6.0, xs: 2.0, ..Workspaces::default() };
        c.slots = vec![slot(1, "1", true, &[]), slot(2, "2", false, &["a", "b"]), slot(3, "", false, &[])];
        c.output = "o".into();
        let chip = |start: f64, icons: usize| Chip {
            start,
            extent: 40.0,
            label_along: 8.0,
            label_drawn: true,
            shows_apps: icons > 0,
            icons,
            overflow_along: 0.0,
        };
        c.chips = vec![chip(0.0, 0), chip(44.0, 2), chip(88.0, 0)];
        c
    }

    fn env(store: &Store) -> Env<'_> {
        Env { store, edge: Edge::Top, output: "o" }
    }

    #[test]
    fn a_wheel_notch_steps_one_workspace_and_wraps() {
        let store = Store::default();
        let mut c = cell();
        assert_eq!(c.wheel(false, &env(&store)), Action::Workspace("2".into()));
        assert_eq!(c.wheel(true, &env(&store)), Action::WorkspaceAt(3, "o".into()));
    }

    #[test]
    fn a_click_lands_on_the_icon_or_the_chip() {
        let store = Store::default();
        let mut c = cell();
        // Past the label (44 + 6 + 8 + 2) the first icon spans 60..70.
        assert_eq!(c.click(Button::Left, (65.0, 5.0), &env(&store)), Action::Window("a".into()));
        assert_eq!(c.click(Button::Left, (77.0, 5.0), &env(&store)), Action::Window("b".into()));
        assert_eq!(c.click(Button::Left, (47.0, 5.0), &env(&store)), Action::Workspace("2".into()));
        assert_eq!(c.click(Button::Left, (90.0, 5.0), &env(&store)), Action::WorkspaceAt(3, "o".into()));
        assert_eq!(c.click(Button::Left, (42.0, 5.0), &env(&store)), Action::None);
        assert_eq!(c.click(Button::Right, (65.0, 5.0), &env(&store)), Action::None);
    }

    #[test]
    fn the_focused_chip_stays_put() {
        assert_eq!(cell().go(0), Action::None);
    }

    #[test]
    fn the_pointer_reports_what_it_changed() {
        let mut c = cell();
        assert_eq!(c.pointer(Some((65.0, 5.0))), (true, false));
        assert_eq!(c.pointer(Some((66.0, 5.0))), (false, false));
        c.show_apps = "hover".into();
        assert_eq!(c.pointer(Some((10.0, 5.0))), (true, true));
        assert_eq!(c.pointer(None), (true, true));
    }

    #[test]
    fn the_breathing_pulse_swings_between_one_and_two_fifths() {
        let t = Instant::now();
        assert_eq!(breathe(t, t), 1.0);
        let mid = breathe(t, t + std::time::Duration::from_millis(PULSE_MS as u64));
        assert!((mid - 0.4).abs() < 1e-6);
        let back = breathe(t, t + std::time::Duration::from_millis(PULSE_MS as u64 * 2));
        assert!((back - 1.0).abs() < 1e-6);
    }
}
