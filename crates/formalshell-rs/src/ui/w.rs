//! Constructors for every widget, one per shadcn part DESIGN.md §2 lists,
//! plus the two composites every panel leans on (`section`, `hero`).
//! Tokens come from `Space`; nothing here writes a literal size.

use fs_chrome::types::Edge;
use fs_menu::field::{Buffer, Field};
use fs_theme::color::Rgba;
use fs_theme::tokens::Space;

use super::el::{CellState, El, Flow, Font, Ink, Kind, Opt, Pic, Series, Type, Variant, Weight};

pub fn column(gap: f64, children: Vec<El>) -> El {
    El::new(Kind::Column { gap, children })
}

pub fn row(gap: f64, children: Vec<El>) -> El {
    El::new(Kind::Row { gap, children })
}

pub fn grid(columns: usize, gap: f64, children: Vec<El>) -> El {
    El::new(Kind::Grid { columns, gap, children }).fill()
}

pub fn space(along: f64) -> El {
    El::new(Kind::Space { along })
}

/// Takes whatever width a row has left, which pushes what follows it to the
/// row's end.
pub fn spacer() -> El {
    text_el("", Type::Caption, Weight::Normal, false, Ink::Fg).elide()
}

fn text_el(s: impl Into<String>, size: Type, weight: Weight, mono: bool, ink: Ink) -> El {
    El::new(Kind::Text { text: s.into(), font: Font { size, weight, mono }, ink, elide: false })
}

/// Body words.
pub fn text(s: impl Into<String>) -> El {
    text_el(s, Type::Body, Weight::Normal, false, Ink::Fg)
}

/// A row's name: body, medium.
pub fn label(s: impl Into<String>) -> El {
    text_el(s, Type::Body, Weight::Medium, false, Ink::Fg)
}

/// A value: mono, so it stays tabular.
pub fn value(s: impl Into<String>) -> El {
    text_el(s, Type::BodySmall, Weight::Normal, true, Ink::Dim)
}

pub fn caption(s: impl Into<String>) -> El {
    text_el(s, Type::Caption, Weight::Normal, false, Ink::Muted)
}

/// Body words that wrap to `lines` lines and then end in an ellipsis.
pub fn para(s: impl Into<String>, size: Type, weight: Weight, ink: Ink, lines: usize) -> El {
    El::new(Kind::Para { text: s.into(), font: Font { size, weight, mono: false }, ink, lines })
}

/// A bitmap (an app's icon) in a `size` square.
pub fn picture(image: Option<crate::scene::Bitmap>, size: f64) -> El {
    El::new(Kind::Picture { pic: Pic(image), size })
}

/// Tiles placed freely over `content` in a framed viewport `size` big,
/// `inset` inside its frame, scrolled to `scroll`.
pub fn strip(size: (f64, f64), inset: f64, content: (f64, f64), scroll: (f64, f64), tiles: Vec<super::el::Tile>) -> El {
    El::new(Kind::Strip { size, inset, content, scroll, tiles })
}

/// Album art in a `size` square.
pub fn cover(image: Option<crate::scene::Bitmap>, size: f64) -> El {
    El::new(Kind::Cover { pic: Pic(image), size })
}

/// The picture a cover of `size` asks for: inside its border, at the radius
/// that leaves concentric with the frame's.
pub fn cover_inner(theme: &fs_theme::theme::Theme, size: f64) -> (u32, f64) {
    let bw = theme.border_width;
    (((size - bw * 2.0).round().max(1.0)) as u32, (theme.cover_radius(size) - bw).max(0.0))
}

pub fn spectrum(style: impl Into<String>, columns: usize, live: bool) -> El {
    El::new(Kind::Spectrum { style: style.into(), columns, live })
}

pub fn icon(name: impl Into<String>) -> El {
    El::new(Kind::Icon { name: name.into(), size: Type::Body, ink: Ink::Fg })
}

/// Section label: caption, medium, muted, sentence case, an optional
/// trailing count. `inset` lines it up with flat rows' own text.
pub fn section_label(s: &Space, text: &str, count: Option<usize>, inset: bool) -> El {
    let text = match count {
        Some(n) => format!("{text} ({n})"),
        None => text.to_owned(),
    };
    let el = text_el(text, Type::Caption, Weight::Medium, false, Ink::Muted);
    if inset { el.pad_start(s.control_padding_x) } else { el }
}

/// A section label heading a group or naming a field.
pub fn heading(s: &Space, text: &str, count: Option<usize>, inset: bool) -> El {
    section_label(s, text, count, inset)
}

/// A named group of rows: the heading `rowGap` over them, the rows abutting.
pub fn section(s: &Space, title: &str, count: Option<usize>, rows: Vec<El>) -> El {
    column(s.row_gap, vec![heading(s, title, count, true), column(0.0, rows)])
}

