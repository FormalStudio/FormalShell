//! The focused Mac app's main menu (fs-tray): the registrar atoll apps name
//! their dbusmenu to, and the tree of the one whose window holds focus,
//! which the menu bar, the open menu card and the launcher's menu route
//! read. Only atoll's apps know the registrar's name, so a process that
//! registered is a Mac app and its windows are Mac windows, tagged or not.

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;

use async_channel::{Receiver, Sender};
use fs_tray::{AppMenuEvent, AppMenus, BarState, MenuEventKind, REGISTRAR_NAME};
use fs_types::{Window, WindowId};

use super::compositor;
use super::tray::{MenuNode, node_of};
use crate::runtime::Ctx;
use crate::store::{self, Store};

/// The focused Mac app's menu: `root`'s children are the bar's titles, the
/// first of them the application menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub pid: u32,
    pub root: MenuNode,
}

/// A Mac app's dock menu (`applicationDockMenu:`), as last read for the
/// dock's menu card.
#[derive(Clone, Debug, PartialEq)]
pub struct DockMenu {
    pub pid: u32,
    pub root: MenuNode,
}

#[derive(Default)]
pub struct State {
    pub menu: Option<Menu>,
    pub dock: Option<DockMenu>,
    /// The processes that registered a main menu: the running Mac apps,
    /// windows or none.
    pub registered: Vec<u32>,
}

pub enum Diff {
    Menu(Option<Menu>),
    Dock(Option<DockMenu>),
    Registered(Vec<u32>),
}

impl State {
    /// Whether `w` is a Mac app's window: atoll's own carry the `atoll`
    /// tag, and an Electron app in atoll's Mac mode, whose windows Chromium
    /// makes, is known by its process's registration.
    pub fn is_mac(&self, w: &Window) -> bool {
        w.is_mac() || u32::try_from(w.pid).is_ok_and(|pid| self.registered.binary_search(&pid).is_ok())
    }

    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Registered(pids) => {
                let changed = pids != self.registered;
                self.registered = pids;
                changed
            }
            Diff::Menu(menu) => {
                let changed = menu != self.menu;
                self.menu = menu;
                changed
            }
            Diff::Dock(dock) => {
                let changed = dock != self.dock;
                self.dock = dock;
                changed
            }
        }
    }

    /// The top-level titles, the application menu first.
    pub fn titles(&self) -> &[MenuNode] {
        self.menu.as_ref().map_or(&[], |m| &m.root.children)
    }

    pub fn node(&self, id: i32) -> Option<&MenuNode> {
        self.menu.as_ref()?.root.find(id)
    }

    /// The node at `path`, labels from a title down joined with `/`, case
    /// ignored.
    pub fn at_path(&self, path: &str) -> Option<&MenuNode> {
        let mut node = &self.menu.as_ref()?.root;
        for part in path.split('/').map(str::trim).filter(|p| !p.is_empty()) {
            node = node.children.iter().find(|c| !c.separator && c.label.eq_ignore_ascii_case(part))?;
        }
        (node.id != 0).then_some(node)
    }
}

/// One command of the menu: a leaf, with the titles above it.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub id: i32,
    pub label: String,
    pub path: Vec<String>,
    pub enabled: bool,
    pub checked: bool,
    pub shortcut: Vec<String>,
}

fn collect(node: &MenuNode, path: &mut Vec<String>, out: &mut Vec<Command>) {
    for child in node.children.iter().filter(|c| !c.separator && !c.section) {
        if child.has_children {
            path.push(child.label.clone());
            collect(child, path, out);
            path.pop();
        } else if !child.label.is_empty() {
            out.push(Command {
                id: child.id,
                label: child.label.clone(),
                path: path.clone(),
                enabled: child.enabled,
                checked: child.checked,
                shortcut: child.shortcut.clone(),
            });
        }
    }
}

impl Menu {
    /// Every leaf, in menu order.
    pub fn commands(&self) -> Vec<Command> {
        let mut out = Vec::new();
        collect(&self.root, &mut Vec::new(), &mut out);
        out
    }

    /// The Super key equivalents of its commands, as Hyprland chords, and
    /// Super+Q, which quits by the Quit item when none carries it.
    pub fn chords(&self) -> Vec<String> {
        let mut out: Vec<String> = self.commands().iter().filter(|c| c.shortcut.iter().any(|k| k == "Super")).map(|c| chord(&c.shortcut)).collect();
        out.push(QUIT_CHORD.to_owned());
        out.sort();
        out.dedup();
        out
    }
}

pub const QUIT_CHORD: &str = "SUPER + Q";

