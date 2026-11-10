//! Model groundwork for the audio panel's mixer: stream filtering, the label
//! fallback chain and the mic cell's state.

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
