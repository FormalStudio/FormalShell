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

use fs_chrome::types::{Edge, Region};
use fs_theme::color::Rgba;
use fs_theme::style::Glow;
use fs_theme::theme::{Face, Theme};
use fs_theme::tokens::WEIGHTS;
use parley::GenericFamily;
use vello_cpu::kurbo::Affine;

use crate::scene::{Bitmap, IRect, NodeId, Paint, Scene};
use crate::store::{Store, Topic};
use crate::text::{self, Family, ShapedText, Text, TextStyle};

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
    /// Open or close the chevron's second bar for its region.
    Overflow(Region),
    Workspace(String),
    /// A persistent placeholder has no id: the workspace by ordinal, on
    /// the cell's output.
    WorkspaceAt(i64, String),
    Window(String),
    /// A click on one tray item; `offset` is that item's centre along the
    /// strip from the cell's own centre, for a menu to hang under it.
    Tray { id: String, click: TrayClick, offset: f64 },
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum Tone {
    #[default]
    Rest,
    Active,
    Selected,
    Destructive,
    Warning,
}

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
        Self::new(vec![Part::Icon { name: name.into(), dim: false, dot: false }], look.xxs)
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
    /// The extents the cell's parts measured to along the strip, 0 for one
    /// that is not drawn, handed over after every measure.
    fn laid(&mut self, _extents: &[f64]) {}
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
    /// Whether the panel or second bar this cell opens is up; true when
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
}

