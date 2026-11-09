//! The element tree a surface describes its content with. Plain data,
//! rebuilt whenever the surface redraws: a widget's identity across
//! rebuilds is its key path, which is where its nodes and its running
//! animations live (`ui::Ui`).

use fs_theme::color::Rgba;


/// How wide an element is in its parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// Its own natural width.
    Hug,
    /// A share of what the parent has left.
    Fill,
    Px(f64),
    /// Its natural width, capped at this share of the parent row's inner
    /// width.
    AtMost(f64),
}

/// Ink as a token, or as the enclosing `Cell`'s own (its `foreground` and
/// `dimForeground`), so a row's label follows its state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ink {
    Fg,
    Dim,
    Muted,
    Primary,
    Destructive,
    Color(Rgba),
}

/// A role in the type scale; `mono` is for values, the rest are words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Caption,
    BodySmall,
    Body,
    Subtitle,
    Title,
    Heading,
    Display,
    DisplayLarge,
    /// A readout that is the panel's subject (the battery's percent),
    /// `displayLarge` and a half.
    Hero,
    /// The launcher's field, `LAUNCHER.field_size` on the base size.
    Field,
    /// A launcher row's words, `LAUNCHER.row_size` on the base size.
    Row,
}

impl Type {
    /// The weight libadwaita draws a heading role at (`.heading`, `.title-4`,
    /// `.title-3` at 700, `.title-1` at 800); `None` leaves the caller's.
    pub fn role_weight(self) -> Option<Weight> {
        match self {
            Type::Subtitle | Type::Title | Type::Heading => Some(Weight::Bold),
            Type::Display => Some(Weight::Heavy),
            Type::Caption | Type::BodySmall | Type::Body | Type::DisplayLarge | Type::Hero | Type::Field | Type::Row => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Normal,
    Medium,
    Semibold,
    Bold,
    Heavy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub size: Type,
    pub weight: Weight,
    pub mono: bool,
}

/// `button.<variant>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Default,
    Outline,
    Ghost,
    Selected,
    Destructive,
}

impl Variant {
    pub fn role(self) -> fs_theme::style::Role {
        use fs_theme::style::Role;
        match self {
            Variant::Default => Role::ButtonDefault,
            Variant::Outline => Role::ButtonOutline,
            Variant::Ghost => Role::ButtonGhost,
            Variant::Selected => Role::ButtonSelected,
            Variant::Destructive => Role::ButtonDestructive,
        }
    }
}

/// A `Cell`'s states.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellState {
    pub ghost: bool,
    pub selected: bool,
    pub active: bool,
    pub destructive: bool,
    pub warning: bool,
    /// A badge inside a row: hugs its label rather than taking a row's height.
    pub chip: bool,
    /// Drawn hovered whatever the pointer does (the gallery's sample).
    pub hovered: bool,
    /// Drawn with the cursor whatever the keyboard does (the gallery's).
    pub cursor: bool,
    /// `radiusSm`, a cell one level inside a card's own frame (a day cell).
    pub small: bool,
    /// A panel's hero, drawn in the `hero` box.
    pub hero: bool,
    /// A launcher row: the `launcher.row` box, its ink unchanged when
    /// selected and no cursor ring.
    pub row: bool,
    /// The launcher's mode chip, in the `launcher.token` box.
    pub token: bool,
    /// A sidebar card, in the `card` box with its own padding all round.
    pub card: bool,
    /// `xs` either side instead of the control padding: an icon chip in a
    /// narrow grid.
    pub tight: bool,
    /// The padding across and along, in place of the state's own.
    pub inset: Option<(f64, f64)>,
    /// The `ghostOpen` soft fill: the row a list is on now (the connected
    /// network), quieter than `selected`.
    pub open: bool,
    /// A panel's inset group of rows, in the `panel.group` box, as tall as
    /// its rows.
    pub group: bool,
    /// A row's icon chip, in the `panel.chip` box, its `on` state with
    /// `open`.
    pub glyph: bool,
    /// Drawn round whatever its role's radius: Pond's session chips.
    pub round: bool,
    /// Taking no pointer: no hover outline or wash.
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub label: String,
    pub icon: String,
    pub enabled: bool,
    pub active: bool,
}

impl Opt {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), icon: String::new(), enabled: true, active: false }
    }

    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = name.into();
        self
    }
}

