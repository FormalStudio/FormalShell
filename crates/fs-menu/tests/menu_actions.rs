// actions_for over every kind the launcher lists today: the ids in order,
// what Enter and Shift+Enter run, and how a menu.jsonc `actions` entry
// overrides, hides and adds.

mod common;

use common::{entries, real_tree};
use fs_menu::abbrev::Source;
use fs_menu::actions::{Action, Run, Subject, TargetKind, WINDOW_KIND, actions_for, filter, first_action, into_action, second_action};
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
    assert_eq!(into_action(&Subject::of(&node)), None);
}

#[test]
fn app_with_windows() {
    let node = firefox();
    let open = actions_for(&Subject { windows: 2, discrete_gpu: true, ..Subject::of(&node) });
    assert_eq!(ids(&open), ["open", "new-window", "desktop:new-private-window", "windows", "discrete-gpu", "move-to-workspace", "copy-path"]);
    assert_eq!(open[3].label, "Show its 2 windows");
    assert_eq!(open[3].run, Run::Into("app:firefox".into()));
    assert_eq!(open[5].run, Run::Target(TargetKind::Workspace));
    assert_eq!(into_action(&Subject { windows: 1, ..Subject::of(&node) }).unwrap().label, "Show its window");
}

#[test]
fn window() {
    let node = Node { path: "0xab".into(), ..Node::new("window:0xab", "Firefox", Kind::Other(WINDOW_KIND.into())) };
    let all = of(&node);
    assert_eq!(ids(&all), ["focus", "close", "move-to-workspace", "float", "fullscreen"]);
    assert_eq!(all[0].run, Run::Ipc("window.focus:0xab".into()));
    assert!(all[1].destructive);
    assert_eq!(second_action(&Subject::of(&node)).unwrap().id, "close");
    assert_eq!(into_action(&Subject::of(&node)), None);
}

#[test]
fn abbreviation() {
    let node = firefox();
    let with = |source| actions_for(&Subject { abbrev: Some(("ff", source)), ..Subject::of(&node) });
    let learned = with(Source::Learned);
    assert_eq!(ids(&learned[4..]), ["abbrev-forget", "abbrev-edit", "abbrev-pin"]);
    assert_eq!(learned[4].run, Run::Ipc("abbrev.forget:ff".into()));
    assert_eq!(learned[5].run, Run::Input("Abbreviation for Firefox".into()));
    assert_eq!(learned[6].run, Run::Ipc("abbrev.pin:ff".into()));
    assert_eq!(ids(&with(Source::Pinned)[4..]), ["abbrev-forget", "abbrev-edit"]);
    let configured = with(Source::Configured);
    assert_eq!(ids(&configured[4..]), ["abbrev-configured"]);
    assert_eq!(configured[4].run, Run::Note);
    // A note never runs, so a subject with nothing else has no default.
    let bare = Node::new("x", "X", Kind::Other("thing".into()));
    assert_eq!(first_action(&Subject { abbrev: Some(("x", Source::Configured)), ..Subject::of(&bare) }), None);
}

#[test]
fn uninstall_is_an_owned_apps_last_action() {
    let node = firefox();
    assert!(actions_for(&Subject::of(&node)).iter().all(|a| a.id != "uninstall"));
    let owned = actions_for(&Subject { abbrev: Some(("ff", Source::Learned)), removable: Some("flatpak:org.mozilla.firefox"), ..Subject::of(&node) });
    let last = owned.last().unwrap();
    assert_eq!(last.id, "uninstall");
    assert_eq!(last.label, "Uninstall Firefox");
    assert!(last.confirm && last.destructive);
    assert_eq!(last.run, Run::Ipc("package.uninstall:flatpak:org.mozilla.firefox".into()));
}

#[test]
fn filtering() {
    let labels = ["Open", "New window", "New private window", "Move to workspace\u{2026}"];
    assert_eq!(filter(&labels, "new"), [1, 2]);
    assert_eq!(filter(&labels, "new priv"), [2]);
    assert_eq!(filter(&labels, "WORK"), [3]);
    assert_eq!(filter(&labels, ""), [0, 1, 2, 3]);
}

