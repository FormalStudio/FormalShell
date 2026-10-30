// actions_for over every kind the launcher lists today: the ids in order,
// what Enter and Shift+Enter run, and how a menu.jsonc `actions` entry
// overrides, hides and adds.

mod common;

use common::{entries, real_tree};
use fs_menu::actions::{Action, Run, Subject, actions_for, first_action, second_action};
use fs_menu::clipboard::history::Entry;
use fs_menu::keybinds::{Bind, rows as bind_rows};
use fs_menu::model::build_tree;
use fs_menu::node::{DesktopEntry, Entries, Kind, Node};
use fs_menu::providers::{
    AudioDevice, BluetoothState, ClipMode, EmojiEntry, EmojiIndex, RadioState, RadioStation, WifiNetwork, WifiState, apps_provider,
    attach_children, audio_rows, bluetooth_rows, clipboard_provider, image_rows, radio_rows, wifi_rows,
};
use serde_json::json;

fn ids(actions: &[Action]) -> Vec<&str> {
    actions.iter().map(|a| a.id.as_str()).collect()
}

fn of(node: &Node) -> Vec<Action> {
    actions_for(&Subject::of(node))
}

fn firefox() -> Node {
    let entry = DesktopEntry {
        id: "firefox".into(),
        name: "Firefox".into(),
        path: "/usr/share/applications/firefox.desktop".into(),
        actions: vec![("new-private-window".into(), "New private window".into())],
        ..DesktopEntry::default()
    };
    apps_provider(&[entry], None, &[], None).remove(0)
}

#[test]
fn app() {
    let node = firefox();
    let plain = of(&node);
    assert_eq!(ids(&plain), ["open", "new-window", "desktop:new-private-window", "copy-path"]);
    assert_eq!(plain[0].run, Run::Ipc("app.open:firefox".into()));
    assert_eq!(plain[1].run, Run::Ipc("app.new:firefox".into()));
    assert_eq!(plain[2].label, "New private window");
    assert_eq!(plain[2].run, Run::Ipc("app.action:firefox:new-private-window".into()));
    assert_eq!(plain[3].run, Run::Command("wl-copy -- '/usr/share/applications/firefox.desktop'".into()));
    let hybrid = actions_for(&Subject { discrete_gpu: true, ..Subject::of(&node) });
    assert_eq!(ids(&hybrid), ["open", "new-window", "desktop:new-private-window", "discrete-gpu", "copy-path"]);
    assert_eq!(hybrid[3].run, Run::Ipc("app.gpu:firefox".into()));
    assert_eq!(second_action(&Subject::of(&node)).unwrap().id, "new-window");
}

#[test]
fn route() {
    let tree = real_tree();
    let clip = &tree.nodes["clipboard"];
    let all = of(clip);
    assert_eq!(ids(&all), ["open"]);
    assert_eq!(all[0].run, Run::Into("clipboard".into()));
    let link = Node { target: Some("system".into()), ..Node::new("power", "Power", Kind::Link) };
    assert_eq!(of(&link)[0].run, Run::Into("system".into()));
    // A single action is both Enter and Shift+Enter.
    assert_eq!(second_action(&Subject::of(clip)).unwrap().id, "open");
}

#[test]
fn action_row() {
    let tree = real_tree();
    let shutdown = of(&tree.nodes["system.shutdown"]);
    assert_eq!(ids(&shutdown), ["run"]);
    assert!(shutdown[0].confirm);
    assert_eq!(shutdown[0].run, Run::Ipc("power.poweroff".into()));
    let logout = of(&tree.nodes["system.logout"]);
    assert_eq!(ids(&logout), ["run", "copy-command"]);
    assert_eq!(logout[1].run, Run::Command("wl-copy -- 'hyprctl dispatch '\\''hl.dsp.exit()'\\'''".into()));
}

#[test]
fn toggle() {
    let tree = real_tree();
    let night = of(&tree.nodes["toggles.nightlight"]);
    assert_eq!(ids(&night), ["run", "panel"]);
    assert_eq!(night[0].label, "Toggle");
    assert!(night[0].keep_open);
    assert_eq!(night[1].run, Run::Ipc("panel.open:display".into()));
    assert_eq!(ids(&of(&tree.nodes["toggles.dnd"])), ["run"]);
}

