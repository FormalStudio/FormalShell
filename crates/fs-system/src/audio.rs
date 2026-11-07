//! Model groundwork for the audio panel's mixer: stream filtering, the label
//! fallback chain, and the two volume clamps (device vs per-app overdrive).


fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() { v } else { lo.max(hi.min(v)) }
}

/// Master output/input sliders clamp to 1.0 (a flat track never overdrives past
/// full); per-app streams allow 0..1.5 overdrive, per omarchy's mixer behavior.
pub fn clamp_device(v: f64) -> f64 {
    clamp(v, 0.0, 1.0)
}

pub fn clamp_stream(v: f64) -> f64 {
    clamp(v, 0.0, 1.5)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    Unavailable,
    Muted,
    Live,
}

impl SourceState {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceState::Unavailable => "unavailable",
            SourceState::Muted => "muted",
            SourceState::Live => "live",
        }
    }
}

/// The single place the mic cell's honest-unavailable branch is decided, so the
/// widget carries no state logic of its own. `available` is whether the default
/// audio source has an audio interface; "unavailable" is a real answer on any
/// host with no capture device, never a stubbed 0%.
pub fn source_state(available: bool, muted: bool) -> SourceState {
    if !available {
        SourceState::Unavailable
    } else if muted {
        SourceState::Muted
    } else {
        SourceState::Live
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // isPlaybackStream

    // streamLabel

    // clampDevice / clampStream

    #[test]
    fn clamp_device_caps_at_one() {
        assert_eq!(clamp_device(1.5), 1.0);
    }

    #[test]
    fn clamp_device_floors_at_zero() {
        assert_eq!(clamp_device(-0.2), 0.0);
    }

    #[test]
    fn clamp_device_passes_through_mid_range() {
        assert_eq!(clamp_device(0.42), 0.42);
    }

    #[test]
    fn clamp_stream_allows_overdrive_up_to_one_point_five() {
        assert_eq!(clamp_stream(1.5), 1.5);
        assert_eq!(clamp_stream(1.3), 1.3);
    }

    #[test]
    fn clamp_stream_caps_above_one_point_five() {
        assert_eq!(clamp_stream(2.0), 1.5);
    }

    #[test]
    fn clamp_stream_floors_at_zero() {
        assert_eq!(clamp_stream(-0.5), 0.0);
    }

    // sourceState

    #[test]
    fn source_state_unavailable_without_input_device() {
        assert_eq!(source_state(false, false).as_str(), "unavailable");
        assert_eq!(source_state(false, true).as_str(), "unavailable");
    }

    #[test]
    fn source_state_muted_when_available_and_muted() {
        assert_eq!(source_state(true, true).as_str(), "muted");
    }

    #[test]
    fn source_state_live_when_available_and_unmuted() {
        assert_eq!(source_state(true, false).as_str(), "live");
    }

    // deviceRows

}
