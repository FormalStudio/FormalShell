//! The panel table: every panel's name, launcher row, width, home and the
//! bar widgets that open it, in one place.
//!
//! A widget and its panel were not always named alike ("battery" opens
//! "power", "clock" opens "calendar", "systemUpdate" opens "systemupdate"),
//! which is why the widgets are listed rather than derived. "activeWindow"
//! is deliberately absent even though it opens the app menu panel: that
//! panel drives the focused window's own menu rather than a shell surface,
//! so a positional keybind landing on it would open something different on
//! every window. Tray, bell, indicators, chevron, launcher, visualizer and
//! keyboardLayout open no panel of the table, and neither `custom:` modules
//! nor `plugin:` entries are keyed here since [`panel_at`] only looks at
//! builtins.

use std::fmt;

use super::layout::{Builtin, EntryKind, Resolved};

/// Every panel the `panel` verbs name, plugins' aside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelId {
    Activity,
    AppMenu,
    Audio,
    Calendar,
    Network,
    Bluetooth,
    Earbuds,
    Iphone,
    Dualsense,
    Power,
    Weather,
    Media,
    Github,
    Usage,
    Tailscale,
    SystemUpdate,
    Display,
    Monitor,
    TrayOverflow,
    Radio,
    Info,
    Settings,
    SettingsAppearance,
    SettingsBar,
    SettingsDesktop,
    SettingsSystem,
    QuickSettings,
}

/// Where a panel is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Home {
    /// A card hanging off the bar.
    Card,
    /// A page the launcher card holds, centred like Pond's settings.
    Launcher,
    /// A modal card of its own (Radio Atlas).
    Modal,
}

/// A card's width, one of the theme's: `popupWidthNarrow`, `Default`,
/// `Wide` and `Menu`, `panelWidth` and `panelWidthWide`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Narrow,
    Default,
    Wide,
    Menu,
    Panel,
    PanelWide,
}

/// One panel: its `panel` name, its launcher row's title, icon and
/// aliases, its card's width, where it is drawn and the bar widgets whose
/// click opens it.
#[derive(Clone, Copy, Debug)]
pub struct Descriptor {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    pub aliases: &'static [&'static str],
    pub width: Width,
    pub home: Home,
    /// "microphone" opens audio from its middle click, so it is here; it
    /// has no panel of its own.
    pub widgets: &'static [Builtin],
}

const fn card(id: &'static str, title: &'static str, icon: &'static str, widgets: &'static [Builtin]) -> Descriptor {
    Descriptor { id, title, icon, aliases: &[], width: Width::Default, home: Home::Card, widgets }
}

const fn page(id: &'static str, title: &'static str, icon: &'static str) -> Descriptor {
    Descriptor { id, title, icon, aliases: &[], width: Width::PanelWide, home: Home::Launcher, widgets: &[] }
}

impl PanelId {
    pub const ALL: [PanelId; 27] = {
        use PanelId::*;
        [
            Activity, AppMenu, Audio, Calendar, Network, Bluetooth, Earbuds, Iphone, Dualsense, Power, Weather, Media, Github, Usage, Tailscale, SystemUpdate,
            Display, Monitor, TrayOverflow, Radio, Info, Settings, SettingsAppearance, SettingsBar, SettingsDesktop, SettingsSystem, QuickSettings,
        ]
    };

