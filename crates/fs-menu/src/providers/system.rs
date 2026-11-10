//! The self-targeted routes: the capture/system/notification/theme entries
//! merged into the default tree, the panel and tray submenus, the GPU routes
//! and the keyboard-lights subtree. Every action is an IPC call, so a menu
//! row and a compositor keybind exercise one implementation.

use fs_chrome::bar::panels::{Home, PanelId};

use crate::actions::{IpcCall, Run};
use crate::intent::Intent;
use crate::node::{Entries, Entry, Kind, Node};
use crate::toggles::Checked;

/// One `menu.customPowerButtons` item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomPowerButton {
    pub label: String,
    pub icon: Option<String>,
    pub command: String,
    pub confirm: bool,
}

/// Expands `menu.customPowerButtons` into entry fragments keyed by dotted id,
/// meant to be merged into the default tree object before `build_tree` runs:
/// `system.custom.<i>` auto-nests under the already-declared `system` node.
pub fn custom_power_button_entries(buttons: &[CustomPowerButton]) -> Entries {
    let mut out = Entries::new();
    for (i, btn) in buttons.iter().enumerate() {
        out.insert(
            format!("system.custom.{i}"),
            Entry {
                label: Some(btn.label.clone()),
                icon: Some(btn.icon.clone().unwrap_or_default()),
                action: Some(Run::parse(&btn.command)),
                confirm: Some(btn.confirm),
                ..Entry::default()
            },
        );
    }
    out
}

/// Root nodes merged into the default tree object like
/// `custom_power_button_entries`. Each parent is declared here rather than
/// left to `build_tree`'s ancestor synthesis so it carries a label and an
/// icon of its own. "Video To GIF" passes an empty path on purpose: `record
/// gif` falls back to the recorder's last path.
///
/// `notifications` is its own root, not nested under `system.notifications`:
/// that id is already an activatable leaf, and an action node cannot also
/// carry children the model would ever enter.
pub fn capture_entries() -> Entries {
    let mut e = Entries::new();
    e.insert("capture".into(), Entry { chord: Some("Super+Ctrl+C".into()), ..Entry::labelled("Capture").with_icon_name("camera").with_icon("\u{F0E51}") });
    e.insert("capture.text".into(), Entry::labelled("Copy Text From Screen").with_icon_name("scan-text").with_icon("\u{F113A}").with_action(IpcCall::new("capture", "text")));
    e.insert("capture.color".into(), Entry::labelled("Pick Color").with_icon_name("pipette").with_icon("\u{F020B}").with_action(IpcCall::new("capture", "color")));
    e.insert("capture.record".into(), Entry::labelled("Record Screen").with_icon_name("video").with_icon("\u{F0567}").with_action(IpcCall::new("record", "toggle").arg("screen").arg("none")));
    e.insert("capture.stop".into(), Entry::labelled("Stop Recording").with_icon_name("square").with_icon("\u{F04DB}").with_action(IpcCall::new("record", "stop")));
    e.insert("capture.gif".into(), Entry::labelled("Video To GIF").with_icon_name("film").with_icon("\u{F0D78}").with_action(IpcCall::new("record", "gif").arg("")));
    e.insert("capture.screenshot".into(), Entry::labelled("Screenshot").with_icon_name("camera").with_icon("\u{F0100}").with_action(IpcCall::new("screenshot", "full").arg("default")));
    e.insert("capture.region".into(), Entry::labelled("Screenshot Region").with_icon_name("crop").with_icon("\u{F0489}").with_action(IpcCall::new("screenshot", "region").arg("default")));
    e.insert("system.console".into(), Entry::labelled("Console").with_icon_name("terminal").with_icon("\u{F018D}").with_action(IpcCall::new("console", "toggle")));
    e.insert("system.screensaver".into(), Entry::labelled("Screensaver").with_icon_name("monitor-off").with_icon("\u{F0D90}").with_action(IpcCall::new("screensaver", "start")));
    e.insert("system.plugins".into(), Entry::labelled("Plugins").with_icon_name("puzzle").with_icon("\u{F0431}"));
    e.insert("system.plugins.list".into(), Entry::labelled("List Plugins").with_icon_name("list").with_icon("\u{F0279}").with_action(IpcCall::new("plugins", "list")));
    e.insert("system.plugins.reload".into(), Entry::labelled("Reload Plugins").with_icon_name("refresh-cw").with_icon("\u{F0453}").with_action(IpcCall::new("plugins", "reload")));
    e.insert("notifications".into(), Entry::labelled("Notifications").with_icon_name("bell").with_icon("\u{F009A}"));
    e.insert("notifications.clear".into(), Entry::labelled("Clear All").with_icon_name("trash").with_icon("\u{F039F}").with_action(IpcCall::new("notifications", "clear")));
    e.insert("notifications.markAllSeen".into(), Entry::labelled("Mark All Seen").with_icon_name("check-check").with_icon("\u{F012D}").with_action(IpcCall::new("notifications", "markAllSeen")));
    e.insert("notifications.dismissAll".into(), Entry::labelled("Dismiss Popups").with_icon_name("x").with_icon("\u{F062A}").with_action(IpcCall::new("notifications", "dismissAll")));
    e.insert("theme".into(), Entry { chord: Some("Super+Shift+Ctrl+Space".into()), ..Entry::labelled("Theme").with_icon_name("palette").with_icon("\u{F0301}") });
    e.insert("theme.retheme".into(), Entry::labelled("Retheme").with_icon_name("palette").with_icon("\u{F0301}").with_action(IpcCall::new("theme", "retheme")));
    e.insert("theme.mode-dark".into(), Entry::labelled("Dark Mode").with_icon_name("moon").with_icon("\u{F0594}").with_action(IpcCall::new("theme", "mode").arg("dark")));
    e.insert("theme.mode-light".into(), Entry::labelled("Light Mode").with_icon_name("sun").with_icon("\u{F0599}").with_action(IpcCall::new("theme", "mode").arg("light")));
    e
}

