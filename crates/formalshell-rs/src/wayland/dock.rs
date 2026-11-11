//! The dock (docs/superpowers/specs/2026-11-11-mac-apps.md, section 4):
//! pointer chrome for Mac apps. Under `dock.show: "mac"` it has no surface
//! at all until a Mac app has a window. Then a one pixel strip lies along
//! the bottom edge (the top one under a bottom bar), and the pointer
//! resting on it for `dock.dwell` brings the card out of that edge's line.
//! The card takes no space and no keyboard. It goes back `HIDE` after the
//! pointer leaves it, at once when another window takes focus with the
//! pointer off it, and stays while its own menu is up or `dock open` holds
//! it.
//!
//! An app's place in it goes by its desktop entry's id where the icon
//! probe finds one, else by its window's app id, which is what a Mac app's
//! LauncherEntry names as well.

use std::time::{Duration, Instant};

use fs_activity::{Origin, State as ItemState};
use fs_chrome::dock::{self as model, Item, Open, Show};
use fs_types::{Edge, IRect, Window, WindowId};
use serde_json::{Value, json};
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};

use super::App;
use super::tray_menu::{Hang, Source};
use crate::input::{BTN_MIDDLE, BTN_RIGHT};
use crate::services::appicon::{self, Query};
use crate::services::tray::MenuNode;
use crate::services::{appmenu, compositor, launch, state};
use crate::settings;
use crate::surface::Ignore;
use crate::surfaces::card::{Card, CardSpec, Sizing};
use crate::surfaces::dock::{self, ICON, Mark, Press};
use crate::surfaces::popup::Popup;
use crate::surfaces::tooltip;
use crate::ui::On;
use crate::ui::el::Pic;

const NAMESPACE: &str = "formalshell:dock";
const HIDE: Duration = Duration::from_millis(400);
/// How long an app asking for attention holds the dock out: its hops.
const ASKING: Duration = Duration::from_millis(1600);

/// The dock menu's own rows, under ids no app's dbusmenu uses.
const KEEP: i32 = -2;
const QUIT: i32 = -3;
const NEW: i32 = -4;
const WINDOWS: i32 = -1000;
const ACTIONS: i32 = -2000;
const RULES: i32 = -9000;

/// The transparent strip the pointer rests on.
struct Strip {
    layer: LayerSurface,
    viewport: WpViewport,
    buffer: Option<WlBuffer>,
    /// What it was built from: the output, the edge and the room left at
    /// either end.
    key: (String, Edge, i32, i32),
}

impl Drop for Strip {
    fn drop(&mut self) {
        self.viewport.destroy();
        if let Some(b) = self.buffer.take() {
            b.destroy();
        }
    }
}

pub(super) struct Shown {
    pub(super) popup: Popup,
    edge: Edge,
    /// What a button went down on, and the button.
    pressed: Option<(On, u32)>,
    /// How many apps and whether the rule stood when the card was sized.
    sized: (usize, bool),
}

/// One app in the dock, with what its marks are read by.
struct Entry {
    item: Item,
    /// The ids its LauncherEntry and its launch may go by, lower case.
    keys: Vec<String>,
    /// A window's process, the Mac app's own where it has one.
    pid: Option<u32>,
    /// The icon's key in `store.appicon`.
    icon: String,
}

/// The app the open menu is for, and what its own rows act on.
struct MenuFor {
    app: String,
    pid: Option<u32>,
    windows: Vec<WindowId>,
    actions: Vec<String>,
}

#[derive(Default)]
pub struct Dock {
    entries: Vec<Entry>,
    /// Whether it has any surface (`fs_chrome::dock::exists`).
    exists: bool,
    strip: Option<Strip>,
    pub(super) card: Option<Shown>,
    /// `dock open` holds it out until `dock close`.
    held: bool,
    /// The pointer is on the strip or the card.
    over: bool,
    dwell_at: Option<Instant>,
    hide_at: Option<Instant>,
    /// The apps asking for attention at the last sync.
    asking: Vec<String>,
    focused: Option<WindowId>,
    queries: Vec<Query>,
    menu: Option<MenuFor>,
}

fn app_of(w: &Window) -> &str {
    if w.app_id.is_empty() { &w.initial_class } else { &w.app_id }
}