#[test]
fn wifi_network() {
    let net = |known| WifiNetwork { name: "Home".into(), known, secured: true, in_range: true, ..WifiNetwork::default() };
    let st = WifiState { has_device: true, enabled: true, networks: vec![net(true)], ..WifiState::default() };
    let saved = of(&wifi_rows(&st)[0]);
    assert_eq!(ids(&saved), ["run", "alternate", "panel"]);
    assert_eq!(saved[0].label, "Connect");
    assert_eq!(saved[1].label, "Forget");
    assert_eq!(saved[1].run, Run::Ipc("wifi.forget:Home".into()));
    assert_eq!(saved[2].run, Run::Ipc("panel.open:network".into()));
    let st = WifiState { networks: vec![net(false)], ..st };
    assert_eq!(ids(&of(&wifi_rows(&st)[0])), ["run", "panel"]);
}

#[test]
fn bluetooth_device() {
    let device = fs_devices::bluetooth::Device { address: "AA:BB".into(), name: "Buds".into(), paired: true, ..Default::default() };
    let rows = bluetooth_rows(&BluetoothState { available: true, enabled: true, devices: vec![device] });
    let all = of(rows.iter().find(|r| r.id.starts_with("bluetooth.dev.")).unwrap());
    assert_eq!(ids(&all), ["run", "forget", "panel"]);
    assert_eq!(all[1].run, Run::Ipc("bluetooth.forget:AA:BB".into()));
    assert!(all[1].destructive);
}

#[test]
fn audio_device() {
    let all = of(&audio_rows(&[AudioDevice { name: "sink0".into(), label: "Speakers".into(), is_sink: true }])[0]);
    assert_eq!(ids(&all), ["run", "panel"]);
    assert_eq!(all[0].run, Run::Ipc("audio.sink:sink0".into()));
}

#[test]
fn station() {
    let fav = RadioStation { uuid: "u1".into(), name: "FM".into(), ..RadioStation::default() };
    let rows = radio_rows(&RadioState { running: false, favorites: vec![fav] });
    let all = of(rows.iter().find(|r| r.id.starts_with("radio.fav.")).unwrap());
    assert_eq!(ids(&all), ["run", "alternate"]);
    assert_eq!(all[1].label, "Remove favorite");
}

#[test]
fn clipboard_entry() {
    let rows = clipboard_provider(&[Entry::image("b", "/img/one.png"), Entry::text("a", "hello")], ClipMode::Copy, true);
    let image = of(&rows[0]);
    assert_eq!(ids(&image), ["run", "alternate", "copy-only", "delete"]);
    assert_eq!(image[0].label, "Paste");
    assert!(image[0].paste_after);
    assert_eq!(image[1].label, "Send over SSH");
    assert_eq!(image[1].run, Run::Ipc("clipssh.image:b:/img/one.png".into()));
    assert!(!image[2].paste_after);
    assert_eq!(image[3].run, Run::Ipc("clipboard.remove:b".into()));
    assert!(image[3].keep_open && image[3].destructive);
    assert_eq!(ids(&of(&rows[1])), ["run", "copy-only", "delete"]);
    let copy = clipboard_provider(&[Entry::text("a", "hello")], ClipMode::Copy, false);
    assert_eq!(ids(&of(&copy[0])), ["run", "delete"]);
}

#[test]
fn emoji() {
    let index = EmojiIndex::new(vec![EmojiEntry { ch: "😭".into(), name: "loudly crying face".into(), group: "g".into(), kw: None }]);
    let paste = of(&index.rows("cry", true, &[], None)[0]);
    assert_eq!(ids(&paste), ["run", "copy-only"]);
    assert_eq!(paste[0].label, "Paste");
    assert!(paste[0].paste_after && !paste[1].paste_after);
    assert_eq!(ids(&of(&index.rows("cry", false, &[], None)[0])), ["run"]);
}

