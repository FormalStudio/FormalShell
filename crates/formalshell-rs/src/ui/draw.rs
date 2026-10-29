//! Measuring and painting each element. A container lays its children out
//! with the same arithmetic it measured them by, so a paint never disagrees
//! with the size it reported.

use fs_theme::color::Rgba;
use fs_theme::style::{BoxStyle, Line};
use fs_theme::tokens::WEIGHTS;
use vello_cpu::kurbo::{Affine, BezPath, Rect};

use super::el::{CellState, Font, Kind, Size, Type, Variant, Weight};
use super::{Cx, El, Hit, HitWhat, Ink, Stop, boxes, irect};
use crate::motion::Kind as Clock;
use crate::scene::{IRect, Paint, Scene};
use crate::text::{self, Family, ShapedText, TextStyle};

mod flow;

fn px(cx: &Cx, t: Type) -> f32 {
    let f = &cx.theme.font_size;
    (match t {
        Type::Caption => f.caption,
        Type::BodySmall => f.body_small,
        Type::Body => f.body,
        Type::Subtitle => f.subtitle,
        Type::Title => f.title,
        Type::Heading => f.heading,
        Type::Display => f.display,
        Type::DisplayLarge => f.display_large,
    }) as f32
}

fn weight(w: Weight) -> f32 {
    (match w {
        Weight::Normal => WEIGHTS.normal,
        Weight::Medium => WEIGHTS.medium,
        Weight::Semibold => WEIGHTS.semibold,
    }) as f32
}

pub fn style(cx: &Cx, font: &Font) -> TextStyle {
    let family = if font.mono { cx.kit.look.mono } else { cx.kit.look.sans };
    TextStyle { family, size: px(cx, font.size), weight: weight(font.weight), tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
}

fn shape(cx: &mut Cx, s: &str, font: &Font) -> ShapedText {
    let st = style(cx, font);
    cx.kit.shape(s, st)
}

fn icon(cx: &mut Cx, name: &str, size: Type) -> ShapedText {
    let g = fs_theme::icons::glyph(&cx.kit.look.icon_set, name);
    let st = TextStyle { family: Family::Named(g.family), size: px(cx, size), weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
    cx.kit.shape(g.text, st)
}

/// The text cut to `max` with an ellipsis, or whole.
pub(super) fn elided(cx: &mut Cx, s: &str, font: &Font, max: f64) -> ShapedText {
    let st = style(cx, font);
    elided_in(cx, s, st, max)
}

fn elided_in(cx: &mut Cx, s: &str, st: TextStyle, max: f64) -> ShapedText {
    let whole = cx.kit.shape(s, st);
    if whole.width as f64 <= max + 0.5 || s.is_empty() {
        return whole;
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let t: String = chars[..mid].iter().collect::<String>().trim_end().to_owned() + "\u{2026}";
        if cx.kit.shape(&t, st).width as f64 <= max { lo = mid } else { hi = mid - 1 }
    }
    let t: String = chars[..lo].iter().collect::<String>().trim_end().to_owned() + "\u{2026}";
    cx.kit.shape(&t, st)
}

/// A word label's style part way to the table's focused axes, quantised so
/// a 200ms widen shapes a handful of styles rather than one a frame.
fn focused(cx: &Cx, font: &Font) -> TextStyle {
    let st = style(cx, font);
    if cx.focus <= 0.0 || font.mono {
        return st;
    }
    let Some(f) = cx.theme.motion().catalogue.focus_text else { return st };
    let k = f64::from(cx.focus);
    let wdth = (100.0 + (f.wdth - 100.0) * k).round() as f32;
    let wght = ((f64::from(st.weight) + (f.wght - f64::from(st.weight)) * k) / 10.0).round() as f32 * 10.0;
    let tracking = ((f64::from(st.size) * f.tracking * k) * 4.0).round() as f32 / 4.0;
    TextStyle { wdth, wght: Some(wght), tracking, ..st }
}

fn token(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish() | 1
}

/// A used row's ghost over its label: the same line in `primary`, scaled
/// about its leading edge's middle as it fades.
fn ghost(cx: &mut Cx, t: &ShapedText, at: (i32, i32), p: f64, path: &str) {
    let Some(u) = cx.theme.motion().catalogue.use_ else { return };
    let (sx, sy, a) = crate::effects::ghost(&u, p);
    let ink = cx.theme.colors.get("primary");
    let ink = cx.a(ink.with_alpha(ink.a * a));
    let (w, h) = t.box_size();
    let pivot = (f64::from(at.0), f64::from(at.1) + f64::from(t.line_height()) / 2.0);
    let full = Affine::translate(pivot)
        * Affine::scale_non_uniform(sx, sy)
        * Affine::translate((-pivot.0, -pivot.1))
        * Affine::translate((f64::from(at.0 - text::PAD), f64::from(at.1 - text::PAD)));
    let bounds = Scene::cover(Rect::new(0.0, 0.0, f64::from(w), f64::from(h)), full, 1.0);
    let transform = full * Affine::translate((-f64::from(bounds.x), -f64::from(bounds.y)));
    let mut p = cx.painter(&format!("{path}.ghost"));
    let clip = p.clip;
    p.text_in(t, bounds, transform, clip, ink, &[]);
    let last = p.last();
    p.finish();
    cx.done(last);
}

/// A stop's rise on open: how far under its place it sits and its opacity,
/// none once it has landed or on a table whose rows do not rise.
fn rise(cx: &mut Cx, el: &El) -> Option<(f64, f32)> {
    if !el.stop {
        return None;
    }
    let at = cx.arrive()?;
    let rows = cx.theme.motion().catalogue.rows?;
    let elapsed = crate::motion::since(at, cx.now) / cx.motion_scale.max(1e-6);
    let index = cx.stops_len();
    if crate::effects::rising(&rows, elapsed, index + 1) {
        cx.animate();
    }
    let (dy, a) = crate::effects::rise(&rows, elapsed, index);
    (dy > 0.0 || a < 1.0).then_some((dy, a))
}

/// `text` broken on spaces to lines no wider than `max`, newlines kept, the
/// last line that fits `lines` ending in an ellipsis when words are left.
fn wrapped(cx: &mut Cx, text: &str, font: &Font, max: f64, lines: usize) -> Vec<ShapedText> {
    let st = style(cx, font);
    let key = super::WrapKey { text: text.to_owned(), style: format!("{st:?}"), max: max.to_bits(), lines };
    let frame = cx.ui.frame;
    if let Some(w) = cx.ui.wraps.get_mut(&key) {
        w.seen = frame;
        if let Some(seen) = &mut cx.kit.seen
            && !seen.contains(&st)
        {
            seen.push(st);
        }
        return w.lines.clone();
    }
    let lines = wrap_now(cx, text, font, max, lines);
    cx.ui.wraps.insert(key, super::Wrapped { lines: lines.clone(), seen: frame });
    lines
}

fn wrap_now(cx: &mut Cx, text: &str, font: &Font, max: f64, lines: usize) -> Vec<ShapedText> {
    let mut out: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let next = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if line.is_empty() || shape(cx, &next, font).width as f64 <= max {
                line = next;
            } else {
                out.push(std::mem::replace(&mut line, word.to_owned()));
            }
        }
        out.push(line);
    }
    let cut = out.len() > lines;
    out.truncate(lines.max(1));
    let last = out.len() - 1;
    out.iter()
        .enumerate()
        .map(|(i, l)| if i == last && cut { elided(cx, &format!("{l}\u{2026}"), font, max) } else { elided(cx, l, font, max) })
        .collect()
}

fn resolve(cx: &Cx, ink: Ink) -> Rgba {
    match ink {
        Ink::Fg => cx.ink.0,
        Ink::Dim => cx.ink.1,
        Ink::Muted => cx.theme.colors.get("mutedForeground"),
        Ink::Primary => cx.theme.colors.get("primary"),
        Ink::Destructive => cx.theme.colors.get("destructive"),
        Ink::Color(c) => c,
    }
}

fn across(cx: &Cx, chip: bool, content: f64) -> f64 {
    let s = &cx.theme.space;
    (if chip { 0.0 } else { s.control_height }).max(content + s.control_padding_y * 2.0)
}

fn button_metrics(cx: &mut Cx, text: &str, icon_name: &str, pad_x: f64) -> (f64, Option<ShapedText>, Option<ShapedText>) {
    let s = cx.theme.space.clone();
    let label = (!text.is_empty()).then(|| shape(cx, text, &Font { size: Type::Body, weight: Weight::Medium, mono: false }));
    let glyph = (!icon_name.is_empty()).then(|| icon(cx, icon_name, Type::Body));
    let mut w = label.as_ref().map_or(0.0, |l| l.width as f64);
    if glyph.is_some() {
        w += cx.theme.font_size.body + if label.is_some() { s.icon_gap } else { 0.0 };
    }
    (w + pad_x * 2.0, label, glyph)
}

fn width_of(el: &El, avail: f64, natural: f64) -> f64 {
    match el.width {
        Size::Fill => avail,
        Size::Px(p) => p,
        Size::Hug => natural.min(avail.max(natural)),
    }
}

/// Child widths along a row: fixed and hugging children first, the rest
/// shared among the filling ones.
fn row_widths(cx: &mut Cx, children: &[El], inner: f64, gap: f64) -> Vec<f64> {
    let gaps = gap * children.len().saturating_sub(1) as f64;
    let mut out = vec![0.0; children.len()];
    let mut used = gaps;
    let mut fills = 0;
    for (i, c) in children.iter().enumerate() {
        match c.width {
            Size::Fill => fills += 1,
            _ => {
                let w = if let Kind::Space { along } = c.kind { along } else { measure(cx, c, inner).0 };
                out[i] = w;
                used += w;
            }
        }
    }
    let share = if fills > 0 { ((inner - used) / fills as f64).max(0.0) } else { 0.0 };
    for (i, c) in children.iter().enumerate() {
        if c.width == Size::Fill {
            out[i] = share;
        }
    }
    out
}

