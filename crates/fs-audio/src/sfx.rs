//! The interface blips: nine short WAVs `dev/sfx/gen.py` synthesises into
//! `sfx/`, embedded here, and the mixer the client thread's playback stream
//! pulls frames from. Pure, so it builds and tests without PipeWire.

use std::sync::LazyLock;
use std::time::Duration;

use crate::linear_from_visual;

/// Every sound plays at this rate, mono, the format the stream declares.
pub const RATE: u32 = 48_000;

/// The stream's `node.name`. The client thread leaves its own node and its
/// links out of the graph it reports, so no panel lists it.
pub const NODE_NAME: &str = "formalshell-sfx";

/// How long a stream with nothing left to play stays up before it goes, so
/// a burst of blips shares one stream and a quiet shell holds none.
pub const LINGER: Duration = Duration::from_millis(1000);

/// A blip asked for longer ago than this is dropped rather than played
/// late, as one queued while PipeWire was down would be.
pub const STALE: Duration = Duration::from_millis(250);

/// The table's events, each also the name of the sound a table maps it to
/// by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sound {
    Move,
    Activate,
    Back,
    Open,
    Close,
    Type,
    Toggle,
    Error,
    Boot,
}

impl Sound {
    pub const ALL: [Sound; 9] = [
        Sound::Move,
        Sound::Activate,
        Sound::Back,
        Sound::Open,
        Sound::Close,
        Sound::Type,
        Sound::Toggle,
        Sound::Error,
        Sound::Boot,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Sound::Move => "move",
            Sound::Activate => "activate",
            Sound::Back => "back",
            Sound::Open => "open",
            Sound::Close => "close",
            Sound::Type => "type",
            Sound::Toggle => "toggle",
            Sound::Error => "error",
            Sound::Boot => "boot",
        }
    }

    pub fn from_name(name: &str) -> Option<Sound> {
        Sound::ALL.into_iter().find(|s| s.name() == name)
    }

    fn wav(self) -> &'static [u8] {
        match self {
            Sound::Move => include_bytes!("../sfx/move.wav"),
            Sound::Activate => include_bytes!("../sfx/activate.wav"),
            Sound::Back => include_bytes!("../sfx/back.wav"),
            Sound::Open => include_bytes!("../sfx/open.wav"),
            Sound::Close => include_bytes!("../sfx/close.wav"),
            Sound::Type => include_bytes!("../sfx/type.wav"),
            Sound::Toggle => include_bytes!("../sfx/toggle.wav"),
            Sound::Error => include_bytes!("../sfx/error.wav"),
            Sound::Boot => include_bytes!("../sfx/boot.wav"),
        }
    }

    /// The decoded samples, decoded once on first use.
    pub fn pcm(self) -> &'static [i16] {
        static PCM: LazyLock<Vec<Vec<i16>>> = LazyLock::new(|| {
            Sound::ALL.iter().map(|s| decode_wav(s.wav()).expect("dev/sfx/gen.py writes 48 kHz mono s16")).collect()
        });
        &PCM[self as usize]
    }
}

/// `theme.soundVolume`: the visual scale every other volume in the shell
/// uses (what `wpctl get-volume` prints), 0..1. The mixer multiplies by
/// [`Volume::linear`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume(f32);

impl Volume {
    pub const DEFAULT: Volume = Volume(0.5);

    /// Clamped into 0..1; a NaN reads as silence.
    pub fn from_visual(v: f64) -> Volume {
        Volume(if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) as f32 })
    }

    pub fn as_visual(self) -> f64 {
        f64::from(self.0)
    }

    pub fn linear(self) -> f32 {
        linear_from_visual(self.0)
    }
}

/// A RIFF/WAVE file of 16-bit PCM, mono, at [`RATE`]; `None` for anything
/// else.
pub fn decode_wav(bytes: &[u8]) -> Option<Vec<i16>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut at = 12;
    let mut format = false;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32_at(bytes, at + 4) as usize;
        let body = bytes.get(at + 8..at + 8 + len)?;
        match id {
            b"fmt " if len >= 16 => {
                let (tag, channels, rate, bits) = (u16_at(body, 0), u16_at(body, 2), u32_at(body, 4), u16_at(body, 14));
                if (tag, channels, rate, bits) != (1, 1, RATE, 16) {
                    return None;
                }
                format = true;
            }
            b"data" if format => {
                return Some(body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect());
            }
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    None
}

