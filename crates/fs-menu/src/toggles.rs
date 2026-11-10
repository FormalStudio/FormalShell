//! Live `checked` conditions for the launcher's toggle rows. A toggle row's
//! checkmark has to flip in the same event-loop turn the toggle itself does,
//! so a `@state:` condition is answered from a typed snapshot the caller
//! hands in and never spawns a process.
//!
//! The paths are the snapshot's fields and nothing else: a condition naming
//! any other path resolves false, so a hand-written `menu.jsonc` has no
//! route into the engine through this field.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

use crate::node::Node;

pub const PREFIX: &str = "@state:";

/// What ticks a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Ticked whatever the state: an installed package's row.
    Always,
    /// `@state:<path>`, a flag of the snapshot, or `@state:<path>=<value>`,
    /// one of its values or a member of one of its lists.
    State { path: String, value: Option<String> },
    /// A command line, ticked while it exits 0, answered off the cache the
    /// condition poll fills.
    Shell(String),
}

impl Checked {
    pub fn parse(text: &str) -> Checked {
        match text.strip_prefix(PREFIX) {
            Some(rest) => match rest.split_once('=') {
                Some((path, value)) => Checked::State { path: path.to_owned(), value: Some(value.to_owned()) },
                None => Checked::State { path: rest.to_owned(), value: None },
            },
            None => Checked::Shell(text.to_owned()),
        }
    }

    pub fn flag(path: &str) -> Checked {
        Checked::State { path: path.to_owned(), value: None }
    }

    pub fn value(path: &str, value: impl Into<String>) -> Checked {
        Checked::State { path: path.to_owned(), value: Some(value.into()) }
    }

    /// Off the snapshot, `None` for a shell line, which the cache answers.
    pub fn resolve(&self, snap: Option<&Snapshot>) -> Option<bool> {
        let empty = Snapshot::default();
        let snap = snap.unwrap_or(&empty);
        match self {
            Checked::Always => Some(true),
            Checked::Shell(_) => None,
            Checked::State { path, value: None } => Some(snap.flag(path) == Some(true)),
            Checked::State { value: Some(v), .. } if v.is_empty() => Some(false),
            Checked::State { path, value: Some(v) } => Some(match (snap.text(path), snap.list(path)) {
                (Some(t), _) => t == v,
                (_, Some(l)) => l.iter().any(|x| x == v),
                _ => false,
            }),
        }
    }
}

impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Checked, D::Error> {
        Ok(Checked::parse(&String::deserialize(d)?))
    }
}

/// The state a `@state:` condition reads, taken whole on every resolve.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub nightlight: bool,
    pub overnight: bool,
    pub caffeinate: bool,
    pub dnd: bool,
    pub dark: bool,
    pub lights_on: bool,
    pub hdr: bool,
    pub ipad_active: bool,
    pub lights_effect: String,
    pub lights_source: String,
    pub lights_colour: String,
    pub lights_speed: String,
    pub lights_brightness: String,
    pub wifi_ssid: String,
    pub audio_sink: String,
    pub audio_source: String,
    pub radio_station: String,
    pub ipad_edge: String,
    pub ipad_stretch: String,
    pub bluetooth_connected: Vec<String>,
}

impl Snapshot {
    /// The flag at `path`, `None` for a path that is no flag.
    pub fn flag(&self, path: &str) -> Option<bool> {
        Some(match path {
            "nightlight.active" => self.nightlight,
            "overnight.active" => self.overnight,
            "caffeinate.active" => self.caffeinate,
            "notifications.dnd" => self.dnd,
            "theme.dark" => self.dark,
            "lights.on" => self.lights_on,
            "hdr.active" => self.hdr,
            "ipad.active" => self.ipad_active,
            _ => return None,
        })
    }

    /// The value at `path`, `None` for a path that holds no value.
    pub fn text(&self, path: &str) -> Option<&str> {
        Some(match path {
            "lights.effect" => &self.lights_effect,
            "lights.source" => &self.lights_source,
            "lights.colour" => &self.lights_colour,
            "lights.speed" => &self.lights_speed,
            "lights.brightness" => &self.lights_brightness,
            "wifi.ssid" => &self.wifi_ssid,
            "audio.sink" => &self.audio_sink,
            "audio.source" => &self.audio_source,
            "radio.station" => &self.radio_station,
            "ipad.edge" => &self.ipad_edge,
            "ipad.stretch" => &self.ipad_stretch,
            _ => return None,
        })
    }

    /// The list at `path`, `None` for a path that holds no list.
    pub fn list(&self, path: &str) -> Option<&[String]> {
        match path {
            "bluetooth.connected" => Some(&self.bluetooth_connected),
            _ => None,
        }
    }
}

/// The single decision the row delegate makes: live state wins outright, so a
/// stale cached process result can never paint over the current one.
pub fn checked_for(node: Option<&Node>, snap: Option<&Snapshot>, cond_results: Option<&HashMap<String, bool>>) -> bool {
    let Some(node) = node else { return false };
    let Some(checked) = &node.checked else { return false };
    if let Some(live) = checked.resolve(snap) {
        return live;
    }
    cond_results.is_some_and(|c| c.get(&node.id) == Some(&true))
}

/// A fresh map with `id` set, leaving `source` alone.
pub fn with_result(source: &HashMap<String, bool>, id: &str, ok: bool) -> HashMap<String, bool> {
    let mut out = source.clone();
    out.insert(id.to_string(), ok);
    out
}