/// (width, height) at `avail` wide.
pub fn measure(cx: &mut Cx, el: &El, avail: f64) -> (f64, f64) {
    // Containers only: they live as long as the tree being drawn, where a
    // separator painted from a temporary could reuse a freed address.
    if !matches!(el.kind, Kind::Column { .. } | Kind::Row { .. } | Kind::Grid { .. }) {
        return measure_now(cx, el, avail);
    }
    let key = (el as *const El as usize, avail.to_bits());
    if let Some(&m) = cx.ui.measured.get(&key) {
        return m;
    }
    let m = measure_now(cx, el, avail);
    cx.ui.measured.insert(key, m);
    m
}

fn measure_now(cx: &mut Cx, el: &El, avail: f64) -> (f64, f64) {
    let s = cx.theme.space.clone();
    let [ps, pt, pe, pb] = el.pad;
    let inner = (avail - ps - pe).max(0.0);
    let (nw, nh) = match &el.kind {
        Kind::Column { gap, children } => {
            let mut w: f64 = 0.0;
            let mut h = 0.0;
            for (i, c) in children.iter().enumerate() {
                let (cw, ch) = if let Kind::Space { along } = c.kind { (0.0, along) } else { measure(cx, c, inner) };
                w = w.max(cw);
                h += ch + if i > 0 { *gap } else { 0.0 };
            }
            (w, h)
        }
        Kind::Row { gap, children } => {
            let widths = row_widths(cx, children, inner, *gap);
            let mut h: f64 = 0.0;
            let mut natural = gap * children.len().saturating_sub(1) as f64;
            for (c, w) in children.iter().zip(&widths) {
                if let Kind::Space { along } = c.kind {
                    natural += along;
                    continue;
                }
                let (cw, ch) = measure(cx, c, *w);
                natural += if c.width == Size::Fill { cw.min(*w) } else { *w };
                h = h.max(ch);
            }
            (natural, h)
        }
        Kind::Grid { columns, gap, children } => {
            let n = (*columns).max(1);
            let cw = ((inner - gap * (n - 1) as f64) / n as f64).max(0.0);
            let mut h = 0.0;
            for (r, line) in children.chunks(n).enumerate() {
                let lh = line.iter().map(|c| measure(cx, c, cw).1).fold(0.0, f64::max);
                h += lh + if r > 0 { *gap } else { 0.0 };
            }
            (inner, h)
        }
        Kind::Space { along } => (*along, *along),
        Kind::Text { text, .. } if el.tag && console(cx) => {
            let st = tag_style(cx);
            let t = cx.kit.shape(&text.to_uppercase(), st);
            (t.width as f64 + TAG_LEAD + cx.theme.metric("panel.tag", "trail"), t.line_height() as f64)
        }
        Kind::Text { text, font, elide, .. } => {
            let t = shape(cx, text, font);
            let mut w = t.width as f64;
            if let Some(g) = &el.gauge {
                w = w.max(shape(cx, g, font).width as f64);
            }
            (if *elide { w.min(inner) } else { w }, t.line_height() as f64)
        }
        Kind::Para { text, font, lines, .. } => {
            let ls = wrapped(cx, text, font, inner, *lines);
            (inner, ls.iter().map(|l| l.line_height() as f64).sum())
        }
        Kind::Picture { size, .. } | Kind::Cover { size, .. } => (*size, *size),
        Kind::Strip { size, .. } => *size,
        Kind::Lyrics(v) => (inner, v.height),
        Kind::Spectrum { columns, .. } => {
            let n = *columns as f64;
            (n * s.track_thickness + (n - 1.0).max(0.0) * s.xxs, s.control_height)
        }
        Kind::Icon { name, size, .. } => {
            let t = icon(cx, name, *size);
            (px(cx, *size) as f64, t.line_height() as f64)
        }
        Kind::Marquee { text, max, .. } => {
            let t = shape(cx, text, &Font { size: Type::Body, weight: Weight::Normal, mono: false });
            ((t.width as f64).min(*max), t.line_height() as f64)
        }
        Kind::Cell { state, child, .. } => {
            let content_avail = (inner - s.control_padding_x * 2.0).max(0.0);
            let (cw, ch) = measure(cx, child, content_avail);
            (cw + s.control_padding_x * 2.0, across(cx, state.chip, ch) + tab_height(cx, el, *state))
        }
        Kind::Button { text, icon, square, .. } => {
            if *square {
                (s.control_height, s.control_height)
            } else {
                (button_metrics(cx, text, icon, s.control_padding_x).0, s.control_height)
            }
        }
        Kind::Switch { .. } => (s.control_height, s.control_height),
        Kind::Track { .. } => (inner, s.track_thickness),
        Kind::Group { options, wrap, .. } => {
            let widest = group_natural(cx, options);
            let rows = if *wrap { group_grid(widest, options.len(), inner, s.xs).0 } else { 1 };
            (widest * options.len() as f64 + s.xs * (options.len() + 1) as f64, s.control_height * rows as f64 + s.xs * (rows + 1) as f64)
        }
        Kind::Segmented { options, .. } => {
            let seg = segment_width(cx, options);
            (seg * options.len() as f64 + s.xxs * 2.0, s.control_height)
        }
        Kind::Edit { size, .. } => (inner, shape(cx, "Ag", &Font { size: *size, weight: Weight::Normal, mono: false }).line_height() as f64),
        Kind::Input { error, .. } => {
            let extra = match error {
                Some(e) if !e.is_empty() => s.xs + shape(cx, e, &Font { size: Type::Caption, weight: Weight::Normal, mono: false }).line_height() as f64,
                _ => 0.0,
            };
            (inner, s.control_height + extra)
        }
        Kind::Separator { vertical, .. } => {
            let line = cx.theme.border_width + edge_width(cx);
            if *vertical { (line, s.control_height) } else { (inner, line) }
        }
        Kind::Keycap { key } => {
            let (text, glyph) = keycap(key);
            let w = if glyph.is_empty() {
                shape(cx, &text, &Font { size: Type::Caption, weight: Weight::Medium, mono: true }).width as f64
            } else {
                px(cx, Type::Caption) as f64
            };
            (s.keycap_height.max(w + s.sm * 2.0), s.keycap_height)
        }
        Kind::Swatch { w, h, .. } => (*w, *h),
        Kind::Sparkline(_) => (inner, s.control_height),
        Kind::Matrix { rows } => {
            let side = matrix_module(inner, rows.len()) * rows.len() as f64;
            (inner, side)
        }
        Kind::Flow(f) => (inner, flow::metrics(cx, f, inner).total),
        Kind::Shoulders { edge, span, depth, run } => {
            if edge.is_vertical() { (*depth, span + run * 2.0) } else { (span + run * 2.0, *depth) }
        }
    };
    let w = match el.kind {
        Kind::Grid { .. } | Kind::Track { .. } | Kind::Input { .. } | Kind::Edit { .. } | Kind::Sparkline(_) | Kind::Matrix { .. } | Kind::Flow(_) | Kind::Lyrics(_) => avail,
        _ => width_of(el, avail, nw + ps + pe),
    };
    (w, el.height.unwrap_or(nh) + pt + pb)
}

fn edge_width(cx: &Cx) -> f64 {
    cx.theme.box_style("separator", None).edge.map_or(0.0, |l| l.width)
}

fn segment_width(cx: &mut Cx, options: &[String]) -> f64 {
    let pad = cx.theme.space.control_padding_x;
    let widest = options
        .iter()
        .map(|o| shape(cx, o, &Font { size: Type::Body, weight: Weight::Medium, mono: false }).width as f64)
        .fold(0.0, f64::max);
    widest.ceil() + pad * 2.0
}

/// Each segment's offset and width across `width`: equal while the widest
/// label fits an equal share, otherwise each as wide as its own label with
/// the slack shared out, so a long label does not push the row past its
/// parent.
fn segment_spans(cx: &mut Cx, options: &[String], width: f64) -> Vec<(f64, f64)> {
    let n = options.len().max(1) as f64;
    let equal = width / n;
    if equal >= segment_width(cx, options) {
        return (0..options.len()).map(|i| (equal * i as f64, equal)).collect();
    }
    let pad = cx.theme.space.control_padding_x;
    let own: Vec<f64> = options
        .iter()
        .map(|o| shape(cx, o, &Font { size: Type::Body, weight: Weight::Medium, mono: false }).width as f64 + pad * 2.0)
        .collect();
    let slack = ((width - own.iter().sum::<f64>()) / n).max(0.0);
    let mut x = 0.0;
    own.iter()
        .map(|w| {
            let span = (x, w + slack);
            x += w + slack;
            span
        })
        .collect()
}

