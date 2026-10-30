// The device routes' pure half: the verb each row carries is checked here.

use std::collections::HashSet;

use fs_devices::bluetooth::{Device as BluetoothDevice, DeviceState};
use fs_menu::node::{Kind, Node};
use fs_menu::providers::{
    AudioDevice, BluetoothState, RadioState, RadioStation, WifiNetwork, WifiState, audio_rows,
    bluetooth_rows, id_part, radio_result_rows, radio_rows, wifi_rows,
};

#[test]
fn id_part_round_trips_and_has_no_dot() {
    for key in ["a.b", "a:b", "a b", "\u{fc}", "plain"] {
        let part = id_part(key);
        assert!(!part.contains('.'), "{key}");
        let decoded = percent_encoding::percent_decode_str(&part).decode_utf8().unwrap();
        assert_eq!(decoded, key);
    }
}

fn net(name: &str) -> WifiNetwork {
    WifiNetwork { name: name.into(), secured: true, in_range: true, signal: 0.5, ..WifiNetwork::default() }
}

fn wifi(networks: Vec<WifiNetwork>) -> Vec<Node> {
    wifi_rows(&WifiState { has_device: true, enabled: true, networks, ..WifiState::default() })
}

fn wifi_with(networks: Vec<WifiNetwork>, f: impl FnOnce(&mut WifiState)) -> Vec<Node> {
    let mut st = WifiState { has_device: true, enabled: true, networks, ..WifiState::default() };
    f(&mut st);
    wifi_rows(&st)
}

fn desc_of<'a>(rows: &'a [Node], name: &str) -> &'a str {
    rows.iter().find(|r| r.label == name).unwrap().desc.as_deref().unwrap()
}

fn labels(rows: &[Node]) -> Vec<&str> {
    rows.iter().map(|r| r.label.as_str()).collect()
}

#[test]
fn wifi_no_device_is_one_note() {
    let rows = wifi_rows(&WifiState::default());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "wifi.unavailable");
    assert_eq!(rows[0].kind, Kind::Note);
}

#[test]
fn wifi_off_offers_to_turn_on() {
    let rows = wifi_rows(&WifiState { has_device: true, ..WifiState::default() });
    assert_eq!(rows[0].id, "wifi.off");
    assert_eq!(rows[0].action.as_deref(), Some("@ipc:wifi.enable"));
}

#[test]
fn wifi_no_networks_is_one_note() {
    let rows = wifi(vec![]);
    assert_eq!(rows[0].id, "wifi.empty");
    assert_eq!(rows[0].kind, Kind::Note);
}

#[test]
fn wifi_order_connected_saved_nearby() {
    let rows = wifi(vec![
        WifiNetwork { signal: 0.9, ..net("Near") },
        WifiNetwork { known: true, signal: 0.2, ..net("Saved") },
        WifiNetwork { known: true, connected: true, signal: 0.1, ..net("Live") },
    ]);
    assert_eq!(labels(&rows), ["Live", "Saved", "Near"]);
    let sections: Vec<_> = rows.iter().map(|r| r.section.as_deref()).collect();
    assert_eq!(sections, [Some("Saved"), Some("Saved"), Some("Nearby")]);
}

#[test]
fn wifi_saved_out_of_range_sits_under_known_networks() {
    let away = |name| WifiNetwork { known: true, in_range: false, signal: 0.0, ..net(name) };
    let rows = wifi(vec![away("Hostel"), WifiNetwork { known: true, ..net("Home") }, net("Cafe"), away("Airport")]);
    let top: Vec<&str> = rows.iter().filter(|r| r.parent_id.is_none()).map(|r| r.label.as_str()).collect();
    assert_eq!(top, ["Home", "Cafe", "Known networks"]);
    let known = rows.iter().find(|r| r.id == "wifi.known").unwrap();
    assert_eq!(known.child_ids, ["wifi.net.Airport", "wifi.net.Hostel"]);
    let hostel = rows.iter().find(|r| r.label == "Hostel").unwrap();
    assert_eq!(hostel.parent_id.as_deref(), Some("wifi.known"));
    assert_eq!(hostel.alternate.as_deref(), Some("@ipc:wifi.forget:Hostel"));
}