/// A self-drawn cell: it measures and paints its own content inside the
/// kit's cell box, and says while it animates.
pub trait Custom {
    fn measure(&mut self, kit: &mut Kit, vertical: bool, band: bool) -> f64;
    fn draw(&mut self, kit: &mut Kit, p: &mut Painter, rect: IRect, ink: &Ink, now: Instant);
    fn animating(&self, now: Instant) -> bool;
    /// `debug r0Spinner`: the herdr badge on the first chip.
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

/// What the theme hands every cell: tokens, inks and the `cell` role's
/// states, read once per theme.
#[derive(Clone)]
pub struct Look {
    pub pad_x: f64,
    pub pad_y: f64,
    pub cell_height: f64,
    pub cell_width: f64,
    pub xxs: f64,
    pub huge: f64,
    pub xs: f64,
    pub sm: f64,
    pub md: f64,
    pub body: f32,
    pub caption: f32,
    /// `letterSpacing.meta`, the tracking of a meta label.
    pub meta_tracking: f32,
    pub narrow: f64,
    pub panel_padding: f64,
    pub sans: Family,
    pub mono: Family,
    pub icon_set: String,
    pub foreground: Rgba,
    pub muted: Rgba,
    pub on_active: Rgba,
    pub on_selected: Rgba,
    pub destructive: Rgba,
    pub warning: Rgba,
    pub primary: Rgba,
    pub background: Rgba,
    /// The flat tracks' trough, its thickness and its corner (`muted`,
    /// `trackThickness`, `radiusSm`).
    pub muted_fill: Rgba,
    pub border: Rgba,
    pub track: f64,
    pub radius_sm: f64,
    pub radius: f32,
    pub active_fill: Rgba,
    pub selected_fill: Rgba,
    pub hover_wash: Option<Rgba>,
    pub ghost_open: Option<Rgba>,
    pub destructive_border: Option<(Rgba, f32)>,
    pub warning_border: Option<(Rgba, f32)>,
    pub mark: Rgba,
    pub mark_radius: f32,
    pub border_width: f64,
    pub effects: f64,
    pub effects_slow: f64,
    pub spatial: f64,
    pub spatial_fast: f64,
    pub emphasized: f64,
    pub motion: bool,
    pub marquee_px: f64,
    pub marquee_hold: f64,
    /// The box under every cell (`bar.cell`), none where it draws nothing.
    pub chip: Option<fs_theme::style::BoxStyle<Rgba>>,
    /// The cursor's fill and its leading block (`cursor`, `cursor.bar`),
    /// which mark a hovered or open cell in place of the wash and the
    /// open mark on a table that has the block.
    pub cursor: Option<(Rgba, fs_theme::style::Bar<Rgba>)>,
    /// `reveal: type`'s duration: a label that changes types in.
    pub reveal: Option<f64>,
}

/// A theme text role's family: the table's face over its alias, or the
/// alias alone.
pub fn face(face: Face) -> Family {
    let generic = match face.generic {
        "monospace" => GenericFamily::Monospace,
        "serif" => GenericFamily::Serif,
        _ => GenericFamily::SansSerif,
    };
    face.name.map_or(Family::Generic(generic), |name| Family::Face(name, generic))
}

impl Look {
    pub fn new(theme: &Theme) -> Self {
        let s = &theme.space;
        let cell = |state| theme.box_style("cell", Some(state));
        let border = |state| cell(state).border.map(|l| (l.color, l.width as f32));
        let motion = theme.motion().families;
        let mark = theme.box_style("cell.mark", None);
        Self {
            pad_x: s.control_padding_x,
            pad_y: s.control_padding_y,
            cell_height: s.bar_cell_height,
            cell_width: s.bar_cell_width,
            xxs: s.xxs,
            huge: s.huge,
            xs: s.xs,
            sm: s.sm,
            md: s.md,
            body: theme.font_size.body as f32,
            caption: theme.font_size.caption as f32,
            meta_tracking: theme.letter_spacing.meta as f32,
            narrow: s.popup_width_narrow,
            panel_padding: s.panel_padding,
            sans: face(theme.faces.words),
            mono: face(theme.faces.values),
            icon_set: theme.icon_set.clone(),
            foreground: theme.colors.get("foreground"),
            muted: theme.colors.get("mutedForeground"),
            on_active: theme.colors.get("primaryForeground"),
            on_selected: theme.colors.get("accentForeground"),
            destructive: theme.colors.get("destructive"),
            warning: theme.colors.get("warning"),
            primary: theme.colors.get("primary"),
            background: theme.colors.get("background"),
            muted_fill: theme.colors.get("muted"),
            border: theme.colors.get("border"),
            track: s.track_thickness,
            radius_sm: theme.radii.sm,
            radius: theme.box_radius(&theme.box_style("cell", None), s.bar_cell_height) as f32,
            active_fill: cell("active").fill,
            selected_fill: cell("selected").fill,
            hover_wash: cell("hover").wash,
            ghost_open: theme.has_state("cell", "ghostOpen").then(|| cell("ghostOpen").fill),
            destructive_border: border("destructive"),
            warning_border: border("warning"),
            mark: mark.fill,
            mark_radius: theme.box_radius(&mark, theme.border_width * 2.0) as f32,
            border_width: theme.border_width,
            effects: motion.effects,
            effects_slow: motion.effects_slow,
            spatial: motion.spatial,
            spatial_fast: motion.spatial_fast,
            emphasized: motion.emphasized,
            motion: theme.motion_enabled,
            marquee_px: motion.marquee_px_per_sec,
            marquee_hold: motion.marquee_hold_ms,
            chip: Some(theme.box_style("bar.cell", None)).filter(|b| b.fill.a > 0.0 || b.border.as_ref().is_some_and(|l| l.width > 0.0)),
            cursor: theme.cursor_bar().map(|bar| (theme.box_style("cursor", None).fill, bar)),
            reveal: theme.motion().catalogue.reveal,
        }
    }

    /// The strip's room across a vertical cell's content.
    pub fn content_across(&self) -> f64 {
        (self.cell_width - self.pad_y * 2.0).max(0.0)
    }

    pub fn label(&self, weight: f32) -> TextStyle {
        TextStyle { family: self.mono, size: self.body, weight, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
    }

    pub fn sans(&self, weight: f32) -> TextStyle {
        TextStyle { family: self.sans, size: self.body, weight, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
    }
}

/// The ink a cell's content resolves to, and the band's glow under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ink {
    pub fg: Rgba,
    pub dim: Rgba,
    pub glow: Vec<Glow<Rgba>>,
    /// On a band: labels go semibold.
    pub band: bool,
    pub alpha: f32,
}

impl Ink {
    pub fn resolve(look: &Look, tone: Tone, band: Option<(Rgba, &[Glow<Rgba>])>, alpha: f32) -> Self {
        let band_ink = band.filter(|(c, _)| c.a > 0.0);
        let fg = match tone {
            Tone::Active => look.on_active,
            Tone::Destructive => look.destructive,
            Tone::Warning => look.warning,
            Tone::Selected => look.on_selected,
            Tone::Rest => band_ink.map_or(look.foreground, |(c, _)| c),
        };
        let filled = matches!(tone, Tone::Active | Tone::Selected);
        let dim = if filled || band_ink.is_some() { fg } else { look.muted };
        let glow = match (tone, band_ink) {
            (Tone::Rest, Some((_, g))) => g.to_vec(),
            _ => Vec::new(),
        };
        Self { fg, dim, glow, band: band_ink.is_some(), alpha }
    }

