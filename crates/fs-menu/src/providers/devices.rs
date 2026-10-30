//! The device routes: Wi-Fi, Bluetooth, audio and radio. Each takes the live
//! source's state as plain data, so the ordering, the ticks and the per-row
//! status text are testable without a radio.

use std::collections::HashSet;

use fs_devices::bluetooth::{Device, activity_text, battery_text, buckets};
use fs_devices::network::model::{WifiRowLike, sort_wifi_rows};

use crate::node::{ActionEntry, Kind, Node};

/// Row ids are dotted tree paths, so a key that can carry a dot (an SSID, a
/// PipeWire node name) is escaped before it goes into one.
pub fn id_part(s: &str) -> String {
    fs_js::encode_uri_component(s).replace('.', "%2E")
}

fn key_row(id: String, label: String, action: &str, verb: &str) -> Node {
    Node {
        action: Some(action.to_string()),
        verb: Some(verb.to_string()),
        keep_open: Some(true),
        ..Node::new(id, label, Kind::Action)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiNetwork {
    pub name: String,
    pub known: bool,
    pub connected: bool,
    pub secured: bool,
    pub enterprise: bool,
    /// An access point broadcasts it; false only for a saved network.
    pub in_range: bool,
    pub signal: f64,
    /// Overrides `signal` when the source carries its own 0..1 fraction.
    pub signal_strength: Option<f64>,
}

/// `LiveMenuSources.wifi`: the device and radio flags, the scanned networks,
/// and the Wi-Fi service's action in flight and last failure.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiState {
    pub has_device: bool,
    pub enabled: bool,
    pub networks: Vec<WifiNetwork>,
    pub action_ssid: String,
    pub action_kind: String,
    pub failure_ssid: String,
    pub failure_text: String,
}

impl WifiRowLike for WifiNetwork {
    fn connected(&self) -> bool {
        self.connected
    }

    fn known(&self) -> bool {
        self.known
    }

    fn signal_strength(&self) -> f64 {
        self.signal_strength.unwrap_or(self.signal)
    }
}

/// Wi-Fi route. The level lists networks in range; saved ones out of range
/// sit one level down under "Known networks". Saved networks are reachable
/// from a root query; nearby ones are `local_only`, so a stranger's SSID
/// never turns up in a search typed outside the level. Enter on a secured
/// unknown network asks for a password, Shift+Enter on a saved one forgets it.
pub fn wifi_rows(st: &WifiState) -> Vec<Node> {
    if !st.has_device {
        return vec![Node::note("wifi.unavailable", "No Wi-Fi device")];
    }
    if !st.enabled {
        return vec![key_row("wifi.off".into(), "Turn Wi-Fi on".into(), "@ipc:wifi.enable", "Turn on")];
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let named: Vec<WifiNetwork> =
        st.networks.iter().filter(|n| !n.name.is_empty() && seen.insert(n.name.as_str())).cloned().collect();
    let (near, mut away): (Vec<WifiNetwork>, Vec<WifiNetwork>) = named.into_iter().partition(|n| n.in_range || n.connected);
    let mut rows: Vec<Node> = sort_wifi_rows(&near).into_iter().map(|n| wifi_row(st, n)).collect();
    if rows.is_empty() {
        rows.push(Node::note("wifi.empty", "No networks found"));
    }
    if !away.is_empty() {
        away.sort_by(|a, b| a.name.cmp(&b.name));
        let known: Vec<Node> =
            away.into_iter().map(|n| Node { parent_id: Some(KNOWN_ID.to_string()), ..wifi_row(st, n) }).collect();
        rows.push(Node {
            child_ids: known.iter().map(|n| n.id.clone()).collect(),
            ..Node::new(KNOWN_ID, "Known networks", Kind::Provider)
        });
        rows.extend(known);
    }
    rows
}

/// The level saved networks out of range sit in, under the Wi-Fi route. A
/// row carrying this as its `parent_id` stays under it when the route's rows
/// are attached.
const KNOWN_ID: &str = "wifi.known";

fn wifi_row(st: &WifiState, n: WifiNetwork) -> Node {
    let busy = match st.action_kind.as_str() {
        "connect" => "Connecting",
        "disconnect" => "Disconnecting",
        "forget" => "Forgetting",
        _ => "",
    };
    let desc = if !st.action_kind.is_empty() && st.action_ssid == n.name {
        busy
    } else if st.failure_ssid == n.name && !st.failure_text.is_empty() {
        st.failure_text.as_str()
    } else if !n.known {
        if n.enterprise {
            "Enterprise"
        } else if n.secured {
            "Secured"
        } else {
            ""
        }
    } else {
        ""
    };
    let mut row = Node {
        desc: Some(desc.to_string()),
        verb: Some(if n.connected { "Disconnect" } else { "Connect" }.to_string()),
        action: Some(format!("@ipc:wifi.activate:{}", n.name)),
        checked: Some(format!("@state:wifi.ssid={}", n.name)),
        keep_open: Some(true),
        local_only: !n.known,
        section: Some(if n.known { "Saved" } else { "Nearby" }.to_string()),
        actions: vec![ActionEntry::new("panel", "Open network panel", "@ipc:panel.open:network")],
        ..Node::new(format!("wifi.net.{}", id_part(&n.name)), n.name.clone(), Kind::Action)
    };
    if n.known {
        row.alternate = Some(format!("@ipc:wifi.forget:{}", n.name));
        row.alternate_label = Some("Forget".to_string());
    }
    row
}

/// `LiveMenuSources.bluetooth`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BluetoothState {
    pub available: bool,
    pub enabled: bool,
    pub devices: Vec<Device>,
}

fn device_label(d: &Device) -> &str {
    if !d.name.is_empty() { &d.name } else { &d.device_name }
}

/// Bluetooth route. Paired devices only: pairing is watching a scan fill in,
/// which stays the panel's job. Rows are reachable from a root query.
pub fn bluetooth_rows(st: &BluetoothState) -> Vec<Node> {
    if !st.available {
        return vec![Node::note("bluetooth.unavailable", "No Bluetooth adapter")];
    }
    if !st.enabled {
        return vec![key_row("bluetooth.off".into(), "Turn Bluetooth on".into(), "@ipc:bluetooth.power:on", "Turn on")];
    }
    let b = buckets(&st.devices, false);
    let devices: Vec<&Device> = b.connected.into_iter().chain(b.known).collect();
    if devices.is_empty() {
        return vec![Node::note("bluetooth.empty", "No paired devices")];
    }
    devices
        .into_iter()
        .map(|d| Node {
            desc: Some(match activity_text(Some(d)) {
                "" => battery_text(Some(d)),
                activity => activity.to_string(),
            }),
            verb: Some(if d.connected { "Disconnect" } else { "Connect" }.to_string()),
            action: Some(format!("@ipc:bluetooth.toggle:{}", d.address)),
            checked: Some(format!("@state:bluetooth.connected={}", d.address.to_uppercase())),
            keep_open: Some(true),
            actions: vec![
                ActionEntry { keep_open: Some(true), ..ActionEntry::new("forget", "Forget", format!("@ipc:bluetooth.forget:{}", d.address)).destructive() },
                ActionEntry::new("panel", "Open Bluetooth panel", "@ipc:panel.open:bluetooth"),
            ],
            ..Node::new(format!("bluetooth.dev.{}", id_part(&d.address)), device_label(d), Kind::Action)
        })
        .collect()
}

/// One entry of the audio model's device rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioDevice {
    pub name: String,
    pub label: String,
    pub is_sink: bool,
}

