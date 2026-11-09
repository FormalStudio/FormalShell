use serde::{Deserialize, Serialize};

/// How loudly a notification asks for attention: freedesktop's `urgency`
/// hint (0 low, 1 normal, 2 critical), which is also what state.json keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl From<Urgency> for u8 {
    fn from(u: Urgency) -> u8 {
        match u {
            Urgency::Low => 0,
            Urgency::Normal => 1,
            Urgency::Critical => 2,
        }
    }
}

impl TryFrom<u8> for Urgency {
    type Error = String;

    fn try_from(n: u8) -> Result<Self, String> {
        match n {
            0 => Ok(Urgency::Low),
            1 => Ok(Urgency::Normal),
            2 => Ok(Urgency::Critical),
            _ => Err(format!("urgency {n} out of range")),
        }
    }
}