#[test]
fn route() {
    let tree = real_tree();
    let clip = &tree.nodes["clipboard"];
    let all = of(clip);
    assert_eq!(ids(&all), ["open", "pin-recent"]);
    assert_eq!(all[0].run, Run::Into("clipboard".into()));
    assert_eq!(all[1].run, Run::Ipc("recent.pin:clipboard".into()));
    assert!(all[1].keep_open);
    let link = Node { target: Some("system".into()), ..Node::new("power", "Power", Kind::Link) };
    assert_eq!(of(&link)[0].run, Run::Into("system".into()));
    assert_eq!(second_action(&Subject::of(clip)).unwrap().id, "pin-recent");
    let pinned = actions_for(&Subject { pinned: true, ..Subject::of(clip) });
    assert_eq!(ids(&pinned), ["open", "unpin-recent"]);
    assert_eq!(pinned[1].run, Run::Ipc("recent.unpin:clipboard".into()));
}

#[test]
fn action_row() {
    let tree = real_tree();
    let shutdown = of(&tree.nodes["system.shutdown"]);
    assert_eq!(ids(&shutdown), ["run"]);
    assert!(shutdown[0].confirm);
    assert_eq!(shutdown[0].run, Run::Ipc("power.poweroff".into()));
    let logout = of(&tree.nodes["system.logout"]);
    assert_eq!(ids(&logout), ["run"]);
    assert_eq!(logout[0].run, Run::Ipc("power.logout".into()));
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
    assert_eq!(ids(&saved), ["run", "alternate", "show-password", "panel", "details"]);
    assert_eq!(saved[0].label, "Connect");
    assert_eq!(saved[1].label, "Forget");
    assert_eq!(saved[1].run, Run::Ipc("wifi.forget:Home".into()));
    assert_eq!(saved[2].label, "Show password");
    assert_eq!(saved[2].run, Run::Ipc("wifi.reveal:Home".into()));
    assert_eq!(saved[3].run, Run::Ipc("panel.open:network".into()));
    assert_eq!(saved[4].run, Run::Into("wifi.net.Home".into()));
    assert_eq!(into_action(&Subject::of(&wifi_rows(&st)[0])).unwrap().id, "details");
    let st = WifiState { networks: vec![net(false)], ..st };
    let row = &wifi_rows(&st)[0];
    assert_eq!(ids(&of(row)), ["run", "panel", "details"]);
    assert_eq!(second_action(&Subject::of(row)).unwrap().id, "panel");
    let open = WifiNetwork { secured: false, ..net(true) };
    let enterprise = WifiNetwork { enterprise: true, ..net(true) };
    for n in [open, enterprise] {
        let st = WifiState { networks: vec![n], ..st.clone() };
        assert_eq!(ids(&of(&wifi_rows(&st)[0])), ["run", "alternate", "panel", "details"]);
    }
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
    assert_eq!(ids(&image), ["run", "alternate", "copy-only", "send-to", "delete"]);
    assert_eq!(image[0].label, "Paste");
    assert!(image[0].paste_after);
    assert_eq!(image[1].label, "Send over SSH");
    assert_eq!(image[1].run, Run::Ipc("clipssh.image:b:/img/one.png".into()));
    assert!(!image[2].paste_after);
    assert_eq!(image[3].run, Run::Target(TargetKind::Peer));
    assert_eq!(image[4].run, Run::Ipc("clipboard.remove:b".into()));
    assert!(image[4].keep_open && image[4].destructive);
    assert_eq!(ids(&of(&rows[1])), ["run", "copy-only", "send-to", "delete"]);
    let copy = clipboard_provider(&[Entry::text("a", "hello")], ClipMode::Copy, false);
    assert_eq!(ids(&of(&copy[0])), ["run", "send-to", "delete"]);
    let share = clipboard_provider(&[Entry::text("a", "hello")], ClipMode::Share, false);
    assert!(!ids(&of(&share[0])).contains(&"send-to"));
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
    let sky = &image_rows(&["sky:6".into()], "")[0];
    assert_eq!(ids(&of(sky)), ["set"]);
}