/// A key equivalent as a Hyprland chord: `["Super", "Shift", "N"]` is
/// "SUPER + SHIFT + N".
pub fn chord(shortcut: &[String]) -> String {
    let key = |k: &String| match k.as_str() {
        "Super" | "Shift" | "Alt" => k.to_uppercase(),
        "Control" => "CTRL".to_owned(),
        _ => k.clone(),
    };
    shortcut.iter().map(key).collect::<Vec<_>>().join(" + ")
}

impl DockMenu {
    /// What can be chosen in it: each enabled leaf's id, and its label
    /// under the titles above it.
    pub fn choices(&self) -> Vec<(i32, String)> {
        let mut out = Vec::new();
        collect(&self.root, &mut Vec::new(), &mut out);
        let label = |c: Command| c.path.into_iter().chain([c.label]).collect::<Vec<_>>().join(" \u{203A} ");
        out.into_iter().filter(|c| c.enabled).map(|c| (c.id, label(c))).collect()
    }
}

/// A shortcut as the caps `ui::keycap` draws: the keys the user
/// presses, Super and Shift as their glyphs.
pub fn keycaps(shortcut: &[String]) -> Vec<String> {
    shortcut
        .iter()
        .map(|key| match key.as_str() {
            "Super" => "super-glyph".to_owned(),
            "Shift" => "shift-glyph".to_owned(),
            "Control" => "Ctrl".to_owned(),
            other => other.to_owned(),
        })
        .collect()
}

static MENUS: OnceLock<AppMenus> = OnceLock::new();

/// How long a Mac app has to answer a call. The pump reads one app at a
/// time, so an app that never answers would hold every menu after it.
const ANSWER: Duration = Duration::from_secs(3);

/// The process to follow, as the UI thread last said; 0 is none.
static FOLLOW: LazyLock<(Sender<u32>, Receiver<u32>)> = LazyLock::new(async_channel::unbounded);
static FOLLOWED: AtomicI64 = AtomicI64::new(-1);

/// The menu bar as `org.atoll.MenuBar` reports it, as the UI thread last
/// said.
static BAR: LazyLock<(Sender<BarState>, Receiver<BarState>)> = LazyLock::new(async_channel::unbounded);
static BAR_SENT: Mutex<Option<BarState>> = Mutex::new(None);

/// The Mac app whose window last held focus, 0 after any other window held
/// it. Set on the UI thread by [`follow`].
static FRONT: AtomicU32 = AtomicU32::new(0);

/// The process of the Mac window that holds focus, 0 with none: focus held
/// across a shell surface taking the keyboard, so an open menu card or the
/// launcher keeps the menu it is showing. With no window in focus it is
/// [`front`]'s.
fn focused(store: &Store) -> u32 {
    let h = &store.compositor;
    match h.window(&h.held_focused_window_id()) {
        Some(w) => mac_pid(store, w),
        None => front(store).unwrap_or(0),
    }
}

fn mac_pid(store: &Store, w: &Window) -> u32 {
    Some(w).filter(|w| store.appmenu.is_mac(w)).and_then(|w| u32::try_from(w.pid).ok()).unwrap_or(0)
}

/// The Mac app still frontmost with no window in focus, as a Mac app stays
/// once its last window closes: the one whose window last had focus, while
/// it runs.
pub fn front(store: &Store) -> Option<u32> {
    let pid = FRONT.load(Ordering::Relaxed);
    (pid != 0 && store.appmenu.registered.binary_search(&pid).is_ok()).then_some(pid)
}

/// Whether the menu held is another process's than the focused window's.
/// The store drops it on the compositor's own change, ahead of the pump
/// that follows the focus: an app that has stopped answering holds the
/// pump for a call's whole wait.
pub fn stale(store: &Store) -> bool {
    store.appmenu.menu.as_ref().is_some_and(|m| m.pid != focused(store))
}

/// The windows a tag was asked for, until the compositor lists them tagged
/// or they go.
static TAGGING: Mutex<Vec<WindowId>> = Mutex::new(Vec::new());

/// A registered process's windows that carry no `atoll` xdg tag (an
/// Electron app in atoll's Mac mode cannot set one) get the compositor's
/// own tag, so the config gives them a Mac window's chrome too.
pub fn tag(store: &Store) {
    if !compositor::ready() {
        return;
    }
    let windows = &store.compositor.snapshot.windows;
    let mut asked = TAGGING.lock().unwrap();
    asked.retain(|id| windows.iter().any(|w| &w.id == id && !w.mac_tagged));
    for w in windows.iter().filter(|w| !w.is_mac() && !w.mac_tagged && store.appmenu.is_mac(w)) {
        if !asked.contains(&w.id) {
            compositor::send(compositor::Command::TagMac(w.id.clone()));
            asked.push(w.id.clone());
        }
    }
}