/// keys.js `cap`: the text on the cap, or the icon an arrow takes.
pub fn keycap(key: &str) -> (String, &'static str) {
    let raw = key.trim();
    let low = raw.to_lowercase();
    let icon = match low.as_str() {
        "up" => "arrow-up",
        "down" => "arrow-down",
        "left" => "arrow-left",
        "right" => "arrow-right",
        _ => "",
    };
    if !icon.is_empty() {
        return (String::new(), icon);
    }
    let name = match low.as_str() {
        "super" | "mod4" => "Super",
        "alt" | "mod1" => "Alt",
        "ctrl" | "control" => "Ctrl",
        "shift" => "Shift",
        "enter" | "return" => "Enter",
        "escape" | "esc" => "Esc",
        "tab" => "Tab",
        "space" => "Space",
        "backspace" => "Backspace",
        "delete" | "del" => "Del",
        "pageup" => "PgUp",
        "pagedown" => "PgDn",
        "home" => "Home",
        "end" => "End",
        _ => "",
    };
    if !name.is_empty() {
        return (name.into(), "");
    }
    if raw.chars().count() == 1 {
        return (raw.to_uppercase(), "");
    }
    (raw.into(), "")
}

/// keys.js `split`: a chord as the keys it presses.
pub fn split_chord(chord: &str) -> Vec<String> {
    let s = chord.trim();
    if s.is_empty() {
        return Vec::new();
    }
    if s == "+" {
        return vec!["+".into()];
    }
    let parts: Vec<&str> = s.split('+').collect();
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let p = p.trim();
        if !p.is_empty() {
            out.push(p.to_owned());
        } else if i == parts.len() - 1 && i > 0 {
            out.push("+".into());
        }
    }
    out
}

fn child_path(path: &str, el: &El, i: usize) -> String {
    match &el.key {
        Some(k) => format!("{path}/{k}"),
        None => format!("{path}/{i}"),
    }
}

/// Paints `el` into `rect` (its full measured box, padding included).
pub fn paint(cx: &mut Cx, el: &El, rect: Rect, path: &str) {
    paint_node(cx, el, rect, path);
    if let Some(list) = &el.float {
        cx.float(rect, (**list).clone(), format!("{path}/float"));
    }
}

fn paint_node(cx: &mut Cx, el: &El, rect: Rect, path: &str) {
    let pulse = el.pulse && cx.theme.motion_enabled;
    let rise = rise(cx, el);
    if !pulse && rise.is_none() && el.swap.is_none() && el.badge.is_none() && el.fade.is_none() {
        return paint_body(cx, el, rect, path);
    }
    let saved = cx.alpha;
    let mut rect = rect;
    if let Some((dy, a)) = rise {
        cx.alpha *= a;
        rect = Rect::new(rect.x0, rect.y0 + dy, rect.x1, rect.y1 + dy);
    }
    if let Some(f) = el.fade {
        cx.alpha *= f.clamp(0.0, 1.0);
    }
    if pulse {
        let born = cx.born(path);
        cx.animate();
        cx.alpha *= crate::motion::breathe(born, cx.now);
    }
    if let Some(token) = el.swap {
        let (progress, sign) = cx.swap(path, token);
        let dx = (1.0 - progress) * sign * cx.theme.space.lg;
        cx.alpha *= progress as f32;
        rect = Rect::new(rect.x0 + dx, rect.y0, rect.x1 + dx, rect.y1);
    }
    paint_body(cx, el, rect, path);
    if let Some(name) = el.badge {
        badge(cx, rect, name, path);
    }
    cx.alpha = saved;
}

/// The source mark riding a slot's corner: a caption-sized disc in the
/// background colour keeping the glyph readable over the picture under it,
/// reaching `xxs` past the slot.
fn badge(cx: &mut Cx, rect: Rect, name: &str, path: &str) {
    let d = cx.theme.font_size.caption;
    let xxs = cx.theme.space.xxs;
    let disc = IRect::new((rect.x1 + xxs - d).round() as i32, (rect.y1 + xxs - d).round() as i32, d.round() as i32, d.round() as i32);
    let glyph = icon(cx, name, Type::Caption);
    let fill = cx.a(cx.theme.colors.get("background"));
    let ink = cx.a(cx.theme.colors.get("mutedForeground"));
    let at = (disc.x + (disc.w - glyph.width) / 2, disc.y + (disc.h - glyph.line_height()) / 2);
    let mut p = cx.painter(&format!("{path}/badge"));
    p.rect(disc, fill, disc.w as f32 / 2.0);
    p.text(&glyph, at, ink, &[]);
    let last = p.last();
    p.finish();
    cx.done(last);
}