impl App {
    /// The dock's edge: the bottom, or the top where the bar has the bottom.
    fn dock_edge(&self) -> Edge {
        if self.chrome().edge == Edge::Bottom { Edge::Top } else { Edge::Bottom }
    }

    fn dock_scale(&self) -> f64 {
        self.store.display.scales.primary()
    }

    /// The windows, the pins or the marks moved: the dock's apps are read
    /// again, and it comes, goes or asks for attention as they say.
    pub(super) fn dock_sync(&mut self) {
        if !self.store.config.loaded {
            return;
        }
        let now = Instant::now();
        let show = Show::parse(settings::DOCK_SHOW.get(&self.store.config)).unwrap_or_default();
        let scale = self.dock_scale();
        let snapshot = &self.store.compositor.snapshot;
        let windows: Vec<&Window> = snapshot.windows.iter().filter(|w| !app_of(w).is_empty()).collect();
        let open: Vec<Open> = windows
            .iter()
            .map(|w| {
                let entry = self.store.appicon.get(&w.id.to_string(), ICON as u32, scale).and_then(|i| i.entry);
                Open {
                    app: entry.map_or_else(|| app_of(w).to_owned(), |e| e.id),
                    window: w.id.clone(),
                    mac: w.is_mac(),
                    urgent: w.is_urgent,
                    focused: w.id == snapshot.focused_window_id,
                }
            })
            .collect();
        let items = model::items(&self.store.state.data.dock_pins, &open);
        let exists = model::exists(show, &items);
        let focused = Some(snapshot.focused_window_id.clone());
        let refocused = focused != self.dock.focused;
        let entries: Vec<Entry> = items
            .into_iter()
            .map(|item| {
                let own: Vec<&&Window> = item.windows.iter().filter_map(|id| windows.iter().find(|w| &w.id == id)).collect();
                let mut keys: Vec<String> = std::iter::once(item.app.as_str())
                    .chain(own.iter().flat_map(|w| [w.app_id.as_str(), w.initial_class.as_str()]))
                    .filter(|k| !k.is_empty())
                    .map(str::to_lowercase)
                    .collect();
                keys.sort();
                keys.dedup();
                let pid = own.iter().find(|w| w.is_mac()).or(own.first()).and_then(|w| u32::try_from(w.pid).ok());
                let icon = own.first().map_or_else(|| format!("dock:{}", item.app), |w| w.id.to_string());
                Entry { item, keys, pid, icon }
            })
            .collect();
        let queries: Vec<Query> = windows
            .iter()
            .map(|w| Query {
                id: w.id.to_string(),
                app_id: w.app_id.clone(),
                initial_class: w.initial_class.clone(),
                initial_title: w.initial_title.clone(),
                pid: w.pid,
            })
            .chain(entries.iter().filter(|e| !e.item.running()).map(|e| Query {
                id: e.icon.clone(),
                app_id: e.item.app.clone(),
                initial_class: e.item.app.clone(),
                initial_title: String::new(),
                pid: 0,
            }))
            .collect();
        self.dock.focused = focused;
        self.dock.entries = entries;
        self.dock.exists = exists;
        if !exists {
            self.dock.held = false;
            self.dock.queries.clear();
            self.dock.asking.clear();
            self.dock_hide(now);
            return;
        }
        if queries != self.dock.queries {
            appicon::probe(ICON as u32, scale, queries.clone());
            self.dock.queries = queries;
        }
        let asking: Vec<String> = self.dock.entries.iter().filter(|e| self.dock_urgent(e)).map(|e| e.item.app.clone()).collect();
        let rose = asking.iter().any(|app| !self.dock.asking.contains(app));
        self.dock.asking = asking;
        if rose {
            self.dock_reveal(now);
            if !self.dock.over && !self.dock.held {
                self.dock.hide_at = Some(now + ASKING);
            }
        } else if refocused && !self.dock.over && !self.dock.held && self.dock_menu_open().is_none() {
            self.dock_hide(now);
        }
    }

    fn dock_badge(&self, entry: &Entry) -> Option<&fs_activity::Badge> {
        self.store.activity.activity.badges().iter().find(|(app, _)| entry.keys.contains(&app.to_lowercase())).map(|(_, b)| b)
    }

