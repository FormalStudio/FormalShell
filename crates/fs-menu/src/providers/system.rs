//! The self-targeted routes: the capture/system/notification/theme entries
//! merged into the default tree, the panel and tray submenus, the GPU routes
//! and the keyboard-lights subtree. Every action names the running shell's
//! own path, which static jsonc cannot express; routing through real IPC
//! means a menu row and a compositor keybind exercise one implementation.

use super::clipboard::shq;
use crate::node::{Entries, Entry, Kind, Node};

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
                action: Some(btn.command.clone()),
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
/// icon of its own. "Video To GIF" takes no argument on purpose: `record gif`
/// falls back to the recorder's last path.
///
/// `notifications` is its own root, not nested under `system.notifications`:
/// that id is already an activatable leaf, and an action node cannot also
/// carry children the model would ever enter.
pub fn capture_entries(ipc: &str) -> Entries {
    let call = format!("{ipc} call ");
    let mut e = Entries::new();
    e.insert("capture".into(), Entry::labelled("Capture").with_icon("\u{F0E51}"));
    e.insert("capture.text".into(), Entry::labelled("Copy Text From Screen").with_icon("\u{F113A}").with_action(format!("{call}capture text")));
    e.insert("capture.color".into(), Entry::labelled("Pick Color").with_icon("\u{F020B}").with_action(format!("{call}capture color")));
    e.insert("capture.record".into(), Entry::labelled("Record Screen").with_icon("\u{F0567}").with_action(format!("{call}record toggle screen none")));
    e.insert("capture.stop".into(), Entry::labelled("Stop Recording").with_icon("\u{F04DB}").with_action(format!("{call}record stop")));
    e.insert("capture.gif".into(), Entry::labelled("Video To GIF").with_icon("\u{F0D78}").with_action(format!("{call}record gif")));
    e.insert("capture.screenshot".into(), Entry::labelled("Screenshot").with_icon("\u{F0100}").with_action(format!("{call}screenshot full")));
    e.insert("capture.region".into(), Entry::labelled("Screenshot Region").with_icon("\u{F0489}").with_action(format!("{call}screenshot region")));
    e.insert("system.console".into(), Entry::labelled("Console").with_icon("\u{F018D}").with_action(format!("{call}console toggle")));
    e.insert("system.screensaver".into(), Entry::labelled("Screensaver").with_icon("\u{F0D90}").with_action(format!("{call}screensaver start")));
    e.insert("system.plugins".into(), Entry::labelled("Plugins").with_icon("\u{F0431}"));
    e.insert("system.plugins.list".into(), Entry::labelled("List Plugins").with_icon("\u{F0279}").with_action(format!("{call}plugins list")));
    e.insert("system.plugins.reload".into(), Entry::labelled("Reload Plugins").with_icon("\u{F0453}").with_action(format!("{call}plugins reload")));
    e.insert("notifications".into(), Entry::labelled("Notifications").with_icon("\u{F009A}"));
    e.insert("notifications.clear".into(), Entry::labelled("Clear All").with_icon("\u{F039F}").with_action(format!("{call}notifications clear")));
    e.insert("notifications.markAllSeen".into(), Entry::labelled("Mark All Seen").with_icon("\u{F012D}").with_action(format!("{call}notifications markAllSeen")));
    e.insert("notifications.dismissAll".into(), Entry::labelled("Dismiss Popups").with_icon("\u{F062A}").with_action(format!("{call}notifications dismissAll")));
    e.insert("theme".into(), Entry::labelled("Theme").with_icon("\u{F0301}"));
    e.insert("theme.retheme".into(), Entry::labelled("Retheme").with_icon("\u{F0301}").with_action(format!("{call}theme retheme")));
    e.insert("theme.mode-dark".into(), Entry::labelled("Dark Mode").with_icon("\u{F0594}").with_action(format!("{call}theme mode dark")));
    e.insert("theme.mode-light".into(), Entry::labelled("Light Mode").with_icon("\u{F0599}").with_action(format!("{call}theme mode light")));
    e
}