/// The panels the Panels route lists, in the table's order: every panel
/// but the pages the launcher holds as routes of their own and the tray's
/// second bar, whose "tray" route already lists its items.
pub fn panel_ids() -> impl Iterator<Item = PanelId> {
    PanelId::ALL.into_iter().filter(|p| p.home() != Home::Launcher && *p != PanelId::TrayOverflow)
}

pub fn panels_provider() -> Vec<Node> {
    panel_ids()
        .map(|p| {
            let d = p.desc();
            Node {
                icon_name: d.icon.to_owned(),
                aliases: d.aliases.iter().map(|a| a.to_string()).collect(),
                action: Some(IpcCall::new("panel", "open").arg(d.id).into()),
                ..Node::new(format!("panels.{}", d.id), d.title, Kind::Action)
            }
        })
        .collect()
}

/// A system tray item, the fields the launcher reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrayItem {
    pub id: String,
    pub title: String,
    pub tooltip_title: String,
}

/// System tray rows: `SystemTray.items` has no launcher path otherwise, only
/// a bar-cell click. The tray IPC target's `activate(id)` exactly. An empty
/// tray renders one dim row, never an empty level.
pub fn tray_provider(items: &[TrayItem]) -> Vec<Node> {
    if items.is_empty() {
        return vec![Node::note("tray.empty", "No tray items")];
    }
    items
        .iter()
        .map(|item| {
            let label = [&item.tooltip_title, &item.title, &item.id]
                .into_iter()
                .find(|s| !s.is_empty())
                .cloned()
                .unwrap_or_default();
            Node {
                action: Some(IpcCall::new("tray", "activate").arg(&item.id).into()),
                ..Node::new(format!("tray.{}", item.id), label, Kind::Action)
            }
        })
        .collect()
}