/// The compositor changed: the menu follows the focused window.
pub fn follow(store: &Store) {
    let h = &store.compositor;
    if let Some(w) = h.window(&h.held_focused_window_id()) {
        FRONT.store(mac_pid(store, w), Ordering::Relaxed);
    }
    let pid = focused(store);
    if FOLLOWED.swap(i64::from(pid), Ordering::Relaxed) != i64::from(pid) {
        let _ = FOLLOW.0.try_send(pid);
    }
}

/// The bar's thickness when it sits on the top edge (0 on any other), and
/// whether it is revealed over a fullscreen window.
pub fn bar(state: BarState) {
    if BAR_SENT.lock().unwrap().replace(state) != Some(state) {
        let _ = BAR.0.try_send(state);
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    // A connection of its own: the registrar's name is taken before any
    // app can call it, and the pump awaits replies while it holds streams.
    let conn = match crate::services::bus::private(&ctx, zbus::connection::Builder::session().map(|b| b.method_timeout(ANSWER))).await {
        Ok(conn) => conn,
        Err(err) => return eprintln!("appmenu: no session bus: {err}"),
    };
    let (menus, mut events) = match AppMenus::start(conn).await {
        Ok(v) => v,
        Err(err) => return eprintln!("appmenu: {REGISTRAR_NAME}: {err}"),
    };
    let _ = MENUS.set(menus.clone());
    let follower = menus.clone();
    ctx.spawn(async move {
        while let Ok(pid) = FOLLOW.1.recv().await {
            follower.follow((pid != 0).then_some(pid));
        }
    });
    let bar = menus.clone();
    ctx.spawn(async move {
        while let Ok(state) = BAR.1.recv().await {
            if let Err(err) = bar.set_bar(state).await {
                eprintln!("appmenu: menu bar: {err}");
            }
        }
    });
    while let Some(event) = events.next().await {
        if let AppMenuEvent::Failed(err) = &event {
            eprintln!("appmenu: {err}");
        }
        // After a failure too: a read that failed had already let go of
        // the menu it was replacing.
        let menu = menus.held().map(|h| Menu { pid: h.pid, root: node_of(&h.tree, 1.0) });
        ctx.publish(store::Diff::AppMenu(Diff::Registered(menus.pids())));
        ctx.publish(store::Diff::AppMenu(Diff::Menu(menu)));
    }
}

/// The submenu `id` is about to show: the app brings it up to date.
pub fn about_to_show(id: i32) {
    if let Some(menus) = MENUS.get() {
        menus.about_to_show(id);
        menus.event(id, MenuEventKind::Opened);
    }
}

pub fn closed(id: i32) {
    if let Some(menus) = MENUS.get() {
        menus.event(id, MenuEventKind::Closed);
    }
}

/// Runs the item `id`, as a click on its row does.
pub fn click(id: i32) {
    if let Some(menus) = MENUS.get() {
        menus.event(id, MenuEventKind::Clicked);
    }
}

/// Where a Mac app serves its dock menu, beside its main menu.
const DOCK_PATH: &str = "/org/atoll/DockMenu";

/// Whether the process named a menu to the registrar, which only a Mac app
/// does.
pub fn registered(pid: u32) -> bool {
    MENUS.get().is_some_and(|menus| menus.registered(pid).is_some())
}

/// Reads the dock menu of the Mac app `pid` into `store.appmenu.dock`, the
/// app told first that it is about to show.
pub fn dock_menu(ctx: &Ctx, pid: u32) {
    let Some(address) = MENUS.get().and_then(|menus| menus.sibling(pid, DOCK_PATH)) else { return };
    let publisher = ctx.clone();
    ctx.spawn(async move {
        let read = async {
            let conn = crate::services::bus::session().await?;
            address.about_to_show(&conn, 0).await?;
            address.layout(&conn, 0).await
        };
        match read.await {
            Ok(tree) => publisher.publish(store::Diff::AppMenu(Diff::Dock(Some(DockMenu { pid, root: node_of(&tree, 1.0) })))),
            Err(err) => eprintln!("appmenu: dock menu of {pid}: {err}"),
        }
    });
}

/// Runs the item `id` of the dock menu of `pid`.
pub fn dock_click(ctx: &Ctx, pid: u32, id: i32) {
    let Some(address) = MENUS.get().and_then(|menus| menus.sibling(pid, DOCK_PATH)) else { return };
    ctx.spawn(async move {
        let sent = async { address.event(&crate::services::bus::session().await?, id, MenuEventKind::Clicked).await };
        if let Err(err) = sent.await {
            eprintln!("appmenu: dock menu of {pid}: {err}");
        }
    });
}

/// The application menu's Quit: the item on Command and Q, else the last
/// one named Quit.
pub fn quit_item(root: &MenuNode) -> Option<i32> {
    let items = &root.children.first()?.children;
    let live = |c: &&MenuNode| !c.separator && !c.has_children && c.enabled;
    let keyed = items.iter().filter(live).find(|c| c.shortcut.len() == 2 && c.shortcut[0] == "Super" && c.shortcut[1].eq_ignore_ascii_case("q"));
    keyed.or_else(|| items.iter().rev().filter(live).find(|c| c.label.starts_with("Quit"))).map(|c| c.id)
}

/// Chooses Quit in the main menu of the Mac app `pid`, so the app asks
/// about unsaved work as it does for its own key. `closed` runs where the
/// menu holds no Quit.
pub fn quit(ctx: &Ctx, pid: u32, closed: impl FnOnce() + Send + 'static) {
    let Some(address) = MENUS.get().and_then(|menus| menus.registered(pid)) else { return closed() };
    ctx.spawn(async move {
        let sent = async {
            let conn = crate::services::bus::session().await?;
            let tree = address.layout(&conn, 0).await?;
            match quit_item(&node_of(&tree, 1.0)) {
                Some(id) => address.event(&conn, id, MenuEventKind::Clicked).await.map(|()| true),
                None => Ok(false),
            }
        };
        match sent.await {
            Ok(true) => {}
            Ok(false) => closed(),
            Err(err) => {
                eprintln!("appmenu: quit {pid}: {err}");
                closed();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i32, label: &str, children: Vec<MenuNode>) -> MenuNode {
        MenuNode {
            id,
            label: label.into(),
            enabled: true,
            separator: false,
            icon: None,
            checked: false,
            has_children: !children.is_empty(),
            shortcut: Vec::new(),
            section: false,
            children,
        }
    }

    fn state() -> State {
        let mut rule = node(5, "", Vec::new());
        rule.separator = true;
        let export = node(6, "Export", vec![node(7, "PDF", Vec::new())]);
        let file = node(2, "File", vec![node(3, "New", Vec::new()), rule, export]);
        let app = node(1, "TextEdit", vec![node(8, "Quit TextEdit", Vec::new())]);
        State { menu: Some(Menu { pid: 9, root: node(0, "", vec![app, file]) }), ..State::default() }
    }

    #[test]
    fn key_equivalents_are_chords_and_super_q_is_always_one() {
        let mut s = state();
        let file = &mut s.menu.as_mut().unwrap().root.children[1];
        file.children[0].shortcut = vec!["Super".into(), "Shift".into(), "N".into()];
        file.children[2].children[0].shortcut = vec!["Control".into(), "P".into()];
        assert_eq!(s.menu.unwrap().chords(), ["SUPER + Q", "SUPER + SHIFT + N"]);
        assert_eq!(chord(&["Control".into(), "Alt".into(), "comma".into()]), "CTRL + ALT + comma");
    }

    #[test]
    fn a_registered_process_makes_its_windows_mac_windows() {
        let s = State { registered: vec![4, 40], ..State::default() };
        let w = |pid| Window { pid, ..Window::default() };
        assert!(s.is_mac(&w(40)) && !s.is_mac(&w(41)));
    }

    #[test]
    fn a_path_walks_the_labels_whatever_their_case() {
        let s = state();
        assert_eq!(s.at_path("file/export/pdf").map(|n| n.id), Some(7));
        assert_eq!(s.at_path("File").map(|n| n.id), Some(2));
        assert_eq!(s.at_path("File/Missing"), None);
        assert_eq!(s.at_path(""), None);
    }

    #[test]
    fn quit_is_the_application_menus_command_q() {
        let mut s = state();
        assert_eq!(quit_item(&s.menu.as_ref().unwrap().root), Some(8));
        let app = &mut s.menu.as_mut().unwrap().root.children[0];
        let mut keyed = node(9, "Beenden", Vec::new());
        keyed.shortcut = vec!["Super".into(), "Q".into()];
        app.children.insert(0, keyed);
        assert_eq!(quit_item(&s.menu.as_ref().unwrap().root), Some(9));
        s.menu.as_mut().unwrap().root.children[0].children.clear();
        assert_eq!(quit_item(&s.menu.as_ref().unwrap().root), None);
    }

    #[test]
    fn commands_are_the_leaves_under_their_titles() {
        let commands = state().menu.unwrap().commands();
        let names: Vec<(String, String)> = commands.iter().map(|c| (c.path.join("/"), c.label.clone())).collect();
        assert_eq!(
            names,
            [("TextEdit".into(), "Quit TextEdit".into()), ("File".into(), "New".into()), ("File/Export".into(), "PDF".into())]
        );
    }
}