/// A `Cell` around one child.
pub fn cell(child: El) -> El {
    El::new(Kind::Cell { state: CellState::default(), interactive: false, child: Box::new(child) })
}

/// The launcher's mode chip: the mode path in `caption` at 600 in
/// `launcher.token`, `System › Power`, the ancestors muted. Past two levels
/// it reads `… › ` and the last two. Each ancestor fires `token:<depth>`,
/// its index in `path`, the `…` the deepest one it hides.
pub fn token<S: AsRef<str>>(s: &Space, path: &[S]) -> El {
    let word = |w: &str, ink| text_el(w, Type::Caption, Weight::Semibold, false, ink);
    let mut parts = Vec::new();
    let skipped = path.len().saturating_sub(2);
    if skipped > 0 {
        parts.extend([word("\u{2026}", Ink::Muted).on(format!("token:{}", skipped - 1)), word("\u{203a}", Ink::Muted)]);
    }
    let shown = &path[skipped..];
    for (i, label) in shown.iter().enumerate() {
        if i + 1 < shown.len() {
            parts.extend([word(label.as_ref(), Ink::Muted).on(format!("token:{}", skipped + i)), word("\u{203a}", Ink::Muted)]);
        } else {
            parts.push(word(label.as_ref(), Ink::Fg));
        }
    }
    cell(row(s.xs, parts)).cell_state(|c| {
        c.chip = true;
        c.token = true;
    })
}

/// A list row: a ghost cell answering the pointer, holding an icon, a name
/// that elides and whatever trails it.
pub fn list_row(s: &Space, glyph: &str, name: &str, trailing: Vec<El>) -> El {
    let mut parts = Vec::new();
    if !glyph.is_empty() {
        parts.push(icon(glyph).ink(Ink::Fg));
    }
    parts.push(label(name).elide());
    parts.extend(trailing);
    cell(row(s.icon_gap, parts).fill()).ghost().interactive()
}

pub fn button(text: impl Into<String>) -> El {
    El::new(Kind::Button { variant: Variant::Default, text: text.into(), icon: String::new(), enabled: true, square: false })
}

pub fn icon_text_button(glyph: impl Into<String>, text: impl Into<String>) -> El {
    El::new(Kind::Button { variant: Variant::Default, text: text.into(), icon: glyph.into(), enabled: true, square: false })
}

/// A ghost button `controlHeight` square.
pub fn icon_button(glyph: impl Into<String>) -> El {
    El::new(Kind::Button { variant: Variant::Ghost, text: String::new(), icon: glyph.into(), enabled: true, square: true })
}

pub fn switch(checked: bool) -> El {
    El::new(Kind::Switch { checked, enabled: true })
}

pub fn track(value: f64) -> El {
    El::new(Kind::Track { value, notch: None, interactive: false })
}

/// A track the pointer sets and the wheel steps.
pub fn slider(value: f64) -> El {
    El::new(Kind::Track { value, notch: None, interactive: true })
}

/// A choice among several when `exclusive`, a set of
/// actions when not.
pub fn group(options: Vec<Opt>, index: usize, exclusive: bool) -> El {
    El::new(Kind::Group { options, index, exclusive, cursor_index: index, wrap: false })
}

pub fn segmented(options: Vec<String>, index: usize) -> El {
    El::new(Kind::Segmented { options, index })
}

pub fn input(text: &str, placeholder: &str, focused: bool, error: Option<&str>) -> El {
    El::new(Kind::Input { text: text.into(), placeholder: placeholder.into(), focused, error: error.map(str::to_owned), caret: None })
}

/// What a field shows: one dot per char of a secret one.
pub fn shown<S: Buffer>(f: &Field<S>) -> String {
    if f.secret { "\u{2022}".repeat(f.text().chars().count()) } else { f.text().to_owned() }
}

/// An input over a [`Field`], its caret and selection where the field has them.
pub fn field<S: Buffer>(f: &Field<S>, placeholder: &str, focused: bool, error: Option<&str>) -> El {
    input(&shown(f), placeholder, focused, error).caret(f.caret_chars())
}

/// A field's line with no frame, at `size`.
pub fn edit<S: Buffer>(f: &Field<S>, placeholder: &str, size: Type) -> El {
    El::new(Kind::Edit { text: shown(f), placeholder: placeholder.into(), caret: f.caret_chars(), size })
}

pub fn separator() -> El {
    El::new(Kind::Separator { vertical: false, inset: 0.0, bleed: 0.0 })
}

pub fn separator_inset(inset: f64) -> El {
    El::new(Kind::Separator { vertical: false, inset, bleed: 0.0 }).pad(inset, 0.0, inset, 0.0)
}