#[test]
fn wifi_with_only_saved_networks_out_of_range_says_none_found() {
    let rows = wifi(vec![WifiNetwork { known: true, in_range: false, ..net("Hostel") }]);
    assert_eq!(rows[0].id, "wifi.empty");
    assert_eq!(rows[1].id, "wifi.known");
}

#[test]
fn wifi_local_only_on_nearby_rows_only() {
    let rows = wifi(vec![net("Near"), WifiNetwork { known: true, ..net("Saved") }]);
    let local: Vec<&str> = rows.iter().filter(|r| r.local_only).map(|r| r.label.as_str()).collect();
    assert_eq!(local, ["Near"]);
}

#[test]
fn wifi_dotted_ssid_has_one_level() {
    let rows = wifi(vec![WifiNetwork { known: true, ..net("a.b") }]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "wifi.net.a%2Eb");
    assert_eq!(rows[0].id.split('.').count(), 3);
}

#[test]
fn wifi_tick_condition_names_the_ssid() {
    let rows = wifi(vec![WifiNetwork { known: true, ..net("Home") }]);
    assert_eq!(rows[0].checked.as_deref(), Some("@state:wifi.ssid=Home"));
}

#[test]
fn wifi_forget_only_on_saved_rows() {
    let rows = wifi(vec![net("Near"), WifiNetwork { known: true, ..net("Saved") }]);
    let by_name = |n: &str| rows.iter().find(|r| r.label == n).unwrap();
    assert_eq!(by_name("Saved").alternate.as_deref(), Some("@ipc:wifi.forget:Saved"));
    assert_eq!(by_name("Saved").alternate_label.as_deref(), Some("Forget"));
    assert_eq!(by_name("Near").alternate, None);
}

#[test]
fn wifi_desc_for_unknown_networks() {
    let rows = wifi(vec![
        net("Sec"),
        WifiNetwork { enterprise: true, ..net("Corp") },
        WifiNetwork { secured: false, ..net("Open") },
    ]);
    assert_eq!(desc_of(&rows, "Sec"), "Secured");
    assert_eq!(desc_of(&rows, "Corp"), "Enterprise");
    assert_eq!(desc_of(&rows, "Open"), "");
}

#[test]
fn wifi_desc_for_the_action_in_flight() {
    let rows = wifi_with(
        vec![WifiNetwork { known: true, ..net("A") }, WifiNetwork { known: true, ..net("B") }],
        |st| {
            st.action_ssid = "A".into();
            st.action_kind = "forget".into();
        },
    );
    assert_eq!(desc_of(&rows, "A"), "Forgetting");
    assert_eq!(desc_of(&rows, "B"), "");
}

#[test]
fn wifi_failure_text_stays_on_its_own_row() {
    let rows = wifi_with(
        vec![WifiNetwork { known: true, ..net("A") }, WifiNetwork { known: true, ..net("B") }],
        |st| {
            st.failure_ssid = "A".into();
            st.failure_text = "Wrong password".into();
        },
    );
    assert_eq!(desc_of(&rows, "A"), "Wrong password");
    assert_eq!(desc_of(&rows, "B"), "");
}

#[test]
fn wifi_hidden_ssid_is_skipped() {
    let rows = wifi(vec![net(""), net("Real")]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "Real");
}

fn bt(devices: Vec<BluetoothDevice>) -> Vec<Node> {
    bluetooth_rows(&BluetoothState { available: true, enabled: true, devices })
}

fn dev(address: &str, name: &str) -> BluetoothDevice {
    BluetoothDevice { address: address.into(), name: name.into(), paired: true, ..BluetoothDevice::default() }
}

#[test]
fn bluetooth_no_adapter_is_one_note() {
    let rows = bluetooth_rows(&BluetoothState::default());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "bluetooth.unavailable");
    assert_eq!(rows[0].kind, Kind::Note);
}