    pub fn of(&self, c: Rgba) -> Rgba {
        c.with_alpha(c.a * self.alpha)
    }
}

/// Writes one owner's nodes in call order, reusing what it wrote last time,
/// so an unchanged frame damages nothing. `finish` hides whatever was not
/// written this time.
pub struct Painter<'a> {
    scene: &'a mut Scene,
    nodes: &'a mut Vec<NodeId>,
    used: usize,
    pub clip: Option<IRect>,
    /// Where a node this painter has not drawn before goes in paint order:
    /// after its own last one, or after this.
    anchor: Option<NodeId>,
}

impl<'a> Painter<'a> {
    pub fn new(scene: &'a mut Scene, nodes: &'a mut Vec<NodeId>, clip: Option<IRect>) -> Self {
        Self { scene, nodes, used: 0, clip, anchor: None }
    }

    /// New nodes start right after `anchor` rather than on top of the scene.
    pub fn after(mut self, anchor: Option<NodeId>) -> Self {
        self.anchor = anchor;
        self
    }

    /// The last node this painter drew, for the next group to follow.
    pub fn last(&self) -> Option<NodeId> {
        self.nodes.last().copied().or(self.anchor)
    }

    pub fn scene(&mut self) -> &mut Scene {
        self.scene
    }

    fn put(&mut self, bounds: IRect, paint: Paint, transform: Affine, clip: Option<IRect>) {
        if self.used == self.nodes.len() {
            let prev = self.nodes.last().copied().or(self.anchor);
            let id = self.scene.add_after(prev, IRect::default(), Paint::Rect { fill: Rgba::TRANSPARENT, radius: 0.0 });
            self.scene.set_visible(id, false);
            self.nodes.push(id);
        }
        let id = self.nodes[self.used];
        self.used += 1;
        self.scene.update_with(id, bounds, paint, true, transform, clip);
    }

    pub fn rect(&mut self, r: IRect, fill: Rgba, radius: f32) {
        if fill.a > 0.0 && !r.is_empty() {
            self.put(r, Paint::Rect { fill, radius }, Affine::IDENTITY, self.clip);
        }
    }

    pub fn framed(&mut self, r: IRect, fill: Rgba, radius: f32, border: Rgba, width: f32) {
        if (fill.a > 0.0 || (border.a > 0.0 && width > 0.0)) && !r.is_empty() {
            self.put(r, Paint::Framed { fill, radius, border, width }, Affine::IDENTITY, self.clip);
        }
    }

    /// Pixels drawn with their top-left on `at`.
    pub fn image(&mut self, image: &Bitmap, at: (i32, i32), alpha: f32) {
        if alpha > 0.0 {
            let (w, h) = image.size();
            let bounds = IRect::new(at.0, at.1, w, h);
            self.put(bounds, Paint::Image { image: image.clone(), alpha }, Affine::IDENTITY, self.clip);
        }
    }

    pub fn shape(&mut self, bounds: IRect, paint: Paint) {
        self.put(bounds, paint, Affine::IDENTITY, self.clip);
    }

    /// A node under its own clip, inside the painter's.
    pub fn shape_in(&mut self, bounds: IRect, paint: Paint, clip: IRect) {
        let clip = self.clip.map_or(clip, |c| c.intersect(&clip));
        self.put(bounds, paint, Affine::IDENTITY, Some(clip));
    }

    /// A line whose line box's top-left lands on `at`.
    pub fn text(&mut self, shaped: &ShapedText, at: (i32, i32), color: Rgba, glow: &[Glow<Rgba>]) {
        let (w, h) = shaped.box_size();
        let bounds = IRect::new(at.0 - text::PAD, at.1 - text::PAD, w, h);
        self.text_in(shaped, bounds, Affine::IDENTITY, self.clip, color, glow);
    }