    pub const fn desc(self) -> Descriptor {
        use Builtin as B;
        match self {
            PanelId::Activity => card("activity", "Activity", "activity", &[]),
            PanelId::AppMenu => card("appmenu", "App Menu", "layout-grid", &[]),
            PanelId::Audio => card("audio", "Audio", "volume-2", &[B::Audio, B::Microphone]),
            PanelId::Calendar => Descriptor { width: Width::Wide, ..card("calendar", "Calendar", "calendar", &[B::Clock]) },
            PanelId::Network => card("network", "Network", "wifi", &[B::Network]),
            PanelId::Bluetooth => card("bluetooth", "Bluetooth", "bluetooth", &[B::Bluetooth]),
            PanelId::Earbuds => Descriptor { width: Width::Wide, ..card("earbuds", "Earbuds", "headphones", &[B::Earbuds]) },
            PanelId::Iphone => card("iphone", "iPhone", "smartphone", &[B::Iphone]),
            PanelId::Dualsense => card("dualsense", "DualSense", "gamepad-2", &[B::Dualsense]),
            PanelId::Power => Descriptor { width: Width::Narrow, ..card("power", "Power", "battery", &[B::Battery]) },
            PanelId::Weather => card("weather", "Weather", "sun", &[B::Weather]),
            PanelId::Media => Descriptor { width: Width::Wide, ..card("media", "Media", "music", &[B::NowPlaying]) },
            PanelId::Github => card("github", "GitHub", "git-branch", &[B::Github]),
            PanelId::Usage => card("usage", "Usage", "bot", &[B::Usage]),
            PanelId::Tailscale => card("tailscale", "Tailscale", "network", &[B::Tailscale]),
            PanelId::SystemUpdate => card("systemupdate", "System Update", "package", &[B::SystemUpdate]),
            PanelId::Display => Descriptor { aliases: &["mirror display", "screen mirroring", "screens", "outputs"], ..card("display", "Display", "monitor", &[B::Display]) },
            PanelId::Monitor => Descriptor { width: Width::Wide, ..card("monitor", "Monitor", "gauge", &[B::Monitor]) },
            PanelId::TrayOverflow => Descriptor { width: Width::Panel, ..card("trayoverflow", "Tray", "inbox", &[]) },
            PanelId::Radio => Descriptor { home: Home::Modal, ..card("radio", "Radio", "radio", &[]) },
            PanelId::Info => Descriptor { width: Width::Menu, ..page("info", "Information", "info") },
            PanelId::Settings => Descriptor { width: Width::Menu, ..page("settings", "Settings", "settings-2") },
            PanelId::SettingsAppearance => page("settings:appearance", "Appearance", "paint-bucket"),
            PanelId::SettingsBar => page("settings:bar", "Bar", "panel-top"),
            PanelId::SettingsDesktop => page("settings:desktop", "Desktop", "monitor"),
            PanelId::SettingsSystem => page("settings:system", "System", "hexagon"),
            PanelId::QuickSettings => Descriptor { aliases: &["control center", "toggles"], width: Width::PanelWide, ..card("quicksettings", "Quick Settings", "sliders-horizontal", &[]) },
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.desc().id
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == name)
    }

    pub const fn home(self) -> Home {
        self.desc().home
    }

    /// The launcher route holding a page: its own, or Settings for a
    /// settings page.
    pub fn route(self) -> Option<PanelId> {
        match self {
            PanelId::SettingsAppearance | PanelId::SettingsBar | PanelId::SettingsDesktop | PanelId::SettingsSystem => Some(PanelId::Settings),
            p if p.home() == Home::Launcher => Some(p),
            _ => None,
        }
    }

    /// The panel a widget's click opens, if it opens one.
    pub fn opened_by(widget: Builtin) -> Option<PanelId> {
        Self::ALL.into_iter().find(|p| p.desc().widgets.contains(&widget))
    }
}

impl fmt::Display for PanelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The nth panel-bearing cell of the resolved right region, 1-based, in
/// layout order (leftmost first, which for a right-anchored region is "from
/// the screen centre outward"). Cells that open no panel are skipped rather
/// than counted, and a cell hidden behind a collapsed chevron still counts:
/// the keybind addresses the layout the user configured, not what is on
/// screen this second. Out of range, including `n < 1`, is `None`.
pub fn panel_at(resolved: &Resolved, n: i64) -> Option<PanelId> {
    let mut seen = 0;
    for entry in &resolved.regions.right {
        let EntryKind::Builtin(widget) = entry.kind else {
            continue;
        };
        let Some(panel) = PanelId::opened_by(widget) else {
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

    /// "" for no panel.
    fn at(bar: Option<Value>, n: i64) -> &'static str {
        panel_at(&resolve(bar.as_ref(), &[]), n).map_or("", PanelId::as_str)
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
        assert_eq!(panel_at(&resolved, 1), Some(PanelId::Power));
        assert_eq!(panel_at(&resolved, 4), Some(PanelId::Bluetooth));
        assert_eq!(panel_at(&resolved, 5), Some(PanelId::Weather));
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
