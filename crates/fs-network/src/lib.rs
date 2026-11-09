//! NetworkManager over zbus.
//!
//! `nmrs` was evaluated and not used: it calls `tokio::spawn` for its
//! monitors, depends on tokio's multi-threaded runtime and `tokio::process`,
//! and panics when polled outside a tokio runtime. The shell's service
//! thread is a single-threaded async-executor, so this crate speaks the
//! NetworkManager D-Bus API itself.
//!
//! Everything takes a [`zbus::Connection`] the caller owns, so the crate runs
//! on whatever executor drives that connection. Nothing here spawns.

mod manager;
mod model;
mod settings;

pub use manager::{ConnectError, NetworkManager};
pub use model::{
    ConnectionState, Device, DeviceKind, FailReason, SavedSecret, Security, SignalStrength,
    Snapshot, WepKeyType, WifiNetwork,
};