fn paint_body(cx: &mut Cx, el: &El, rect: Rect, path: &str) {
    let [ps, pt, pe, pb] = el.pad;
    let inner = Rect::new(rect.x0 + ps, rect.y0 + pt, rect.x1 - pe, rect.y1 - pb);
    if cx.clip.is_some_and(|c| !c.intersects(&irect(rect).union(&IRect::new(irect(rect).x - 4, irect(rect).y - 4, irect(rect).w + 8, irect(rect).h + 8)))) {
        // Wholly outside the clip: its nodes go dark, and nothing is hit.
        return;
    }
    if el.stop && let Some(key) = &el.key {
        let radius = stop_radius(cx, el, inner.height());
        cx.stop(Stop { key: key.clone(), rect: irect(inner), radius });
    }
    let stop_key = el.stop.then(|| el.key.clone()).flatten();
    match &el.kind {
        Kind::Column { gap, children } => {
            let mut y = inner.y0;
            for (i, c) in children.iter().enumerate() {
                if i > 0 {
                    y += gap;
                }
                if let Kind::Space { along } = c.kind {
                    y += along;
                    continue;
                }
                let (cw, ch) = measure(cx, c, inner.width());
                let x = if c.centred { inner.x0 + (inner.width() - cw) / 2.0 } else { inner.x0 };
                paint(cx, c, Rect::new(x, y, x + cw, y + ch), &child_path(path, c, i));
                y += ch;
            }
        }
        Kind::Row { gap, children } => {
            let widths = row_widths(cx, children, inner.width(), *gap);
            let mut x = inner.x0;
            for (i, (c, w)) in children.iter().zip(widths).enumerate() {
                if let Kind::Space { .. } = c.kind {
                    x += w + gap;
                    continue;
                }
                let (cw, ch) = measure(cx, c, w);
                let cw = if c.width == Size::Fill { w } else { cw };
                // The lyrics pane stands as tall as the column beside it.
                let ch = if matches!(c.kind, Kind::Lyrics(_)) { ch.max(inner.height()) } else { ch };
                let y = if el.top { inner.y0 } else { inner.y0 + (inner.height() - ch) / 2.0 };
                paint(cx, c, Rect::new(x, y, x + cw, y + ch), &child_path(path, c, i));
                x += w + gap;
            }
        }
        Kind::Grid { columns, gap, children } => {
            let n = (*columns).max(1);
            let cw = ((inner.width() - gap * (n - 1) as f64) / n as f64).max(0.0);
            let mut y = inner.y0;
            for (r, line) in children.chunks(n).enumerate() {
                let lh = line.iter().map(|c| measure(cx, c, cw).1).fold(0.0, f64::max);
                for (k, c) in line.iter().enumerate() {
                    let x = inner.x0 + (cw + gap) * k as f64;
                    let (w, h) = measure(cx, c, cw);
                    paint(cx, c, Rect::new(x, y, x + w, y + h), &child_path(path, c, r * n + k));
                }
                y += lh + gap;
            }
        }
        Kind::Space { .. } => {}
        Kind::Text { text, .. } if el.tag && console(cx) => tag(cx, inner, text, path),
        Kind::Text { text, font, ink, elide } => {
            let st = focused(cx, font);
            let shown = match cx.theme.motion().catalogue.reveal.filter(|_| el.typed) {
                Some(ms) => {
                    let at = cx.changed(&format!("{path}.typed"), token(text));
                    let p = cx.effect(at, ms);
                    crate::effects::prefix(text, crate::effects::typed(text.chars().count(), p))
                }
                None => text.as_str(),
            };
            let t = if *elide { elided_in(cx, shown, st, inner.width()) } else { cx.kit.shape(shown, st) };
            // A line typing in is placed by its whole width, so it never reflows.
            let whole = if shown.len() < text.len() { cx.kit.shape(text, st).width.min(inner.width() as i32) } else { t.width };
            let color = resolve(cx, *ink);
            let color = cx.color(&format!("{path}.ink"), color);
            let color = cx.a(color);
            let y = (inner.y0 + (inner.height() - t.line_height() as f64) / 2.0).round() as i32;
            let x = if el.mid { inner.x0 + (inner.width() - whole as f64) / 2.0 } else { inner.x0 };
            let mut p = cx.painter(path);
            p.text(&t, (x.round() as i32, y), color, &[]);
            let last = p.last();
            p.finish();
            cx.done(last);
            // Only the cell's own label widens, never a trailing word.
            if !font.mono {
                cx.focus = 0.0;
            }
            if let Some(g) = cx.ghost.take() {
                ghost(cx, &t, (x.round() as i32, y), g, path);
            }
        }
        Kind::Para { text, font, ink, lines } => {
            let ls = wrapped(cx, text, font, inner.width(), *lines);
            let color = resolve(cx, *ink);
            let color = cx.color(&format!("{path}.ink"), color);
            let color = cx.a(color);
            let mut y = inner.y0.round() as i32;
            let mut p = cx.painter(path);
            for l in &ls {
                p.text(l, (inner.x0.round() as i32, y), color, &[]);
                y += l.line_height();
            }
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Strip { inset, scroll, tiles, .. } => super::strip::paint(cx, el, inner, *inset, *scroll, tiles, path),
        Kind::Picture { pic, .. } => {
            let alpha = cx.alpha;
            let r = irect(inner);
            let landing = if el.lands { landing(cx, pic, path) } else { None };
            let mut p = cx.painter(path);
            match &pic.0 {
                Some(image) if el.hex && landing.is_none() => {
                    let (w, h) = image.size();
                    p.shape(IRect::new(r.x + (r.w - w) / 2, r.y + (r.h - h) / 2, w, h), Paint::Cut { image: image.clone(), alpha, cut: crate::scene::Cut::Hex });
                }
                Some(image) => land(&mut p, image, r, alpha, landing),
                None => {}
            }
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Cover { pic, size } => {
            let t = cx.theme;
            let radius = t.cover_radius(*size) as f32;
            let bw = t.border_width;
            let (well, line) = (cx.a(t.colors.get("muted")), cx.a(t.colors.get("border")));
            let alpha = cx.alpha;
            let r = irect(inner);
            let landing = landing(cx, pic, path);
            let mut p = cx.painter(path);
            p.framed(r, well, radius, Rgba::TRANSPARENT, 0.0);
            if let Some(image) = &pic.0 {
                land(&mut p, image, r, alpha, landing);
            }
            p.framed(r, Rgba::TRANSPARENT, radius, line, bw as f32);
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Spectrum { style, columns, live } => super::spectrum::paint(cx, inner, style, *columns, *live, path),
        Kind::Lyrics(v) => super::lyrics::paint(cx, inner, v, path),
        Kind::Icon { name, size, ink } => {
            let t = icon(cx, name, *size);
            let color = cx.a(resolve(cx, *ink));
            let x = (inner.x0 + (inner.width() - t.width as f64) / 2.0).round() as i32;
            let y = (inner.y0 + (inner.height() - t.line_height() as f64) / 2.0).round() as i32;
            let mut p = cx.painter(path);
            p.text(&t, (x, y), color, &[]);
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Marquee { text, ink, .. } => marquee(cx, inner, text, *ink, path),
        Kind::Cell { state, interactive, child } => cell(cx, el, inner, *state, *interactive, child, path, stop_key),
        Kind::Button { variant, text, icon, enabled, square } => {
            button(cx, el, inner, *variant, text, icon, *enabled, *square, path, None, stop_key, None)
        }
        Kind::Switch { checked, enabled } => switch(cx, el, inner, *checked, *enabled, path, stop_key),
        Kind::Track { value, notch, interactive } => track(cx, el, inner, *value, *notch, *interactive, path, stop_key),
        Kind::Group { options, index, exclusive, cursor_index, wrap } => {
            group(cx, el, inner, options, *index, *exclusive, *cursor_index, *wrap, path, stop_key)
        }
        Kind::Segmented { options, index } => segmented(cx, el, inner, options, *index, path, stop_key),
        Kind::Input { text, placeholder, focused, error, caret } => {
            let end = text.chars().count();
            let caret = focused.then(|| caret.unwrap_or((end, end)));
            input(cx, inner, text, placeholder, caret, error.as_deref(), path)
        }
        Kind::Edit { text, placeholder, caret, size } => {
            let font = Font { size: *size, weight: Weight::Normal, mono: false };
            let room = irect(inner);
            let line = line_parts(cx, text, placeholder, Some(*caret), &font, room.w);
            let ty = room.y + (room.h - line.shown.line_height()) / 2;
            let t = cx.theme;
            let alpha = cx.alpha;
            let mut p = cx.painter(path);
            paint_line(&mut p, t, &line, (room.x, ty), room, alpha);
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Separator { vertical, bleed, .. } => {
            let b = cx.theme.box_style("separator", None);
            let bw = cx.theme.border_width;
            let fill = cx.a(b.fill);
            let edge = b.edge.clone().map(|l| (cx.a(l.color), l.width));
            let r = if *vertical {
                IRect::new(inner.x0.round() as i32, inner.y0.round() as i32, bw as i32, inner.height().round() as i32)
            } else {
                IRect::new((inner.x0 - bleed).round() as i32, inner.y0.round() as i32, (inner.width() + bleed * 2.0).round() as i32, bw as i32)
            };
            let mut p = cx.painter(path);
            p.rect(r, fill, 0.0);
            if let Some((c, w)) = edge {
                let w = w.round() as i32;
                let e = if *vertical { IRect::new(r.right(), r.y, w, r.h) } else { IRect::new(r.x, r.bottom(), r.w, w) };
                p.rect(e, c, 0.0);
            }
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Keycap { key } => {
            let b = cx.theme.box_style("keycap", None);
            let ink = if b.ink.a > 0.0 { b.ink } else { cx.theme.colors.get("mutedForeground") };
            let ink = cx.a(ink);
            let (text, glyph) = keycap(key);
            let t = if glyph.is_empty() {
                shape(cx, &text, &Font { size: Type::Caption, weight: Weight::Medium, mono: true })
            } else {
                icon(cx, glyph, Type::Caption)
            };
            let r = irect(inner);
            let radius = cx.theme.box_radius(&b, r.h as f64);
            let alpha = cx.alpha;
            let x = r.x + (r.w - t.width) / 2;
            let y = r.y + (r.h - t.line_height()) / 2;
            let mut p = cx.painter(path);
            boxes::paint(&mut p, r, &b, radius, alpha, 0.0);
            p.text(&t, (x, y), ink, &[]);
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Swatch { color, radius, border, .. } => {
            let line = cx.theme.colors.get("border");
            let bw = if *border { cx.theme.border_width as f32 } else { 0.0 };
            let (fill, line) = (cx.a(*color), cx.a(line));
            let mut p = cx.painter(path);
            p.framed(irect(inner), fill, *radius as f32, line, bw);
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        Kind::Sparkline(series) => sparkline(cx, inner, series, path),
        Kind::Matrix { rows } => matrix(cx, inner, rows, path),
        Kind::Flow(f) => flow::paint(cx, inner, f, path),
        Kind::Shoulders { edge, span, depth, run } => shoulders(cx, inner, *edge, *span, *depth, *run, path),
    }
    if let Some(on) = &el.on
        && !matches!(el.kind, Kind::Cell { .. } | Kind::Button { .. } | Kind::Switch { .. } | Kind::Track { .. } | Kind::Group { .. } | Kind::Segmented { .. } | Kind::Strip { .. })
    {
        cx.hit(Hit { rect: irect(inner), path: path.into(), on: Some(on.clone()), tip: el.tip.clone(), stop: None, what: HitWhat::Click });
    } else if el.on.is_none() && el.tip.is_some() && !matches!(el.kind, Kind::Cell { .. } | Kind::Button { .. }) {
        cx.hit(Hit { rect: irect(inner), path: path.into(), on: None, tip: el.tip.clone(), stop: None, what: HitWhat::Hover });
    }
}

/// Where a picture is in its landing (`picture: zoom`): none once it rests or
/// on a table that does not zoom, `Some(None)` while it is not shown yet.
fn landing(cx: &mut Cx, pic: &super::el::Pic, path: &str) -> Option<Option<(f64, f32, f32)>> {
    let ms = cx.theme.motion().catalogue.picture?;
    let image = pic.0.as_ref()?;
    let at = cx.changed(&format!("{path}.zoom"), std::sync::Arc::as_ptr(&image.pixmap) as usize as u64);
    let p = cx.effect(at, ms);
    (p < 1.0).then(|| crate::effects::zoom(p))
}

/// A picture centred in `r`, or part way through its landing: stretched
/// about the centre, cut to `r`, under black for the brightness it lacks.
fn land(p: &mut crate::surfaces::bar::cell::Painter, image: &crate::scene::Bitmap, r: IRect, alpha: f32, landing: Option<Option<(f64, f32, f32)>>) {
    let (w, h) = image.size();
    match landing {
        None => p.image(image, (r.x + (r.w - w) / 2, r.y + (r.h - h) / 2), alpha),
        Some(None) => {}
        Some(Some((scale, bright, a))) => {
            let (sw, sh) = ((f64::from(w) * scale).round() as i32, (f64::from(h) * scale).round() as i32);
            let at = IRect::new(r.x + (r.w - sw) / 2, r.y + (r.h - sh) / 2, sw, sh);
            p.shape_in(at, Paint::Picture { image: image.clone(), alpha: alpha * a, radius: 0.0 }, r);
            p.rect(at.intersect(&r), Rgba { r: 0.0, g: 0.0, b: 0.0, a: (1.0 - bright) * alpha * a }, 0.0);
        }
    }
}

/// Whether the table's panels take the console grammar.
fn console(cx: &Cx) -> bool {
    cx.theme.habit_str("panel") == "console"
}

/// The room a field tag leaves ahead of its label.
const TAG_LEAD: f64 = 4.0;

/// A field tag's label: caps in the display face, at the axes `panel.tag` sets.
fn tag_style(cx: &Cx) -> TextStyle {
    let t = cx.theme;
    let size = px(cx, Type::Caption);
    let wght = t.metric("panel.tag", "wght") as f32;
    let wdth = t.metric("panel.tag", "wdth") as f32;
    TextStyle {
        family: crate::surfaces::bar::cell::face(t.faces.display),
        size,
        weight: if wght > 0.0 { wght } else { weight(Weight::Semibold) },
        tracking: (size * 0.08).round(),
        wdth: if wdth > 0.0 { wdth } else { TextStyle::NORMAL_WIDTH },
        wght: (wght > 0.0).then_some(wght),
    }
}

/// A section label as a field tag: its caps on an accent bar that fades out
/// over the tag's trail, in steps.
fn tag(cx: &mut Cx, r: Rect, text: &str, path: &str) {
    const STEPS: usize = 8;
    let b = cx.theme.box_style("panel.tag", None);
    let st = tag_style(cx);
    let shaped = cx.kit.shape(&text.to_uppercase(), st);
    let (fill, ink) = (cx.a(b.fill), cx.a(b.ink));
    let ri = irect(r);
    let along = shaped.width as f64 + TAG_LEAD + cx.theme.metric("panel.tag", "trail");
    let (x0, w) = (ri.x, (along.ceil() as i32).min(ri.w).max(0));
    let mut p = cx.painter(path);
    for i in 0..STEPS {
        let a = x0 + w * i as i32 / STEPS as i32;
        let z = x0 + w * (i as i32 + 1) / STEPS as i32;
        let fade = 1.0 - i as f32 / STEPS as f32;
        p.rect(IRect::new(a, ri.y, z - a, ri.h), fill.with_alpha(fill.a * fade), 0.0);
    }
    let y = (r.y0 + (r.height() - shaped.line_height() as f64) / 2.0).round() as i32;
    p.text(&shaped, ((r.x0 + TAG_LEAD).round() as i32, y), ink, &[]);
    let last = p.last();
    p.finish();
    cx.done(last);
}

/// The height a hero's tab takes over its frame, 0 on a table with no tab.
fn tab_height(cx: &Cx, el: &El, state: CellState) -> f64 {
    if state.hero && el.tab.is_some() { cx.theme.metric("hero.tab", "height").max(0.0) } else { 0.0 }
}

/// The tab over a hero: its label in the values face on a block whose
/// trailing side slants down to the frame.
fn hero_tab(cx: &mut Cx, r: Rect, text: &str, path: &str) {
    let t = cx.theme;
    let b = t.box_style("hero.tab", None);
    let slant = t.metric("hero.tab", "slant").max(0.0);
    let shaped = shape(cx, &format!("*{text}"), &Font { size: Type::Caption, weight: Weight::Normal, mono: true });
    let lead = cx.theme.space.xs;
    let w = (lead + shaped.width as f64 + lead + slant).min(r.width());
    let tab = Rect::new(r.x0, r.y0, r.x0 + w, r.y1);
    let (fill, ink) = (cx.a(b.fill), cx.a(b.ink));
    let mut p = cx.painter(&format!("{path}.tab"));
    let mut outline = BezPath::new();
    outline.move_to((tab.x0, tab.y0));
    outline.line_to(((tab.x1 - slant).max(tab.x0), tab.y0));
    outline.line_to((tab.x1, tab.y1));
    outline.line_to((tab.x0, tab.y1));
    outline.close_path();
    p.shape(irect(tab), Paint::Shape { fill: Some((outline, fill)), strokes: Vec::new() });
    let y = (r.y0 + (r.height() - shaped.line_height() as f64) / 2.0).round() as i32;
    p.text(&shaped, ((r.x0 + lead).round() as i32, y), ink, &[]);
    let last = p.last();
    p.finish();
    cx.done(last);
}

fn stop_radius(cx: &Cx, el: &El, h: f64) -> f64 {
    match &el.kind {
        Kind::Cell { .. } => cx.theme.box_radius(&cx.theme.box_style("cell", None), h),
        Kind::Track { .. } => cx.theme.box_radius(&cx.theme.box_style("track.groove", None), h),
        Kind::Group { .. } | Kind::Segmented { .. } => cx.theme.box_radius(&cx.theme.box_style("trough", None), h),
        Kind::Switch { .. } => cx.theme.box_radius(&cx.theme.box_style("switch.track", Some("off")), cx.theme.space.huge),
        _ => cx.theme.radii.md,
    }
}

/// A cell's box over its states, with the cursor and the hover wash.
#[allow(clippy::too_many_arguments)]
fn cell(cx: &mut Cx, el: &El, r: Rect, state: CellState, interactive: bool, child: &El, path: &str, stop: Option<String>) {
    let t = cx.theme;
    let owned = cx.fill_owned();
    let fill_state = if state.active {
        "active"
    } else if state.selected && !owned {
        "selected"
    } else if state.ghost {
        "ghost"
    } else {
        "rest"
    };
    let border_state = if state.destructive {
        "destructive"
    } else if state.warning {
        "warning"
    } else if state.ghost {
        "ghost"
    } else {
        "rest"
    };
    let mut b = if state.hero {
        t.box_style("hero", None)
    } else {
        let mut b = t.box_style("cell", Some(fill_state));
        let rest_border = t.box_style("cell", None).border.map_or(Rgba::TRANSPARENT, |l| l.color);
        b.border = Some(t.box_style("cell", Some(border_state)).border.unwrap_or(Line { color: rest_border, width: 0.0 }));
        b
    };
    let tab_h = tab_height(cx, el, state);
    if tab_h > 0.0 {
        hero_tab(cx, Rect::new(r.x0, r.y0, r.x1, r.y0 + tab_h), el.tab.as_deref().unwrap_or_default(), path);
    }
    let r = Rect::new(r.x0, r.y0 + tab_h, r.x1, r.y1);
    let hovered = state.hovered || (interactive && cx.hover(path));
    let washing = hovered && !state.active && !state.selected;
    b.wash = t.box_style("cell", Some("hover")).wash;
    let (on, ring) = cx.cursor(stop.as_deref());
    let ring = (ring || state.cursor) && !owned;
    let mut b = if state.brackets { t.with_brackets(b, true, true) } else { t.with_cursor(b, ring, !cx.halo_owned() || state.cursor) };
    b.hex = el.hex;
    let focus_on = on || hovered || state.cursor || (owned && state.selected);
    let catalogue = t.motion().catalogue;
    let focus = match &catalogue.focus_text {
        Some(f) => cx.tween_on(&format!("{path}.focus"), if focus_on { 1.0 } else { 0.0 }, f.clock.duration, crate::motion::Curve::from_table(&f.clock.curve)) as f32,
        None => 0.0,
    };
    let used = catalogue.use_.as_ref().and_then(|u| cx.used(stop.as_deref(), u.clock.duration));
    let ri = irect(r);
    let radius = if state.small { t.radii.sm } else { t.box_radius(&b, ri.h as f64) };
    let fill = cx.color(&format!("{path}.fill"), b.fill);
    let border = b.border.clone().map(|l| Line { color: cx.color(&format!("{path}.border"), l.color), width: l.width });
    let b = BoxStyle { fill, border, ..b };
    let wash = cx.tween(&format!("{path}.wash"), if washing { 1.0 } else { 0.0 }, Clock::Effects) as f32;
    let fg = if state.active {
        t.colors.get("primaryForeground")
    } else if state.destructive {
        t.colors.get("destructive")
    } else if state.warning {
        t.colors.get("warning")
    } else if state.selected {
        t.colors.get("accentForeground")
    } else {
        t.colors.get("foreground")
    };
    let dim = if state.active || state.selected { fg } else { t.colors.get("mutedForeground") };
    let fg = cx.color(&format!("{path}.fg"), fg);
    let dim = cx.color(&format!("{path}.dim"), dim);
    let alpha = cx.alpha;
    let mut p = cx.painter(path);
    boxes::paint(&mut p, ri, &b, radius, alpha, wash);
    let last = p.last();
    p.finish();
    cx.done(last);

    let s = &cx.theme.space;
    let pad_x = s.control_padding_x;
    let content_w = (r.width() - pad_x * 2.0).max(0.0);
    let (cw, ch) = measure(cx, child, content_w);
    let cw = if child.width == Size::Fill { content_w } else { cw };
    let y = r.y0 + (r.height() - ch) / 2.0;
    let saved = (cx.ink, cx.focus, cx.ghost);
    cx.ink = (fg, dim);
    cx.focus = focus;
    cx.ghost = used;
    let under = cx.hits_len();
    paint(cx, child, Rect::new(r.x0 + pad_x, y, r.x0 + pad_x + cw, y + ch), &format!("{path}/0"));
    (cx.ink, cx.focus, cx.ghost) = saved;
    if interactive || el.on.is_some() || el.tip.is_some() {
        let what = if el.on.is_some() { HitWhat::Click } else { HitWhat::Hover };
        cx.hit_at(under, Hit { rect: ri, path: path.into(), on: el.on.clone(), tip: el.tip.clone(), stop, what });
    }
}

#[allow(clippy::too_many_arguments)]
fn button(
    cx: &mut Cx,
    el: &El,
    r: Rect,
    variant: Variant,
    text: &str,
    icon_name: &str,
    enabled: bool,
    square: bool,
    path: &str,
    radius: Option<f64>,
    stop: Option<String>,
    pick: Option<(usize, bool)>,
) {
    let t = cx.theme;
    let hovered = enabled && cx.hover(path);
    let state = if cx.pressed(path) { "press" } else if hovered { "hover" } else { "rest" };
    let mut b = t.box_style(variant.role(), Some(state));
    let held = b.wash.or(t.box_style(variant.role(), Some("hover")).wash);
    let (_, ring) = match pick {
        Some((_, on)) => (on, on && cx.ui_ring()),
        None => cx.cursor(stop.as_deref()),
    };
    b = t.with_cursor(b, ring, !cx.halo_owned());
    let washing = b.wash.is_some();
    b.wash = held;
    let ri = irect(r);
    let radius = radius.unwrap_or(t.radii.md);
    let fill = cx.color(&format!("{path}.fill"), b.fill);
    let border = b.border.clone().map(|l| Line { color: cx.color(&format!("{path}.border"), l.color), width: l.width });
    let b = BoxStyle { fill, border, ..b };
    let wash = cx.tween(&format!("{path}.wash"), if washing { 1.0 } else { 0.0 }, Clock::Effects) as f32;
    let ink = match variant {
        Variant::Default => t.colors.get("primaryForeground"),
        Variant::Destructive => t.colors.get("destructiveForeground"),
        _ => t.colors.get("foreground"),
    };
    let saved = cx.alpha;
    if !enabled {
        cx.alpha *= 0.5;
    }
    let pad_x = if pick.is_some() { t.space.md } else { t.space.control_padding_x };
    let s = t.space.clone();
    let body = t.font_size.body;
    let (_, label, glyph) = button_metrics(cx, text, icon_name, pad_x);
    // A label wider than the button it sits in (a group divides its trough
    // evenly) is cut rather than spilling out.
    let budget = (r.width() - pad_x * 2.0 - glyph.as_ref().map_or(0.0, |_| body + if label.is_some() { s.icon_gap } else { 0.0 })).max(0.0);
    let label = match label {
        Some(l) if square => Some(l),
        Some(l) if l.width as f64 > budget + 0.5 => Some(elided(cx, text, &Font { size: Type::Body, weight: Weight::Medium, mono: false }, budget)),
        other => other,
    };
    let content = label.as_ref().map_or(0.0, |l| l.width as f64)
        + glyph.as_ref().map_or(0.0, |_| body + if label.is_some() { s.icon_gap } else { 0.0 });
    let ink = cx.a(ink);
    let alpha = cx.alpha;
    let mut x = (r.x0 + (r.width() - content) / 2.0).round();
    let mid = r.y0 + r.height() / 2.0;
    let mut p = cx.painter(path);
    boxes::paint(&mut p, ri, &b, radius, alpha, wash);
    if let Some(g) = &glyph {
        let gx = (x + (body - g.width as f64) / 2.0).round() as i32;
        p.text(g, (gx, (mid - g.line_height() as f64 / 2.0).round() as i32), ink, &[]);
        x += body + s.icon_gap;
    }
    if let Some(l) = &label {
        p.text(l, (x as i32, (mid - l.line_height() as f64 / 2.0).round() as i32), ink, &[]);
    }
    let last = p.last();
    p.finish();
    cx.done(last);
    cx.alpha = saved;
    if enabled {
        let what = pick.map_or(HitWhat::Click, |(i, _)| HitWhat::Pick(i));
        cx.hit(Hit { rect: ri, path: path.into(), on: el.on.clone(), tip: el.tip.clone(), stop, what });
    }
}

fn switch(cx: &mut Cx, el: &El, r: Rect, checked: bool, enabled: bool, path: &str, stop: Option<String>) {
    let t = cx.theme;
    let s = t.space.clone();
    let inset = t.border_width * 2.0;
    let mut track = t.box_style("switch.track", Some(if checked { "on" } else { "off" }));
    let (_, ring) = cx.cursor(stop.as_deref());
    if ring {
        track = t.with_cursor(track, true, !cx.halo_owned());
    } else {
        let cursor = t.box_style("cursor", None).border.map_or(Rgba::TRANSPARENT, |l| l.color);
        track.border = Some(Line { color: cursor, width: 0.0 });
    }
    let fill = cx.color(&format!("{path}.fill"), track.fill);
    let track = BoxStyle { fill, ..track };
    let knob = t.box_style("switch.knob", None);
    let tr = Rect::new(r.x0, r.y0 + (r.height() - s.huge) / 2.0, r.x1, r.y0 + (r.height() + s.huge) / 2.0);
    let size = s.huge - inset * 2.0;
    let target = if checked { r.width() - size - inset } else { inset };
    let kx = cx.tween(&format!("{path}.knob"), target, Clock::SpatialFast);
    let kr = IRect::new((r.x0 + kx).round() as i32, (tr.y0 + inset).round() as i32, size.round() as i32, size.round() as i32);
    let tri = irect(tr);
    let tr_radius = t.box_radius(&track, tri.h as f64);
    let kn_radius = t.box_radius(&knob, size);
    // A disabled switch dims the way a button does.
    let alpha = if enabled { cx.alpha } else { cx.alpha * 0.5 };
    let mut p = cx.painter(path);
    boxes::paint(&mut p, tri, &track, tr_radius, alpha, 0.0);
    boxes::paint(&mut p, kr, &knob, kn_radius, alpha, 0.0);
    let last = p.last();
    p.finish();
    cx.done(last);
    if enabled {
        cx.hit(Hit { rect: irect(r), path: path.into(), on: el.on.clone(), tip: el.tip.clone(), stop, what: HitWhat::Switch(checked) });
    }
}

#[allow(clippy::too_many_arguments)]
fn track(cx: &mut Cx, el: &El, r: Rect, value: f64, notch: Option<f64>, interactive: bool, path: &str, stop: Option<String>) {
    let t = cx.theme;
    let mut groove = t.box_style("track.groove", None);
    let (_, ring) = cx.cursor(stop.as_deref());
    if ring {
        groove = t.with_cursor(groove, true, !cx.halo_owned());
    } else {
        let cursor = t.box_style("cursor", None).border.map_or(Rgba::TRANSPARENT, |l| l.color);
        groove.border = Some(Line { color: cursor, width: 0.0 });
    }
    let fill = t.box_style("track.fill", None);
    let notch_fill = t.box_style("track.notch", None).fill;
    let ri = irect(r);
    let radius = t.box_radius(&groove, ri.h as f64);
    let fill_radius = t.box_radius(&fill, ri.h as f64);
    let dither = t.dither;
    let w = cx.tween(&format!("{path}.fill"), ri.w as f64 * value.clamp(0.0, 1.0), Clock::SpatialFast);
    let alpha = cx.alpha;
    let mut p = cx.painter(path);
    boxes::paint(&mut p, ri, &groove, radius, alpha, 0.0);
    if dither {
        // DitherFill: every other pixel of the groove takes the ink, the
        // retro pass's checker at its smallest chunk.
        let ink = fill.fill.with_alpha(fill.fill.a * alpha * 0.35);
        let mut x = ri.x;
        while x < ri.right() {
            p.rect(IRect::new(x, ri.y + ((x - ri.x) % 2), 1, 1), ink, 0.0);
            x += 1;
        }
    }
    p.rect(IRect::new(ri.x, ri.y, w.round() as i32, ri.h), fill.fill.with_alpha(fill.fill.a * alpha), fill_radius as f32);
    if let Some(n) = notch {
        let bw = cx_border(t);
        p.rect(IRect::new(ri.x + (ri.w as f64 * n - bw as f64 / 2.0).round() as i32, ri.y, bw, ri.h), notch_fill.with_alpha(notch_fill.a * alpha), 0.0);
    }
    let last = p.last();
    p.finish();
    cx.done(last);
    if interactive || el.on.is_some() {
        // The groove is a few pixels tall; the row height is what a pointer
        // can hit.
        let s = cx.theme.space.control_height as i32;
        let hit = IRect::new(ri.x, ri.y + ri.h / 2 - s / 2, ri.w, s);
        cx.hit(Hit { rect: hit, path: path.into(), on: el.on.clone(), tip: el.tip.clone(), stop, what: HitWhat::Track });
    }
}

fn cx_border(t: &fs_theme::theme::Theme) -> i32 {
    t.border_width.round().max(1.0) as i32
}

/// The widest whole label.
fn group_natural(cx: &mut Cx, options: &[super::el::Opt]) -> f64 {
    let s = cx.theme.space.clone();
    options.iter().map(|o| button_metrics(cx, &o.label, &o.icon, s.md).0).fold(s.control_height - s.xs * 2.0, f64::max)
}

/// Button group wrap: as many columns as fit the widest button, then
/// the fewest rows that hold every option and the fewest columns filling
/// them. (rows, columns).
fn group_grid(natural: f64, count: usize, width: f64, pad: f64) -> (usize, usize) {
    if count == 0 {
        return (1, 1);
    }
    let fit = (((width - pad) / (natural + pad)).floor() as usize).max(1);
    let rows = count.div_ceil(fit);
    (rows, count.div_ceil(rows).max(1))
}

#[allow(clippy::too_many_arguments)]
fn group(cx: &mut Cx, el: &El, r: Rect, options: &[super::el::Opt], index: usize, exclusive: bool, cursor_index: usize, wrap: bool, path: &str, stop: Option<String>) {
    let t = cx.theme;
    let s = t.space.clone();
    let trough = t.box_style("trough", None);
    let ri = irect(r);
    let tr_radius = t.box_radius(&trough, if wrap { s.control_height + s.xs * 2.0 } else { ri.h as f64 });
    let radius = t.radii.sm.max(tr_radius - s.xs);
    let alpha = cx.alpha;
    let mut p = cx.painter(path);
    boxes::paint(&mut p, ri, &trough, tr_radius, alpha, 0.0);
    let last = p.last();
    p.finish();
    cx.done(last);
    let n = options.len().max(1);
    let cols = if wrap { group_grid(group_natural(cx, options), options.len(), r.width(), s.xs).1 } else { n };
    let bw = ((r.width() - s.xs * 2.0 - s.xs * (cols - 1) as f64) / cols as f64).max(0.0);
    let bh = if wrap { s.control_height } else { (r.height() - s.xs * 2.0).max(0.0) };
    let (on, _) = cx.cursor(stop.as_deref());
    for (i, o) in options.iter().enumerate() {
        let (row, col) = (i / cols, i % cols);
        let x = r.x0 + s.xs + (bw + s.xs) * col as f64;
        let y = r.y0 + s.xs + (bh + s.xs) * row as f64;
        let variant = if exclusive && i == index {
            Variant::Selected
        } else if !exclusive && o.active {
            Variant::Default
        } else {
            Variant::Ghost
        };
        let b = Rect::new(x, y, x + bw, y + bh);
        let child = El::new(Kind::Button { variant, text: o.label.clone(), icon: o.icon.clone(), enabled: o.enabled, square: false });
        let child = El { on: el.on.clone(), ..child };
        button(cx, &child, b, variant, &o.label, &o.icon, o.enabled, false, &format!("{path}/{i}"), Some(radius), stop.clone(), Some((i, on && i == cursor_index)));
    }
}

#[allow(clippy::too_many_arguments)]
fn segmented(cx: &mut Cx, el: &El, r: Rect, options: &[String], index: usize, path: &str, stop: Option<String>) {
    let t = cx.theme;
    let s = t.space.clone();
    let (_, ring) = cx.cursor(stop.as_deref());
    let trough = t.with_cursor(t.box_style("trough", None), ring, !cx.halo_owned());
    let chip = t.box_style("segmented.chip", None);
    let hover_wash = t.box_style("button.ghost", Some("hover")).wash.unwrap_or(Rgba::TRANSPARENT);
    let press_wash = t.box_style("button.ghost", Some("press")).wash.unwrap_or(Rgba::TRANSPARENT);
    let ri = irect(r);
    let tr_radius = t.box_radius(&trough, ri.h as f64);
    let radius = t.radii.sm.max(tr_radius - s.xxs);
    let spans = segment_spans(cx, options, r.width() - s.xxs * 2.0);
    let (at, cw) = spans.get(index).copied().unwrap_or((0.0, 0.0));
    let x = cx.tween(&format!("{path}.chip"), s.xxs + at, Clock::SpatialFast);
    let chip_w = cx.tween(&format!("{path}.chipw"), cw, Clock::SpatialFast);
    let fg = t.colors.get("foreground");
    let muted = t.colors.get("mutedForeground");
    let mut labels = Vec::new();
    for (i, o) in options.iter().enumerate() {
        let sp = format!("{path}/{i}");
        let hovered = cx.hover(&sp);
        let pressed = cx.pressed(&sp);
        let ink = cx.color(&format!("{sp}.ink"), if i == index || hovered { fg } else { muted });
        let wash = cx.tween(&format!("{sp}.wash"), if i != index && (hovered || pressed) { 1.0 } else { 0.0 }, Clock::Effects) as f32;
        let shaped = shape(cx, o, &Font { size: Type::Body, weight: Weight::Medium, mono: false });
        labels.push((shaped, ink, wash, if pressed { press_wash } else { hover_wash }));
    }
    let alpha = cx.alpha;
    let mut p = cx.painter(path);
    boxes::paint(&mut p, ri, &trough, tr_radius, alpha, 0.0);
    let cr = IRect::new((r.x0 + x).round() as i32, (r.y0 + s.xxs).round() as i32, chip_w.round() as i32, (r.height() - s.xxs * 2.0).round() as i32);
    if !options.is_empty() {
        boxes::paint(&mut p, cr, &chip, radius, alpha, 0.0);
    }
    for (i, (l, ink, wash, wc)) in labels.iter().enumerate() {
        let (at, sw) = spans[i];
        let sr = IRect::new((r.x0 + s.xxs + at).round() as i32, cr.y, sw.round() as i32, cr.h);
        if *wash > 0.0 {
            p.rect(sr, wc.with_alpha(wc.a * wash * alpha), radius as f32);
        }
        let lx = sr.x + (sr.w - l.width) / 2;
        let ly = sr.y + (sr.h - l.line_height()) / 2;
        p.text(l, (lx, ly), ink.with_alpha(ink.a * alpha), &[]);
    }
    let last = p.last();
    p.finish();
    cx.done(last);
    for (i, (at, sw)) in spans.iter().enumerate() {
        let sr = IRect::new((r.x0 + s.xxs + at).round() as i32, cr.y, sw.round() as i32, cr.h);
        cx.hit(Hit { rect: sr, path: format!("{path}/{i}"), on: el.on.clone(), tip: None, stop: stop.clone(), what: HitWhat::Pick(i) });
    }
}

/// `caret` is none on an unfocused field.
fn input(cx: &mut Cx, r: Rect, text: &str, placeholder: &str, caret: Option<(usize, usize)>, error: Option<&str>, path: &str) {
    let focused = caret.is_some();
    let t = cx.theme;
    let s = t.space.clone();
    let state = if error.is_some() { "error" } else if focused { "focus" } else { "rest" };
    let mut b = t.box_style("input", Some(state));
    b.rings.clear();
    let focus_ring = t.box_style("input", Some("focus")).rings.into_iter().next();
    let frame = IRect::new(r.x0.round() as i32, r.y0.round() as i32, r.width().round() as i32, s.control_height as i32);
    let radius = t.box_radius(&t.box_style("input", None), frame.h as f64);
    let fill = cx.color(&format!("{path}.fill"), b.fill);
    let border = b.border.clone().map(|l| Line { color: cx.color(&format!("{path}.border"), l.color), width: l.width });
    let b = BoxStyle { fill, border, ..b };
    let ring_a = cx.tween(&format!("{path}.ring"), if focused { 1.0 } else { 0.0 }, Clock::Effects) as f32;
    let body = Font { size: Type::Body, weight: Weight::Normal, mono: false };
    let inner_w = (frame.w as f64 - s.control_padding_x * 2.0).max(0.0);
    let line = line_parts(cx, text, placeholder, caret, &body, inner_w as i32);
    let shake = cx.theme.motion().catalogue.error.map(|ms| {
        let at = cx.changed(&format!("{path}.error"), error.filter(|e| !e.is_empty()).map_or(0, token));
        (at, ms)
    });
    let err = error.filter(|e| !e.is_empty()).map(|e| {
        let st = style(cx, &Font { size: Type::Caption, weight: Weight::Normal, mono: false });
        let Some((at, ms)) = shake else { return cx.kit.shape(e, st) };
        let p = cx.effect(at, ms);
        let (wght, wdth, em) = crate::effects::shake(p, (f64::from(st.weight), 100.0, 0.0));
        let shaken = TextStyle { wght: Some(((wght / 10.0).round() * 10.0) as f32), wdth: wdth.round() as f32, tracking: ((f64::from(st.size) * em) * 4.0).round() as f32 / 4.0, ..st };
        cx.kit.shape(e, if p < 1.0 { shaken } else { st })
    });
    let destructive = t.colors.get("destructive");
    let alpha = cx.alpha;
    let tx = frame.x + s.control_padding_x as i32;
    let ty = frame.y + (frame.h - line.shown.line_height()) / 2;
    let mut p = cx.painter(path);
    if let Some(ring) = focus_ring.filter(|_| ring_a > 0.0) {
        let sp = ring.spread.round() as i32;
        p.rect(IRect::new(frame.x - sp, frame.y - sp, frame.w + sp * 2, frame.h + sp * 2), ring.color.with_alpha(ring.color.a * ring_a * alpha), (radius + ring.spread) as f32);
    }
    boxes::paint(&mut p, frame, &b, radius, alpha, 0.0);
    paint_line(&mut p, t, &line, (tx, ty), IRect::new(tx, frame.y, inner_w as i32, frame.h), alpha);
    if let Some(e) = &err {
        p.text(e, (frame.x, frame.bottom() + s.xs as i32), destructive.with_alpha(destructive.a * alpha), &[]);
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}

/// One field's line, shaped: the text or its placeholder, the caret's x
/// along it, the selection's span, and how far it scrolls left so the caret
/// stays in a room `room` wide. No caret is an unfocused field.
struct LineParts {
    shown: ShapedText,
    empty: bool,
    caret: Option<i32>,
    selection: Option<(i32, i32)>,
    scroll: i32,
}

fn line_parts(cx: &mut Cx, text: &str, placeholder: &str, caret: Option<(usize, usize)>, font: &Font, room: i32) -> LineParts {
    let empty = text.is_empty();
    let caret_w = cx.theme.border_width.max(1.0) as i32;
    if empty {
        let gap = if caret.is_some() { caret_w } else { 0 };
        let shown = elided(cx, placeholder, font, f64::from((room - gap).max(0)));
        return LineParts { shown, empty, caret: caret.map(|_| 0), selection: None, scroll: 0 };
    }
    let shown = shape(cx, text, font);
    let Some((at, anchor)) = caret else {
        return LineParts { shown, empty, caret: None, selection: None, scroll: 0 };
    };
    let whole = shown.width;
    let mut x_at = |n: usize| match text.char_indices().nth(n) {
        Some((0, _)) => 0,
        Some((b, _)) => shape(cx, &text[..b], font).width,
        None => whole,
    };
    let x = x_at(at);
    let other = if anchor == at { x } else { x_at(anchor) };
    let scroll = (x + caret_w - room).max(0);
    LineParts { shown, empty, caret: Some(x), selection: (other != x).then(|| (x.min(other), x.max(other))), scroll }
}

/// The line at `at`, cut to `room`: the selection's fill behind the text it
/// covers, that text in `primaryForeground`, and the caret, which holds
/// still rather than blinking (a blink is a clock) and steps aside while
/// anything is selected.
fn paint_line(p: &mut crate::surfaces::bar::cell::Painter, t: &fs_theme::theme::Theme, line: &LineParts, at: (i32, i32), room: IRect, alpha: f32) {
    let caret_w = t.border_width.max(1.0) as i32;
    let h = line.shown.line_height();
    let x0 = at.0 - line.scroll;
    let clip = p.clip.map_or(room, |c| c.intersect(&room));
    let (w, bh) = line.shown.box_size();
    let pad = if line.empty && line.caret.is_some() { caret_w } else { 0 };
    let bounds = IRect::new(x0 + pad - text::PAD, at.1 - text::PAD, w, bh);
    let ink = if line.empty { t.colors.get("mutedForeground") } else { t.colors.get("foreground") };
    let sel = line.selection.map(|(a, b)| IRect::new(x0 + a, at.1, b - a, h));
    if let Some(r) = sel {
        let style = t.box_style("input.selection", None);
        let fill = style.fill;
        p.shape_in(r, Paint::Rect { fill: fill.with_alpha(fill.a * alpha), radius: t.box_radius(&style, f64::from(h)) as f32 }, clip);
    }
    p.text_in(&line.shown, bounds, Affine::IDENTITY, Some(clip), ink.with_alpha(ink.a * alpha), &[]);
    if let Some(r) = sel {
        let on = t.colors.get("primaryForeground");
        p.text_in(&line.shown, bounds, Affine::IDENTITY, Some(clip.intersect(&r)), on.with_alpha(on.a * alpha), &[]);
    }
    if let Some(x) = line.caret.filter(|_| sel.is_none()) {
        let cx = (x0 + x).min(room.x + room.w - caret_w);
        p.rect(IRect::new(cx, at.1, caret_w, h), t.colors.get("foreground").with_alpha(alpha), 0.0);
    }
}

fn marquee(cx: &mut Cx, r: Rect, text: &str, ink: Ink, path: &str) {
    let font = Font { size: Type::Body, weight: Weight::Normal, mono: false };
    let t = shape(cx, text, &font);
    let color = cx.a(resolve(cx, ink));
    let look = cx.kit.look.clone();
    let ri = irect(r);
    let overflow = t.width > ri.w;
    let gap = (look.body as f64 * 2.5).round();
    let loop_w = t.width as f64 + gap;
    let mut scroll = 0.0;
    if overflow && look.motion {
        // Hold, then one whole loop at a steady speed.
        let hold = look.marquee_hold / 1000.0;
        let run = loop_w / look.marquee_px.max(1.0);
        let born = cx.born(path);
        let period = hold + run;
        let at = cx.now.saturating_duration_since(born).as_secs_f64() % period;
        if at < hold {
            let resume = cx.now + std::time::Duration::from_secs_f64(hold - at);
            cx.wake_at(resume);
        } else {
            scroll = (at - hold) * look.marquee_px;
            cx.animate();
        }
    }
    let y = ri.y + (ri.h - t.line_height()) / 2;
    let (w, h) = t.box_size();
    let mut p = cx.painter(path);
    let clip = Some(p.clip.map_or(ri, |c| c.intersect(&ri)));
    let copies: &[f64] = if overflow { &[0.0, 1.0] } else { &[0.0] };
    for k in copies {
        let x = ri.x as f64 - scroll + k * loop_w;
        let bounds = IRect::new(x.round() as i32 - text::PAD, y - text::PAD, w, h);
        p.text_in(&t, bounds, Affine::IDENTITY, clip, color, &[]);
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}

/// Monitor/history.js `points`: newest at the right edge, one step per
/// sample of `capacity`, the value read against `ceiling` from the bottom.
fn points(values: &[f64], w: f64, h: f64, ceiling: f64, capacity: usize) -> Vec<(f64, f64)> {
    if values.is_empty() || capacity < 2 {
        return Vec::new();
    }
    let step = w / (capacity - 1) as f64;
    let n = values.len();
    let ceiling = if ceiling > 0.0 { ceiling } else { 1.0 };
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = w - (n - 1 - i) as f64 * step;
            let y = h - (v / ceiling).clamp(0.0, 1.0) * h;
            (x, y)
        })
        .collect()
}

/// The largest whole module `n` of which fit across `width`.
fn matrix_module(width: f64, n: usize) -> f64 {
    if n == 0 { 0.0 } else { (width / n as f64).floor().max(1.0) }
}

/// The QR canvas: the quiet zone in whichever of
/// foreground and background is lighter, the modules in the darker, one
/// rect per run of set modules along a row.
fn matrix(cx: &mut Cx, r: Rect, rows: &[String], path: &str) {
    let n = rows.len();
    let m = matrix_module(r.width(), n);
    if m <= 0.0 {
        return;
    }
    let side = m * n as f64;
    let x0 = (r.x0 + ((r.width() - side) / 2.0).floor()).round();
    let y0 = r.y0.round();
    let (fg, bg) = (cx.theme.colors.get("foreground"), cx.theme.colors.get("background"));
    let light = |c: Rgba| c.r.max(c.g).max(c.b) + c.r.min(c.g).min(c.b);
    let (module, quiet) = if light(fg) < light(bg) { (fg, bg) } else { (bg, fg) };
    let (module, quiet) = (cx.a(module), cx.a(quiet));
    let mut p = cx.painter(path);
    p.rect(irect(Rect::new(x0, y0, x0 + side, y0 + side)), quiet, 0.0);
    for (y, row) in rows.iter().enumerate() {
        let bits = row.as_bytes();
        let mut x = 0;
        while x < bits.len() {
            if bits[x] != b'1' {
                x += 1;
                continue;
            }
            let start = x;
            while x < bits.len() && bits[x] == b'1' {
                x += 1;
            }
            let rx = x0 + start as f64 * m;
            let ry = y0 + y as f64 * m;
            p.rect(irect(Rect::new(rx, ry, rx + (x - start) as f64 * m, ry + m)), module, 0.0);
        }
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}

fn sparkline(cx: &mut Cx, r: Rect, series: &super::el::Series, path: &str) {
    let t = cx.theme;
    let primary = cx.a(t.colors.get("primary"));
    let muted = cx.a(t.colors.get("mutedForeground"));
    let line = cx.a(t.colors.get("border"));
    let stroke = t.space.xxs;
    let bw = cx_border(t);
    let ri = irect(r);
    let poly = |vals: &[f64]| -> BezPath {
        let mut path = BezPath::new();
        for (i, (x, y)) in points(vals, r.width(), r.height(), series.ceiling, series.capacity).into_iter().enumerate() {
            let pt = (r.x0 + x, r.y0 + y);
            if i == 0 { path.move_to(pt) } else { path.line_to(pt) }
        }
        path
    };
    let main = poly(&series.values);
    let mut area = main.clone();
    let pts = points(&series.values, r.width(), r.height(), series.ceiling, series.capacity);
    let mut strokes = Vec::new();
    if series.secondary.len() > 1 {
        strokes.push((poly(&series.secondary), muted, stroke));
    }
    let fill = if pts.len() > 1 {
        area.line_to((r.x0 + pts[pts.len() - 1].0, r.y1));
        area.line_to((r.x0 + pts[0].0, r.y1));
        area.close_path();
        strokes.push((main, primary, stroke));
        Some((area, primary.with_alpha(primary.a * 0.15)))
    } else {
        None
    };
    let mut p = cx.painter(path);
    p.rect(IRect::new(ri.x, ri.bottom() - bw, ri.w, bw), line, 0.0);
    p.shape(Scene::cover(r, Affine::IDENTITY, stroke + 1.0), Paint::Shape { fill, strokes });
    let last = p.last();
    p.finish();
    cx.done(last);
}

#[allow(clippy::too_many_arguments)]
fn shoulders(cx: &mut Cx, r: Rect, edge: fs_chrome::types::Edge, span: f64, depth: f64, run: f64, path: &str) {
    use fs_chrome::types::Edge;
    let t = cx.theme;
    let card = t.box_style("card", None);
    let radius = t.box_radius(&card, depth);
    let border = card.border.clone().map_or((Rgba::TRANSPARENT, 0.0), |l| (l.color, l.width));
    let bw = t.border_width;
    let sep = t.box_style("separator", None).fill;
    let (fill, line, sep) = (cx.a(card.fill), cx.a(border.0), cx.a(sep));
    let paths = crate::surfaces::shoulders::paths(span + radius * 2.0, depth, radius, border.1, 1.0, 0.0);
    // Built for a top line, then turned onto the gallery's edge.
    let local = match edge {
        Edge::Top => Affine::translate((run - radius, 0.0)),
        Edge::Bottom => Affine::new([1.0, 0.0, 0.0, -1.0, run - radius, depth]),
        Edge::Left => Affine::new([0.0, 1.0, 1.0, 0.0, 0.0, run - radius]),
        Edge::Right => Affine::new([0.0, 1.0, -1.0, 0.0, depth, run - radius]),
    };
    let at = Affine::translate((r.x0, r.y0)) * local;
    let mut fill_path = paths.fill.clone();
    fill_path.apply_affine(at);
    let mut outer = paths.outer.clone();
    outer.apply_affine(at);
    let vertical = edge.is_vertical();
    let ri = irect(r);
    let b = bw.round() as i32;
    let (a0, a1) = (run.round() as i32, (run + span).round() as i32);
    let segs: Vec<IRect> = if vertical {
        let x = if edge == Edge::Right { ri.right() - b } else { ri.x };
        vec![IRect::new(x, ri.y, b, a0), IRect::new(x, ri.y + a1, b, ri.h - a1)]
    } else {
        let y = if edge == Edge::Bottom { ri.bottom() - b } else { ri.y };
        vec![IRect::new(ri.x, y, a0, b), IRect::new(ri.x + a1, y, ri.w - a1, b)]
    };
    let mut p = cx.painter(path);
    p.shape(Scene::cover(r, Affine::IDENTITY, 2.0), Paint::Shape { fill: Some((fill_path, fill)), strokes: vec![(outer, line, border.1)] });
    for s in segs {
        p.rect(s, sep, 0.0);
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}
