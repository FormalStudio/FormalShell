//! The Bluetooth audio devices the headset connect card watches: BlueZ's
//! devices whose icon says audio.

use fs_types::{BtAddress, Fraction};

#[derive(Clone, Debug, PartialEq)]
pub struct Headset {
    pub address: BtAddress,
    pub name: String,
    /// BlueZ's `Icon`: `audio-headset`, `audio-headphones`, `audio-speakers`.
    pub icon: String,
    pub connected: bool,
    /// BlueZ's Battery1; none when nothing reports one.
    pub battery: Option<Fraction>,
}

pub fn read(state: &fs_bluez::state::State) -> Vec<Headset> {
    let Some(adapter) = state.default_adapter() else { return Vec::new() };
    state
        .devices_of(&adapter.path)
        .into_iter()
        .filter(|d| d.icon.starts_with("audio-"))
        .map(|d| Headset {
            address: d.info.address,
            name: if d.info.name.is_empty() { d.info.device_name.clone() } else { d.info.name.clone() },
            icon: d.icon.clone(),
            connected: d.info.connected,
            battery: d.info.battery,
        })
        .collect()
}