    /// A line drawn through `transform` (rotated, scrolled), covering
    /// `bounds` in device pixels.
    pub fn text_in(&mut self, shaped: &ShapedText, bounds: IRect, transform: Affine, clip: Option<IRect>, color: Rgba, glow: &[Glow<Rgba>]) {
        if color.a <= 0.0 || shaped.glyphs.is_empty() {
            return;
        }
        for g in glow.iter().rev() {
            let reach = (g.blur + g.x.abs().max(g.y.abs())).ceil() as i32 + 1;
            let wide = IRect::new(bounds.x - reach, bounds.y - reach, bounds.w + reach * 2, bounds.h + reach * 2);
            let shift = Affine::translate((reach as f64, reach as f64));
            let ink = g.color.with_alpha(g.color.a * color.a);
            let paint = Paint::Glow { text: shaped.clone(), color: ink, x: g.x as f32, y: g.y as f32, blur: g.blur as f32 };
            self.put(wide, paint, transform * shift, clip);
        }
        self.put(bounds, Paint::Text { text: shaped.clone(), color }, transform, clip);
    }

    /// A line under a Gaussian blur, its bounds grown by the blur's reach and
    /// cut to `clip`.
    pub fn blurred(&mut self, shaped: &ShapedText, at: (i32, i32), color: Rgba, blur: f32, clip: Option<IRect>) {
        self.blurred_with(shaped, at, color, blur, clip, Affine::IDENTITY);
    }

    /// [`blurred`](Self::blurred) under a transform about the surface's
    /// origin; it must not carry the box past its grown bounds.
    pub fn blurred_with(&mut self, shaped: &ShapedText, at: (i32, i32), color: Rgba, blur: f32, clip: Option<IRect>, about: Affine) {
        if color.a <= 0.0 || shaped.glyphs.is_empty() {
            return;
        }
        let (w, h) = shaped.box_size();
        let reach = (blur * 3.0).ceil() as i32 + 1;
        let bounds = IRect::new(at.0 - text::PAD - reach, at.1 - text::PAD - reach, w + reach * 2, h + reach * 2);
        let shift = Affine::translate((reach as f64, reach as f64));
        self.put(bounds, Paint::Blur { text: shaped.clone(), color, blur }, about * shift, clip);
    }

