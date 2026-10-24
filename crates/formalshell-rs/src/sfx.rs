//! The table's blips (spec C10): which event a launcher key was, and the
//! rate limits every blip passes before it reaches fs-audio's stream. The
//! table, `theme.sounds`, do-not-disturb and the lock are the App's to
//! check (`App::sfx`).

use std::time::{Duration, Instant};

use fs_audio::sfx::Sound;
use fs_menu::nav::Key;

use crate::surfaces::launcher::Out;

/// One blip per frame at most, whatever asked for it: a burst of moves
/// plays one, and an activate that also closes the launcher plays only
/// the activate.
const FRAME: Duration = Duration::from_millis(16);

/// A held key's repeat or a fast typist plays at most this often.
const TYPE_EVERY: Duration = Duration::from_millis(50);

#[derive(Default)]
pub struct Gate {
    last: Option<Instant>,
    last_type: Option<Instant>,
}

impl Gate {
    /// Whether a blip may play at `now`, counted as played when it may.
    pub fn admit(&mut self, event: Sound, now: Instant) -> bool {
        let within = |t: Option<Instant>, d: Duration| t.is_some_and(|t| now.saturating_duration_since(t) < d);
        if within(self.last, FRAME) || (event == Sound::Type && within(self.last_type, TYPE_EVERY)) {
            return false;
        }
        self.last = Some(now);
        if event == Sound::Type {
            self.last_type = Some(now);
        }
        true
    }
}

/// What a launcher key can change that a blip answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub level: Option<String>,
    pub query: String,
    pub cursor: usize,
}

/// The blip for one launcher key, read off what it changed. A close is
/// none here: closing the window plays its own.
pub fn launcher_cue(key: Key, out: &Out, before: &Mark, after: &Mark, open: bool) -> Option<Sound> {
    let back = matches!(key, Key::Escape | Key::Backspace);
    if before.level != after.level {
        return Some(if back { Sound::Back } else { Sound::Activate });
    }
    if matches!(out, Out::Internal(_)) && open {
        return Some(Sound::Toggle);
    }
    if matches!(key, Key::Return | Key::Enter) {
        return Some(Sound::Activate);
    }
    if before.query != after.query {
        return Some(if key == Key::Escape { Sound::Back } else { Sound::Type });
    }
    (before.cursor != after.cursor).then_some(Sound::Move)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(level: Option<&str>, query: &str, cursor: usize) -> Mark {
        Mark { level: level.map(str::to_owned), query: query.to_owned(), cursor }
    }

    #[test]
    fn a_burst_plays_one_blip_a_frame() {
        let mut g = Gate::default();
        let t = Instant::now();
        assert!(g.admit(Sound::Move, t));
        assert!(!g.admit(Sound::Move, t + Duration::from_millis(5)));
        assert!(!g.admit(Sound::Close, t + Duration::from_millis(10)));
        assert!(g.admit(Sound::Move, t + Duration::from_millis(17)));
    }

    #[test]
    fn typing_is_held_to_its_own_rate() {
        let mut g = Gate::default();
        let t = Instant::now();
        assert!(g.admit(Sound::Type, t));
        assert!(!g.admit(Sound::Type, t + Duration::from_millis(30)));
        assert!(g.admit(Sound::Move, t + Duration::from_millis(30)));
        assert!(g.admit(Sound::Type, t + Duration::from_millis(60)));
    }

    #[test]
    fn each_key_reads_as_its_event() {
        let root = mark(None, "", 0);
        assert_eq!(launcher_cue(Key::Down, &Out::None, &root, &mark(None, "", 1), true), Some(Sound::Move));
        assert_eq!(launcher_cue(Key::Down, &Out::None, &root, &root, true), None);
        assert_eq!(launcher_cue(Key::Return, &Out::None, &root, &mark(Some("wifi"), "", 0), true), Some(Sound::Activate));
        assert_eq!(launcher_cue(Key::Escape, &Out::None, &mark(Some("wifi"), "", 2), &root, true), Some(Sound::Back));
        assert_eq!(launcher_cue(Key::Escape, &Out::Close, &root, &root, false), None);
        assert_eq!(launcher_cue(Key::Escape, &Out::None, &mark(None, "fi", 0), &root, true), Some(Sound::Back));
        assert_eq!(launcher_cue(Key::Other, &Out::None, &root, &mark(None, "f", 0), true), Some(Sound::Type));
        assert_eq!(launcher_cue(Key::Backspace, &Out::None, &mark(None, "f", 3), &mark(None, "", 0), true), Some(Sound::Type));
        assert_eq!(launcher_cue(Key::Return, &Out::Internal("caffeinate.toggle".into()), &root, &root, true), Some(Sound::Toggle));
        assert_eq!(launcher_cue(Key::Return, &Out::Close, &root, &root, false), Some(Sound::Activate));
    }
}
