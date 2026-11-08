//! The cell kit (the pieces a bar widget is built
//! from: Icon, CellLabel, CellRow, MarqueeText).
//!
//! A cell is one module under `cells/` implementing [`Cell`]: which store
//! slices it reads, how it re-reads them, and the [`View`] it shows (its
//! parts, tone, tooltip and the panel it opens), plus what a click or a wheel
//! notch on it does. The kit measures and draws a view, so a cell never
//! touches the scene; one that needs its own drawing (the workspaces' pill)
//! implements [`Custom`] instead. `cells::build` is where a cell is
//! registered against its `bar.layout` name.

use std::time::Instant;

use fs_types::{Edge, Region, WindowId, WorkspaceId};
use fs_theme::color::Rgba;
use fs_theme::style::Glow;
use fs_theme::tokens::WEIGHTS;
use vello_cpu::kurbo::Affine;

use crate::paint::{Ink, Kit, Look, Painter, Tone};
use crate::scene::{Fade, IRect, Scene};
use crate::store::{Store, Topic};
use crate::text::{self, Family, ShapedText, TextStyle};
use crate::ui::el::Ring;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// What an input on a cell asks the shell to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    /// Open or close the named panel from this cell.
    Panel(&'static str),
    /// Open or close quick settings from its region's chevron.
    Overflow(Region),
    Workspace(WorkspaceId),
    /// A persistent placeholder has no id: the workspace by ordinal.
    WorkspaceAt(i64),
    Window(WindowId),
    /// A click on one tray item; `offset` is that item's centre along the
    /// strip from the cell's own centre, for a menu to hang under it.
    Tray { key: String, click: TrayClick, offset: f64 },
    /// A write to a device service (a volume step, a mute, a radio).
    Device(crate::services::devices::Op),
    /// Caffeinate on or off (the indicator's click).
    Caffeinate(bool),
    /// The overnight indicator's click: end it.
    OvernightOff,
    /// The recording indicator's click: stop it.
    RecordStop,
    /// The reminder indicator's click: a toast listing what is pending.
    ReminderSummary,
    MediaNext,
    MediaPrevious,
    /// The notification centre, open or shut.
    Center,
    Dnd(bool),
    /// The launcher, open or shut.
    Launcher,
    /// The sidebar folded to its rail, or opened out of it.
    Sidebar(bool),
    /// The Software card's Apply or Undo.
    Software(crate::services::packages::Change),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayClick {
    Activate,
    Secondary,
    Menu,
}

/// What a cell reads when it re-reads.
pub struct Env<'a> {
    pub store: &'a Store,
    pub edge: Edge,
    /// The output the bar is on.
    pub output: &'a str,
}


/// Nerd Font distro logos sit small in their em box next to Lucide icons of the same size.
const GLYPH_SCALE: f32 = 1.4;

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    /// An icon by name, through `theme.icons`; `dim` takes the meta ink,
    /// `dot` is the bell's pending mark on its corner.
    Icon { name: String, dim: bool, dot: bool },
    /// An icon centred in a fixed-width slot, MonitorWidget's `huge` one.
    SlotIcon { name: String, width: f64 },
    /// A glyph in a named family (the distro logo).
    Glyph { text: String, family: &'static str },
    /// Mono, body, medium unless `weight` says otherwise.
    Label { text: String, weight: Option<f32> },
    /// A `Label` in the dim ink (`color: dimForeground`).
    DimLabel { text: String },
    /// The `meta` label: the caption size in sans, in the meta
    /// ink, for the honest words (NO AUTH, NO LAYOUT). The text arrives
    /// already upper case.
    Meta { text: String },
    /// A sans name elided at `max`, hidden on a vertical bar.
    Name { text: String, max: f64, dim: bool },
    /// Free text drawn at the budget the bar hands this
    /// cell, scrolling once it outgrows it. `lead` is its own padding,
    /// `ceiling` the most it ever draws.
    Free { text: String, dim: bool, lead: f64, ceiling: f64, cross: bool },
    /// A cover in the icon's slot, `size` square: the `muted` well under a
    /// border, the picture inside it once decoded. Content imagery, so it
    /// keeps its own colours on any cell fill.
    Cover { image: crate::ui::el::Pic, size: f64 },
    /// A window's app icon, `size` square, bare: the bar's one image icon.
    AppIcon { image: crate::ui::el::Pic, size: f64 },
    /// A progress ring in the icon's slot (`ui::ring_strokes`), in the
    /// cell's own ink; an unknown one turns while motion is on.
    Ring(Ring),
}

