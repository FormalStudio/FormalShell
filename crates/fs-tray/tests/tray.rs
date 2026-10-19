use std::{
    collections::HashMap,
    future::Future,
    io::{BufRead, BufReader},
    path::PathBuf,
    pin::pin,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use async_io::{Timer, block_on};
use fs_tray::{
    CheckState, Event, IconSource, ItemField, MenuItem, Status, Toggle, Tray, TrayEvents,
};
use futures_util::future::{Either, select};
use zbus::{
    Connection, connection, interface,
    object_server::SignalEmitter,
    zvariant::{OwnedObjectPath, Value},
};

struct Bus {
    child: Child,
    address: String,
    config: PathBuf,
}

/// Distro default session configs live under a sysconfdir the build sandbox does not have, so
/// the bus gets its own minimal one.
const BUS_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=TMPDIR</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

impl Bus {
    fn start() -> Bus {
        let tmp = std::env::temp_dir();
        let config = tmp.join(format!(
            "fs-tray-bus-{}-{}.conf",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &config,
            BUS_CONFIG.replace("TMPDIR", tmp.to_str().unwrap()),
        )
        .unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon on PATH");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        Bus {
            child,
            address: line.trim().to_owned(),
            config,
        }
    }

    async fn connect(&self) -> Connection {
        connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.config);
    }
}

async fn within<T>(what: &str, fut: impl Future<Output = T>) -> T {
    let wait = pin!(async {
        Timer::after(Duration::from_secs(10)).await;
    });
    match select(pin!(fut), wait).await {
        Either::Left((out, _)) => out,
        Either::Right(_) => panic!("timed out waiting for {what}"),
    }
}

async fn next(events: &mut TrayEvents) -> Event {
    within("a tray event", events.next())
        .await
        .expect("event stream ended")
}

type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

#[derive(Clone)]
struct Props {
    id: String,
    title: String,
    status: String,
    category: String,
    icon_name: String,
    icon_pixmap: Pixmaps,
    theme_path: String,
    overlay_name: String,
    attention_name: String,
    tooltip: (String, Pixmaps, String, String),
    item_is_menu: bool,
    menu: String,
}

struct FakeState {
    props: Props,
    calls: Vec<String>,
    menu_events: Vec<(i32, String)>,
    muted: i32,
    sub_label: String,
}

struct Sni(Arc<Mutex<FakeState>>);

#[interface(name = "org.kde.StatusNotifierItem")]
impl Sni {
    fn activate(&self, x: i32, y: i32) {
        self.0
            .lock()
            .unwrap()
            .calls
            .push(format!("activate {x} {y}"));
    }

    fn secondary_activate(&self, x: i32, y: i32) {
        self.0
            .lock()
            .unwrap()
            .calls
            .push(format!("secondary {x} {y}"));
    }

    fn context_menu(&self, x: i32, y: i32) {
        self.0
            .lock()
            .unwrap()
            .calls
            .push(format!("context {x} {y}"));
    }

    fn scroll(&self, delta: i32, orientation: &str) {
        self.0
            .lock()
            .unwrap()
            .calls
            .push(format!("scroll {delta} {orientation}"));
    }

    #[zbus(property)]
    fn id(&self) -> String {
        self.0.lock().unwrap().props.id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.0.lock().unwrap().props.title.clone()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        self.0.lock().unwrap().props.status.clone()
    }