    pub fn finish(self) {
        for id in &self.nodes[self.used..] {
            self.scene.set_visible(*id, false);
        }
    }
}

/// Shaping, cached by text and style, with the look it shapes for.
pub struct Kit {
    pub text: Text,
    pub look: Look,
    /// `debug motionScale`, on top of the theme's own clocks.
    pub motion_scale: f64,
    /// While set, every style shaped in, so a surface's styles can be
    /// warmed ahead of its next open.
    pub seen: Option<Vec<TextStyle>>,
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
    /// A label part typing in: its index and how many characters show.
    pub typed: Option<(usize, usize)>,
    pub band: Option<(Rgba, &'a [Glow<Rgba>])>,
    /// The free label's scroll, in pixels, while its marquee runs.
    pub scroll: Option<f64>,
    /// A text part's outgoing string while its replacement fades in: the
    /// part's index, the old part and the new one's opacity.
    pub ghost: Option<(usize, &'a Part, f32)>,
}

impl Measured {
    pub fn extents(&self) -> Vec<f64> {
        self.boxes.iter().map(|b| b.0).collect()
    }
}

impl Kit {
    pub fn new(theme: &Theme) -> Self {
        Self { text: Text::new(), look: Look::new(theme), motion_scale: 1.0, seen: None }
    }

    pub fn set_theme(&mut self, theme: &Theme) {
        self.look = Look::new(theme);
    }

    pub fn shape(&mut self, source: &str, style: TextStyle) -> ShapedText {
        if let Some(seen) = &mut self.seen
            && !seen.contains(&style)
        {
            seen.push(style);
        }
        self.text.shape(source, style)
    }

    pub fn icon(&mut self, name: &str) -> ShapedText {
        let g = fs_theme::icons::glyph(&self.look.icon_set, name);
        let style = TextStyle { family: Family::Named(g.family), size: self.look.body, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
        self.shape(g.text, style)
    }

    fn part_shape(&mut self, part: &Part, band: bool) -> ShapedText {
        let look = self.look.clone();
        match part {
            Part::Icon { name, .. } | Part::SlotIcon { name, .. } => self.icon(name),
            Part::Cover { .. } | Part::AppIcon { .. } => self.icon("music"),
            Part::Glyph { text, family } => self.shape(text, TextStyle { family: Family::Named(family), size: look.body, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }),
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
                    let along = if let Part::SlotIcon { width, .. } = part { *width } else { look.body as f64 };
                    if vertical { (icon_h, along) } else { (along, icon_h) }
                }
                Part::Cover { size, .. } | Part::AppIcon { size, .. } => {
                    let across = icon_h.max(*size);
                    if vertical { (across, *size) } else { (*size, across) }
                }
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
    /// state resolve to, the pointer's wash, and the open mark along the
    /// edge facing the desktop.
    pub fn draw_box(&mut self, p: &mut Painter, tone: Tone, f: &Frame) {
        let look = self.look.clone();
        let r = f.rect;
        let alpha = f.alpha;
        let a = |c: Rgba| c.with_alpha(c.a * alpha);
        let open_fill = look.ghost_open.filter(|_| f.open.1 > 0.0);
        let fill = match tone {
            Tone::Active => look.active_fill,
            Tone::Selected => look.selected_fill,
            _ => open_fill.map_or(Rgba::TRANSPARENT, |c| c.with_alpha(c.a * f.open.1 as f32)),
        };
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
        if let Some((cursor, bar)) = &look.cursor {
            // The block on the cell's leading edge, whole while the pointer
            // is on it, fading in with its panel.
            let k = if f.hovered { 1.0 } else { f.open.1 as f32 };
            if k > 0.0 {
                p.rect(r, a(cursor.with_alpha(cursor.a * k)), look.radius);
                let w = (bar.width.round() as i32).min(r.w.min(r.h));
                let block = if f.edge.is_vertical() { IRect::new(r.x, r.y, r.w, w) } else { IRect::new(r.x, r.y, w, r.h) };
                p.rect(block, a(bar.color.with_alpha(bar.color.a * k)), 0.0);
            }
            return;
        }
        if let Some(wash) = look.hover_wash.filter(|_| f.hovered && matches!(tone, Tone::Rest | Tone::Destructive | Tone::Warning)) {
            p.rect(r, a(wash), look.radius);
        }
        if look.ghost_open.is_none() && f.open.1 > 0.0 {
            let t = (look.border_width * 2.0).round().max(1.0) as i32;
            let edge_margin = bw as i32;
            let inset = look.xs as i32;
            let mark = match f.edge {
                Edge::Top => IRect::new(r.x + inset, r.bottom() - t - edge_margin, r.w - inset * 2, t),
                Edge::Bottom => IRect::new(r.x + inset, r.y + edge_margin, r.w - inset * 2, t),
                Edge::Left => IRect::new(r.right() - t - edge_margin, r.y + inset, t, r.h - inset * 2),
                Edge::Right => IRect::new(r.x + edge_margin, r.y + inset, t, r.h - inset * 2),
            };
            // Grows out of the cell's own centre along the bar.
            let s = f.open.0.max(0.0);
            let mark = if f.edge.is_vertical() {
                let h = (mark.h as f64 * s).round() as i32;
                IRect::new(mark.x, mark.y + (mark.h - h) / 2, mark.w, h)
            } else {
                let w = (mark.w as f64 * s).round() as i32;
                IRect::new(mark.x + (mark.w - w) / 2, mark.y, w, mark.h)
            };
            p.rect(mark, look.mark.with_alpha(look.mark.a * (f.open.1 as f32) * alpha), look.mark_radius);
        }
    }

    /// A view's content inside its cell box, centred across the strip.
    pub fn draw(&mut self, view: &View, m: &Measured, p: &mut Painter, f: &Frame) {
        self.draw_box(p, view.tone, f);
        let look = self.look.clone();
        let ink = Ink::resolve(&look, view.tone, f.band, f.alpha);
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
                        let size = if let Part::SlotIcon { width, .. } = part { *width } else { look.body as f64 };
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
                        let shaped = match (f.typed.filter(|t| t.0 == idx && pass == 0), part) {
                            (Some((_, n)), Part::Label { text, .. } | Part::DimLabel { text }) => {
                                let w = if ink.band { WEIGHTS.semibold } else if let Part::Label { weight: Some(w), .. } = part { f64::from(*w) } else { WEIGHTS.medium };
                                self.shape(crate::effects::prefix(text, n), look.label(w as f32))
                            }
                            _ => shaped,
                        };
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
                    }
                }
            }
            at += b.0 + view.gap;
        }
    }
}