#[test]
fn bluetooth_off_offers_to_turn_on() {
    let rows = bluetooth_rows(&BluetoothState { available: true, ..BluetoothState::default() });
    assert_eq!(rows[0].id, "bluetooth.off");
    assert_eq!(rows[0].action.as_deref(), Some("@ipc:bluetooth.power:on"));
}

#[test]
fn bluetooth_no_paired_devices_is_one_note() {
    let rows = bt(vec![BluetoothDevice { paired: false, ..dev("AA:01", "Seen") }]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "bluetooth.empty");
    assert_eq!(rows[0].kind, Kind::Note);
}

#[test]
fn bluetooth_order_connected_then_paired() {
    let rows = bt(vec![
        dev("AA:01", "Alpha"),
        BluetoothDevice { connected: true, ..dev("AA:02", "Zed") },
        dev("AA:03", "Beta"),
    ]);
    assert_eq!(labels(&rows), ["Zed", "Alpha", "Beta"]);
}

#[test]
fn bluetooth_unnamed_devices_are_dropped() {
    let rows = bt(vec![dev("AA:01", ""), dev("AA:02", "AA:BB:CC:DD:EE:FF"), dev("AA:03", "Real")]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "Real");
}

#[test]
fn bluetooth_desc_prefers_activity_then_battery() {
    let connected = |address: &str, name: &str| BluetoothDevice { connected: true, ..dev(address, name) };
    let rows = bt(vec![
        BluetoothDevice { state: DeviceState::Connecting, battery: 0.8, battery_available: true, ..connected("AA:01", "A") },
        BluetoothDevice { battery: 0.5, battery_available: true, ..connected("AA:02", "B") },
        connected("AA:03", "C"),
    ]);
    assert_eq!(desc_of(&rows, "A"), "Connecting\u{2026}");
    assert_eq!(desc_of(&rows, "B"), "50%");
    assert_eq!(desc_of(&rows, "C"), "");
}

#[test]
fn bluetooth_row_ids_action_and_tick() {
    let rows = bt(vec![dev("aa:bb", "Buds")]);
    assert_eq!(rows[0].id, "bluetooth.dev.aa%3Abb");
    assert_eq!(rows[0].action.as_deref(), Some("@ipc:bluetooth.toggle:aa:bb"));
    assert_eq!(rows[0].checked.as_deref(), Some("@state:bluetooth.connected=AA:BB"));
    assert_eq!(rows[0].keep_open, Some(true));
}

// A device row names what Enter does to it.
#[test]
fn device_rows_carry_their_own_verb() {
    let w = wifi(vec![
        WifiNetwork { known: true, connected: true, ..net("Home") },
        net("Cafe"),
    ]);
    assert_eq!(w[0].verb.as_deref(), Some("Disconnect"));
    assert_eq!(w[1].verb.as_deref(), Some("Connect"));
    let b = bt(vec![BluetoothDevice { connected: true, ..dev("aa:bb", "Buds") }, dev("cc:dd", "Pad")]);
    assert_eq!(b[0].verb.as_deref(), Some("Disconnect"));
    assert_eq!(b[1].verb.as_deref(), Some("Connect"));
    let audio = audio_rows(&[AudioDevice { name: "sink".into(), label: "Speakers".into(), is_sink: true }]);
    assert_eq!(audio[0].verb.as_deref(), Some("Use"));
    let fav = RadioStation { uuid: "u1".into(), name: "Jazz".into(), ..RadioStation::default() };
    let radio = radio_rows(&RadioState { running: true, favorites: vec![fav] });
    assert_eq!(radio[0].verb.as_deref(), Some("Stop"));
    assert_eq!(radio[1].verb.as_deref(), Some("Play"));
    let result = RadioStation { uuid: "u2".into(), name: "Rock".into(), ..RadioStation::default() };
    assert_eq!(radio_result_rows(&[result], &HashSet::new())[0].verb.as_deref(), Some("Play"));
}

