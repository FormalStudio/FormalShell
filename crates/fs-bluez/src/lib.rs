//! BlueZ client for the shell: adapters, devices, discovery, pairing, an
//! optional pairing agent and remotes' AVRCP players, over zbus on a
//! caller-owned connection.

pub mod agent;
mod client;
mod proxies;
pub mod state;

pub use client::{AgentSession, Bluez, Error, Monitor, PlayerCommand, Result};
use fs_types::BtAddress;

/// Whether `path` is `address`'s `Device1` object, `.../dev_AA_BB_CC_DD_EE_FF`.
pub fn is_device_path(path: &str, address: BtAddress) -> bool {
    path.ends_with(&format!("/dev_{}", address.path_segment()))
}

/// Whether `node` (a PipeWire node.name) is the stream `address` plays
/// into: BlueZ's PipeWire plugin names it
/// `bluez_input.<address, colons as underscores>.<profile>`. The address is
/// the device's identity one; the object path can carry a random resolvable
/// address instead.
pub fn is_input_node(node: &str, address: BtAddress) -> bool {
    let want = format!("bluez_input.{}", address.path_segment());
    node.get(..want.len()).is_some_and(|n| n.eq_ignore_ascii_case(&want))
}
