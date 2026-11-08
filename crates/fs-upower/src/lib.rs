//! UPower and power-profiles-daemon over zbus.
//!
//! Everything takes a [`zbus::Connection`] the caller owns, so the crate runs
//! on whatever executor drives that connection. Nothing here spawns.

mod device;
mod profiles;
mod upower;

pub use device::{Device, DeviceKind, DeviceState};
pub use profiles::{PowerProfiles, Profile, ProfilesState};
pub use upower::{Snapshot, UPower};