/// A decoded picture compared by identity, so a rebuilt tree that holds the
/// same bitmap is the same tree.
#[derive(Clone, Debug)]
pub struct Pic(pub Option<crate::scene::Bitmap>);

impl PartialEq for Pic {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => a.same_as(b),
            (None, None) => true,
            _ => false,
        }
    }
}

/// What a [`Kind::Ring`] says: how far something is, that it cannot say,
/// or how it ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ring {
    /// 0..1 of the way.
    Progress(f64),
    /// Running with no measure: a short arc that turns.
    Unknown,
    Done,
    Failed,
}

/// A series for a `Sparkline`: newest last, read against `ceiling`.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub values: Vec<f64>,
    pub secondary: Vec<f64>,
    pub ceiling: f64,
    pub capacity: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Column { gap: f64, children: Vec<El> },
    Row { gap: f64, children: Vec<El> },
    /// Children laid `columns` to a line, each line `gap` apart.
    Grid { columns: usize, gap: f64, children: Vec<El> },
    /// Room and nothing else.
    Space { along: f64 },
    Text { text: String, font: Font, ink: Ink, elide: bool },
    Icon { name: String, size: Type, ink: Ink },
    Cell { state: CellState, interactive: bool, child: Box<El> },
    Button { variant: Variant, text: String, icon: String, enabled: bool, square: bool },
    Switch { checked: bool, enabled: bool },
    Track { value: f64, notch: Option<f64>, interactive: bool },
    /// `wrap`: rows of equal buttons when every whole label cannot fit one.
    Group { options: Vec<Opt>, index: usize, exclusive: bool, cursor_index: usize, wrap: bool, squares: bool },
    Segmented { options: Vec<String>, index: usize },
    /// `caret`: the caret and the selection's other end as char counts
    /// into `text`; none puts the caret at its end.
    Input { text: String, placeholder: String, focused: bool, error: Option<String>, caret: Option<(usize, usize)> },
    /// The same line with no frame of its own, at `size` (the launcher's
    /// query, whose header is the frame).
    Edit { text: String, placeholder: String, caret: (usize, usize), size: Type },
    Separator { vertical: bool, inset: f64, bleed: f64 },
    Keycap { key: String },
    /// A row's number in a hairline ring, a `sm` step under the row's text.
    Ordinal { n: usize },
    Swatch { color: Rgba, w: f64, h: f64, radius: f64, border: bool },
    /// A `controlHeight` progress ring, or with `indicator` Pond's 16px one.
    Ring { ring: Ring, indicator: bool },
    Sparkline(Series),
    Marquee { text: String, ink: Ink, max: f64 },
    /// Words wrapped at the width they get, cut with an ellipsis past `lines`.
    Para { text: String, font: Font, ink: Ink, lines: usize },
    /// A bitmap fitted at its own size in a `size` box (width, height);
    /// blank without one. `checked` marks it the one in use with a check on
    /// a disc in its bottom-right corner, `ringed` the choice in effect with
    /// the `ring` round its edge, and `badge` names an icon on a disc in its
    /// top-left one. `well` draws a `muted` field where there is no bitmap.
    Picture { pic: Pic, size: (f64, f64), checked: bool, ringed: bool, badge: Option<&'static str>, well: bool },
    /// A square of modules, one string of `1`s and `0`s per row (a QR
    /// code), centred at the largest whole module that fits.
    Matrix { rows: Vec<String> },
    /// A `muted` well under a border, the picture filling it
    /// inside the border; the well alone until the picture lands.
    Cover { pic: Pic, size: f64 },
    /// A feature's own drawing, beside the panel it belongs to.
    Custom(Box<dyn super::Widget>),
}