#[test]
fn file() {
    let row = &fs_menu::providers::files_rows(&[], &[fs_menu::providers::Found { path: "/h/My Report, v2.pdf".into(), dir: false }], "/h")[0];
    let all = of(row);
    assert_eq!(ids(&all), ["open", "open-with", "show-in-folder", "copy-path", "send-to"]);
    assert_eq!(all[0].run, Run::Command("xdg-open '/h/My Report, v2.pdf'".into()));
    assert_eq!(all[1].run, Run::Target(TargetKind::App));
    assert_eq!(all[4].run, Run::Target(TargetKind::Peer));
    let Run::Command(show) = &all[2].run else { panic!("show-in-folder is not a command") };
    assert!(show.contains("'array:string:file:///h/My%20Report%2C%20v2.pdf'") && show.ends_with("|| xdg-open '/h'"), "{show}");
    assert_eq!(all[3].run, Run::Command("wl-copy -- '/h/My Report, v2.pdf'".into()));
    assert_eq!(second_action(&Subject::of(row)).unwrap().id, "open-with");
    assert_eq!(into_action(&Subject::of(row)), None);
}

#[test]
fn folder() {
    let row = &fs_menu::providers::files_rows(&[fs_menu::providers::Found { path: "/h/Documents".into(), dir: true }], &[], "/h")[0];
    let all = of(row);
    assert_eq!(ids(&all), ["open", "copy-path", "contents"]);
    assert_eq!(all[0].label, "Open in the file manager");
    assert_eq!(all[0].run, Run::Command("xdg-open '/h/Documents'".into()));
    assert_eq!(into_action(&Subject::of(row)).unwrap().run, Run::Into("file:/h/Documents".into()));
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

// A menu.jsonc entry on a provider's row overrides its alternate as well as
// its actions, so Shift+Enter on a saved network runs the declared one.
#[test]
fn a_declared_alternate_survives_the_provider() {
    let default = entries(json!({ "wifi": { "provider": "wifi" } }));
    let user = entries(json!({ "wifi.net.Home": { "alternate": "notify-send home", "alternateLabel": "Say hi" } }));
    let mut tree = build_tree(&default, &user);
    let net = WifiNetwork { name: "Home".into(), known: true, secured: true, in_range: true, ..WifiNetwork::default() };
    let st = WifiState { has_device: true, enabled: true, networks: vec![net], ..WifiState::default() };
    attach_children(&mut tree, "wifi", wifi_rows(&st));
    let row = &tree.nodes["wifi.net.Home"];
    let second = second_action(&Subject::of(row)).unwrap();
    assert_eq!((second.label.as_str(), &second.run), ("Say hi", &Run::Command("notify-send home".into())));
    assert_eq!(of(row)[0].label, "Connect");
}

// Files, folders and emoji are known by what they are, not by a row id
// that only lives in one listing.
#[test]
fn subjects_outside_the_tree_carry_their_own_ids() {
    use fs_menu::actions::subject_id;
    use fs_menu::providers::subject;
    assert_eq!(subject_id(&subject("/home/u/a b.txt", false, None)), "file:/home/u/a b.txt");
    assert_eq!(subject_id(&subject("/home/u/Music", true, None)), "file:/home/u/Music");
    let index = EmojiIndex::new(vec![EmojiEntry { ch: "\u{1F62D}".into(), name: "loudly crying face".into(), ..EmojiEntry::default() }]);
    let row = index.row_for("\u{1F62D}", true).unwrap();
    assert_eq!(subject_id(&row), "emoji:\u{1F62D}");
    assert_eq!(row, index.rows("loudly", true, &[], None).remove(0));
    assert!(index.row_for("x", true).is_none());
}
