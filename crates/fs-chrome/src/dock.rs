//! The dock's contents and whether it exists at all
//! (docs/superpowers/specs/2026-11-11-mac-apps.md, section 4). The dock is
//! pointer chrome for every app: under `dock.show: "always"` it exists while
//! anything is pinned or open, and under `"mac"` it has no surface, not even
//! its hot edge, until a Mac app runs.

use fs_types::WindowId;

/// `dock.show`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Show {
    /// Only while a Mac app runs, with a window or without.
    Mac,
    /// While anything is pinned or open.
    #[default]
    Always,
    Never,
}

impl Show {
    pub fn parse(word: &str) -> Option<Show> {
        Some(match word {
            "mac" => Show::Mac,
            "always" => Show::Always,
            "never" => Show::Never,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Show::Mac => "mac",
            Show::Always => "always",
            Show::Never => "never",
        }
    }
}

/// One window, under the app the dock files it with: its desktop entry's
/// id, or its own app id where no entry matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub app: String,
    pub window: WindowId,
    pub mac: bool,
    pub urgent: bool,
    pub focused: bool,
}

/// A Mac app running with no window, which a Mac app does once its last
/// window closes: filed under its desktop entry's id like a window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Windowless {
    pub app: String,
    pub pid: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub app: String,
    pub pinned: bool,
    /// One of its windows or processes is a Mac app's.
    pub mac: bool,
    /// In the order the compositor lists them.
    pub windows: Vec<WindowId>,
    /// Its Mac app processes that have no window.
    pub windowless: Vec<u32>,
    pub urgent: bool,
    pub focused: bool,
}

impl Item {
    pub fn running(&self) -> bool {
        !self.windows.is_empty() || !self.windowless.is_empty()
    }

    /// The window a click goes to: the one after `focused` among its own,
    /// round to the first, so clicks walk an app's windows.
    pub fn next_window(&self, focused: Option<&WindowId>) -> Option<&WindowId> {
        let at = focused.and_then(|f| self.windows.iter().position(|w| w == f));
        match at {
            Some(i) => self.windows.get((i + 1) % self.windows.len()),
            None => self.windows.first(),
        }
    }
}

/// The pinned apps in pin order, then the open ones that are not pinned in
/// the order their first window is listed, then the Mac apps running with
/// no window. Ids compare without case, as app ids and desktop ids do.
pub fn items(pins: &[String], open: &[Open], windowless: &[Windowless]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for pin in pins {
        if !pin.is_empty() && !out.iter().any(|i| i.app.eq_ignore_ascii_case(pin)) {
            out.push(Item { app: pin.clone(), pinned: true, ..Item::default() });
        }
    }
    fn slot<'a>(out: &'a mut Vec<Item>, app: &str) -> &'a mut Item {
        let at = match out.iter().position(|i| i.app.eq_ignore_ascii_case(app)) {
            Some(at) => at,
            None => {
                out.push(Item { app: app.to_owned(), ..Item::default() });
                out.len() - 1
            }
        };
        &mut out[at]
    }
    for o in open.iter().filter(|o| !o.app.is_empty()) {
        let item = slot(&mut out, &o.app);
        item.windows.push(o.window.clone());
        item.mac |= o.mac;
        item.urgent |= o.urgent;
        item.focused |= o.focused;
    }
    for w in windowless.iter().filter(|w| !w.app.is_empty()) {
        let item = slot(&mut out, &w.app);
        item.windowless.push(w.pid);
        item.mac = true;
    }
    out
}

/// Where the rule between the pinned apps and the rest goes: before this
/// index, none when either side is empty.
pub fn rule(items: &[Item]) -> Option<usize> {
    let pinned = items.iter().take_while(|i| i.pinned).count();
    (pinned > 0 && pinned < items.len()).then_some(pinned)
}

/// Whether the dock has any surface.
pub fn exists(show: Show, items: &[Item]) -> bool {
    match show {
        Show::Never => false,
        Show::Always => !items.is_empty(),
        Show::Mac => items.iter().any(|i| i.mac && i.running()),
    }
}