pub fn separator_vertical() -> El {
    El::new(Kind::Separator { vertical: true, inset: 0.0, bleed: 0.0 })
}

pub fn keycap(key: &str) -> El {
    El::new(Kind::Keycap { key: key.into() })
}

/// One cap per key, `xxs` apart.
pub fn chord(s: &Space, keys: &str) -> El {
    row(s.xxs, super::draw::split_chord(keys).iter().map(|k| keycap(k)).collect())
}

pub fn chord_keys(s: &Space, keys: &[&str]) -> El {
    row(s.xxs, keys.iter().map(|k| keycap(k)).collect())
}

pub fn swatch(color: Rgba, w: f64, h: f64, radius: f64) -> El {
    El::new(Kind::Swatch { color, w, h, radius, border: true })
}

/// An indicator dot: a filled circle with no border.
pub fn dot(color: Rgba, d: f64) -> El {
    El::new(Kind::Swatch { color, w: d, h: d, radius: d / 2.0, border: false })
}

/// A chip carrying an icon, what is picked and a chevron,
/// `selected` while its menu is open. The label elides at the width its
/// parent has left.
pub fn trigger(s: &Space, glyph: &str, label: &str, open: bool) -> El {
    let mut parts = Vec::new();
    if !glyph.is_empty() {
        parts.push(icon(glyph).size(Type::BodySmall));
    }
    parts.push(text_el(label, Type::BodySmall, Weight::Medium, false, Ink::Fg).elide().hug());
    parts.push(icon(if open { "chevron-up" } else { "chevron-down" }).size(Type::Caption).ink(Ink::Dim));
    cell(row(s.xs, parts))
        .cell_state(|c| {
            c.chip = true;
            c.small = true;
            c.selected = open;
        })
        .interactive()
}

pub fn ring(ring: super::el::Ring) -> El {
    El::new(Kind::Ring(ring))
}

pub fn sparkline(values: Vec<f64>, secondary: Vec<f64>, ceiling: f64, capacity: usize) -> El {
    El::new(Kind::Sparkline(Series { values, secondary, ceiling, capacity })).fill()
}

pub fn shoulders(edge: Edge, span: f64, depth: f64, run: f64) -> El {
    El::new(Kind::Shoulders { edge, span, depth, run })
}

pub fn flow(flow: Flow) -> El {
    El::new(Kind::Flow(Box::new(flow))).fill()
}

pub fn marquee(text: impl Into<String>, max: f64) -> El {
    El::new(Kind::Marquee { text: text.into(), ink: Ink::Fg, max })
}

pub fn matrix(rows: Vec<String>) -> El {
    El::new(Kind::Matrix { rows })
}

/// The parts of a panel hero.
pub struct Hero {
    pub glyph: String,
    pub title: String,
    pub meta: String,
    pub readout: String,
    pub trailing: Option<El>,
    /// The rail under it, 0 to 1, or none.
    pub rail: Option<f64>,
    pub rail_on: Option<String>,
}

/// Panel hero: a flat ghost cell leading the content column, a
/// `subtitle` title over a `bodySmall` meta, a `display` readout, a trailing
/// control, and an optional rail under all of it.
pub fn hero(s: &Space, h: Hero) -> El {
    hero_with(s, h, false, Type::Display, None)
}

/// `hero` with `metaMono`, `readoutSize` and a `leading` element standing in
/// for the glyph (a weather range, a pairing code, an app's picture).
pub fn hero_with(s: &Space, h: Hero, meta_mono: bool, readout_size: Type, leading: Option<El>) -> El {
    let mut top = Vec::new();
    if let Some(l) = leading {
        top.push(l.width(super::Size::Px(s.lg * 2.0)));
    } else if !h.glyph.is_empty() {
        top.push(El::new(Kind::Icon { name: h.glyph.clone(), size: Type::Heading, ink: Ink::Fg }).width(super::Size::Px(s.lg * 2.0)));
    }
    let mut words = vec![text_el(h.title, Type::Subtitle, Weight::Bold, false, Ink::Fg).elide()];
    if !h.meta.is_empty() {
        words.push(text_el(h.meta, Type::BodySmall, Weight::Normal, meta_mono, Ink::Dim).elide());
    }
    top.push(column(s.xxs, words).fill());
    if !h.readout.is_empty() {
        top.push(text_el(h.readout, readout_size, Weight::Normal, true, Ink::Fg));
    }
    if let Some(t) = h.trailing {
        top.push(t);
    }
    let mut parts = vec![row(s.icon_gap, top).fill()];
    if let Some(v) = h.rail {
        let rail = match h.rail_on {
            Some(on) => slider(v).on(on),
            None => track(v),
        };
        parts.push(rail);
    }
    cell(column(s.xxs, parts).fill()).ghost().cell_state(|c| c.hero = true)
}