    fn dock_urgent(&self, entry: &Entry) -> bool {
        entry.item.urgent || self.dock_badge(entry).is_some_and(|b| b.urgent)
    }

    /// The apps as the row draws them.
    fn dock_marks(&self) -> (Vec<Mark>, Option<usize>) {
        let scale = self.dock_scale();
        let activity = &self.store.activity.activity;
        let items: Vec<Item> = self.dock.entries.iter().map(|e| e.item.clone()).collect();
        let marks = self
            .dock
            .entries
            .iter()
            .map(|e| {
                let icon = self.store.appicon.get(&e.icon, ICON as u32, scale);
                let name = icon.as_ref().and_then(|i| i.entry.as_ref()).map(|entry| entry.name.clone()).filter(|n| !n.is_empty());
                let named = |app: &Option<String>| app.as_ref().is_some_and(|a| e.keys.contains(&a.to_lowercase()));
                let running = |i: &&fs_activity::Item| i.state == ItemState::Running && named(&i.app);
                let progress = activity.live().iter().filter(running).find(|i| i.origin == Origin::LauncherEntry);
                Mark {
                    app: e.item.app.clone(),
                    name: name.unwrap_or_else(|| e.item.app.clone()),
                    image: Pic(icon.and_then(|i| i.image)),
                    running: e.item.running(),
                    focused: e.item.focused,
                    badge: self.dock_badge(e).and_then(fs_activity::Badge::label),
                    urgent: self.dock_urgent(e),
                    progress: progress.and_then(|i| i.progress.fraction()).map(|f| f.as_fraction()),
                    launching: activity.live().iter().filter(running).any(|i| i.origin == Origin::Launch),
                }
            })
            .collect();
        (marks, model::rule(&items))
    }

    /// The card's (width, height) for the apps it holds.
    fn dock_size(&self) -> (f64, f64) {
        let theme = &self.store.theme.theme;
        let rule = self.dock.entries.iter().take_while(|e| e.item.pinned).count();
        let ruled = rule > 0 && rule < self.dock.entries.len();
        let (w, h) = dock::size(theme, self.dock.entries.len(), ruled);
        let pad = theme.space.panel_padding;
        (w + pad * 2.0, h + pad * 2.0)
    }