/// One element: what it is, how wide it sits, and how it is addressed.
#[derive(Clone, Debug, PartialEq)]
pub struct El {
    pub kind: Kind,
    pub width: Size,
    /// Insets: start, top, end, bottom.
    pub pad: [f64; 4],
    /// Identity across rebuilds; the index in the parent otherwise.
    pub key: Option<String>,
    /// A keyboard cursor stop, addressed by `key`.
    pub stop: bool,
    /// What a click fires, addressed to the surface by this id.
    pub on: Option<String>,
    pub tip: Option<String>,
    /// Centres a child across a column, rather than starting it at the edge.
    pub centred: bool,
    /// A row's children start at its top rather than its middle.
    pub top: bool,
    /// Text centred in the width it is given.
    pub mid: bool,
    /// Text against the far end of the width it is given.
    pub end: bool,
    /// Text laid out at least as wide as this one shaped in its own font
    /// (a column of times sized off its widest).
    pub gauge: Option<String>,
    /// Breathes between full and 0.4 opacity on the pulse clock.
    pub pulse: bool,
    /// What this node shows, so a change of it slides and fades the node in.
    pub swap: Option<i64>,
    /// An icon on a disc over the bottom-right corner (a notification's
    /// source mark).
    pub badge: Option<&'static str>,
    /// The node's whole opacity, for a parent that fades it itself.
    pub fade: Option<f32>,
    /// A height set outright, over what the node measures to.
    pub height: Option<f64>,
    /// The share of its measured height the node takes, 0 to 1, its
    /// content cut at that edge (an in-place menu growing open).
    pub reveal: Option<f64>,
    /// A list floating over everything else under this node (a dropdown's
    /// options): it takes no room in the layout, and its stops follow this
    /// node's.
    pub float: Option<Box<El>>,
    /// In a row, as tall as the row whatever it measures to.
    pub stretch: bool,
}

impl El {
    pub fn new(kind: Kind) -> Self {
        let width = match &kind {
            Kind::Column { .. } | Kind::Separator { vertical: false, .. } | Kind::Track { .. } | Kind::Group { .. } => Size::Fill,
            Kind::Cell { state, .. } if !state.chip => Size::Fill,
            Kind::Input { .. } | Kind::Edit { .. } | Kind::Para { .. } | Kind::Matrix { .. } => Size::Fill,
            _ => Size::Hug,
        };
        Self { kind, width, pad: [0.0; 4], key: None, stop: false, on: None, tip: None, centred: false, top: false, mid: false, end: false, gauge: None, pulse: false, swap: None, badge: None, fade: None, height: None, reveal: None, float: None, stretch: false }
    }

    pub fn custom(widget: impl super::Widget) -> Self {
        Self::new(Kind::Custom(Box::new(widget)))
    }

    pub fn stretch(mut self) -> Self {
        self.stretch = true;
        self
    }

    pub fn float(mut self, list: El) -> Self {
        self.float = Some(Box::new(list));
        self
    }

    pub fn fade(mut self, alpha: f32) -> Self {
        self.fade = Some(alpha);
        self
    }


    pub fn reveal(mut self, share: f64) -> Self {
        self.reveal = Some(share.clamp(0.0, 1.0));
        self
    }

    pub fn badge(mut self, icon: &'static str) -> Self {
        self.badge = Some(icon);
        self
    }

    pub fn swap(mut self, token: i64) -> Self {
        self.swap = Some(token);
        self
    }

