//! BlueZ client for the shell: adapters, devices, discovery, pairing, an
//! optional pairing agent and remotes' AVRCP players, over zbus on a
//! caller-owned connection.

pub mod agent;
mod client;
mod proxies;
pub mod state;

pub use client::{Bluez, Error, Monitor, PlayerCommand, Result, SMOKE_ENV};