impl Part {
    pub fn icon(name: impl Into<String>) -> Self {
        Part::Icon { name: name.into(), dim: false, dot: false }
    }

    /// An icon in the meta ink when `on`.
    pub fn dim(mut self, on: bool) -> Self {
        if let Part::Icon { dim, .. } = &mut self {
            *dim = on;
        }
        self
    }

    /// An icon with the pending mark on its corner when `on`.
    pub fn dot(mut self, on: bool) -> Self {
        if let Part::Icon { dot, .. } = &mut self {
            *dot = on;
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// False drops the cell off the strip (a Battery with no battery).
    pub shown: bool,
    pub parts: Vec<Part>,
    /// The CellRow spacing between parts.
    pub gap: f64,
    pub tone: Tone,
    /// The panel this cell opens, for its open mark and as its anchor.
    pub panel: Option<&'static str>,
    pub tooltip: String,
    pub interactive: bool,
    /// Whole-cell opacity (the iPhone out of range).
    pub opacity: f32,
}

impl View {
    pub fn new(parts: Vec<Part>, gap: f64) -> Self {
        Self { shown: true, parts, gap, tone: Tone::Rest, panel: None, tooltip: String::new(), interactive: true, opacity: 1.0 }
    }

    pub fn hidden() -> Self {
        Self { shown: false, ..Self::new(Vec::new(), 0.0) }
    }

    pub fn icon(name: &str, look: &Look) -> Self {
        Self::new(vec![Part::icon(name)], look.xxs)
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = text.into();
        self
    }

    pub fn panel(mut self, panel: &'static str) -> Self {
        self.panel = Some(panel);
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// A part moves on its own every frame: a ring with no progress.
    pub fn turning(&self, look: &Look) -> bool {
        look.motion && self.shown && self.parts.iter().any(|p| matches!(p, Part::Ring(Ring::Unknown)))
    }
}

/// What a quick settings info tile shows of a cell it stands for
/// (specs/2026-11-04-quick-settings.md §7): its icon, its name, its value
/// in words, and its tone.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub icon: String,
    pub name: String,
    pub value: Option<String>,
    pub tone: Tone,
}

impl Summary {
    pub fn new(icon: impl Into<String>, name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { icon: icon.into(), name: name.into(), value: Some(value.into()).filter(|v: &String| !v.is_empty()), tone: Tone::Rest }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

/// A free-running label's ceiling and floor (`labelCap`, `labelMin`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub cap: f64,
    pub min: f64,
}

/// One bar cell. `reads` names the store slices whose change calls `read`
/// again; `read` returns whether what the cell shows changed.
pub trait Cell {
    fn reads(&self) -> &'static [Topic];
    fn read(&mut self, env: &Env) -> bool;
    fn view(&self, look: &Look) -> View;
    /// The two cells carrying a free label (the track, the window title)
    /// answer their own limits; `along` is the strip's length and `rest`
    /// the cell's extent less its label.
    fn limits(&self, _look: &Look, _along: f64, _rest: f64) -> Option<Limits> {
        None
    }
    fn click(&mut self, _button: Button, _at: (f64, f64), _env: &Env) -> Action {
        Action::None
    }
    /// The tooltip of the part under the pointer, with where that part sits
    /// along the cell (start, extent): a cell whose icons each have words.
    fn tip_at(&self) -> Option<(String, f64, f64)> {
        None
    }
    /// The next instant the cell has something to change on its own (a
    /// countdown's next second), so it is read again then.
    fn wake(&self) -> Option<Instant> {
        None
    }
    fn wheel(&mut self, _up: bool, _env: &Env) -> Action {
        Action::None
    }
    /// One axis event in notches (a touchpad's fractions included), the
    /// vertical and the sideways axis; a cell that wants the magnitude
    /// overrides this, the rest step once per event.
    fn wheel_by(&mut self, vertical: f64, _sideways: f64, env: &Env) -> Action {
        if vertical == 0.0 { Action::None } else { self.wheel(vertical < 0.0, env) }
    }
    /// Whether the window this cell is on is on screen, told on every paint
    /// (a cell running a child process only while it is seen).
    fn visible(&mut self, _on: bool) {}
    /// Whether the panel this cell opens is up; true when
    /// the view changed with it.
    fn set_open(&mut self, _open: bool) -> bool {
        false
    }
    /// A cell that draws itself rather than through its view.
    fn custom(&mut self) -> Option<&mut dyn Custom> {
        None
    }
    /// What `workspaces status` reads off the cell that carries it.
    fn status(&self) -> Option<serde_json::Value> {
        None
    }
    /// The cell as a quick settings info tile, read after `read`; none
    /// where its view is hidden or it has nothing to say in words.
    fn summary(&self) -> Option<Summary> {
        None
    }
}

/// A self-drawn cell: it measures and paints its own content inside the
/// kit's cell box, and says while it animates.
pub trait Custom {
    fn measure(&mut self, kit: &mut Kit, vertical: bool, band: bool) -> f64;
    fn draw(&mut self, kit: &mut Kit, p: &mut Painter, rect: IRect, ink: &Ink, now: Instant);
    fn animating(&self, now: Instant) -> bool;
    /// `debug r0Spinner`: the agent badge on the first chip.
    fn spinner(&mut self, _on: bool, _now: Instant) {}
    /// The cell washes its own children under the pointer, so the kit keeps
    /// its whole-box wash off.
    fn own_hover(&self) -> bool {
        false
    }
    /// The pointer's place in the cell's box, or none once it left. Answers
    /// whether what the cell draws moved with it and whether it changed what
    /// the cell measures.
    fn pointer(&mut self, _at: Option<(f64, f64)>) -> (bool, bool) {
        (false, false)
    }
    /// The room the strip has for this cell (its own extent plus what the
    /// strip has left over); true when the cell's answer to it changed, which
    /// measures it again.
    fn room(&mut self, _budget: f64) -> bool {
        false
    }
}

/// One measured view: its extent along the strip, and where its free label
/// landed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measured {
    pub along: f64,
    /// Each part's (along, across) box; 0 along means not drawn.
    boxes: Vec<(f64, f64)>,
    pub free: Option<FreeLabel>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FreeLabel {
    /// Lead plus the whole text (`naturalLabelWidth`).
    pub natural: f64,
    /// What it draws (`labelExtent`).
    pub extent: f64,
    pub overflow: bool,
    /// The marquee's loop: the text plus its gap.
    pub loop_width: f64,
}

/// How a slot is drawn this frame, past its view.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    pub rect: IRect,
    pub edge: Edge,
    pub hovered: bool,
    /// The open mark's presence (scale) and fade, 0 to 1.
    pub open: (f64, f64),
    /// The slot's presence fade.
    pub alpha: f32,
    pub band: Option<(Rgba, &'a [Glow<Rgba>])>,
    /// The free label's scroll, in pixels, while its marquee runs.
    pub scroll: Option<f64>,
    /// A text part's outgoing string while its replacement fades in: the
    /// part's index, the old part and the new one's opacity.
    pub ghost: Option<(usize, &'a Part, f32)>,
    pub now: Instant,
}

impl Kit {
    fn part_shape(&mut self, part: &Part, band: bool) -> ShapedText {
        let look = self.look.clone();
        match part {
            Part::Icon { name, .. } | Part::SlotIcon { name, .. } => self.icon(name),
            Part::Cover { .. } | Part::AppIcon { .. } => self.icon("music"),
            Part::Ring(_) => self.icon("circle"),
            Part::Glyph { text, family } => self.shape(text, TextStyle { family: Family::Named(family), size: look.body * GLYPH_SCALE, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }),
            Part::Label { text, weight } => {
                let w = if band { WEIGHTS.semibold } else { weight.unwrap_or(WEIGHTS.medium as f32) as f64 };
                self.shape(text, look.label(w as f32))
            }
            Part::DimLabel { text } => {
                let w = if band { WEIGHTS.semibold } else { WEIGHTS.medium };
                self.shape(text, look.label(w as f32))
            }
            Part::Meta { text } => {
                let w = if band { WEIGHTS.semibold } else { WEIGHTS.medium };
                self.shape(text, TextStyle { family: look.sans, size: look.caption, weight: w as f32, tracking: look.meta_tracking, wdth: TextStyle::NORMAL_WIDTH, wght: None })
            }
            Part::Name { text, .. } => self.shape(text, look.sans(WEIGHTS.medium as f32)),
            Part::Free { text, .. } => {
                let w = if band { WEIGHTS.semibold } else { WEIGHTS.normal };
                self.shape(text, look.sans(w as f32))
            }
        }
    }

    /// A view's extent along the strip at a given free-label budget.
    pub fn measure(&mut self, view: &View, vertical: bool, budget: f64, band: bool) -> Measured {
        let look = self.look.clone();
        let icon_h = self.icon("circle").line_height() as f64;
        let mut boxes = Vec::with_capacity(view.parts.len());
        let mut free = None;
        for part in &view.parts {
            let shaped = self.part_shape(part, band);
            let w = shaped.width as f64;
            let h = shaped.line_height() as f64;
            let b = match part {
                Part::Icon { .. } | Part::Glyph { .. } | Part::SlotIcon { .. } => {
                    let along = match part {
                        Part::SlotIcon { width, .. } => *width,
                        Part::Glyph { .. } => (look.body * GLYPH_SCALE) as f64,
                        _ => look.body as f64,
                    };
                    if vertical { (icon_h, along) } else { (along, icon_h) }
                }
                Part::Cover { size, .. } | Part::AppIcon { size, .. } => {
                    let across = icon_h.max(*size);
                    if vertical { (across, *size) } else { (*size, across) }
                }
                Part::Ring(_) => (icon_h, icon_h),
                Part::Label { .. } | Part::DimLabel { .. } | Part::Meta { .. } => {
                    if vertical && w > look.content_across() + 0.5 {
                        (0.0, 0.0)
                    } else if vertical {
                        (h, w)
                    } else {
                        (w, h)
                    }
                }
                Part::Name { max, .. } => {
                    if vertical || w <= 0.0 { (0.0, 0.0) } else { (w.min(*max), h) }
                }
                Part::Free { text, lead, ceiling, .. } => {
                    let lead = if vertical { 0.0 } else { *lead };
                    let natural = if text.is_empty() { 0.0 } else { lead + w };
                    let extent = natural.min(budget).min(*ceiling).max(0.0);
                    let gap = (look.body as f64 * 2.5).round();
                    free = Some(FreeLabel { natural, extent, overflow: natural > extent + 0.5, loop_width: w + gap });
                    (extent, h)
                }
            };
            boxes.push(b);
        }
        let drawn: Vec<&(f64, f64)> = boxes.iter().filter(|b| b.0 > 0.0).collect();
        let content = drawn.iter().map(|b| b.0).sum::<f64>() + view.gap * drawn.len().saturating_sub(1) as f64;
        Measured { along: content + look.pad_x * 2.0, boxes, free }
    }

    /// The cell box under any content: the fill and border its tone and
    /// state resolve to, the open indicator filling it, and the pointer's
    /// wash.
    pub fn draw_box(&mut self, p: &mut Painter, tone: Tone, f: &Frame) {
        let look = self.look.clone();
        let r = f.rect;
        let alpha = f.alpha;
        let a = |c: Rgba| c.with_alpha(c.a * alpha);
        let fill = look.ghost_open.with_alpha(look.ghost_open.a * f.open.1.max(0.0) as f32);
        let border = match tone {
            Tone::Destructive => look.destructive_border,
            Tone::Warning => look.warning_border,
            _ => None,
        };
        let (bc, bw) = border.unwrap_or((Rgba::TRANSPARENT, 0.0));
        if let Some(chip) = &look.chip {
            crate::ui::boxes::paint(p, r, chip, f64::from(look.radius), alpha, 0.0);
        }
        p.framed(r, a(fill), look.radius, a(bc), bw);
        if let Some(wash) = look.hover_wash.filter(|_| f.hovered && matches!(tone, Tone::Rest | Tone::Destructive | Tone::Warning)) {
            p.rect(r, a(wash), look.radius);
        }
    }

    /// A view's content inside its cell box, centred across the strip.
    pub fn draw(&mut self, view: &View, m: &Measured, p: &mut Painter, f: &Frame) {
        self.draw_box(p, view.tone, f);
        let look = self.look.clone();
        let vertical = f.edge.is_vertical();
        let r = f.rect;
        let mut at = if vertical { r.y as f64 + look.pad_x } else { r.x as f64 + look.pad_x };
        for (idx, (part, b)) in view.parts.iter().zip(&m.boxes).enumerate() {
            if b.0 <= 0.0 {
                continue;
            }
            let passes: Vec<(&Part, f32)> = match f.ghost.filter(|g| g.0 == idx) {
                Some((_, old, mix)) => vec![(part, mix), (old, 1.0 - mix)],
                None => vec![(part, 1.0)],
            };
            for (pass, (part, mix)) in passes.into_iter().enumerate() {
                let ink = Ink::resolve(&look, view.tone, f.band, f.alpha * mix);
                let scroll_now = if pass == 0 { f.scroll } else { None };
                let shaped = self.part_shape(part, ink.band);
                let lh = shaped.line_height();
                let across_mid = if vertical { r.x + r.w / 2 } else { r.y + r.h / 2 };
                match part {
                    Part::Icon { .. } | Part::Glyph { .. } | Part::SlotIcon { .. } => {
                        let dim = matches!(part, Part::Icon { dim: true, .. });
                        let dot = matches!(part, Part::Icon { dot: true, .. });
                        let color = ink.of(if dim { ink.dim } else { ink.fg });
                        let size = match part {
                            Part::SlotIcon { width, .. } => *width,
                            Part::Glyph { .. } => (look.body * GLYPH_SCALE) as f64,
                            _ => look.body as f64,
                        };
                        let (bx, by) = if vertical {
                            (across_mid as f64 - size / 2.0, at)
                        } else {
                            (at, across_mid as f64 - b.1 / 2.0)
                        };
                        let x = (bx + (size - shaped.width as f64) / 2.0).round() as i32;
                        let y = if vertical { by.round() as i32 } else { across_mid - lh / 2 };
                        p.text(&shaped, (x, y), color, &ink.glow);
                        if dot {
                            let d = look.md as i32;
                            let dot_r = IRect::new((bx + size) as i32 - d, (if vertical { by } else { across_mid as f64 - b.1 / 2.0 }) as i32, d, d);
                            p.rect(dot_r, ink.of(look.primary), d as f32 / 2.0);
                        }
                    }
                    Part::Ring(ring) => {
                        let d = b.0;
                        let centre = if vertical { (across_mid as f64, at + d / 2.0) } else { (at + d / 2.0, across_mid as f64) };
                        let fill = if *ring == Ring::Failed { look.destructive } else { ink.fg };
                        let turn = if look.motion { crate::ui::ring_turn(f.now, self.motion_scale) } else { 0.0 };
                        let strokes = crate::ui::ring_strokes(centre, d, look.xxs, *ring, turn, ink.of(look.muted_fill), ink.of(fill));
                        let bounds = crate::scene::Scene::cover(
                            vello_cpu::kurbo::Rect::new(centre.0 - d / 2.0, centre.1 - d / 2.0, centre.0 + d / 2.0, centre.1 + d / 2.0),
                            Affine::IDENTITY,
                            2.0,
                        );
                        p.shape(bounds, crate::scene::Paint::Shape { fill: None, strokes });
                        let mark = match ring {
                            Ring::Done => Some("check"),
                            Ring::Failed => Some("x"),
                            _ => None,
                        };
                        if let Some(name) = mark {
                            let g = fs_theme::icons::glyph(name);
                            let size = (d - look.xxs * 4.0).max(1.0) as f32;
                            let glyph = self.shape(g.text, TextStyle { family: Family::Named(g.family), size, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
                            let at = ((centre.0 - glyph.width as f64 / 2.0).round() as i32, (centre.1 - glyph.line_height() as f64 / 2.0).round() as i32);
                            p.text(&glyph, at, ink.of(fill), &[]);
                        }
                    }
                    Part::AppIcon { image, size } => {
                        let n = size.round() as i32;
                        let (x, y) = if vertical { (across_mid - n / 2, at.round() as i32) } else { (at.round() as i32, across_mid - n / 2) };
                        if let Some(image) = &image.0 {
                            let (w, h) = image.size();
                            p.image(image, (x + (n - w) / 2, y + (n - h) / 2), f.alpha * mix);
                        }
                    }
                    Part::Cover { image, size } => {
                        let n = size.round() as i32;
                        let (x, y) = if vertical { (across_mid - n / 2, at.round() as i32) } else { (at.round() as i32, across_mid - n / 2) };
                        let slot = IRect::new(x, y, n, n);
                        let radius = fs_theme::tokens::cover_radius(look.radius_sm, *size) as f32;
                        p.framed(slot, ink.of(look.muted_fill), radius, Rgba::TRANSPARENT, 0.0);
                        if let Some(image) = &image.0 {
                            let (w, h) = image.size();
                            p.image(image, (x + (n - w) / 2, y + (n - h) / 2), f.alpha);
                        }
                        p.framed(slot, Rgba::TRANSPARENT, radius, ink.of(look.border), look.border_width as f32);
                    }
                    Part::Label { .. } | Part::DimLabel { .. } | Part::Meta { .. } => {
                        let color = ink.of(if matches!(part, Part::Label { .. }) { ink.fg } else { ink.dim });
                        let (x, y) = if vertical {
                            ((across_mid - shaped.width / 2), at.round() as i32)
                        } else {
                            (at.round() as i32, across_mid - lh / 2)
                        };
                        p.text(&shaped, (x, y), color, &ink.glow);
                    }
                    Part::Name { dim, .. } => {
                        let color = ink.of(if *dim { ink.dim } else { ink.fg });
                        let x = at.round() as i32;
                        let y = across_mid - lh / 2;
                        let clip = IRect::new(x, r.y, b.0.ceil() as i32, r.h);
                        let (w, h) = shaped.box_size();
                        let bounds = IRect::new(x - text::PAD, y - text::PAD, w, h);
                        let clip = Some(p.clip.map_or(clip, |c| c.intersect(&clip)));
                        p.text_in(&shaped, bounds, Affine::IDENTITY, clip, color, &ink.glow);
                    }
                    Part::Free { dim, lead, .. } => {
                        let color = ink.of(if *dim { ink.dim } else { ink.fg });
                        let lead = if vertical { 0.0 } else { *lead };
                        let extent = b.0;
                        let view_len = (extent - lead).max(0.0);
                        let scroll = scroll_now.unwrap_or(0.0);
                        let loop_w = m.free.as_ref().map_or(0.0, |l| l.loop_width);
                        let (w, h) = shaped.box_size();
                        let (vp, transform_base): (IRect, Affine) = if vertical {
                            let len = view_len.ceil() as i32;
                            let vp = IRect::new(across_mid - lh / 2, at.round() as i32, lh, len);
                            let rot = match f.edge {
                                // Bottom to top on the left, top to bottom on the right.
                                Edge::Left => Affine::new([0.0, -1.0, 1.0, 0.0, vp.x as f64, vp.bottom() as f64]),
                                _ => Affine::new([0.0, 1.0, -1.0, 0.0, vp.right() as f64, vp.y as f64]),
                            };
                            (vp, rot)
                        } else {
                            let vp = IRect::new((at + lead).round() as i32, across_mid - lh / 2, view_len.ceil() as i32, lh);
                            (vp, Affine::translate((vp.x as f64, vp.y as f64)))
                        };
                        let clip = Some(p.clip.map_or(vp, |c| c.intersect(&vp)));
                        let copies: &[f64] = if scroll_now.is_some() { &[0.0, 1.0] } else { &[0.0] };
                        // A cut label fades at its cut ends: the far end
                        // whenever it overflows, the near end growing in as
                        // the scroll leaves its hold and out as the next copy
                        // arrives, so neither end ever pops.
                        let overflow = m.free.as_ref().is_some_and(|l| l.overflow) && pass == 0;
                        let ramp = look.fade;
                        let near = scroll_now.map_or(0.0, |s| s.min(loop_w - s).clamp(0.0, ramp));
                        let (low, high) = match (vertical, f.edge) {
                            (true, Edge::Left) => (ramp, near),
                            _ => (near, ramp),
                        };
                        let faded = overflow.then_some(Fade { view: vp, vertical, low, high });
                        let outer = std::mem::replace(&mut p.fade, faded);
                        for k in copies {
                            let shift = -scroll + k * loop_w;
                            let local = Affine::translate((shift - text::PAD as f64, -text::PAD as f64));
                            let full = transform_base * local;
                            let bounds = Scene::cover(vello_cpu::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), full, 1.0)
                                .intersect(&vp);
                            let bounds = if bounds.is_empty() { continue } else { bounds };
                            let transform = full * Affine::translate((-bounds.x as f64, -bounds.y as f64));
                            p.text_in(&shaped, bounds, transform, clip, color, &ink.glow);
                        }
                        p.fade = outer;
                    }
                }
            }
            at += b.0 + view.gap;
        }
    }
}
