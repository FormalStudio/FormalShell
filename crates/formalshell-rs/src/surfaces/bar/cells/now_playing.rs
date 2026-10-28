//! The music glyph and the active source's track as the
//! strip's first free label, which keeps its room longest. Right click skips
//! ahead and the wheel steps previous and next. Once the source has art, its
//! cover takes the glyph's slot, animated off the media panel's own decode
//! behind `media.animatedBarCover` while the bar is on screen.

use crate::scene::Bitmap;
use crate::services::motion_art;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Limits, Look, Part, View};

/// The cell's own ceiling on a strip long enough to afford it.
const CAP: f64 = 220.0;

pub struct NowPlaying {
    track: Option<(String, String)>,
    /// The cover's slot and picture, while the source has art.
    cover: Option<(f64, Option<Bitmap>)>,
    /// This cell's `motion_art` slot, what it asks for there, and whether
    /// its bar is on screen to show it.
    slot: u64,
    motion: Option<motion_art::Want>,
    seen: bool,
}

impl Default for NowPlaying {
    fn default() -> Self {
        Self { track: None, cover: None, slot: motion_art::slot(), motion: None, seen: false }
    }
}

impl Drop for NowPlaying {
    fn drop(&mut self) {
        motion_art::want(self.slot, None);
    }
}

/// The slot ActiveWindow's app icon takes: a body-size line of text.
pub(super) fn slot(env: &Env) -> f64 {
    (env.store.theme.theme.font_size.body * 1.25).round()
}

impl NowPlaying {
    fn sync_motion(&self) {
        motion_art::want(self.slot, self.motion.clone().filter(|_| self.seen));
    }
}

impl Cell for NowPlaying {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Media, Topic::Config, Topic::Theme]
    }

    fn read(&mut self, env: &Env) -> bool {
        let active = env.store.media.active();
        let config = &env.store.config;
        let animated = env.store.theme.theme.motion_enabled
            && config.bool("media.appleMusicArt").unwrap_or(false)
            && config.bool("media.animatedBarCover").unwrap_or(true);
        let mut motion = None;
        let cover = active.as_ref().filter(|a| !a.art_url.is_empty()).map(|a| {
            let size = slot(env);
            let (px, radius) = crate::ui::w::cover_inner(&env.store.theme.theme, size);
            let scale = crate::scene::raster_scale();
            if animated && !a.artist.is_empty() && !a.album.is_empty() {
                motion = Some(motion_art::Want { artist: a.artist.clone(), album: a.album.clone(), size: px, radius, playing: a.playing }.at(scale));
            }
            let frame = motion.as_ref().and_then(|w| env.store.media.motion(&w.key(), px, scale));
            (size, frame.or_else(|| env.store.media.cover(&a.art_url, px, radius, scale)))
        });
        self.motion = motion;
        self.sync_motion();
        let track = active.map(|a| (if a.title.is_empty() { a.identity } else { a.title }, a.artist));
        let same_cover = match (&cover, &self.cover) {
            (None, None) => true,
            (Some((s1, a)), Some((s2, b))) => s1 == s2 && crate::ui::el::Pic(a.clone()) == crate::ui::el::Pic(b.clone()),
            _ => false,
        };
        let changed = track != self.track || !same_cover;
        self.track = track;
        self.cover = cover;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let available = self.track.is_some();
        let mut parts = vec![match &self.cover {
            Some((size, image)) => Part::Cover { image: crate::ui::el::Pic(image.clone()), size: *size },
            None => Part::Icon { name: "music".into(), dim: !available, dot: false },
        }];
        if let Some((title, _)) = &self.track {
            parts.push(Part::Free { text: title.clone(), dim: false, lead: 0.0, ceiling: CAP, cross: true });
        }
        let tooltip = match &self.track {
            None => "NOTHING PLAYING / CLICK FOR THE RADIO".to_owned(),
            Some((title, artist)) => {
                let by = if artist.is_empty() { String::new() } else { format!("{artist} / ") };
                format!("NOW PLAYING / {by}{title} / RIGHT NEXT / SCROLL PREV NEXT")
            }
        };
        View::new(parts, look.xs).panel("media").tooltip(tooltip)
    }

    fn limits(&self, look: &Look, along: f64, _rest: f64) -> Option<Limits> {
        let cap = CAP.min(along * 0.15);
        Some(Limits { cap, min: cap.min(look.narrow / 2.0) })
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Left => Action::Panel("media"),
            Button::Right => Action::MediaNext,
            Button::Middle => Action::None,
        }
    }

    fn wheel(&mut self, up: bool, _: &Env) -> Action {
        if up { Action::MediaNext } else { Action::MediaPrevious }
    }

    fn visible(&mut self, on: bool) {
        if on != self.seen {
            self.seen = on;
            self.sync_motion();
        }
    }
}
