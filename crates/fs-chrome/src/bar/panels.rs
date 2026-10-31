//! Which bar widget opens which panel, the one place that mapping lives.
//!
//! The two vocabularies differ where a widget and its panel were never named
//! alike ("battery" opens "power", "clock" opens "calendar", "systemUpdate"
//! registers lowercase), which is why this is a table and not a lowercase
//! call. "microphone" is here because its middle click opens the audio panel;
//! it has none of its own. "activeWindow" is deliberately absent even though
//! it opens the app menu panel: that panel drives the focused window's own
//! menu rather than a shell surface, so a positional keybind landing on it
//! would open something different on every window. Everything else left out
//! (tray, bell, indicators, chevron, launcher, visualizer, keyboardLayout)
//! opens no panel at all, and neither `custom:` modules nor `plugin:` entries
//! are keyed here since [`panel_at`] only looks at builtins.

use super::layout::{Builtin, EntryKind, Resolved};

/// The panels `panel` can open, each one's module in the shell.
pub const PANELS: [&str; 26] = [
    "activity",
    "appmenu",
    "audio",
    "calendar",
    "network",
    "bluetooth",
    "earbuds",
    "iphone",
    "dualsense",
    "power",
    "weather",
    "media",
    "github",
    "usage",
    "tailscale",
    "systemupdate",
    "display",
    "monitor",
    TRAY_OVERFLOW,
    "radio",
    "info",
    "settings",
    "settings:appearance",
    "settings:bar",
    "settings:desktop",
    "settings:system",
];

/// The tray's second bar, which the launcher reaches through its "tray"
/// route rather than a Panels row.
pub const TRAY_OVERFLOW: &str = "trayoverflow";

pub fn known(name: &str) -> Option<&'static str> {
    PANELS.iter().find(|n| **n == name).copied()
}

/// The launcher route a panel lives in: the launcher card holds these, centred
/// like Pond's settings, rather than a card hanging off the bar.
pub fn launcher_route(name: &str) -> Option<&'static str> {
    match name {
        "info" => Some("info"),
        "settings" => Some("settings"),
        page if page.starts_with("settings:") && known(page).is_some() => Some("settings"),
        _ => None,
    }
}

/// The panel registry key a widget opens, if it opens one.
pub fn widget_panel(widget: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match widget {
        Audio | Microphone => "audio",
        Battery => "power",
        Bluetooth => "bluetooth",
        Clock => "calendar",
        Display => "display",
        Dualsense => "dualsense",
        Earbuds => "earbuds",
        Github => "github",
        Iphone => "iphone",
        Monitor => "monitor",
        Network => "network",
        NowPlaying => "media",
        SystemUpdate => "systemupdate",
        Tailscale => "tailscale",
        Usage => "usage",
        Weather => "weather",
        Launcher | Workspaces | ActiveWindow | Tray | Visualizer | Bell | Indicators
        | KeyboardLayout | Chevron => {
            return None;
        }
    })
}

/// The nth panel-bearing cell of the resolved right region, 1-based, in
/// layout order (leftmost first, which for a right-anchored region is "from
/// the screen centre outward"). Cells that open no panel are skipped rather
/// than counted, and a cell hidden behind a collapsed chevron still counts:
/// the keybind addresses the layout the user configured, not what is on
/// screen this second. Out of range, including `n < 1`, is `None`.
pub fn panel_at(resolved: &Resolved, n: i64) -> Option<&'static str> {
    let mut seen = 0;
    for entry in &resolved.regions.right {
        let EntryKind::Builtin(widget) = entry.kind else {
            continue;
        };
        let Some(panel) = widget_panel(widget) else {
            continue;
        };
        seen += 1;
        if seen == n {
            return Some(panel);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::bar::layout::{collapsed_names, resolve};

    /// The JS answered "" for no panel.
    fn at(bar: Option<Value>, n: i64) -> &'static str {
        panel_at(&resolve(bar.as_ref(), &[]), n).unwrap_or("")
    }

    #[test]
    fn default_right_region_counts_from_the_centre_out() {
        assert_eq!(at(None, 1), "power");
        assert_eq!(at(None, 2), "audio");
        assert_eq!(at(None, 3), "network");
        assert_eq!(at(None, 4), "bluetooth");
        assert_eq!(at(None, 5), "weather");
    }

    #[test]
    fn out_of_range_is_empty() {
        assert_eq!(at(None, 6), "");
        assert_eq!(at(None, 0), "");
        assert_eq!(at(None, -1), "");
    }

    #[test]
    fn tray_bell_and_indicators_are_not_counted() {
        let bar = json!({ "layout": { "right": ["tray", "bell", "indicators", "audio"] } });
        assert_eq!(at(Some(bar.clone()), 1), "audio");
        assert_eq!(at(Some(bar), 2), "");
    }

    // The resolver annotates the first four collapsible here (a right
    // region's chevron governs what precedes it), and the count has to be
    // blind to that: the keybind addresses the configured layout.
    #[test]
    fn chevron_and_the_cells_it_hides_still_count() {
        let bar = json!({ "layout": { "right": ["battery", "audio", "network", "bluetooth", "chevron", "weather"] } });
        let resolved = resolve(Some(&bar), &[]);
        assert_eq!(
            collapsed_names(&resolved.regions.right).join(","),
            "battery,audio,network,bluetooth"
        );
        assert_eq!(panel_at(&resolved, 1), Some("power"));
        assert_eq!(panel_at(&resolved, 4), Some("bluetooth"));
        assert_eq!(panel_at(&resolved, 5), Some("weather"));
        assert_eq!(panel_at(&resolved, 6), None);
    }

    #[test]
    fn unknown_widget_names_are_skipped() {
        let bar =
            json!({ "layout": { "right": ["notreal", "weather", "alsonotreal", "monitor"] } });
        assert_eq!(at(Some(bar.clone()), 1), "weather");
        assert_eq!(at(Some(bar.clone()), 2), "monitor");
        assert_eq!(at(Some(bar), 3), "");
    }

    #[test]
    fn modules_and_activewindow_are_skipped() {
        let bar = json!({
            "layout": { "right": ["custom:disk", "activeWindow", "usage"] },
            "modules": [{ "id": "disk", "type": "command", "command": ["echo", "hi"] }]
        });
        assert_eq!(at(Some(bar.clone()), 1), "usage");
        assert_eq!(at(Some(bar), 2), "");
    }

    #[test]
    fn widget_and_panel_names_differ_where_the_registry_does() {
        let bar = json!({ "layout": { "right": ["battery", "clock", "nowPlaying", "microphone", "systemUpdate"] } });
        assert_eq!(at(Some(bar.clone()), 1), "power");
        assert_eq!(at(Some(bar.clone()), 2), "calendar");
        assert_eq!(at(Some(bar.clone()), 3), "media");
        assert_eq!(at(Some(bar.clone()), 4), "audio");
        assert_eq!(at(Some(bar), 5), "systemupdate");
    }

    #[test]
    fn only_the_right_region_is_counted() {
        let bar =
            json!({ "layout": { "left": ["audio"], "center": ["network"], "right": ["weather"] } });
        assert_eq!(at(Some(bar.clone()), 1), "weather");
        assert_eq!(at(Some(bar), 2), "");
    }
}