#[test]
fn wallpaper() {
    let row = &image_rows(&["/w/sea.jpg".into()], "")[0];
    let all = of(row);
    assert_eq!(ids(&all), ["set", "open-file", "copy-path"]);
    assert_eq!(all[0].label, "Set wallpaper");
    assert_eq!(all[0].run, Run::Ipc("wallpaper.set:/w/sea.jpg".into()));
    assert_eq!(actions_for(&Subject { picker_select: true, ..Subject::of(row) })[0].label, "Choose");
}

#[test]
fn calc_result() {
    let node = fs_menu::calc::result_node("2+2").unwrap();
    let all = of(&node);
    assert_eq!(ids(&all), ["run", "paste"]);
    assert_eq!(all[0].label, "Copy");
    assert!(all[1].paste_after);
}

#[test]
fn keybind() {
    let bind = Bind { chord: "Super+K".into(), action: "exec".into(), ..Bind::default() };
    let row = &bind_rows(&[bind], "")[0];
    assert_eq!(ids(&of(row)), ["copy-chord"]);
    assert!(Subject::of(row).inert());
    assert_eq!(first_action(&Subject::of(row)), None);
    assert_eq!(second_action(&Subject::of(row)), None);
}

#[test]
fn inert_note() {
    let note = Node::note("x.empty", "Nothing");
    assert!(of(&note).is_empty());
    assert_eq!(first_action(&Subject::of(&note)), None);
}

#[test]
fn menu_jsonc_actions() {
    let default = entries(json!({
        "apps": { "provider": "apps" },
        "tools": { "label": "Tools" },
        "tools.build": { "action": "make", "actions": [{ "id": "clean", "label": "Clean", "action": "make clean" }] },
        "tools.test": { "action": "make test", "alternate": "make test-all", "alternateLabel": "Test everything" },
    }));
    let user: Entries = entries(json!({
        "apps.org.gnome.Files": { "actions": [{ "id": "copy-path", "hidden": true }] },
        "apps.firefox": { "actions": [
            { "id": "new-window", "hidden": true },
            { "id": "open", "label": "Launch" },
            { "id": "profile", "label": "Profiles", "action": "firefox -P", "confirm": true },
        ] },
        "tools.build": { "actions": [{ "id": "clean", "label": "Clean all" }, { "id": "copy-command", "hidden": true }] },
    }));
    let mut tree = build_tree(&default, &user);
    let files = DesktopEntry { id: "org.gnome.Files".into(), name: "Files".into(), path: "/f.desktop".into(), ..DesktopEntry::default() };
    attach_children(&mut tree, "apps", vec![firefox(), apps_provider(&[files], None, &[], None).remove(0)]);
    assert!(tree.nodes["apps"].child_ids.iter().filter(|c| *c == "apps.firefox").count() == 1);
    assert!(tree.nodes["apps"].child_ids.contains(&"apps.org.gnome.Files".to_string()));
    assert_eq!(ids(&of(&tree.nodes["apps.org.gnome.Files"])), ["open", "new-window"]);
    let test = of(&tree.nodes["tools.test"]);
    assert_eq!(ids(&test), ["run", "alternate", "copy-command"]);
    assert_eq!(test[1].label, "Test everything");
    assert_eq!(second_action(&Subject::of(&tree.nodes["tools.test"])).unwrap().run, Run::Command("make test-all".into()));
    let app = of(&tree.nodes["apps.firefox"]);
    assert_eq!(ids(&app), ["open", "desktop:new-private-window", "copy-path", "profile"]);
    assert_eq!(app[0].label, "Launch");
    assert_eq!(app[0].run, Run::Ipc("app.open:firefox".into()));
    assert_eq!(app[3].run, Run::Command("firefox -P".into()));
    assert!(app[3].confirm);
    let build = of(&tree.nodes["tools.build"]);
    assert_eq!(ids(&build), ["run", "clean"]);
    assert_eq!(build[1].label, "Clean all");
    assert_eq!(build[1].run, Run::Command("make clean".into()));
}