struct PanelName {
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    aliases: &'static [&'static str],
}

/// One row per name in the shell's panel IPC registry, `trayoverflow` aside
/// (the "tray" route already lists its items). The registry is declared in
/// the shell, not discoverable at runtime, so a new panel needs a new entry
/// here too; the reachability test is the guard that fails when one is
/// missed.
const PANEL_NAMES: &[PanelName] = &[
    PanelName { id: "appmenu", label: "App Menu", icon: "\u{F003B}", aliases: &[] },
    PanelName { id: "audio", label: "Audio", icon: "\u{F057E}", aliases: &[] },
    PanelName { id: "calendar", label: "Calendar", icon: "\u{F00EE}", aliases: &[] },
    PanelName { id: "network", label: "Network", icon: "\u{F05A9}", aliases: &[] },
    PanelName { id: "bluetooth", label: "Bluetooth", icon: "\u{F00AF}", aliases: &[] },
    PanelName { id: "earbuds", label: "Earbuds", icon: "\u{F184F}", aliases: &[] },
    PanelName { id: "iphone", label: "iPhone", icon: "\u{F011C}", aliases: &[] },
    PanelName { id: "dualsense", label: "DualSense", icon: "\u{F0297}", aliases: &[] },
    PanelName { id: "power", label: "Power", icon: "\u{F0079}", aliases: &[] },
    PanelName { id: "weather", label: "Weather", icon: "\u{F0599}", aliases: &[] },
    PanelName { id: "media", label: "Media", icon: "\u{F0387}", aliases: &[] },
    PanelName { id: "github", label: "GitHub", icon: "\u{F408}", aliases: &[] },
    PanelName { id: "usage", label: "Usage", icon: "\u{F16A3}", aliases: &[] },
    PanelName { id: "tailscale", label: "Tailscale", icon: "\u{F0318}", aliases: &[] },
    PanelName { id: "systemupdate", label: "System Update", icon: "\u{F03D3}", aliases: &[] },
    PanelName { id: "display", label: "Display", icon: "\u{F0379}", aliases: &["mirror display", "screen mirroring", "screens", "outputs"] },
    PanelName { id: "monitor", label: "Monitor", icon: "\u{F029A}", aliases: &[] },
    PanelName { id: "radio", label: "Radio", icon: "\u{F0439}", aliases: &[] },
];