    #[zbus(property)]
    fn category(&self) -> String {
        self.0.lock().unwrap().props.category.clone()
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        self.0.lock().unwrap().props.icon_name.clone()
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        self.0.lock().unwrap().props.icon_pixmap.clone()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> String {
        self.0.lock().unwrap().props.theme_path.clone()
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> String {
        self.0.lock().unwrap().props.overlay_name.clone()
    }

    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> String {
        self.0.lock().unwrap().props.attention_name.clone()
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        self.0.lock().unwrap().props.tooltip.clone()
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        self.0.lock().unwrap().props.item_is_menu
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(self.0.lock().unwrap().props.menu.clone()).unwrap()
    }

    #[zbus(signal)]
    async fn new_title(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_attention_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_status(emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

struct Node {
    id: i32,
    props: Vec<(&'static str, Value<'static>)>,
    children: Vec<Node>,
}

impl Node {
    fn find(&self, id: i32) -> Option<&Node> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }

    fn wire(&self) -> (i32, HashMap<String, Value<'static>>, Vec<Value<'static>>) {
        let props = self
            .props
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        let children = self
            .children
            .iter()
            .map(|c| Value::from(zbus::zvariant::Structure::from(c.wire())))
            .collect();
        (self.id, props, children)
    }
}

fn leaf(id: i32, props: Vec<(&'static str, Value<'static>)>) -> Node {
    Node {
        id,
        props,
        children: Vec::new(),
    }
}

struct Dbusmenu(Arc<Mutex<FakeState>>);

impl Dbusmenu {
    fn tree(&self) -> Node {
        let st = self.0.lock().unwrap();
        Node {
            id: 0,
            props: vec![("children-display", "submenu".into())],
            children: vec![
                leaf(
                    1,
                    vec![
                        ("label", "_Open".into()),
                        ("icon-name", "document-open".into()),
                    ],
                ),
                leaf(2, vec![("type", "separator".into())]),
                leaf(
                    3,
                    vec![
                        ("label", "Mute".into()),
                        ("toggle-type", "checkmark".into()),
                        ("toggle-state", st.muted.into()),
                    ],
                ),
                Node {
                    id: 4,
                    props: vec![
                        ("label", "More".into()),
                        ("children-display", "submenu".into()),
                    ],
                    children: vec![leaf(5, vec![("label", st.sub_label.clone().into())])],
                },
                leaf(
                    6,
                    vec![("label", "Ghost".into()), ("visible", false.into())],
                ),
                leaf(7, vec![("label", "Off".into()), ("enabled", false.into())]),
            ],
        }
    }
}

type Layout = (i32, HashMap<String, Value<'static>>, Vec<Value<'static>>);

#[interface(name = "com.canonical.dbusmenu")]
impl Dbusmenu {
    fn get_layout(&self, parent: i32, _depth: i32, _props: Vec<String>) -> (u32, Layout) {
        let tree = self.tree();
        let node = tree.find(parent).unwrap_or(&tree);
        (1, node.wire())
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn event(&self, id: i32, event_id: &str, _data: Value<'_>, _timestamp: u32) {
        let mut st = self.0.lock().unwrap();
        st.menu_events.push((id, event_id.to_owned()));
        if id == 3 && event_id == "clicked" {
            st.muted = 1 - st.muted;
        }
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn items_properties_updated(
        emitter: &SignalEmitter<'_>,
        updated: Vec<(i32, HashMap<String, Value<'_>>)>,
        removed: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;
}

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Fake {
    conn: Connection,
    state: Arc<Mutex<FakeState>>,
    path: &'static str,
    /// The bus name the item is reachable under: well-known when asked for, else unique.
    service: String,
}

fn props(id: &str) -> Props {
    Props {
        id: id.to_owned(),
        title: format!("{id} title"),
        status: "Active".into(),
        category: "ApplicationStatus".into(),
        icon_name: "mail-unread".into(),
        icon_pixmap: vec![(2, 1, vec![255, 1, 2, 3, 128, 4, 5, 6])],
        theme_path: "/themes".into(),
        overlay_name: String::new(),
        attention_name: "mail-attention".into(),
        tooltip: ("tip-icon".into(), Vec::new(), "Tip".into(), "Body".into()),
        item_is_menu: false,
        menu: "/MenuBar".into(),
    }
}

impl Fake {
    /// Serves an item at `path` on its own connection. `well_known` registers under a bus name
    /// and passes only that name to the watcher, otherwise it passes just the path.
    async fn start(bus: &Bus, id: &str, path: &'static str, well_known: bool) -> Fake {
        let state = Arc::new(Mutex::new(FakeState {
            props: props(id),
            calls: Vec::new(),
            menu_events: Vec::new(),
            muted: 1,
            sub_label: "Sub A".into(),
        }));
        let conn = bus.connect().await;
        conn.object_server()
            .at(path, Sni(state.clone()))
            .await
            .unwrap();
        conn.object_server()
            .at("/MenuBar", Dbusmenu(state.clone()))
            .await
            .unwrap();
        let service = if well_known {
            let name = format!(
                "org.kde.StatusNotifierItem-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            );
            conn.request_name(name.as_str()).await.unwrap();
            name
        } else {
            conn.unique_name().unwrap().to_string()
        };
        Fake {
            conn,
            state,
            path,
            service,
        }
    }

    async fn register(&self, arg: &str) {
        self.conn
            .call_method(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                Some("org.kde.StatusNotifierWatcher"),
                "RegisterStatusNotifierItem",
                &(arg,),
            )
            .await
            .unwrap();
    }

    async fn sni(&self) -> zbus::object_server::InterfaceRef<Sni> {
        self.conn
            .object_server()
            .interface::<_, Sni>(self.path)
            .await
            .unwrap()
    }

    async fn menu(&self) -> zbus::object_server::InterfaceRef<Dbusmenu> {
        self.conn
            .object_server()
            .interface::<_, Dbusmenu>("/MenuBar")
            .await
            .unwrap()
    }
}

#[test]
fn registers_two_items_and_drops_them_on_name_loss() {
    let bus = Bus::start();
    block_on(async {
        let (tray, mut events) = Tray::start(bus.connect().await).await.unwrap();
        assert!(tray.items().is_empty());

        let named = Fake::start(&bus, "named", "/StatusNotifierItem", true).await;
        let service = named.service.clone();
        named.register(&service).await;
        let by_path = Fake::start(&bus, "bypath", "/org/ayatana/Item", false).await;
        by_path.register("/org/ayatana/Item").await;

        let Event::Added(first) = next(&mut events).await else {
            panic!("expected Added");
        };
        let Event::Added(second) = next(&mut events).await else {
            panic!("expected Added");
        };
        assert_eq!(first.key, format!("{service}/StatusNotifierItem"));
        assert_eq!(first.id, "named");
        assert_eq!(first.title, "named title");
        assert_eq!(first.status, Status::Active);
        assert_eq!(first.icon_name, "mail-unread");
        assert_eq!(first.icon_theme_path, "/themes");
        assert_eq!(first.attention_icon_name, "mail-attention");
        assert_eq!(first.tooltip.title, "Tip");
        assert_eq!(first.tooltip.description, "Body");
        assert!(first.has_menu());
        assert_eq!(first.menu_path.as_deref(), Some("/MenuBar"));
        assert_eq!(first.icon_pixmap.len(), 1);
        assert_eq!(first.icon_pixmap[0].width, 2);
        assert_eq!(first.icon_pixmap[0].rgba, vec![1, 2, 3, 255, 4, 5, 6, 128]);
        assert_eq!(
            first.icon_source(),
            IconSource::Named {
                name: "mail-unread",
                theme_path: "/themes"
            }
        );
        let by_path_unique = by_path.service.clone();
        assert_eq!(second.key, format!("{by_path_unique}/org/ayatana/Item"));
        assert_eq!(tray.items().len(), 2);

        by_path.conn.close().await.unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::Removed {
                key: second.key.clone()
            }
        );
        assert_eq!(tray.items().len(), 1);
        assert_eq!(tray.item(&first.key).unwrap().id, "named");
    });
}

#[test]
fn property_signals_report_the_fields_that_moved() {
    let bus = Bus::start();
    block_on(async {
        let (tray, mut events) = Tray::start(bus.connect().await).await.unwrap();
        let fake = Fake::start(&bus, "p", "/StatusNotifierItem", false).await;
        fake.register("/StatusNotifierItem").await;
        let Event::Added(item) = next(&mut events).await else {
            panic!("expected Added");
        };

        fake.state.lock().unwrap().props.title = "renamed".into();
        Sni::new_title(fake.sni().await.signal_emitter())
            .await
            .unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::Changed {
                key: item.key.clone(),
                fields: vec![ItemField::Title]
            }
        );

        fake.state.lock().unwrap().props.icon_name = "mail-read".into();
        Sni::new_icon(fake.sni().await.signal_emitter())
            .await
            .unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::Changed {
                key: item.key.clone(),
                fields: vec![ItemField::IconName]
            }
        );

        fake.state.lock().unwrap().props.status = "NeedsAttention".into();
        Sni::new_status(fake.sni().await.signal_emitter(), "NeedsAttention")
            .await
            .unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::Changed {
                key: item.key.clone(),
                fields: vec![ItemField::Status]
            }
        );

        fake.state.lock().unwrap().props.tooltip.2 = "New tip".into();
        Sni::new_tool_tip(fake.sni().await.signal_emitter())
            .await
            .unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::Changed {
                key: item.key.clone(),
                fields: vec![ItemField::ToolTip]
            }
        );

        // NeedsAttention switches the icon to the attention name.
        let now = tray.item(&item.key).unwrap();
        assert_eq!(
            now.icon_source(),
            IconSource::Named {
                name: "mail-attention",
                theme_path: "/themes"
            }
        );
    });
}

#[test]
fn item_calls_land_on_the_fake() {
    let bus = Bus::start();
    block_on(async {
        let (tray, mut events) = Tray::start(bus.connect().await).await.unwrap();
        let fake = Fake::start(&bus, "calls", "/StatusNotifierItem", false).await;
        fake.register("/StatusNotifierItem").await;
        let Event::Added(item) = next(&mut events).await else {
            panic!("expected Added");
        };

        tray.activate(&item.key).await.unwrap();
        tray.secondary_activate(&item.key).await.unwrap();
        tray.context_menu(&item.key, 4, 5).await.unwrap();
        tray.scroll(&item.key, -120, false).await.unwrap();
        tray.scroll(&item.key, 15, true).await.unwrap();
        assert_eq!(
            fake.state.lock().unwrap().calls,
            vec![
                "activate 0 0",
                "secondary 0 0",
                "context 4 5",
                "scroll -120 vertical",
                "scroll 15 horizontal"
            ]
        );
    });
}

fn labels(menu: &MenuItem) -> Vec<String> {
    menu.visible_children().map(|c| c.label.clone()).collect()
}

#[test]
fn menu_layout_has_submenu_toggle_separator_and_disabled_rows() {
    let bus = Bus::start();
    block_on(async {
        let (tray, mut events) = Tray::start(bus.connect().await).await.unwrap();
        let fake = Fake::start(&bus, "menu", "/StatusNotifierItem", false).await;
        fake.register("/StatusNotifierItem").await;
        let Event::Added(item) = next(&mut events).await else {
            panic!("expected Added");
        };
        assert!(tray.menu(&item.key).is_none());

        let root = tray.open_menu(&item.key).await.unwrap();
        assert_eq!(root.id, 0);
        assert!(root.has_children);
        assert_eq!(root.children.len(), 6);
        assert_eq!(labels(&root), vec!["Open", "", "Mute", "More", "Off"]);

        let open = root.find(1).unwrap();
        assert_eq!(open.icon_name, "document-open");
        assert!(root.find(2).unwrap().separator);

        let mute = root.find(3).unwrap();
        assert_eq!(mute.toggle, Toggle::Checkmark);
        assert_eq!(mute.check, CheckState::Checked);

        let more = root.find(4).unwrap();
        assert!(more.has_children);
        assert_eq!(more.children.len(), 1);
        assert_eq!(more.children[0].label, "Sub A");

        assert!(!root.find(6).unwrap().visible);
        assert!(!root.find(7).unwrap().enabled);
        assert_eq!(tray.menu(&item.key).unwrap(), root);

        tray.close_menu(&item.key);
        assert!(tray.menu(&item.key).is_none());
    });
}

#[test]
fn menu_click_event_and_updates() {
    let bus = Bus::start();
    block_on(async {
        let (tray, mut events) = Tray::start(bus.connect().await).await.unwrap();
        let fake = Fake::start(&bus, "click", "/StatusNotifierItem", false).await;
        fake.register("/StatusNotifierItem").await;
        let Event::Added(item) = next(&mut events).await else {
            panic!("expected Added");
        };
        tray.open_menu(&item.key).await.unwrap();

        tray.menu_click(&item.key, 3).await.unwrap();
        assert_eq!(
            fake.state.lock().unwrap().menu_events,
            vec![(3, "clicked".to_owned())]
        );

        // The app flips the toggle and says so with ItemsPropertiesUpdated.
        let mut updated = HashMap::new();
        updated.insert("toggle-state".to_owned(), Value::from(0i32));
        Dbusmenu::items_properties_updated(
            fake.menu().await.signal_emitter(),
            vec![(3, updated)],
            Vec::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::MenuChanged {
                key: item.key.clone()
            }
        );
        let menu = tray.menu(&item.key).unwrap();
        let mute = menu.find(3).unwrap();
        assert_eq!(mute.check, CheckState::Unchecked);
        assert_eq!(
            mute.label, "Mute",
            "a partial update keeps the other properties"
        );
        assert_eq!(mute.toggle, Toggle::Checkmark);

        // A removed property goes back to its default.
        Dbusmenu::items_properties_updated(
            fake.menu().await.signal_emitter(),
            Vec::new(),
            vec![(3, vec!["toggle-type".to_owned()])],
        )
        .await
        .unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::MenuChanged {
                key: item.key.clone()
            }
        );
        assert_eq!(
            tray.menu(&item.key).unwrap().find(3).unwrap().toggle,
            Toggle::None
        );

        // LayoutUpdated refetches that subtree.
        fake.state.lock().unwrap().sub_label = "Sub B".into();
        Dbusmenu::layout_updated(fake.menu().await.signal_emitter(), 2, 4)
            .await
            .unwrap();
        assert_eq!(
            next(&mut events).await,
            Event::MenuChanged {
                key: item.key.clone()
            }
        );
        let menu = tray.menu(&item.key).unwrap();
        assert_eq!(menu.find(5).unwrap().label, "Sub B");
        assert_eq!(menu.find(1).unwrap().label, "Open");
    });
}

#[test]
fn joins_an_existing_watcher_and_takes_over_when_it_goes() {
    let bus = Bus::start();
    block_on(async {
        let first_conn = bus.connect().await;
        let (first, mut first_events) = Tray::start(first_conn.clone()).await.unwrap();
        let (second, mut second_events) = Tray::start(bus.connect().await).await.unwrap();

        let fake = Fake::start(&bus, "shared", "/StatusNotifierItem", false).await;
        fake.register("/StatusNotifierItem").await;
        let Event::Added(a) = next(&mut first_events).await else {
            panic!("expected Added");
        };
        let Event::Added(b) = next(&mut second_events).await else {
            panic!("expected Added");
        };
        assert_eq!(a.key, b.key);
        assert_eq!(first.items().len(), 1);
        assert_eq!(second.items().len(), 1);

        // The second tray is a plain host of the first one's watcher: it still reaches the item.
        second.activate(&b.key).await.unwrap();
        assert_eq!(fake.state.lock().unwrap().calls, vec!["activate 0 0"]);

        first_conn.close().await.unwrap();
        assert_eq!(
            next(&mut second_events).await,
            Event::Removed { key: b.key.clone() }
        );
        assert!(second.items().is_empty());

        // The survivor now owns the watcher: an item registering again is picked up.
        fake.register("/StatusNotifierItem").await;
        let Event::Added(c) = next(&mut second_events).await else {
            panic!("expected Added");
        };
        assert_eq!(c.key, b.key);
    });
}

#[test]
fn a_signal_burst_ahead_of_a_reply_does_not_wedge_the_connection() {
    let bus = Bus::start();
    block_on(async {
        let conn = bus.connect().await;
        let (tray, mut events) = Tray::start(conn).await.unwrap();
        let fake = Fake::start(&bus, "burst", "/StatusNotifierItem", false).await;
        fake.register("/StatusNotifierItem").await;
        let Event::Added(item) = next(&mut events).await else {
            panic!("expected Added");
        };

        // Far more than a stream queue holds, all on the wire before the re-read the first
        // one triggers can be answered.
        fake.state.lock().unwrap().props.title = "renamed".into();
        let sni = fake.sni().await;
        for _ in 0..500 {
            Sni::new_title(sni.signal_emitter()).await.unwrap();
        }
        assert_eq!(
            next(&mut events).await,
            Event::Changed {
                key: item.key.clone(),
                fields: vec![ItemField::Title]
            }
        );

        let menu = within("the menu", tray.open_menu(&item.key))
            .await
            .unwrap();
        assert_eq!(menu.children.len(), 6);

        let other = bus.connect().await;
        let version = within(
            "the watcher",
            other.call_method(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.kde.StatusNotifierWatcher", "ProtocolVersion"),
            ),
        )
        .await
        .unwrap();
        let version: zbus::zvariant::OwnedValue = version.body().deserialize().unwrap();
        assert_eq!(i32::try_from(version).unwrap(), 0);
    });
}