struct Voice {
    pcm: &'static [i16],
    at: usize,
    gain: f32,
}

/// The blips playing now, summed into whatever the stream asks for.
#[derive(Default)]
pub struct Mixer {
    voices: Vec<Voice>,
    /// Frames written since the last voice ended.
    silent: usize,
}

impl Mixer {
    pub fn play(&mut self, sound: Sound, volume: Volume) {
        let gain = volume.linear();
        if gain > 0.0 {
            self.voices.push(Voice { pcm: sound.pcm(), at: 0, gain });
            self.silent = 0;
        }
    }

    pub fn playing(&self) -> bool {
        !self.voices.is_empty()
    }

    /// Nothing has played for [`LINGER`]: the stream can go.
    pub fn idle(&self) -> bool {
        self.voices.is_empty() && self.silent as u128 >= LINGER.as_millis() * u128::from(RATE) / 1000
    }

    /// Fills `out` with the next frames, silence past the last voice.
    pub fn fill(&mut self, out: &mut [i16]) {
        let mut mix = vec![0.0f32; out.len()];
        for v in &mut self.voices {
            let n = (v.pcm.len() - v.at).min(out.len());
            for (m, s) in mix.iter_mut().zip(&v.pcm[v.at..v.at + n]) {
                *m += f32::from(*s) * v.gain;
            }
            v.at += n;
        }
        self.voices.retain(|v| v.at < v.pcm.len());
        for (o, m) in out.iter_mut().zip(mix) {
            *o = m.clamp(-32768.0, 32767.0) as i16;
        }
        if self.voices.is_empty() {
            self.silent += out.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_decodes_and_is_short() {
        for s in Sound::ALL {
            let pcm = s.pcm();
            assert!(!pcm.is_empty(), "{}", s.name());
            assert!(pcm.len() < RATE as usize / 2, "{} is {} frames", s.name(), pcm.len());
            assert!(pcm.iter().any(|v| v.unsigned_abs() > 1000), "{} is silent", s.name());
            assert_eq!(Sound::from_name(s.name()), Some(s));
        }
    }

    #[test]
    fn a_wav_in_any_other_format_is_refused() {
        let mut stereo = Sound::Move.wav().to_vec();
        stereo[22] = 2;
        assert!(decode_wav(&stereo).is_none());
        assert!(decode_wav(b"RIFF\0\0\0\0WAVE").is_none());
    }

    #[test]
    fn the_volume_is_the_visual_scale() {
        assert_eq!(Volume::DEFAULT.linear(), 0.125);
        assert_eq!(Volume::from_visual(3.0).as_visual(), 1.0);
        assert_eq!(Volume::from_visual(f64::NAN).linear(), 0.0);
    }

    #[test]
    fn the_mixer_plays_a_sound_through_then_lingers() {
        let mut m = Mixer::default();
        m.play(Sound::Type, Volume::from_visual(1.0));
        let len = Sound::Type.pcm().len();
        let mut out = vec![0i16; len + 10];
        m.fill(&mut out);
        assert_eq!(&out[..len], Sound::Type.pcm());
        assert!(out[len..].iter().all(|v| *v == 0));
        assert!(!m.playing() && !m.idle());
        let mut rest = vec![0i16; RATE as usize];
        m.fill(&mut rest);
        assert!(m.idle());
    }

    #[test]
    fn overlapping_voices_sum_and_clip() {
        let mut m = Mixer::default();
        for _ in 0..40 {
            m.play(Sound::Activate, Volume::from_visual(1.0));
        }
        let mut out = vec![0i16; 4096];
        m.fill(&mut out);
        assert!(out.iter().any(|v| *v == 32767 || *v == -32768));
    }

    #[test]
    fn silence_plays_nothing() {
        let mut m = Mixer::default();
        m.play(Sound::Boot, Volume::from_visual(0.0));
        assert!(!m.playing());
    }
}
