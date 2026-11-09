//! What the bar's device cells read: the default sink and source
//! (PipeWire), the default Bluetooth adapter (fs-bluez), NetworkManager's
//! devices, UPower's battery and controllers, and every connected pair of
//! earbuds. Each is a slice of its own in the store and answers its own
//! honest unavailable state when the daemon behind it is not there.

mod action;
pub mod audio;
pub mod bluetooth;
pub mod earbuds;
pub mod headsets;
pub mod network;
pub mod power;

pub use audio::Audio;
pub use bluetooth::Bluetooth;
pub use network::Network;
pub use power::Power;
