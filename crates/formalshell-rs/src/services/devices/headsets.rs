//! The Bluetooth audio devices the headset connect card watches: BlueZ's
//! devices whose icon says audio, and which device each one is once the
//! earbuds backend and state.json have their say.

use fs_devices::earbuds::Device;
use fs_types::{BtAddress, Fraction};

use super::earbuds;
use crate::services::state;
use crate::store::Store;

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
            address: d.address,
            name: d.label().to_owned(),
            icon: d.icon.clone(),
            connected: d.connected,
            battery: d.battery,
        })
        .collect()
}

/// Who the device at `address` is: BlueZ's headset, the earbuds backend's
/// device that claims it, and the name and picture set to show, the
/// backend's own else what state.json remembers it called the device last.
pub struct Identity<'a> {
    pub seen: Option<&'a Headset>,
    pub claimed: Option<&'a Device>,
    /// The backend's name, the one remembered, BlueZ's, else the address.
    pub name: String,
    /// The backend's picture set, else the one remembered; "" for none.
    pub art: String,
}

/// Whether the backend's device `d`, which reports no address (the AirPods
/// daemon's status file carries none), is BlueZ's `h`, by name.
fn named(d: &Device, h: &Headset) -> bool {
    d.address.is_none() && !h.name.is_empty() && h.name.eq_ignore_ascii_case(&d.name)
}

pub fn identity(store: &Store, address: BtAddress) -> Identity<'_> {
    let seen = store.bluetooth.headsets.as_ref().and_then(|list| list.iter().find(|h| h.address == address));
    let claimed = store.earbuds.devices().iter().find(|d| d.address == Some(address) || (d.connected && seen.is_some_and(|h| named(d, h))));
    // BlueZ holds the name the device advertises, which some cut short
    // ("CMF Head" for a CMF Headphone Pro), and the backend's own comes up
    // to ~40s after connecting, so the one it gave last stands in until then.
    let name = claimed
        .map(|d| d.name.as_str())
        .filter(|n| !n.is_empty())
        .or_else(|| remembered(store, address, |f| f.name.as_deref()))
        .or_else(|| seen.map(|h| h.name.as_str()).filter(|n| !n.is_empty()))
        .map_or_else(|| address.to_string(), str::to_owned);
    // nothingctl takes up to ~40s to name a headphone it has just seen
    // connect, far past the card's hold, so the set it named last time
    // stands in until it does.
    let art = claimed
        .map(|d| d.art.clone())
        .filter(|a| !a.is_empty())
        .or_else(|| remembered(store, address, |f| f.art.as_deref()).map(str::to_owned))
        .unwrap_or_default();
    Identity { seen, claimed, name, art }
}

/// What the earbuds backend last called `address` (`field` the name) or the
/// picture set it named (the art).
fn remembered(store: &Store, address: BtAddress, field: fn(&earbuds::Face) -> Option<&str>) -> Option<&str> {
    store.state.data.earbuds_faces.get(&address.to_string()).and_then(field).filter(|s| !s.is_empty())
}

/// What the earbuds backend reports now that state.json does not hold yet,
/// written there by address: a device's own address, or for one with none
/// the connected Bluetooth headset of its name. A set it has not named yet
/// keeps the one remembered.
pub fn learn(store: &Store) {
    let headsets = store.bluetooth.headsets.as_deref().unwrap_or_default();
    let learned: Vec<(String, earbuds::Face)> = store
        .earbuds
        .devices()
        .iter()
        .filter(|d| !d.art.is_empty() || !d.name.is_empty())
        .filter_map(|d| {
            let address = match d.address {
                Some(address) => address,
                None => headsets.iter().find(|h| h.connected && named(d, h))?.address,
            };
            let art = if d.art.is_empty() { remembered(store, address, |f| f.art.as_deref()).unwrap_or_default() } else { d.art.as_str() };
            let key = address.to_string();
            let held = store.state.data.earbuds_faces.get(&key);
            let entry = earbuds::Face { art: Some(art.to_owned()), name: Some(d.name.clone()), ..held.cloned().unwrap_or_default() };
            (held != Some(&entry)).then_some((key, entry))
        })
        .collect();
    if learned.is_empty() {
        return;
    }
    let mut faces = store.state.data.earbuds_faces.clone();
    faces.extend(learned);
    state::set(vec![state::Field::EarbudsFaces(faces)]);
}
