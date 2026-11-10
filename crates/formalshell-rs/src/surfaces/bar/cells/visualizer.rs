//! Six per-column tracks, each the `muted` trough with a
//! fill rising to that column's own level, beside NowPlaying.
//!
//! Opt-in through bar.layout since it spawns a real process. Hidden until the
//! `cava` probe answers; then a dim NO CAVA cell when it is missing, else it
//! appears and goes with NowPlaying. The fill is empty whenever nothing runs:
//! paused, off screen, motion off. The levels are carried toward cava's
//! latest frame every screen frame, which is why the cell animates while the
//! process does.

use std::time::Instant;

use fs_types::{Edge, IRect};
use fs_media::visualizer::model::{self, CELL_BAR_COUNT};
use fs_theme::tokens::WEIGHTS;

use crate::services::proc::Avail;
use crate::services::visualizer;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Cell, Custom, Env, View};
use crate::paint::{Ink, Kit, Look, Painter};
use crate::ui::spectrum::band::{self, Band};

const NO_CAVA: &str = "NO CAVA";

pub struct Visualizer {
    avail: Avail,
    media_available: bool,
    tempo: bool,
    running: bool,
    feed: visualizer::Feed,
    edge: Edge,
    shown_levels: Vec<f64>,
    last: Option<Instant>,
    /// Held while a bar showing the cell is on screen.
    spectrum: Want,
    vertical: bool,
}

impl Default for Visualizer {
    fn default() -> Self {
        Self {
            avail: Avail::Unknown,
            media_available: false,
            tempo: false,
            running: false,
            feed: visualizer::Feed::Cava,
            edge: Edge::Top,
            shown_levels: model::baseline_levels(),
            last: None,
            spectrum: Want::held(Source::Spectrum, false),
            vertical: false,
        }
    }
}

impl Visualizer {
    fn shown(&self) -> bool {
        self.tempo || self.avail == Avail::Missing || (self.avail == Avail::Available && self.media_available)
    }

    /// The tracks turn as a whole on a vertical bar (a row of plain bars has no
    /// upright reading of its own); the NO CAVA words stay upright.
    fn turned(&self) -> bool {
        self.vertical && self.avail != Avail::Missing
    }

    fn tracks_along(look: &Look) -> f64 {
        CELL_BAR_COUNT as f64 * look.track + (CELL_BAR_COUNT as f64 - 1.0) * look.xxs
    }
}

impl Cell for Visualizer {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Media, Topic::Visualizer]
    }

    fn read(&mut self, env: &Env) -> bool {
        let v = &env.store.visualizer;
        let active = env.store.media.active();
        let media_available = active.is_some();
        let tempo = active.as_ref().is_some_and(|a| a.kind.tempo_driven());
        let changed = self.avail != v.avail
            || self.media_available != media_available
            || self.tempo != tempo
            || self.running != v.running
            || self.edge != env.edge;
        self.avail = v.avail;
        self.media_available = media_available;
        self.tempo = tempo;
        self.running = v.running;
        self.feed = env.store.visualizer_feed();
        self.edge = env.edge;
        self.vertical = env.edge.is_vertical();
        if !self.running {
            self.shown_levels = model::baseline_levels();
            self.last = None;
        }
        changed
    }

    fn view(&self, _: &Look) -> View {
        let mut view = View::new(Vec::new(), 0.0);
        view.shown = self.shown();
        view.interactive = false;
        view
    }

    fn visible(&mut self, on: bool) {
        self.spectrum.hold(on);
    }

    fn custom(&mut self) -> Option<&mut dyn Custom> {
        Some(self)
    }
}

impl Custom for Visualizer {
    fn measure(&mut self, kit: &mut Kit, vertical: bool, _band: bool) -> f64 {
        self.vertical = vertical;
        let look = kit.look.clone();
        let content = if self.avail == Avail::Missing {
            let shaped = kit.shape(NO_CAVA, look.label(WEIGHTS.medium as f32));
            let w = shaped.width as f64;
            if vertical && w > look.content_across() + 0.5 { 0.0 } else if vertical { shaped.line_height() as f64 } else { w }
        } else {
            Self::tracks_along(&look)
        };
        if content <= 0.0 { 0.0 } else { content + look.pad_x * 2.0 }
    }

    fn draw(&mut self, kit: &mut Kit, p: &mut Painter, rect: IRect, ink: &Ink, now: Instant) {
        let look = kit.look.clone();
        if self.avail == Avail::Missing {
            let shaped = kit.shape(NO_CAVA, look.label(WEIGHTS.medium as f32));
            let (lh, w) = (shaped.line_height(), shaped.width);
            let (x, y) = if self.vertical {
                (rect.x + (rect.w - w) / 2, rect.y + look.pad_x as i32)
            } else {
                (rect.x + look.pad_x as i32, rect.y + (rect.h - lh) / 2)
            };
            p.text(&shaped, (x, y), ink.of(ink.dim), &ink.glow);
            return;
        }

        let dt = self.last.map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f64());
        self.last = Some(now);
        let target = if !self.running {
            model::baseline_levels()
        } else {
            self.feed.frame(now).mono
        };
        self.shown_levels = if self.running { model::smooth_levels(&self.shown_levels, &target, dt) } else { target };
        let levels = model::downsample(&self.shown_levels, CELL_BAR_COUNT);

        let (w, h) = (look.track.round() as i32, look.body.round() as i32);
        let step = (look.track + look.xxs).round() as i32;
        let radius = look.radius_sm.min(look.track / 2.0) as f32;
        let turned = self.turned();
        let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
        for (i, level) in levels.iter().enumerate() {
            let i = i as i32;
            // A vertical bar reads its first column from the bottom on the left
            // edge and from the top on the right, the way the turn lays it out.
            let (track, fill_edge) = if turned {
                let along = Self::tracks_along(&look).round() as i32;
                let y = match self.edge {
                    Edge::Left => rect.y + (rect.h + along) / 2 - w - i * step,
                    _ => rect.y + (rect.h - along) / 2 + i * step,
                };
                (IRect::new(cx - h / 2, y, h, w), self.edge)
            } else {
                let along = Self::tracks_along(&look).round() as i32;
                (IRect::new(rect.x + (rect.w - along) / 2 + i * step, cy - h / 2, w, h), Edge::Bottom)
            };
            p.rect(track, ink.of(look.muted_fill), radius);
            if *level <= 0.0 {
                continue;
            }
            let colour = match band::level_color_band(*level) {
                Band::Accent => look.primary,
                Band::Content => ink.fg,
                Band::Dim => ink.dim,
            };
            // Never shorter than a full pair of rounded ends, so a low level
            // draws one whole dot instead of a squashed sliver.
            let rounded = (radius * 2.0).ceil() as i32;
            let fill = match fill_edge {
                Edge::Left => {
                    let len = ((track.w as f64 * level).round() as i32).max(rounded).min(track.w);
                    IRect::new(track.x, track.y, len, track.h)
                }
                Edge::Right => {
                    let len = ((track.w as f64 * level).round() as i32).max(rounded).min(track.w);
                    IRect::new(track.right() - len, track.y, len, track.h)
                }
                _ => {
                    let len = ((track.h as f64 * level).round() as i32).max(rounded).min(track.h);
                    IRect::new(track.x, track.bottom() - len, track.w, len)
                }
            };
            p.rect(fill, ink.of(colour), radius);
        }
    }

    fn animating(&self, _: Instant) -> bool {
        self.running && self.shown() && self.avail != Avail::Missing
    }
}