const LIGHT_COLORS: &[(&str, &str)] = &[
    ("ff0000", "Red"),
    ("ff4000", "Orange"),
    ("ffc000", "Yellow"),
    ("00ff00", "Green"),
    ("00ffff", "Cyan"),
    ("0000ff", "Blue"),
    ("8000ff", "Purple"),
    ("ff0080", "Pink"),
    ("ffffff", "White"),
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightEffect {
    pub id: String,
    pub label: String,
}

/// The "lights" route, present only while the lights service found a
/// keyboard it can drive. Every row dispatches in-process
/// (`@ipc:lights.<verb>:<value>`) and keeps the launcher open, and every
/// choice row carries an `@state:<path>=<value>` check so the tick moves the
/// moment it lands. `effects` is what the chassis reports, not the whole
/// table.
pub fn lights_entries(available: bool, effects: &[LightEffect]) -> Entries {
    let mut out = Entries::new();
    if !available {
        return out;
    }
    fn choice(verb: &str, path: &str, value: &str, label: &str) -> Entry {
        Entry::labelled(label)
            .with_action(IpcCall::new("lights", verb).arg(value))
            .with_checked(Checked::value(&format!("lights.{path}"), value))
            .keeping_open()
    }
    out.insert(
        "lights".into(),
        Entry::labelled("Keyboard Lights").with_icon_name("keyboard").with_aliases(&["rgb", "aura", "led", "leds", "backlight"]),
    );
    out.insert(
        "lights.power".into(),
        Entry::labelled("Lights On").with_icon_name("power")
            .with_action(IpcCall::new("lights", "toggle"))
            .with_checked(Checked::flag("lights.on"))
            .keeping_open(),
    );
    out.insert("lights.effect".into(), Entry::labelled("Effect").with_icon_name("zap"));
    out.insert("lights.color".into(), Entry::labelled("Color").with_icon_name("palette"));
    out.insert("lights.source".into(), Entry::labelled("Color Source").with_icon_name("image"));
    out.insert("lights.source.wallpaper".into(), choice("source", "source", "wallpaper", "Wallpaper"));
    out.insert("lights.source.custom".into(), choice("source", "source", "custom", "Custom Color"));
    out.insert("lights.speed".into(), Entry::labelled("Speed").with_icon_name("gauge"));
    out.insert("lights.brightness".into(), Entry::labelled("Brightness").with_icon_name("sun"));
    for e in effects {
        out.insert(format!("lights.effect.{}", e.id), choice("effect", "effect", &e.id, &e.label));
    }
    for (hex, label) in LIGHT_COLORS {
        out.insert(format!("lights.color.{hex}"), choice("color", "colour", hex, label));
    }
    out.insert("lights.color.hex".into(), Entry::labelled("Hex Color").with_action(Intent::LightsColorInput));
    for (id, label) in [("low", "Low"), ("med", "Medium"), ("high", "High")] {
        out.insert(format!("lights.speed.{id}"), choice("speed", "speed", id, label));
    }
    for (id, label) in [("1", "Low"), ("2", "Medium"), ("3", "High")] {
        out.insert(format!("lights.brightness.{id}"), choice("brightness", "brightness", id, label));
    }
    out
}

/// The stretches of an edge the iPad route offers, `(id, label, start,
/// end)`, each end a fraction of the edge from its top or left end.
pub const IPAD_STRETCHES: &[(&str, &str, f64, f64)] = &[
    ("whole", "Whole edge", 0.0, 1.0),
    ("first", "First half", 0.0, 0.5),
    ("second", "Second half", 0.5, 1.0),
    ("middle", "Middle", 0.25, 0.75),
];

/// The "ipad" route, present only while padbridge is on the bus: switching
/// input to the iPad, bringing its link back, the edge it sits past and
/// the stretch of that edge. Every row dispatches in-process and all but
/// Switch keep the launcher open, the tick moving off `@state:ipad.*`.
pub fn ipad_entries(available: bool) -> Entries {
    let mut out = Entries::new();
    if !available {
        return out;
    }
    let section = |e: Entry, s: &str| Entry { section: Some(s.to_owned()), ..e };
    out.insert("ipad".into(), Entry::labelled("iPad").with_icon_name("tablet").with_aliases(&["tablet", "continuity", "padbridge", "second screen"]));
    out.insert("ipad.switch".into(), Entry::labelled("Switch to iPad").with_icon_name("arrow-right-left").with_action(IpcCall::new("ipad", "switch")).with_checked(Checked::flag("ipad.active")));
    out.insert("ipad.reconnect".into(), Entry::labelled("Reconnect").with_icon_name("refresh-cw").with_action(IpcCall::new("ipad", "reconnect")).keeping_open());
    for (edge, label) in [("left", "Left"), ("right", "Right"), ("top", "Above"), ("bottom", "Below")] {
        let row = Entry::labelled(label)
            .with_icon_name(&format!("panel-{edge}"))
            .with_action(IpcCall::new("ipad", "position").arg(edge))
            .with_checked(Checked::value("ipad.edge", edge))
            .keeping_open();
        out.insert(format!("ipad.edge-{edge}"), section(row, "Position"));
    }
    for (id, label, start, end) in IPAD_STRETCHES {
        let icon = match *id {
            "whole" => "move-horizontal",
            "first" => "align-start-vertical",
            "second" => "align-end-vertical",
            _ => "align-center-vertical",
        };
        let row = Entry::labelled(label)
            .with_icon_name(icon)
            .with_action(IpcCall::new("ipad", "stretch").arg(start.to_string()).arg(end.to_string()))
            .with_checked(Checked::value("ipad.stretch", *id))
            .keeping_open();
        out.insert(format!("ipad.stretch-{id}"), section(row, "Stretch"));
    }
    out
}
