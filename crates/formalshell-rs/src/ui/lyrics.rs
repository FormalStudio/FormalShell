//! The media panel's lyrics card. Every line is a row in a
//! column that rests its anchor (the lit main line, else the latest lit
//! secondary one) at the comfort offset while it follows the song, and sits
//! where a wheel left it once it does not. A lit row wipes chunk by chunk,
//! each text row of a chunk in reading order, with a glow on the chunk being
//! sung; every other row fades and, behind `media.lyricsBlur`, blurs with
//! its distance from the anchor.
//!
//! Cost: rows are laid out once per line set and width. Each row is its own
//! node group, so between line changes only the lit row's nodes change (the
//! wipe), and a line change repaints the rows whose distance changed.

use std::collections::HashMap;
use std::sync::Arc;

use fs_media::lyrics::{self as model, Line, RowBand, TextRow};
use fs_theme::color::Rgba;
use fs_theme::style::Glow;
use vello_cpu::kurbo::{Affine, Rect};

use super::{Cx, Hit, HitWhat, Stop, irect};
use crate::motion::Kind as Clock;
use crate::scene::IRect;
use crate::text::{self, ShapedText, TextStyle};

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub lines: Arc<Vec<Line>>,
    /// The held position the lit set is judged at.
    pub t: f64,
    pub follow: bool,
    pub blur: bool,
    pub strength: f64,
    pub height: f64,
    /// The keyboard cursor's row, or `lines.len()` for the resync control.
    pub cursor: Option<usize>,
}

struct Piece {
    text: ShapedText,
    x: f64,
    y: f64,
    w: f64,
    /// The chunk this piece belongs to and its text row inside the chunk.
    chunk: Option<(usize, usize)>,
}

struct Row {
    y: f64,
    h: f64,
    pieces: Vec<Piece>,
    bands: HashMap<usize, Vec<RowBand>>,
}

pub struct Pane {
    laid: Option<(usize, u64)>,
    rows: Vec<Row>,
    /// The column's y while a wheel holds it, and as last drawn.
    wheel_y: f64,
    last_y: f64,
    following: bool,
    last_anchor: usize,
    /// Which rows were lit last frame, for the arrival fade.
    lit: Vec<bool>,
    /// Rows whose scale was still travelling last frame.
    moving: std::collections::HashSet<usize>,
    pub seen: u64,
}

impl Pane {
    fn new(seen: u64) -> Self {
        Self { laid: None, rows: Vec::new(), wheel_y: 0.0, last_y: 0.0, following: true, last_anchor: 0, lit: Vec::new(), moving: Default::default(), seen }
    }

    /// A wheel's `delta` px over the pane: the first one takes the column
    /// from wherever it is.
    pub fn scroll(&mut self, delta: f64) {
        if self.following {
            self.wheel_y = self.last_y;
            self.following = false;
        }
        self.wheel_y += delta;
    }
}

