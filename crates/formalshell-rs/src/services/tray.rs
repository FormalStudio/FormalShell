//! The system tray (fs-tray): the StatusNotifier host the bar's tray cell
//! reads, and the dbusmenu tree behind an item's menu while the shell holds
//! one open. Icons are decoded on the pool so the UI thread only scales
//! them.

use std::sync::{LazyLock, OnceLock};

use async_channel::{Receiver, Sender};

use fs_tray::{CheckState, Event, IconSource, Item as WireItem, MenuEventKind, MenuItem, Status, Toggle, closest_pixmap};

use super::icons::{self, Raw};
use crate::runtime::Ctx;
use crate::store;

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The watcher's address for the item, the handle every action takes.
    pub key: String,
    /// The item's own `Id`, which IPC names it by.
    pub id: String,
    pub title: String,
    /// The item's own words for hover text, never rewritten.
    pub tooltip: String,
    pub has_menu: bool,
    /// SNI `ItemIsMenu`: activation does nothing, the menu is the click.
    pub only_menu: bool,
    pub icon: Option<Raw>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuNode {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub icon: Option<Raw>,
    pub checked: bool,
    pub has_children: bool,
    /// The row's key equivalent: its modifiers, then the key, as the bus
    /// names them (`Super`, `Shift`, `S`).
    pub shortcut: Vec<String>,
    /// A heading over the rows after it, not a choice.
    pub section: bool,
    pub children: Vec<MenuNode>,
}

impl MenuNode {
    pub fn find(&self, id: i32) -> Option<&MenuNode> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }
}

/// The tree the shell holds open for one item.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub key: String,
    pub root: MenuNode,
}

#[derive(Default)]
pub struct State {
    pub items: Vec<Item>,
    pub menu: Option<Menu>,
}

pub enum Diff {
    Items(Vec<Item>),
    Menu(Option<Menu>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Items(items) => {
                let changed = items != self.items;
                self.items = items;
                changed
            }
            Diff::Menu(menu) => {
                let changed = menu != self.menu;
                self.menu = menu;
                changed
            }
        }
    }

    /// IPC names an item by its `Id`, which two running copies of one app share.
    pub fn by_id(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn by_key(&self, key: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.key == key)
    }
}

static TRAY: OnceLock<fs_tray::Tray> = OnceLock::new();

/// The size an item's picture is picked for, near the bar's body type it is
/// drawn at.
const PICK: u32 = 22;