/// Audio route.
pub fn audio_rows(devices: &[AudioDevice]) -> Vec<Node> {
    if devices.is_empty() {
        return vec![Node::note("audio.unavailable", "No audio devices")];
    }
    devices
        .iter()
        .map(|d| {
            let verb = if d.is_sink { "sink" } else { "source" };
            Node {
                section: Some(if d.is_sink { "Output" } else { "Input" }.to_string()),
                action: Some(format!("@ipc:audio.{verb}:{}", d.name)),
                verb: Some("Use".to_string()),
                checked: Some(format!("@state:audio.{verb}={}", d.name)),
                keep_open: Some(true),
                actions: vec![ActionEntry::new("panel", "Open audio panel", "@ipc:panel.open:audio")],
                ..Node::new(
                    format!("audio.{}{}", if d.is_sink { "output." } else { "input." }, id_part(&d.name)),
                    d.label.clone(),
                    Kind::Action,
                )
            }
        })
        .collect()
}

/// `":r"` root trigger: the station query after it ("" for the bare `":r"`),
/// or `None` when `text` is not the trigger.
pub fn radio_trigger_query(text: &str) -> Option<&str> {
    if text == ":r" {
        return Some("");
    }
    text.strip_prefix(":r ")
}

fn radio_tick(uuid: &str) -> String {
    format!("@state:radio.station={uuid}")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioStation {
    pub uuid: String,
    pub name: String,
    pub country: String,
    pub codec: String,
}

/// `LiveMenuSources.radio`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioState {
    pub running: bool,
    pub favorites: Vec<RadioStation>,
}

/// Radio route. Favourites are reachable from a root query; station search
/// results come from `radio_result_rows` and are `local_only`.
pub fn radio_rows(st: &RadioState) -> Vec<Node> {
    let mut rows = Vec::new();
    if st.running {
        rows.push(key_row("radio.stop".into(), "Stop".into(), "@ipc:radio.stop", "Stop"));
    }
    for s in &st.favorites {
        rows.push(Node {
            desc: Some(s.country.clone()),
            action: Some(format!("@ipc:radio.fav:{}", s.uuid)),
            verb: Some("Play".to_string()),
            checked: Some(radio_tick(&s.uuid)),
            keep_open: Some(true),
            alternate: Some(format!("@ipc:radio.unfavorite:{}", s.uuid)),
            alternate_label: Some("Remove favorite".to_string()),
            ..Node::new(format!("radio.fav.{}", id_part(&s.uuid)), s.name.clone(), Kind::Action)
        });
    }
    rows
}

/// Enter closes the launcher on a result: a stream starting is the answer, and
/// the list is a one-off search rather than a set to flip through.
pub fn radio_result_rows(results: &[RadioStation], favorite_set: &HashSet<String>) -> Vec<Node> {
    results
        .iter()
        .map(|s| {
            let fav = favorite_set.contains(&s.uuid);
            let desc = [s.country.as_str(), s.codec.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ");
            Node {
                desc: Some(desc),
                action: Some(format!("@ipc:radio.play:{}", s.uuid)),
                verb: Some("Play".to_string()),
                checked: Some(radio_tick(&s.uuid)),
                local_only: true,
                alternate: Some(format!("@ipc:radio.{}:{}", if fav { "unfavorite" } else { "favorite" }, s.uuid)),
                alternate_label: Some(if fav { "Remove favorite" } else { "Add favorite" }.to_string()),
                ..Node::new(format!("radio.result.{}", id_part(&s.uuid)), s.name.clone(), Kind::Action)
            }
        })
        .collect()
}

pub fn radio_searching_row() -> Node {
    Node::note("radio.searching", "Searching")
}

pub fn radio_no_results_row() -> Node {
    Node::note("radio.noresults", "No results")
}

pub fn radio_failed_row() -> Node {
    Node::note("radio.failed", "Search failed")
}