    /// The strip on the dock's edge of the live bar's output, clear of the
    /// hot corners at its ends and of a bar down either side. None while
    /// the dock has no surface or a fullscreen window holds the output.
    fn dock_strip_sync(&mut self) {
        let chrome = self.chrome();
        let output = self.bar_wl().cloned().filter(|_| self.dock.exists && !chrome.bar_covered());
        let Some(output) = output else {
            self.dock.strip = None;
            self.dock.dwell_at = None;
            return;
        };
        let edge = self.dock_edge();
        let corner = self.hot_corner_size();
        let (left, right) = (corner.max(chrome.inset(Edge::Left).ceil() as i32), corner.max(chrome.inset(Edge::Right).ceil() as i32));
        let key = (self.output_name(&output), edge, left, right);
        if self.dock.strip.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Top, Some(NAMESPACE), Some(&output));
        layer.set_anchor(Anchor::LEFT | Anchor::RIGHT | if edge == Edge::Top { Anchor::TOP } else { Anchor::BOTTOM });
        layer.set_size(0, 1);
        layer.set_margin(0, right, 0, left);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        let viewport = self.pixels.viewporter.get_viewport(layer.wl_surface(), &self.qh, Ignore);
        self.dock.strip = Some(Strip { layer, viewport, buffer: None, key });
        self.dock.dwell_at = None;
    }

    pub(super) fn dock_strip_owns(&self, surface: &WlSurface) -> bool {
        self.dock.strip.as_ref().is_some_and(|s| s.layer.wl_surface() == surface)
    }

    /// Mapped with one transparent pixel stretched along the edge.
    pub(super) fn dock_strip_configure(&mut self, width: i32) {
        let Some(s) = self.dock.strip.as_mut().filter(|s| s.buffer.is_none() && width > 0) else { return };
        let buffer = self.pixels.single_pixel.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, Ignore);
        s.viewport.set_destination(width, 1);
        let surface = s.layer.wl_surface();
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, 1, 1);
        s.layer.commit();
        s.buffer = Some(buffer);
    }

    pub(super) fn dock_output_gone(&mut self, output: &WlOutput) {
        if self.bar_wl() == Some(output) {
            self.dock.strip = None;
            self.dock.card = None;
        }
    }

    /// The pointer on the strip: resting there brings the dock out, a
    /// click at once.
    pub(super) fn dock_strip_pointer(&mut self, kind: &PointerEventKind) {
        let now = Instant::now();
        match kind {
            PointerEventKind::Enter { .. } => {
                self.dock.over = true;
                self.dock.hide_at = None;
                if !self.dock.card.as_ref().is_some_and(|d| d.popup.is_open()) {
                    let dwell = settings::DOCK_DWELL.get(&self.store.config).max(0.0);
                    self.dock.dwell_at = Some(now + Duration::from_millis(dwell as u64));
                }
            }
            PointerEventKind::Leave { .. } => {
                self.dock.over = false;
                self.dock.dwell_at = None;
                self.dock.hide_at = Some(now + HIDE);
            }
            PointerEventKind::Release { .. } => self.dock_reveal(now),
            _ => {}
        }
    }

    fn dock_reveal(&mut self, now: Instant) {
        self.dock.dwell_at = None;
        if !self.dock.exists || self.chrome().bar_covered() {
            return;
        }
        match &mut self.dock.card {
            Some(d) => {
                if !d.popup.is_open() {
                    d.popup.set_open(now, true);
                }
            }
            None => self.dock_create(now),
        }
    }

    fn dock_hide(&mut self, now: Instant) {
        self.dock.hide_at = None;
        self.dock.dwell_at = None;
        if matches!(self.tray_menu.as_ref().map(|p| &p.source), Some(Source::Dock(_))) {
            self.close_tray_menu();
        }
        if let Some(d) = self.dock.card.as_mut().filter(|d| d.popup.is_open()) {
            d.popup.set_open(now, false);
            d.popup.hover(None);
            d.pressed = None;
        }
    }

    fn dock_create(&mut self, now: Instant) {
        let (ow, oh) = self.output_size();
        if ow <= 0.0 || oh <= 0.0 || self.dock.entries.is_empty() {
            return;
        }
        let (edge, chrome) = (self.dock_edge(), self.chrome());
        let size = self.dock_size();
        let theme = &self.store.theme.theme;
        let spec = CardSpec { ends: chrome.ends(edge), ..CardSpec::new(edge, chrome.inset(edge), Sizing::Card { size, anchor: ow / 2.0, length: ow as i32 }) };
        let card = Card::new(theme, spec, self.cast);
        let surface = self.popout_surface(&card, Layer::Top);
        let mut popup = Popup::new(surface, card);
        popup.set_open(now, true);
        let rule = self.dock.entries.iter().take_while(|e| e.item.pinned).count();
        let sized = (self.dock.entries.len(), rule > 0 && rule < self.dock.entries.len());
        self.dock.card = Some(Shown { popup, edge, pressed: None, sized });
        crate::trace::mapped(crate::trace::Surface::Dock, crate::trace::State::Mapped);
    }

    pub(super) fn dock_owns(&self, surface: &WlSurface) -> bool {
        self.dock.card.as_ref().is_some_and(|d| d.popup.win.owns(surface))
    }

    pub(super) fn dock_configure(&mut self) {
        if let Some(d) = &mut self.dock.card {
            d.popup.win.configure();
        }
    }

    pub(super) fn dock_frame(&mut self, now: Instant) {
        if let Some(d) = &mut self.dock.card {
            d.popup.win.frame(now);
        }
        self.sync_join();
    }

    pub(super) fn dock_closed(&mut self) {
        self.dock.card = None;
    }

    pub(super) fn dock_wake(&self) -> Option<Instant> {
        [self.dock.dwell_at, self.dock.hide_at].into_iter().flatten().min()
    }

    /// The pointer on the card: a click on an app goes to its next window
    /// or starts it, the middle button starts another, the right one opens
    /// its menu.
    pub(super) fn dock_pointer(&mut self, kind: &PointerEventKind, at: (f64, f64)) {
        let now = Instant::now();
        let dock = &mut self.dock;
        let Some(d) = &mut dock.card else { return };
        let chosen = match kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                dock.over = true;
                dock.hide_at = None;
                d.popup.hover(Some(at));
                None
            }
            PointerEventKind::Leave { .. } => {
                dock.over = false;
                dock.hide_at = Some(now + HIDE);
                d.popup.hover(None);
                d.popup.press(false);
                d.pressed = None;
                None
            }
            PointerEventKind::Press { button, .. } => {
                d.popup.hover(Some(at));
                d.popup.press(true);
                d.pressed = d.popup.hit(at.0, at.1).map(|on| (on, *button));
                None
            }
            PointerEventKind::Release { .. } => {
                d.popup.press(false);
                let here = d.popup.hit(at.0, at.1);
                let pressed = d.pressed.take().filter(|(on, _)| Some(on) == here.as_ref());
                pressed.and_then(|(on, button)| Some((on.get::<Press>()?.index, button)))
            }
            PointerEventKind::Axis { .. } => None,
        };
        match chosen {
            Some((index, BTN_RIGHT)) => self.dock_menu(index),
            Some((index, BTN_MIDDLE)) => self.dock_start(index),
            Some((index, _)) => self.dock_activate(index),
            None => {}
        }
    }

    pub(super) fn dock_hand(&self) -> bool {
        self.dock.card.as_ref().is_some_and(|d| d.popup.tooltip().is_some())
    }

    /// The name of the app under the pointer, opening away from the edge.
    pub(super) fn dock_tip(&self) -> Option<tooltip::Ask> {
        let d = self.dock.card.as_ref().filter(|d| d.popup.is_open())?;
        let (text, r) = d.popup.tooltip()?;
        let (ox, oy) = d.popup.win.card.origin(self.output_size());
        let rect = IRect::new(r.x + ox, r.y + oy, r.w, r.h);
        Some(tooltip::Ask { owner: format!("dock:{}:{}", rect.x, rect.y), text, rect, bar: Some(d.edge) })
    }

    /// The app's next window after the focused one, or the app started.
    fn dock_activate(&mut self, index: usize) {
        let Some(e) = self.dock.entries.get(index) else { return };
        match e.item.next_window(self.dock.focused.as_ref()) {
            Some(window) => compositor::focus_window(window),
            None => launch::open(&self.store, &e.item.app),
        }
    }

    /// Another instance, whatever windows it has.
    fn dock_start(&mut self, index: usize) {
        let Some(e) = self.dock.entries.get(index) else { return };
        if let Some(argv) = launch::app_argv(&self.store.menu, &e.item.app) {
            launch::spawn(&self.store, &e.item.app, &argv);
        }
    }

    fn dock_index(&self, app: &str) -> Result<usize, String> {
        self.dock.entries.iter().position(|e| e.item.app.eq_ignore_ascii_case(app)).ok_or_else(|| format!("no app '{app}' in the dock"))
    }

    /// The rows of an app's menu: its own dock menu where it is a Mac app
    /// that has one, its windows, its desktop entry's actions, then the
    /// dock's own.
    fn dock_menu_rows(&self, index: usize) -> Option<(MenuNode, MenuFor)> {
        let e = self.dock.entries.get(index)?;
        let row = |id: i32, label: &str| MenuNode {
            id,
            label: label.to_owned(),
            enabled: true,
            separator: false,
            icon: None,
            checked: false,
            has_children: false,
            shortcut: Vec::new(),
            section: false,
            children: Vec::new(),
        };
        let mut groups: Vec<Vec<MenuNode>> = Vec::new();
        if let Some(own) = self.store.appmenu.dock.as_ref().filter(|d| Some(d.pid) == e.pid) {
            groups.push(own.root.children.clone());
        }
        let snapshot = &self.store.compositor.snapshot;
        let windows: Vec<MenuNode> = e
            .item
            .windows
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let w = snapshot.windows.iter().find(|w| &w.id == id)?;
                let mut node = row(WINDOWS - i as i32, if w.title.is_empty() { &e.item.app } else { &w.title });
                node.checked = *id == snapshot.focused_window_id;
                Some(node)
            })
            .collect();
        groups.push(windows);
        let entry = self.store.appicon.get(&e.icon, ICON as u32, self.dock_scale()).and_then(|i| i.entry);
        let actions = entry.as_ref().map_or_else(Vec::new, |entry| entry.actions.clone());
        let mut launches: Vec<MenuNode> = actions.iter().enumerate().map(|(i, a)| row(ACTIONS - i as i32, &a.name)).collect();
        if entry.is_some() && e.item.running() {
            launches.insert(0, row(NEW, "New Window"));
        }
        groups.push(launches);
        let mut own = vec![row(KEEP, if e.item.pinned { "Remove from Dock" } else { "Keep in Dock" })];
        if e.item.running() {
            own.push(row(QUIT, "Quit"));
        }
        groups.push(own);
        let mut children = Vec::new();
        for (n, group) in groups.into_iter().filter(|g| !g.is_empty()).enumerate() {
            if n > 0 {
                children.push(MenuNode { separator: true, ..row(RULES - n as i32, "") });
            }
            children.extend(group);
        }
        let root = MenuNode { has_children: true, children, ..row(0, "") };
        let menu = MenuFor { app: e.item.app.clone(), pid: e.pid, windows: e.item.windows.clone(), actions: actions.into_iter().map(|a| a.key).collect() };
        Some((root, menu))
    }

    /// The menu of the app at `index`, hung off the dock over its icon.
    fn dock_menu(&mut self, index: usize) {
        let now = Instant::now();
        self.dock_reveal(now);
        let Some((root, menu)) = self.dock_menu_rows(index) else { return };
        let Some(d) = &self.dock.card else { return };
        let (ow, oh) = self.output_size();
        let theme = &self.store.theme.theme;
        let rest = d.popup.win.card.rest();
        let (cw, _) = dock::cell(theme);
        let ruled = d.sized.1 && index >= self.dock.entries.iter().take_while(|e| e.item.pinned).count();
        let along = theme.space.panel_padding + index as f64 * (cw + dock::GAP) + if ruled { dock::GAP + theme.border_width } else { 0.0 };
        let anchor = rest.x0 + along + cw / 2.0;
        let line_at = rest.y1;
        let cap = oh - line_at - theme.space.bar_margin - theme.space.screen_padding;
        let hang = Hang { edge: d.edge, line_at, length: ow as i32, cap };
        let (app, pid) = (menu.app.clone(), menu.pid.filter(|pid| appmenu::registered(*pid)));
        let title = self.dock_marks().0.get(index).map_or_else(|| app.clone(), |m| m.name.clone());
        self.dock.menu = Some(menu);
        self.open_menu_card_on(Source::Dock(app.clone()), format!("dock:{app}"), title, anchor, hang, true);
        if let Some(m) = self.tray_menu.as_mut().and_then(|p| p.menu.as_mut()) {
            m.set_tree(Some(&root));
        }
        self.resize_menu();
        if let Some(pid) = pid {
            self.runtime.service(move |ctx| appmenu::dock_menu(ctx, pid));
        }
    }

    /// The app whose menu is up.
    pub fn dock_menu_open(&self) -> Option<&str> {
        match self.tray_menu.as_ref().filter(|p| p.win.card.is_open()).map(|p| &p.source) {
            Some(Source::Dock(app)) => Some(app),
            _ => None,
        }
    }

    /// The app's own dock menu landed, or its windows changed: the open
    /// card takes the rows again, and shuts when the app left the dock.
    pub(super) fn dock_menu_changed(&mut self) {
        let Some(app) = self.dock_menu_open().map(str::to_owned) else { return };
        let rows = self.dock_index(&app).ok().and_then(|index| self.dock_menu_rows(index));
        let Some((root, menu)) = rows else { return self.close_tray_menu() };
        self.dock.menu = Some(menu);
        if let Some(m) = self.tray_menu.as_mut().and_then(|p| p.menu.as_mut()) {
            m.set_tree(Some(&root));
        }
        self.resize_menu();
    }

    pub(super) fn dock_menu_closed(&mut self, now: Instant) {
        self.dock.menu = None;
        if !self.dock.over && !self.dock.held {
            self.dock.hide_at = Some(now + HIDE);
        }
    }

    /// A row of the open menu was chosen.
    pub(super) fn dock_menu_click(&mut self, id: i32) {
        let Some(menu) = self.dock.menu.take() else { return };
        let app = menu.app.clone();
        match id {
            KEEP => {
                let _ = self.dock_pin(&app, !self.store.state.data.dock_pins.iter().any(|p| p.eq_ignore_ascii_case(&app)));
            }
            QUIT => self.dock_quit_app(menu.pid, menu.windows.clone()),
            NEW => {
                if let Some(argv) = launch::app_argv(&self.store.menu, &app) {
                    launch::spawn(&self.store, &app, &argv);
                }
            }
            id if id > 0 => {
                if let Some(pid) = menu.pid {
                    self.runtime.service(move |ctx| appmenu::dock_click(ctx, pid, id));
                }
            }
            id if id <= ACTIONS && id > RULES => {
                let key = menu.actions.get((ACTIONS - id) as usize);
                if let Some(argv) = key.and_then(|key| launch::action_argv(&self.store.menu, &app, key)) {
                    launch::spawn(&self.store, &app, &argv);
                }
            }
            id if id <= WINDOWS => {
                if let Some(window) = menu.windows.get((WINDOWS - id) as usize) {
                    compositor::focus_window(window);
                }
            }
            _ => {}
        }
    }

    /// Quit through the app's own menu where it is a Mac app, so it asks
    /// about unsaved work; any other app has each window closed.
    fn dock_quit_app(&mut self, pid: Option<u32>, windows: Vec<WindowId>) {
        let close = move || windows.iter().for_each(compositor::close_window);
        match pid.filter(|pid| appmenu::registered(*pid)) {
            Some(pid) => self.runtime.service(move |ctx| appmenu::quit(ctx, pid, close)),
            None => close(),
        }
    }

    /// `dock pin` and `dock unpin`: the app kept in the dock or let go,
    /// by the id the dock files it under.
    pub fn dock_pin(&mut self, app: &str, pin: bool) -> Result<(), String> {
        if app.is_empty() {
            return Err("no app named".into());
        }
        let mut pins: Vec<String> = self.store.state.data.dock_pins.iter().filter(|p| !p.eq_ignore_ascii_case(app)).cloned().collect();
        if pin {
            pins.push(app.to_owned());
        }
        state::set(vec![state::Field::DockPins(pins)]);
        Ok(())
    }

    /// `dock open`: out until `dock close`.
    pub fn dock_open(&mut self) -> Result<(), String> {
        if !self.dock.exists {
            return Err(format!("the dock has nothing to show under dock.show \"{}\"", settings::DOCK_SHOW.get(&self.store.config)));
        }
        self.dock.held = true;
        self.dock.hide_at = None;
        self.dock_reveal(Instant::now());
        Ok(())
    }

    /// `dock close`.
    pub fn dock_close(&mut self) {
        self.dock.held = false;
        self.dock_hide(Instant::now());
    }

    /// `dock menu`: the app's menu, as a right click on its icon opens it.
    pub fn dock_menu_for(&mut self, app: &str) -> Result<(), String> {
        let index = self.dock_index(app)?;
        self.dock_menu(index);
        Ok(())
    }

    /// `dock activate`: what a click on the app's icon does.
    pub fn dock_activate_app(&mut self, app: &str) -> Result<(), String> {
        let index = self.dock_index(app)?;
        self.dock_activate(index);
        Ok(())
    }

    /// `dock quit`: what Quit in the app's menu does.
    pub fn dock_quit(&mut self, app: &str) -> Result<(), String> {
        let e = &self.dock.entries[self.dock_index(app)?];
        if !e.item.running() {
            return Err(format!("'{app}' is not running"));
        }
        let (pid, windows) = (e.pid, e.item.windows.clone());
        self.dock_quit_app(pid, windows);
        Ok(())
    }

    /// `dock status`, and `debug dump`'s `dock`: whether it has a surface,
    /// whether it is out, each app with its marks and its cell's rect, and
    /// the open menu's rows, all in the output's pixels.
    pub fn dock_status(&self) -> Value {
        let out = self.output_size();
        let shown = self.dock.card.as_ref().filter(|d| d.popup.is_open());
        let rect = |r: IRect, (ox, oy): (i32, i32)| json!({"x": r.x + ox, "y": r.y + oy, "width": r.w, "height": r.h});
        let cells: Vec<(String, IRect)> = shown.map_or_else(Vec::new, |d| d.popup.controls());
        let origin = shown.map_or((0, 0), |d| d.popup.win.card.origin(out));
        let (marks, rule) = self.dock_marks();
        let apps: Vec<Value> = marks
            .iter()
            .zip(&self.dock.entries)
            .map(|(m, e)| {
                let cell = cells.iter().find(|(on, _)| *on == format!("dock:{}", m.app)).map(|(_, r)| rect(*r, origin));
                json!({
                    "app": m.app,
                    "name": m.name,
                    "pinned": e.item.pinned,
                    "running": m.running,
                    "mac": e.item.mac,
                    "focused": m.focused,
                    "windows": e.item.windows.len(),
                    "badge": m.badge,
                    "urgent": m.urgent,
                    "progress": m.progress,
                    "launching": m.launching,
                    "icon": m.image.0.is_some(),
                    "rect": cell,
                })
            })
            .collect();
        let menu = self.dock_menu_open().map(|app| {
            let p = self.tray_menu.as_ref();
            let at = p.map_or((0, 0), |p| p.win.card.origin(out));
            let rows: Vec<Value> = p
                .and_then(|p| p.menu.as_ref())
                .map(|m| m.row_names().map(|(id, label, r)| json!({"id": id, "label": label, "rect": rect(r, at)})).collect())
                .unwrap_or_default();
            json!({"app": app, "rows": rows})
        });
        json!({
            "show": settings::DOCK_SHOW.get(&self.store.config),
            "exists": self.dock.exists,
            "strip": self.dock.strip.as_ref().is_some_and(|s| s.buffer.is_some()),
            "shown": shown.is_some(),
            "held": self.dock.held,
            "edge": shown.map(|d| d.edge.as_str()),
            "rect": shown.map(|d| rect(d.popup.win.card.rest_rect(), origin)),
            "rule": rule,
            "apps": apps,
            "menu": menu,
        })
    }

    pub(super) fn dock_present(&mut self, now: Instant) {
        self.dock_strip_sync();
        if self.dock.dwell_at.is_some_and(|t| t <= now) {
            self.dock_reveal(now);
        }
        if self.dock.hide_at.is_some_and(|t| t <= now) {
            if self.dock.over || self.dock.held || self.dock_menu_open().is_some() {
                self.dock.hide_at = None;
            } else {
                self.dock_hide(now);
            }
        }
        if self.dock.card.as_ref().is_some_and(|d| d.popup.win.finished(now)) {
            self.dock.card = None;
            self.sync_join();
            crate::trace::mapped(crate::trace::Surface::Dock, crate::trace::State::Unmapped);
            return;
        }
        if self.dock.card.is_none() {
            return;
        }
        let (marks, rule) = self.dock_marks();
        let size = self.dock_size();
        let (ow, _) = self.output_size();
        let theme = &self.store.theme.theme;
        let Some(d) = &mut self.dock.card else { return };
        let sized = (marks.len(), rule.is_some());
        if sized != d.sized && !marks.is_empty() {
            d.popup.win.card.resize(now, ow / 2.0, ow as i32, theme.space.screen_padding, size);
            d.sized = sized;
        }
        // The card and the margin between it and its line, so the pointer
        // coming up off the strip never falls through to a window.
        let (r, margin) = (d.popup.win.card.rest_rect(), theme.space.bar_margin.ceil() as i32);
        let want = match (d.popup.is_open(), d.edge) {
            (false, _) => IRect::default(),
            (true, Edge::Top) => IRect::new(r.x, r.y - margin, r.w, r.h + margin),
            (true, _) => IRect::new(r.x, r.y, r.w, r.h + margin),
        };
        if d.popup.win.sync_region(&self.compositor, want, &[]) {
            d.popup.win.surface.layer.commit();
        }
        let el = dock::row(theme, marks, rule);
        let drawn = d.popup.draw(&el, theme, &mut self.kit, now);
        let animating = d.popup.win.animating(now) || drawn.animating;
        let qh = self.qh.clone();
        d.popup.win.present(animating, &qh);
    }
}