/// What to call an app with no desktop entry to name it. A Mac app goes by
/// its bundle id, whose last part is as near its name as the id gets.
pub fn unnamed(item: &Item) -> &str {
    let last = item.app.rsplit('.').next().unwrap_or_default();
    if item.mac && !last.is_empty() { last } else { &item.app }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(app: &str, window: &str, mac: bool) -> Open {
        Open { app: app.into(), window: WindowId::from(window.to_owned()), mac, urgent: false, focused: false }
    }

    fn apps(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.app.as_str()).collect()
    }

    #[test]
    fn pinned_first_then_open_in_window_order() {
        let pins = ["firefox".to_owned(), "org.gnome.Nautilus".to_owned()];
        let open = [open("foot", "a", false), open("Firefox", "b", false), open("foot", "c", false), open("com.coteditor.CotEditor", "d", true)];
        let items = items(&pins, &open, &[]);
        assert_eq!(apps(&items), ["firefox", "org.gnome.Nautilus", "foot", "com.coteditor.CotEditor"]);
        assert_eq!(items[0].windows.len(), 1);
        assert!(items[0].pinned && items[0].running());
        assert!(items[1].pinned && !items[1].running());
        assert_eq!(items[2].windows.len(), 2);
        assert!(items[3].mac && !items[2].mac);
        assert_eq!(rule(&items), Some(2));
        assert_eq!(unnamed(&items[3]), "CotEditor");
        assert_eq!(unnamed(&items[1]), "org.gnome.Nautilus");
    }

    #[test]
    fn no_rule_with_one_side_empty() {
        assert_eq!(rule(&items(&[], &[open("foot", "a", false)], &[])), None);
        assert_eq!(rule(&items(&["foot".to_owned()], &[open("foot", "a", false)], &[])), None);
    }

    #[test]
    fn exists_by_what_dock_show_says() {
        let native = items(&["firefox".to_owned()], &[open("foot", "a", false)], &[]);
        assert!(!exists(Show::Mac, &native));
        assert!(exists(Show::Always, &native));
        assert!(!exists(Show::Never, &native));
        let mac = items(&[], &[open("foot", "a", false), open("com.coteditor.CotEditor", "b", true)], &[]);
        assert!(exists(Show::Mac, &mac));
        assert!(!exists(Show::Never, &mac));
        assert!(!exists(Show::Always, &[]));
    }

    #[test]
    fn a_mac_app_with_no_window_still_runs() {
        let windowless = [Windowless { app: "com.example.app".into(), pid: 40 }];
        let items = items(&["com.example.App".to_owned()], &[open("foot", "a", false)], &windowless);
        assert_eq!(apps(&items), ["com.example.App", "foot"]);
        assert!(items[0].pinned && items[0].mac && items[0].running());
        assert_eq!((items[0].windows.len(), items[0].windowless.as_slice()), (0, [40].as_slice()));
        assert!(exists(Show::Mac, &items));
        assert_eq!(items[0].next_window(None), None);
    }

    #[test]
    fn clicks_walk_an_apps_windows() {
        let items = items(&[], &[open("foot", "a", false), open("foot", "b", false)], &[]);
        let (a, b) = (WindowId::from("a".to_owned()), WindowId::from("b".to_owned()));
        let other = WindowId::from("z".to_owned());
        assert_eq!(items[0].next_window(None), Some(&a));
        assert_eq!(items[0].next_window(Some(&other)), Some(&a));
        assert_eq!(items[0].next_window(Some(&a)), Some(&b));
        assert_eq!(items[0].next_window(Some(&b)), Some(&a));
        assert_eq!(Item::default().next_window(None), None);
    }

    #[test]
    fn show_round_trips() {
        for show in [Show::Mac, Show::Always, Show::Never] {
            assert_eq!(Show::parse(show.name()), Some(show));
        }
        assert_eq!(Show::parse("sometimes"), None);
    }
}
