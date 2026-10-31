//! Route icons for the launcher. A node's own `icon` is a raw glyph string
//! (a provider, a user's `menu.jsonc`), which `Icon` cannot render: it
//! resolves a NAME through the active set. This maps the shipped route ids
//! onto Lucide names, so a row drawn from data still gets a named icon in
//! whichever set `theme.icons` selects.
//!
//! An id that is not here resolves to "", and the row falls back to the
//! node's own glyph in the mono font, which keeps a user-defined route, an
//! emoji row (its icon IS the emoji) and a provider row carrying its own
//! glyph working unchanged.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::node::{Kind, Node};

/// Root routes, the sub-rows each ships, the injected ones (`capture`,
/// `lights`, `gpu.mode`), the device routes and one row per panel.
pub const ROUTE_ICONS: &[(&str, &str)] = &[
    ("apps", "layout-grid"),
    ("clipboard", "clipboard"),
    ("calc", "calculator"),
    ("emoji", "smile"),
    ("keybinds", "keyboard"),
    ("files", "folder-open"),
    ("wallpaper", "image"),
    ("monitor", "cpu"),
    ("mirror", "camera"),
    ("settings", "settings-2"),
    ("info", "info"),
    ("panels", "grid-2x2"),
    ("tray", "inbox"),
    ("gpu", "gpu"),
    ("system", "settings"),
    ("system.lock", "lock"),
    ("system.suspend", "moon"),
    ("system.reboot", "refresh-cw"),
    ("system.shutdown", "power"),
    ("system.logout", "log-out"),
    ("system.notifications", "bell"),
    ("reminder", "alarm-clock"),
    ("reminder.set", "timer"),
    ("reminder.show", "clock"),
    ("reminder.clear", "x"),
    ("share", "share-2"),
    ("share.send", "send"),
    ("share.history", "history"),
    ("share.receive", "download"),
    ("clipssh", "terminal"),
    ("toggles", "toggle-left"),
    ("toggles.nightlight", "lightbulb"),
    ("toggles.overnight", "moon-star"),
    ("toggles.caffeinate", "coffee"),
    ("toggles.hdr", "sun"),
    ("toggles.dnd", "bell-off"),
    ("toggles.dark-mode", "moon"),
    ("lights", "keyboard"),
    ("lights.power", "power"),
    ("lights.effect", "zap"),
    ("lights.color", "palette"),
    ("lights.source", "image"),
    ("lights.speed", "gauge"),
    ("lights.brightness", "sun"),
    ("wifi", "wifi"),
    ("wifi.off", "wifi-off"),
    ("wifi.known", "history"),
    ("bluetooth", "bluetooth"),
    ("bluetooth.off", "bluetooth"),
    ("audio", "volume-2"),
    ("radio", "radio"),
    ("radio.search", "search"),
    ("radio.stop", "square"),
    ("install", "package-plus"),
    ("capture", "camera"),
    ("capture.text", "scan-text"),
    ("capture.color", "pipette"),
    ("capture.record", "video"),
    ("capture.stop", "square"),
    ("capture.gif", "film"),
    ("capture.screenshot", "camera"),
    ("capture.region", "crop"),
    ("system.console", "terminal"),
    ("system.screensaver", "monitor-off"),
    ("system.plugins", "puzzle"),
    ("system.plugins.list", "list"),
    ("system.plugins.reload", "refresh-cw"),
    ("notifications", "bell"),
    ("notifications.clear", "trash"),
    ("notifications.markAllSeen", "check-check"),
    ("notifications.dismissAll", "x"),
    ("theme", "palette"),
    ("theme.retheme", "palette"),
    ("theme.mode-dark", "moon"),
    ("theme.mode-light", "sun"),
    ("gpu.mode", "arrow-left-right"),
    ("gpu.mode.integrated", "laptop"),
    ("gpu.mode.hybrid", "git-fork"),
    ("panels.appmenu", "layout-grid"),
    ("panels.audio", "volume-2"),
    ("panels.calendar", "calendar"),
    ("panels.network", "wifi"),
    ("panels.bluetooth", "bluetooth"),
    ("panels.earbuds", "headphones"),
    ("panels.dualsense", "gamepad-2"),
    ("panels.power", "battery"),
    ("panels.weather", "sun"),
    ("panels.media", "music"),
    ("panels.github", "git-branch"),
    ("panels.usage", "bot"),
    ("panels.tailscale", "network"),
    ("panels.systemupdate", "package"),
    ("panels.display", "monitor"),
    ("panels.monitor", "gauge"),
    ("panels.iphone", "smartphone"),
    ("panels.radio", "radio"),
];

static ROUTE_ICON_INDEX: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| ROUTE_ICONS.iter().copied().collect());

/// Rows minted per device by the device routes, matched by id prefix after an
/// exact miss.
pub const ROUTE_ICON_PREFIXES: &[(&str, &str)] = &[
    ("wifi.net.", "wifi"),
    ("bluetooth.dev.", "bluetooth"),
    ("audio.output.", "volume-2"),
    ("audio.input.", "mic"),
    ("radio.fav.", "radio"),
    ("radio.result.", "radio"),
    ("wifi.details.security", "shield"),
    ("wifi.details.signal", "signal"),
    ("wifi.details.address.", "network"),
];

/// "" means "this row has no named icon": the caller draws the node's own
/// glyph instead.
pub fn icon_for(node: Option<&Node>) -> &'static str {
    let Some(node) = node.filter(|n| !n.id.is_empty()) else { return "" };
    if let Some(name) = ROUTE_ICON_INDEX.get(node.id.as_str()) {
        return name;
    }
    ROUTE_ICON_PREFIXES
        .iter()
        .find(|(prefix, _)| node.id.starts_with(prefix))
        .map_or("", |(_, name)| name)
}

/// The named icon a row draws when it has none of its own, by what the row
/// is. A row whose own icon is a raw glyph draws this instead: the icon set
/// is `theme.icons`' choice, and a codepoint from one set is a missing-glyph
/// box in another.
pub fn fallback_for(node: Option<&Node>) -> &'static str {
    match node.map(|n| &n.kind) {
        Some(Kind::App) => "app-window",
        Some(Kind::Submenu | Kind::Provider | Kind::Link) => "folder",
        Some(Kind::Action) => "terminal",
        Some(Kind::Other(k)) if k == crate::actions::WINDOW_KIND => "app-window",
        Some(Kind::Other(k)) if k == crate::providers::FOLDER_KIND => "folder",
        Some(Kind::Other(k)) if k == crate::providers::PACKAGE_KIND => "package",
        Some(Kind::Other(k)) if k == crate::providers::FILE_KIND => {
            if node.is_some_and(|n| crate::providers::is_image(&n.path)) { "file-image" } else { "file" }
        }
        _ => "circle-help",
    }
}