pub fn panels_provider(ipc: &str) -> Vec<Node> {
    PANEL_NAMES
        .iter()
        .map(|p| Node {
            icon: p.icon.to_string(),
            aliases: p.aliases.iter().map(|a| a.to_string()).collect(),
            action: Some(format!("{ipc} call panel open {}", p.id)),
            ..Node::new(format!("panels.{}", p.id), p.label, Kind::Action)
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
/// a bar-cell click. Mirrors the tray IPC target's `activate(id)` exactly.
/// `shq` guards an id containing whitespace from splitting into extra argv
/// tokens on the way through `sh -c`. An empty tray renders one dim row, never
/// an empty level.
pub fn tray_provider(items: &[TrayItem], ipc: &str) -> Vec<Node> {
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
                action: Some(format!("{ipc} call tray activate {}", shq(&item.id))),
                ..Node::new(format!("tray.{}", item.id), label, Kind::Action)
            }
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuOutput {
    pub name: String,
    pub connected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuCard {
    pub card: String,
    pub name: String,
    pub discrete: bool,
    pub driver: String,
    pub outputs: Vec<GpuOutput>,
}

fn gpu_outputs_desc(outputs: &[GpuOutput]) -> String {
    let connected: Vec<&str> = outputs.iter().filter(|o| o.connected).map(|o| o.name.as_str()).collect();
    if connected.is_empty() { "no outputs".to_string() } else { connected.join(", ") }
}

/// One inert note row per card: name, integrated/discrete, driver, connected
/// outputs. No cards renders an honest "No GPU" row instead of an empty
/// level. There is no "launch on GPU" route: it was a second full copy of the
/// app list in every root search, for a choice the Shift+Enter accelerator on
/// the app row already offers.
pub fn gpu_provider(cards: &[GpuCard]) -> Vec<Node> {
    if cards.is_empty() {
        return vec![Node::note("gpu.empty", "No GPU")];
    }
    cards
        .iter()
        .map(|card| Node {
            desc: Some(format!(
                "{} \u{b7} {} \u{b7} {}",
                if card.discrete { "Discrete" } else { "Integrated" },
                card.driver,
                gpu_outputs_desc(&card.outputs)
            )),
            ..Node::new(
                format!("gpu.card.{}", card.card),
                if card.name.is_empty() { &card.card } else { &card.name },
                Kind::Note,
            )
        })
        .collect()
}

/// The offload launch Shift+Enter runs against the cursor's app row. `shq`
/// guards a desktop id containing characters a bare interpolation would
/// break (flatpak's reverse-DNS ids can carry a trailing instance suffix).
pub fn gpu_launch_action(ipc: &str, desktop_id: &str, card: &str) -> String {
    format!("{ipc} call monitor launch {} {}", shq(desktop_id), shq(card))
}

/// The "gpu.mode" fragment, gated on supergfxctl's own presence rather than
/// on card count, so a machine without it has nothing at all rather than an
/// empty placeholder. `supported` is the parsed `gfx` state's flag.
pub fn gpu_mode_entry(ipc: &str, supported: bool) -> Entries {
    let mut e = Entries::new();
    if !supported {
        return e;
    }
    let call = format!("{ipc} call ");
    e.insert("gpu.mode".into(), Entry::labelled("GPU Mode").with_icon("\u{F04E1}"));
    e.insert(
        "gpu.mode.integrated".into(),
        Entry::labelled("Integrated").with_icon("\u{F0322}").with_action(format!("{call}monitor mode integrated")),
    );
    e.insert(
        "gpu.mode.hybrid".into(),
        Entry::labelled("Hybrid").with_icon("\u{F00FB}").with_action(format!("{call}monitor mode hybrid")),
    );
    e
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
            .with_action(format!("@ipc:lights.{verb}:{value}"))
            .with_checked(format!("@state:lights.{path}={value}"))
            .keeping_open()
    }
    out.insert(
        "lights".into(),
        Entry::labelled("Keyboard Lights").with_aliases(&["rgb", "aura", "led", "leds", "backlight"]),
    );
    out.insert(
        "lights.power".into(),
        Entry::labelled("Lights On")
            .with_action("@ipc:lights.toggle")
            .with_checked("@state:lights.on")
            .keeping_open(),
    );
    out.insert("lights.effect".into(), Entry::labelled("Effect"));
    out.insert("lights.color".into(), Entry::labelled("Color"));
    out.insert("lights.source".into(), Entry::labelled("Color Source"));
    out.insert("lights.source.wallpaper".into(), choice("source", "source", "wallpaper", "Wallpaper"));
    out.insert("lights.source.custom".into(), choice("source", "source", "custom", "Custom Color"));
    out.insert("lights.speed".into(), Entry::labelled("Speed"));
    out.insert("lights.brightness".into(), Entry::labelled("Brightness"));
    for e in effects {
        out.insert(format!("lights.effect.{}", e.id), choice("effect", "effect", &e.id, &e.label));
    }
    for (hex, label) in LIGHT_COLORS {
        out.insert(format!("lights.color.{hex}"), choice("color", "colour", hex, label));
    }
    out.insert("lights.color.hex".into(), Entry::labelled("Hex Color").with_action("@ipc:lights.colorInput"));
    for (id, label) in [("low", "Low"), ("med", "Medium"), ("high", "High")] {
        out.insert(format!("lights.speed.{id}"), choice("speed", "speed", id, label));
    }
    for (id, label) in [("1", "Low"), ("2", "Medium"), ("3", "High")] {
        out.insert(format!("lights.brightness.{id}"), choice("brightness", "brightness", id, label));
    }
    out
}