#[test]
fn audio_no_devices_is_one_note() {
    let rows = audio_rows(&[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "audio.unavailable");
    assert_eq!(rows[0].kind, Kind::Note);
}

#[test]
fn audio_sections_actions_and_ticks() {
    let rows = audio_rows(&[
        AudioDevice { name: "alsa_output.pci-0000_00_1f.3.analog-stereo".into(), label: "Speakers".into(), is_sink: true },
        AudioDevice { name: "alsa_input.usb".into(), label: "Mic".into(), is_sink: false },
    ]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "audio.output.alsa_output%2Epci-0000_00_1f%2E3%2Eanalog-stereo");
    assert_eq!(rows[0].section.as_deref(), Some("Output"));
    assert_eq!(rows[0].action.as_deref(), Some("@ipc:audio.sink:alsa_output.pci-0000_00_1f.3.analog-stereo"));
    assert_eq!(rows[0].checked.as_deref(), Some("@state:audio.sink=alsa_output.pci-0000_00_1f.3.analog-stereo"));
    assert_eq!(rows[1].id, "audio.input.alsa_input%2Eusb");
    assert_eq!(rows[1].section.as_deref(), Some("Input"));
    assert_eq!(rows[1].action.as_deref(), Some("@ipc:audio.source:alsa_input.usb"));
    assert_eq!(rows[1].checked.as_deref(), Some("@state:audio.source=alsa_input.usb"));
    assert_eq!(rows[0].keep_open, Some(true));
    assert_eq!(rows[1].keep_open, Some(true));
}

#[test]
fn audio_outputs_only_has_no_input_note() {
    let rows = audio_rows(&[AudioDevice { name: "o".into(), label: "O".into(), is_sink: true }]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, Kind::Action);
}

#[test]
fn radio_favorites_ticks_and_alternates() {
    let fav = RadioStation { uuid: "u-1".into(), name: "Groove".into(), country: "France".into(), ..RadioStation::default() };
    let rows = radio_rows(&RadioState { running: false, favorites: vec![fav] });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "radio.fav.u-1");
    assert_eq!(rows[0].label, "Groove");
    assert_eq!(rows[0].desc.as_deref(), Some("France"));
    assert_eq!(rows[0].action.as_deref(), Some("@ipc:radio.fav:u-1"));
    assert_eq!(rows[0].checked.as_deref(), Some("@state:radio.station=u-1"));
    assert_eq!(rows[0].keep_open, Some(true));
    assert_eq!(rows[0].alternate.as_deref(), Some("@ipc:radio.unfavorite:u-1"));
    assert_eq!(rows[0].alternate_label.as_deref(), Some("Remove favorite"));
    assert!(!rows[0].local_only);
}

#[test]
fn radio_stop_only_while_running() {
    assert_eq!(radio_rows(&RadioState { running: false, favorites: vec![] }).len(), 0);
    let rows = radio_rows(&RadioState { running: true, favorites: vec![] });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "radio.stop");
    assert_eq!(rows[0].action.as_deref(), Some("@ipc:radio.stop"));
}

#[test]
fn radio_results_are_local_only_with_alternate_per_favorite_set() {
    let a = RadioStation { uuid: "a".into(), name: "A".into(), country: "Spain".into(), codec: "MP3".into() };
    let b = RadioStation { uuid: "b".into(), name: "B".into(), country: String::new(), codec: "AAC".into() };
    let favs = HashSet::from(["a".to_string()]);
    let rows = radio_result_rows(&[a, b], &favs);
    assert!(rows[0].local_only);
    assert_eq!(rows[0].desc.as_deref(), Some("Spain MP3"));
    assert_eq!(rows[0].action.as_deref(), Some("@ipc:radio.play:a"));
    assert_eq!(rows[0].alternate.as_deref(), Some("@ipc:radio.unfavorite:a"));
    assert_eq!(rows[0].alternate_label.as_deref(), Some("Remove favorite"));
    assert_eq!(rows[1].desc.as_deref(), Some("AAC"));
    assert_eq!(rows[1].alternate.as_deref(), Some("@ipc:radio.favorite:b"));
    assert_eq!(rows[1].alternate_label.as_deref(), Some("Add favorite"));
    assert_ne!(rows[1].keep_open, Some(true));
}