    pub fn pulse(mut self, on: bool) -> Self {
        self.pulse = on;
        self
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// A cursor stop: the keyboard can land here, and Enter activates it.
    pub fn stop(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self.stop = true;
        self
    }

    pub fn on(mut self, action: impl Into<String>) -> Self {
        self.on = Some(action.into());
        self
    }

    pub fn tip(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        self.tip = (!text.is_empty()).then_some(text);
        self
    }

    pub fn width(mut self, width: Size) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    pub fn fill(self) -> Self {
        self.width(Size::Fill)
    }

    pub fn hug(self) -> Self {
        self.width(Size::Hug)
    }

    pub fn pad(mut self, start: f64, top: f64, end: f64, bottom: f64) -> Self {
        self.pad = [start, top, end, bottom];
        self
    }

    pub fn pad_start(mut self, start: f64) -> Self {
        self.pad[0] = start;
        self
    }

    pub fn pad_top(mut self, top: f64) -> Self {
        self.pad[1] = top;
        self
    }

    pub fn top(mut self) -> Self {
        self.top = true;
        self
    }

    pub fn centred(mut self) -> Self {
        self.centred = true;
        self
    }

    pub fn gauge(mut self, widest: impl Into<String>) -> Self {
        self.gauge = Some(widest.into());
        self
    }

    pub fn mid(mut self) -> Self {
        self.mid = true;
        self
    }

    pub fn end(mut self) -> Self {
        self.end = true;
        self
    }

    /// The caret and the selection's other end, as char counts.
    pub fn caret(mut self, at: (usize, usize)) -> Self {
        if let Kind::Input { caret, .. } = &mut self.kind {
            *caret = Some(at);
        }
        self
    }

    pub fn elide(mut self) -> Self {
        if let Kind::Text { elide, .. } = &mut self.kind {
            *elide = true;
            self.width = Size::Fill;
        }
        self
    }

    pub fn ink(mut self, to: Ink) -> Self {
        match &mut self.kind {
            Kind::Text { ink, .. } | Kind::Icon { ink, .. } | Kind::Marquee { ink, .. } | Kind::Para { ink, .. } => *ink = to,
            _ => {}
        }
        self
    }

    pub fn mono(mut self) -> Self {
        if let Kind::Text { font, .. } | Kind::Para { font, .. } = &mut self.kind {
            font.mono = true;
        }
        self
    }

    pub fn weight(mut self, w: Weight) -> Self {
        if let Kind::Text { font, .. } | Kind::Para { font, .. } = &mut self.kind {
            font.weight = w;
        }
        self
    }

    pub fn size(mut self, t: Type) -> Self {
        match &mut self.kind {
            Kind::Text { font, .. } | Kind::Para { font, .. } => {
                font.size = t;
                if let Some(w) = t.role_weight() {
                    font.weight = w;
                }
            }
            Kind::Icon { size, .. } => *size = t,
            _ => {}
        }
        self
    }

    pub fn variant(mut self, v: Variant) -> Self {
        if let Kind::Button { variant, .. } = &mut self.kind {
            *variant = v;
        }
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        if let Kind::Button { enabled, .. } | Kind::Switch { enabled, .. } = &mut self.kind {
            *enabled = on;
        }
        self
    }

    /// A group's keyboard ring, on an option other than the selected one.
    pub fn ring(mut self, at: usize) -> Self {
        if let Kind::Group { cursor_index, .. } = &mut self.kind {
            *cursor_index = at;
        }
        self
    }

    /// §6's segmented choice: each option its own `panel.chip` square,
    /// `panelRow` tall and `panelGap` apart, with no trough round them.
    pub fn squares(mut self) -> Self {
        if let Kind::Group { squares, .. } = &mut self.kind {
            *squares = true;
        }
        self
    }

    /// A group breaks into rows rather than cut a label.
    pub fn wrap(mut self) -> Self {
        if let Kind::Group { wrap, .. } = &mut self.kind {
            *wrap = true;
        }
        self
    }

    pub fn notch(mut self, at: f64) -> Self {
        if let Kind::Track { notch, .. } = &mut self.kind {
            *notch = Some(at);
        }
        self
    }

    pub fn cell_state(mut self, f: impl FnOnce(&mut CellState)) -> Self {
        if let Kind::Cell { state, .. } = &mut self.kind {
            f(state);
            if state.chip {
                self.width = Size::Hug;
            }
        }
        self
    }

    pub fn ghost(self) -> Self {
        self.cell_state(|s| s.ghost = true)
    }

    pub fn selected(self, on: bool) -> Self {
        self.cell_state(|s| s.selected = on)
    }

    pub fn active(self, on: bool) -> Self {
        self.cell_state(|s| s.active = on)
    }

    /// The row a list is on now: its leading icon chip tinted, the row
    /// itself left bare. A filled row inside a group reads as a card in a
    /// card (specs/2026-10-30-panel-language.md §2), so the hover wash is
    /// the only fill a row takes.
    pub fn current(mut self, on: bool) -> Self {
        if let Kind::Cell { child, .. } = &mut self.kind {
            if let Kind::Row { children, .. } = &mut child.kind
                && let Some(Kind::Cell { state, child, .. }) = children.first_mut().map(|c| &mut c.kind)
                && state.glyph
            {
                state.open = on;
                if let Kind::Row { children, .. } = &mut child.kind {
                    for c in children {
                        if let Kind::Icon { ink, .. } = &mut c.kind {
                            *ink = if on { Ink::Primary } else { Ink::Fg };
                        }
                    }
                }
            }
        }
        self
    }

    pub fn interactive(mut self) -> Self {
        if let Kind::Cell { interactive, .. } = &mut self.kind {
            *interactive = true;
        }
        self
    }
}