/// The largest scale any output draws at, as the UI thread last said: what
/// a picture is picked for.
static SCALE: LazyLock<(Sender<f64>, Receiver<f64>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread; only the latest is kept.
pub fn scale(largest: f64) {
    while SCALE.1.try_recv().is_ok() {}
    let _ = SCALE.0.try_send(largest);
}

fn latest(largest: &mut f64) {
    while let Ok(s) = SCALE.1.try_recv() {
        *largest = s;
    }
}

fn icon_of(item: &WireItem, largest: f64) -> Option<Raw> {
    let named = match item.icon_source() {
        IconSource::Named { name, theme_path } => icons::named(name, theme_path, icons::pick(PICK, largest)),
        IconSource::Pixmaps => None,
    };
    named.or_else(|| {
        let pixmaps = if item.status == Status::NeedsAttention && !item.attention_icon_pixmap.is_empty() {
            &item.attention_icon_pixmap
        } else {
            &item.icon_pixmap
        };
        let pick = icons::pick(PICK, largest) as i32;
        let pm = closest_pixmap(pixmaps, pick, pick)?;
        icons::from_rgba(pm.width, pm.height, &pm.rgba)
    })
}

fn tooltip_of(item: &WireItem) -> String {
    [&item.tooltip.title, &item.title, &item.id].into_iter().find(|s| !s.is_empty()).cloned().unwrap_or_default()
}

/// What an item's picture depends on; a change elsewhere keeps the decode.
fn picture_key(item: &WireItem) -> (String, String, String, Status, Vec<fs_tray::Pixmap>, Vec<fs_tray::Pixmap>) {
    (
        item.icon_name.clone(),
        item.attention_icon_name.clone(),
        item.icon_theme_path.clone(),
        item.status,
        item.icon_pixmap.clone(),
        item.attention_icon_pixmap.clone(),
    )
}

fn menu_icon(node: &MenuItem, largest: f64) -> Option<Raw> {
    if !node.icon_png.is_empty() {
        return icons::from_bytes(&node.icon_png);
    }
    (!node.icon_name.is_empty()).then(|| icons::named(&node.icon_name, "", icons::pick(PICK, largest))).flatten()
}

pub(super) fn node_of(node: &MenuItem, largest: f64) -> MenuNode {
    MenuNode {
        id: node.id,
        label: node.label.clone(),
        enabled: node.enabled,
        separator: node.separator,
        icon: menu_icon(node, largest),
        checked: node.toggle != Toggle::None && node.check == CheckState::Checked,
        has_children: node.has_children,
        shortcut: node.shortcut.clone(),
        section: node.section,
        children: node.visible_children().map(|c| node_of(c, largest)).collect(),
    }
}

async fn publish_menu(ctx: &Ctx, tray: &fs_tray::Tray, key: &str, largest: f64) {
    let Some(tree) = tray.menu(key) else { return };
    let key = key.to_owned();
    let menu = ctx.pool().run(move || Menu { key, root: node_of(&tree, largest) }).await;
    // The tree may have been closed while it decoded.
    if let Some(menu) = menu.filter(|m| tray.menu(&m.key).is_some()) {
        ctx.publish(store::Diff::Tray(Diff::Menu(Some(menu))));
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let conn = match crate::services::bus::session().await {
        Ok(conn) => conn,
        Err(err) => return eprintln!("tray: no session bus: {err}"),
    };
    let (tray, mut events) = match fs_tray::Tray::start(conn).await {
        Ok(v) => v,
        Err(err) => return eprintln!("tray: {err}"),
    };
    let _ = TRAY.set(tray.clone());
    let mut shown: Vec<(WireItem, Option<Raw>)> = Vec::new();
    let mut largest = 1.0;
    loop {
        latest(&mut largest);
        let mut next: Vec<(WireItem, Option<Raw>)> = Vec::new();
        for item in tray.items() {
            let kept = shown.iter().find(|(old, _)| old.key == item.key && picture_key(old) == picture_key(&item));
            let icon = match kept {
                Some((_, icon)) => icon.clone(),
                None => {
                    let for_decode = item.clone();
                    ctx.pool().run(move || icon_of(&for_decode, largest)).await.flatten()
                }
            };
            next.push((item, icon));
        }
        shown = next;
        let items = shown
            .iter()
            .map(|(item, icon)| Item {
                key: item.key.clone(),
                id: item.id.clone(),
                title: item.title.clone(),
                tooltip: tooltip_of(item),
                has_menu: item.has_menu(),
                only_menu: item.item_is_menu,
                icon: icon.clone(),
            })
            .collect();
        ctx.publish(store::Diff::Tray(Diff::Items(items)));
        // Everything but a menu change re-reads the items.
        loop {
            match events.next().await {
                None => return,
                Some(Event::MenuChanged { key }) => {
                    latest(&mut largest);
                    publish_menu(&ctx, &tray, &key, largest).await;
                }
                Some(_) => break,
            }
        }
    }
}

/// A failed tray call, which no surface has a place for.
fn logged<T>(action: &str, key: &str, result: zbus::Result<T>) -> Option<T> {
    result.map_err(|err| eprintln!("tray: {action} {key}: {err}")).ok()
}

fn spawn_on(ctx: &Ctx, f: impl AsyncFnOnce(&fs_tray::Tray) + 'static) {
    ctx.spawn(async move {
        if let Some(tray) = TRAY.get() {
            f(tray).await;
        }
    });
}

/// `token` is an xdg-activation token for the click, handed over first.
pub fn activate(ctx: &Ctx, key: String, token: Option<String>) {
    spawn_on(ctx, async move |tray| {
        if let Some(token) = token {
            logged("activation token", &key, tray.provide_activation_token(&key, &token).await);
        }
        logged("activate", &key, tray.activate(&key).await);
    });
}

pub fn secondary_activate(ctx: &Ctx, key: String) {
    spawn_on(ctx, async move |tray| {
        logged("secondary activate", &key, tray.secondary_activate(&key).await);
    });
}

/// Loads the whole tree, tells the app it is showing, and publishes it,
/// its pictures picked for `largest`.
pub fn open_menu(ctx: &Ctx, key: String, largest: f64) {
    let ctx = ctx.clone();
    let publisher = ctx.clone();
    spawn_on(&publisher, async move |tray| {
        if logged("open menu", &key, tray.open_menu(&key).await).is_none() {
            return;
        }
        logged("menu opened", &key, tray.menu_event(&key, 0, MenuEventKind::Opened).await);
        publish_menu(&ctx, tray, &key, largest).await;
    });
}

pub fn close_menu(ctx: &Ctx, key: String) {
    let ctx2 = ctx.clone();
    spawn_on(ctx, async move |tray| {
        logged("menu closed", &key, tray.menu_event(&key, 0, MenuEventKind::Closed).await);
        tray.close_menu(&key);
        ctx2.publish(store::Diff::Tray(Diff::Menu(None)));
    });
}

pub fn menu_click(ctx: &Ctx, key: String, id: i32) {
    spawn_on(ctx, async move |tray| {
        logged("menu click", &key, tray.menu_click(&key, id).await);
    });
}

/// A submenu's own `AboutToShow`, for apps that fill it lazily.
pub fn menu_about_to_show(ctx: &Ctx, key: String, id: i32, largest: f64) {
    let ctx = ctx.clone();
    let publisher = ctx.clone();
    spawn_on(&publisher, async move |tray| {
        if logged("menu about to show", &key, tray.menu_about_to_show(&key, id).await).is_some() {
            publish_menu(&ctx, tray, &key, largest).await;
        }
    });
}
