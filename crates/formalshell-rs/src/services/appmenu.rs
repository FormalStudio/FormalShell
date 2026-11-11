//! The focused Mac app's main menu (fs-tray): the registrar atoll apps name
//! their dbusmenu to, and the tree of the one whose window holds focus,
//! which the menu bar, the open menu card and the launcher's menu route
//! read. A window that is not a Mac app's has no menu here, whatever its
//! process registered.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, OnceLock};

use async_channel::{Receiver, Sender};
use fs_tray::{AppMenuEvent, AppMenus, MenuEventKind, REGISTRAR_NAME};

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

#[derive(Default)]
pub struct State {
    pub menu: Option<Menu>,
}

pub enum Diff {
    Menu(Option<Menu>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Menu(menu) => {
                let changed = menu != self.menu;
                self.menu = menu;
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

/// The process to follow, as the UI thread last said; 0 is none.
static FOLLOW: LazyLock<(Sender<u32>, Receiver<u32>)> = LazyLock::new(async_channel::unbounded);
static FOLLOWED: AtomicI64 = AtomicI64::new(-1);

/// The compositor changed: the menu follows the window that holds focus
/// across a shell surface taking the keyboard, so an open menu card or the
/// launcher keeps the menu it is showing.
pub fn follow(store: &Store) {
    let h = &store.compositor;
    let pid = h.window(&h.held_focused_window_id()).filter(|w| w.is_mac()).and_then(|w| u32::try_from(w.pid).ok()).unwrap_or(0);
    if FOLLOWED.swap(i64::from(pid), Ordering::Relaxed) != i64::from(pid) {
        let _ = FOLLOW.0.try_send(pid);
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    // A connection of its own: the registrar's name is taken before any
    // app can call it, and the pump awaits replies while it holds streams.
    let conn = match crate::services::bus::private(&ctx, zbus::connection::Builder::session()).await {
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
    while let Some(event) = events.next().await {
        match event {
            AppMenuEvent::Changed => {
                let menu = menus.held().map(|h| Menu { pid: h.pid, root: node_of(&h.tree, 1.0) });
                ctx.publish(store::Diff::AppMenu(Diff::Menu(menu)));
            }
            AppMenuEvent::Failed(err) => eprintln!("appmenu: {err}"),
        }
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
        State { menu: Some(Menu { pid: 9, root: node(0, "", vec![app, file]) }) }
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
    fn commands_are_the_leaves_under_their_titles() {
        let commands = state().menu.unwrap().commands();
        let names: Vec<(String, String)> = commands.iter().map(|c| (c.path.join("/"), c.label.clone())).collect();
        assert_eq!(
            names,
            [("TextEdit".into(), "Quit TextEdit".into()), ("File".into(), "New".into()), ("File/Export".into(), "PDF".into())]
        );
    }
}