fn style(cx: &Cx, background: bool) -> TextStyle {
    let f = &cx.theme.font_size;
    let size = if background { f.body } else { f.title } as f32;
    TextStyle { family: cx.kit.look.sans, size, weight: fs_theme::tokens::WEIGHTS.medium as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
}

/// `s` cut into runs no wider than `max`, at character boundaries: a chunk
/// wider than the pane wraps inside itself, as Text.Wrap does.
fn break_chars(cx: &mut Cx, s: &str, st: TextStyle, max: f64) -> Vec<ShapedText> {
    let mut out = Vec::new();
    let mut line = String::new();
    for ch in s.chars() {
        let next = format!("{line}{ch}");
        if !line.is_empty() && cx.kit.shape(&next, st).width as f64 > max {
            out.push(cx.kit.shape(&line, st));
            line = ch.to_string();
        } else {
            line = next;
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(cx.kit.shape(&line, st));
    }
    out
}

fn layout_row(cx: &mut Cx, line: &Line, width: f64) -> (f64, Vec<Piece>, HashMap<usize, Vec<RowBand>>) {
    let st = style(cx, line.background);
    let lh = cx.kit.shape("Ag", st).line_height() as f64;
    let mut pieces = Vec::new();
    let mut bands = HashMap::new();
    if line.interlude {
        let g = fs_theme::icons::glyph(&cx.kit.look.icon_set, "music");
        let t = cx.kit.shape(g.text, TextStyle { family: text::Family::Named(g.family), size: cx.theme.font_size.heading as f32, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
        let h = t.line_height() as f64;
        let w = t.width as f64;
        pieces.push(Piece { text: t, x: 0.0, y: 0.0, w, chunk: None });
        return (h, pieces, bands);
    }
    if line.words.is_empty() {
        let mut y = 0.0;
        let mut text_line = String::new();
        let mut lines = Vec::new();
        for word in line.text.split_whitespace() {
            let next = if text_line.is_empty() { word.to_owned() } else { format!("{text_line} {word}") };
            if text_line.is_empty() || cx.kit.shape(&next, st).width as f64 <= width {
                text_line = next;
            } else {
                lines.push(std::mem::replace(&mut text_line, word.to_owned()));
            }
        }
        lines.push(text_line);
        for l in lines {
            for t in break_chars(cx, &l, st, width) {
                let w = t.width as f64;
                pieces.push(Piece { text: t, x: 0.0, y, w, chunk: None });
                y += lh;
            }
        }
        if line.opposite_turn {
            for p in &mut pieces {
                p.x = width - p.w;
            }
        }
        return (y.max(lh), pieces, bands);
    }
    let space = cx.kit.shape("a a", st).width as f64 - cx.kit.shape("aa", st).width as f64;
    let (mut x, mut y) = (0.0, 0.0);
    let mut index = 0;
    for group in model::chunk_words(&line.words) {
        let shaped: Vec<Vec<ShapedText>> = group.iter().map(|w| break_chars(cx, &w.text, st, width)).collect();
        let first_w: f64 = shaped.iter().map(|runs| runs[0].width as f64).sum();
        if x > 0.0 && x + first_w > width {
            x = 0.0;
            y += lh;
        }
        for runs in shaped {
            let mut text_rows = Vec::new();
            let n = runs.len();
            for (k, t) in runs.into_iter().enumerate() {
                if k > 0 || (x > 0.0 && x + t.width as f64 > width) {
                    x = 0.0;
                    y += lh;
                }
                let w = t.width as f64;
                text_rows.push(TextRow { y: k as f64 * lh, height: lh, width: w });
                pieces.push(Piece { text: t, x, y, w, chunk: Some((index, k)) });
                x += w;
                if k + 1 < n {
                    x = 0.0;
                }
            }
            let box_w = text_rows.iter().map(|r| r.width).fold(0.0, f64::max);
            bands.insert(index, model::chunk_row_bands(&text_rows, lh * n as f64, box_w));
            index += 1;
        }
        x += space;
    }
    if line.opposite_turn {
        let used = pieces.iter().map(|p| p.x + p.w).fold(0.0, f64::max);
        for p in &mut pieces {
            p.x += width - used;
        }
    }
    (y + lh, pieces, bands)
}

#[allow(clippy::too_many_lines)]
pub fn paint(cx: &mut Cx, r: Rect, v: &View, path: &str) {
    let t = cx.theme;
    let s = t.space.clone();
    let card = t.box_style("card", None);
    let radius = t.radii.md;
    let alpha = cx.alpha;
    {
        let mut p = cx.painter(&format!("{path}/frame"));
        super::boxes::paint(&mut p, irect(r), &card, radius, alpha, 0.0);
        let last = p.last();
        p.finish();
        cx.done(last);
    }
    let pad = s.panel_padding;
    let label = cx.kit.shape("Lyrics", TextStyle { family: cx.kit.look.sans, size: t.font_size.caption as f32, weight: fs_theme::tokens::WEIGHTS.medium as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
    let muted = t.colors.get("mutedForeground");
    let fg = t.colors.get("foreground");
    {
        let at = ((r.x0 + pad).round() as i32, (r.y0 + pad).round() as i32);
        let mut p = cx.painter(&format!("{path}/label"));
        p.text(&label, at, muted.with_alpha(muted.a * alpha), &[]);
        let last = p.last();
        p.finish();
        cx.done(last);
    }
    let vp = Rect::new(r.x0 + pad, r.y0 + pad + label.line_height() as f64 + s.row_gap, r.x1 - pad, r.y1 - pad);
    let vpi = irect(vp);
    let clip = cx.clip.map_or(vpi, |c| c.intersect(&vpi));
    cx.hit(Hit { rect: vpi, path: path.into(), on: Some("lyrics-pane".into()), tip: None, stop: None, what: HitWhat::Scroll });

    let lines = &v.lines;
    let opposite = lines.iter().any(|l| l.opposite_turn);
    let box_w = vp.width() * if opposite { 0.9 } else { 1.0 };
    let key = (Arc::as_ptr(lines) as usize, box_w.to_bits());
    let frame = cx.ui.frame;
    let laid = cx.ui.panes.get(path).and_then(|p| p.laid) == Some(key);
    if !laid {
        let mut rows = Vec::new();
        let mut y = 0.0;
        for line in lines.iter() {
            let inner = box_w - s.control_padding_x * 2.0 - if line.background { s.control_padding_x } else { 0.0 };
            let (h, pieces, bands) = layout_row(cx, line, inner.max(1.0));
            let h = h + s.control_padding_y * 2.0;
            rows.push(Row { y, h, pieces, bands });
            y += h;
        }
        let pane = cx.ui.panes.entry(path.to_owned()).or_insert_with(|| Pane::new(frame));
        pane.rows = rows;
        pane.laid = Some(key);
        pane.lit.clear();
    }

    let main = model::main_line_indices(lines);
    let active = model::active_main_line_index(lines, &main, v.t);
    let secondary = model::active_secondary_lines(lines, &main, v.t, active);
    let pane = cx.ui.panes.get_mut(path).expect("pane");
    pane.seen = frame;
    let lit_anchor = active.or_else(|| secondary.iter().max().copied()).unwrap_or(pane.last_anchor);
    pane.last_anchor = lit_anchor;
    // The keyboard cursor takes the anchor over while it sits on a line.
    let on_line = v.cursor.filter(|c| *c < lines.len());
    let anchor = on_line.unwrap_or(lit_anchor);
    let lit_now: Vec<bool> = (0..lines.len()).map(|i| Some(i) == active || secondary.contains(&i)).collect();
    let arriving: Vec<bool> = if pane.lit.len() == lit_now.len() {
        lit_now.iter().zip(&pane.lit).map(|(now, was)| *now && !*was).collect()
    } else {
        vec![false; lit_now.len()]
    };
    pane.lit = lit_now;
    let vh = vp.height();
    let total: f64 = pane.rows.last().map_or(0.0, |r| r.y + r.h);
    let pitch = if lines.is_empty() { 0.0 } else { total / lines.len() as f64 };
    let spans = model::row_spans(vh, pitch);
    let resting = |rows: &[Row], i: usize| model::comfort_y(vh, rows.get(i).map_or(0.0, |r| r.y));
    let follow_y = resting(&pane.rows, anchor);
    if !v.follow && pane.following {
        pane.wheel_y = pane.last_y;
    }
    pane.following = v.follow;
    let top_limit = resting(&pane.rows, 0);
    let bottom_limit = resting(&pane.rows, pane.rows.len().saturating_sub(1));
    pane.wheel_y = pane.wheel_y.clamp(bottom_limit.min(top_limit), top_limit);
    let wheel_y = pane.wheel_y;
    // While a wheel holds the column the follow tween rides along with it,
    // so going back to the song travels from where the column is.
    let col_y = if v.follow {
        cx.tween(&format!("{path}.y"), follow_y, Clock::Spatial)
    } else {
        cx.jump(&format!("{path}.y"), wheel_y, Clock::Spatial);
        wheel_y
    };
    let pane = cx.ui.panes.get_mut(path).expect("pane");
    pane.last_y = col_y;

    let ramp = s.control_height.max(cx.kit.shape(" ", style(cx, false)).line_height() as f64 + s.control_padding_y * 2.0);
    let rows: Vec<(f64, f64)> = cx.ui.panes[path].rows.iter().map(|r| (r.y, r.h)).collect();
    for (i, line) in lines.iter().enumerate() {
        let (ry, rh) = rows[i];
        let top = col_y + ry;
        let rpath = format!("{path}/{i}");
        let cursor_row = v.cursor == Some(i);
        if top + rh < -1.0 || top > vh + 1.0 {
            if cursor_row {
                // Off the viewport while the column is still travelling to
                // it: the cursor keeps its place in the stop order.
                let edge = if top < 0.0 { vpi.y } else { vpi.bottom() - 1 };
                cx.stop(Stop { key: format!("lyric:{i}"), rect: IRect::new(vpi.x, edge, vpi.w, 1), radius: t.radii.md });
                cx.animate();
            }
            continue;
        }
        let lit = Some(i) == active || secondary.contains(&i);
        let distance = i as f64 - anchor as f64;
        let span = if distance < 0.0 { spans.above } else { spans.below };
        let edge = model::edge_fraction(top, rh, vh, Some(ramp));
        let depth = if lit { 1.0 } else { model::depth_opacity(distance, Some(span)) };
        // A line arriving at lit fades in from 0.68 rather than stepping to
        // its depth opacity (kopuz's fadeLineIn).
        let arrival_key = format!("{rpath}.arrival");
        if arriving[i] {
            cx.jump(&arrival_key, 0.68, Clock::EffectsSlow);
        }
        let arrival = cx.tween(&arrival_key, 1.0, Clock::EffectsSlow);
        let row_alpha = cx.tween(&format!("{rpath}.a"), depth * if line.background { 0.7 } else { 1.0 }, Clock::Effects) * edge * arrival;
        let hovered = cx.ui.hover.as_deref() == Some(rpath.as_str());
        let blur_target = if !v.blur || lit || hovered || cursor_row { 0.0 } else { model::blur_for(distance, v.strength, Some(span)) };
        let blur = cx.tween(&format!("{rpath}.blur"), blur_target, Clock::Effects);
        let blur = (blur / model::BLUR_QUANTUM_PX).round() * model::BLUR_QUANTUM_PX;
        let a = (row_alpha as f32) * alpha;
        let ink = cx.color(&format!("{rpath}.ink"), if lit { fg } else { muted });
        let x0 = vp.x0 + if line.opposite_turn { vp.width() - box_w } else { 0.0 } + s.control_padding_x + if line.background && !line.opposite_turn { s.control_padding_x } else { 0.0 };
        let y0 = vp.y0 + top + s.control_padding_y;
        let line_end = finite(line.end).or_else(|| lines.get(i + 1).map(|n| n.time));
        let interlude_progress = if line.interlude && lit {
            match line.end {
                Some(e) if e > line.time => ((v.t - line.time) / (e - line.time)).clamp(0.0, 1.0),
                _ => f64::from(u8::from(v.t >= line.time)),
            }
        } else {
            0.0
        };
        let chunked = lit && !line.interlude && !line.words.is_empty();
        if chunked || (line.interlude && lit) {
            cx.animate();
        }
        let pane = &cx.ui.panes[path];
        let row = &pane.rows[i];
        let mut draws: Vec<(ShapedText, (i32, i32), Rgba, Option<IRect>, Vec<Glow<Rgba>>, f32)> = Vec::new();
        for piece in &row.pieces {
            let mut px = x0 + piece.x;
            if line.interlude && !opposite {
                px = vp.x0 + (vp.width() - piece.w) / 2.0;
            }
            let at = (px.round() as i32, (y0 + piece.y).round() as i32);
            if line.interlude {
                draws.push((piece.text.clone(), at, muted.with_alpha(muted.a * a * 0.35), None, Vec::new(), 0.0));
                if interlude_progress > 0.0 {
                    let c = IRect::new(at.0, at.1 - text::PAD, (piece.w * interlude_progress).round() as i32, piece.text.line_height() + text::PAD * 2);
                    draws.push((piece.text.clone(), at, fg.with_alpha(fg.a * a), Some(c), Vec::new(), 0.0));
                }
                continue;
            }
            match (chunked, piece.chunk) {
                (true, Some((ci, k))) => {
                    draws.push((piece.text.clone(), at, muted.with_alpha(muted.a * a), None, Vec::new(), 0.0));
                    let progress = model::chunk_progress(&line.words, ci, line_end, v.t, line.estimated);
                    let frac = row.bands.get(&ci).map_or(progress, |b| model::row_wipe(b, k, progress));
                    if frac > 0.0 {
                        let c = IRect::new(at.0 - text::PAD, at.1 - text::PAD, (piece.w * frac).round() as i32 + text::PAD, piece.text.line_height() + text::PAD * 2);
                        // MultiEffect's shadow under the sung part: a
                        // Gaussian of (4 + 6g)px at 0.3g of the ink.
                        let glow = model::chunk_glow(&line.words, ci, line_end, v.t);
                        if glow > 0.0 {
                            let sigma = ((4.0 + glow * 6.0) / 2.0) as f32;
                            draws.push((piece.text.clone(), at, fg.with_alpha(fg.a * 0.3 * glow as f32 * a), Some(c), Vec::new(), sigma));
                        }
                        draws.push((piece.text.clone(), at, fg.with_alpha(fg.a * a), Some(c), Vec::new(), 0.0));
                    }
                }
                _ => draws.push((piece.text.clone(), at, ink.with_alpha(ink.a * a), None, Vec::new(), blur as f32)),
            }
        }
        let row_rect = IRect::new(vpi.x, (vp.y0 + top).round() as i32, vpi.w, rh.round() as i32);
        // The lit/dark tell is a transform, never a relayout: a row keeps
        // its shape and scales about its own reading edge (a note about its
        // centre).
        let scale_target = if lit { if line.background || line.interlude { 0.9 } else { 1.0 } } else { 0.85 };
        let scale = cx.tween(&format!("{rpath}.scale"), scale_target, Clock::SpatialFast);
        let origin_x = if line.interlude && !opposite {
            vp.x0 + vp.width() / 2.0
        } else if line.opposite_turn {
            vp.x1 - s.control_padding_x
        } else {
            vp.x0 + s.control_padding_x
        };
        let origin_y = vp.y0 + top + rh / 2.0;
        let about = Affine::translate((origin_x, origin_y)) * Affine::scale(scale) * Affine::translate((-origin_x, -origin_y));
        // A wrapped row's lines shrink toward the row's middle, which can
        // leave their own boxes: the whole row is damaged while it moves.
        let moving = (scale - scale_target).abs() > 1e-3;
        let pane = cx.ui.panes.get_mut(path).expect("pane");
        let damaged = moving || pane.moving.contains(&i);
        if moving {
            pane.moving.insert(i);
        } else {
            pane.moving.remove(&i);
        }
        let shown = row_rect.intersect(&vpi);
        if damaged && !shown.is_empty() {
            cx.scene.touch(shown);
        }
        let mut p = cx.painter(&rpath);
        for (shaped, at, color, c, glows, b) in draws {
            if b > 0.0 {
                p.blurred_with(&shaped, at, color, b, Some(c.map_or(clip, |c| c.intersect(&clip))), about);
                continue;
            }
            let (w, h) = shaped.box_size();
            let bounds = IRect::new(at.0 - text::PAD, at.1 - text::PAD, w, h);
            let c = c.map_or(clip, |c| c.intersect(&clip));
            p.text_in(&shaped, bounds, about, Some(c), color, &glows);
        }
        let last = p.last();
        p.finish();
        cx.done(last);
        if cursor_row {
            cx.stop(Stop { key: format!("lyric:{i}"), rect: row_rect.intersect(&vpi), radius: t.radii.md });
            cx.animate();
        } else if v.cursor.is_some_and(|c| c < lines.len()) {
            cx.stop(Stop { key: format!("lyric:{i}"), rect: row_rect.intersect(&vpi), radius: t.radii.md });
        }
        cx.hit(Hit { rect: row_rect.intersect(&clip), path: rpath, on: Some(format!("lyric:{i}")), tip: None, stop: None, what: HitWhat::Click });
    }

    // The resync control (spec P9): present once a wheel has taken the
    // column over, and the last entry in the keyboard's order while it is.
    let shown = !v.follow || v.cursor == Some(lines.len());
    let fade = cx.tween(&format!("{path}.resync"), if shown { 1.0 } else { 0.0 }, Clock::Effects);
    if fade > 0.0 {
        let size = s.control_height;
        let button = super::w::icon_button("refresh-cw").tip("Follow the song").stop("lyric-resync").on("lyrics-follow");
        let before = cx.alpha;
        cx.alpha = before * fade as f32;
        super::draw::paint(cx, &button, Rect::new(r.x1 - size, r.y1 - size, r.x1, r.y1), &format!("{path}/resync"));
        cx.alpha = before;
    }
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
}
