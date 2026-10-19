use std::{
    collections::{HashMap, VecDeque},
    future::{Future, poll_fn},
    pin::{Pin, pin},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    task::Poll,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::{
    Stream, StreamExt,
    future::{AbortHandle, AbortRegistration},
    stream::{Abortable, SelectAll},
};
use zbus::{
    Connection, MatchRule, MessageStream,
    fdo::{DBusProxy, RequestNameFlags, RequestNameReply},
    message::{Message, Type},
    names::BusName,
    zvariant::{OwnedValue, Value},
};

use crate::{
    types::{Item, ItemField, MenuItem, apply_properties, diff},
    watcher::{
        SharedWatcher, WATCHER_IFACE, WATCHER_NAME, WATCHER_PATH, Watcher, WatcherState, qualify,
        register_own_host, service_gone,
    },
};

const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const MENU_IFACE: &str = "com.canonical.dbusmenu";

static HOST_SEQ: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// An item finished its first property read and joined the tray.
    Added(Box<Item>),
    Removed {
        key: String,
    },
    /// A New* signal or PropertiesChanged re-read the item and these fields moved.
    Changed {
        key: String,
        fields: Vec<ItemField>,
    },
    /// The open menu of this item changed (LayoutUpdated or ItemsPropertiesUpdated).
    MenuChanged {
        key: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEventKind {
    Clicked,
    Hovered,
    Opened,
    Closed,
}

impl MenuEventKind {
    fn wire(self) -> &'static str {
        match self {
            Self::Clicked => "clicked",
            Self::Hovered => "hovered",
            Self::Opened => "opened",
            Self::Closed => "closed",
        }
    }
}

struct Entry {
    item: Item,
    /// Unique name of the item's owner; the sender its signals carry.
    owner: String,
    /// `Some` while the shell holds the menu open (between `open_menu` and `close_menu`).
    menu: Option<MenuItem>,
    aborts: Vec<AbortHandle>,
}

struct Shared {
    conn: Connection,
    entries: Mutex<Vec<Entry>>,
}

/// Handle for reading the tray and acting on its items. Cheap to clone.
#[derive(Clone)]
pub struct Tray {
    shared: Arc<Shared>,
}

enum Input {
    WatcherItem { key: String, registered: bool },
    NameOwner { name: String, new_owner: String },
    ItemSignal { key: String },
    LayoutUpdated { key: String, parent: i32 },
    PropertiesUpdated { key: String, body: Message },
}

type InputStream = Pin<Box<dyn Stream<Item = Input> + Send>>;

type Inputs = Arc<Mutex<SelectAll<InputStream>>>;

/// The pump: poll [`TrayEvents::next`] on the service executor. Nothing in the tray moves
/// unless this is being polled.
pub struct TrayEvents {
    core: Core,
    /// Inputs read off the streams while a handler was waiting on the bus.
    backlog: VecDeque<Input>,
}

struct Core {
    shared: Arc<Shared>,
    watcher: SharedWatcher,
    host_name: String,
    owns_watcher: bool,
    inputs: Inputs,
    queue: VecDeque<Event>,
}

/// zbus's socket reader waits for room when any `MessageStream` queue is full (64 messages),
/// and method replies come through that same reader. So the streams are drained into the
/// backlog for as long as `fut` waits on a reply; otherwise a burst of signals arriving ahead
/// of the reply wedges the whole connection, object server included.
async fn drain_while<T>(
    inputs: &Inputs,
    backlog: &mut VecDeque<Input>,
    fut: impl Future<Output = T>,
) -> T {
    let mut fut = pin!(fut);
    poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(out);
        }
        let mut inputs = inputs.lock().unwrap();
        while let Poll::Ready(Some(input)) = inputs.poll_next_unpin(cx) {
            push_input(backlog, input);
        }
        Poll::Pending
    })
    .await
}

/// A re-read answers every signal before it, so repeats of one already waiting are dropped.
fn push_input(backlog: &mut VecDeque<Input>, input: Input) {
    let dup = backlog.iter().any(|queued| match (queued, &input) {
        (Input::ItemSignal { key: a }, Input::ItemSignal { key: b }) => a == b,
        (
            Input::LayoutUpdated { key: a, parent: p },
            Input::LayoutUpdated { key: b, parent: q },
        ) => a == b && p == q,
        _ => false,
    });
    if !dup {
        backlog.push_back(input);
    }
}

fn split_key(key: &str) -> (&str, String) {
    match key.split_once('/') {
        Some((service, rest)) => (service, format!("/{rest}")),
        None => (key, "/StatusNotifierItem".to_owned()),
    }
}

async fn call<R>(
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    member: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> zbus::Result<R>
where
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    let msg = conn
        .call_method(Some(dest), path, Some(iface), member, body)
        .await?;
    msg.body().deserialize::<R>()
}

fn now_secs() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

impl Tray {
    /// Own `org.kde.StatusNotifierWatcher` if it is free, else use the one that is there, then
    /// register a host and load the items already on the bus. The returned snapshot
    /// ([`Tray::items`]) is complete; `TrayEvents` carries everything after it.
    ///
    /// The connection decides the executor: build it with `internal_executor(false)` and tick
    /// `Connection::executor()` on the service thread, or let zbus run its own.
    pub async fn start(conn: Connection) -> zbus::Result<(Tray, TrayEvents)> {
        let watcher: SharedWatcher = Arc::new(Mutex::new(WatcherState::default()));
        conn.object_server()
            .at(
                WATCHER_PATH,
                Watcher {
                    state: watcher.clone(),
                },
            )
            .await?;

        let shared = Arc::new(Shared {
            conn: conn.clone(),
            entries: Mutex::new(Vec::new()),
        });
        let host_name = format!(
            "org.kde.StatusNotifierHost-{}-{}",
            std::process::id(),
            HOST_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        conn.request_name(host_name.as_str()).await?;
        let dbus = DBusProxy::new(&conn).await?;

        let mut core = Core {
            shared: shared.clone(),
            watcher,
            host_name,
            owns_watcher: false,
            inputs: Arc::new(Mutex::new(SelectAll::new())),
            queue: VecDeque::new(),
        };
        let inputs = core.inputs.clone();
        let mut backlog = VecDeque::new();

        let owner_changes = dbus.receive_name_owner_changed().await?;
        inputs
            .lock()
            .unwrap()
            .push(Box::pin(owner_changes.filter_map(|sig| async move {
                let args = sig.args().ok()?;
                Some(Input::NameOwner {
                    name: args.name().to_string(),
                    new_owner: args
                        .new_owner()
                        .as_ref()
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                })
            })));

        let watcher_rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(WATCHER_IFACE)?
            .path(WATCHER_PATH)?
            .build();
        let watcher_signals = drain_while(
            &inputs,
            &mut backlog,
            MessageStream::for_match_rule(watcher_rule, &conn, None),
        )
        .await?;
        inputs
            .lock()
            .unwrap()
            .push(Box::pin(watcher_signals.filter_map(|msg| async move {
                let msg = msg.ok()?;
                let registered = match msg.header().member()?.as_str() {
                    "StatusNotifierItemRegistered" => true,
                    "StatusNotifierItemUnregistered" => false,
                    _ => return None,
                };
                let key: String = msg.body().deserialize().ok()?;
                Some(Input::WatcherItem { key, registered })
            })));

        drain_while(&inputs, &mut backlog, async {
            core.owns_watcher = core.try_own_watcher().await;
            core.connect_to_watcher().await;
        })
        .await;
        // Events produced while loading the snapshot are already in it.
        core.queue.clear();
        Ok((Tray { shared }, TrayEvents { core, backlog }))
    }

    pub fn items(&self) -> Vec<Item> {
        self.shared
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.item.clone())
            .collect()
    }

    pub fn item(&self, key: &str) -> Option<Item> {
        self.shared
            .entries
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.item.key == key)
            .map(|e| e.item.clone())
    }

    fn target(&self, key: &str) -> zbus::Result<(String, String)> {
        let entries = self.shared.entries.lock().unwrap();
        let e = entries
            .iter()
            .find(|e| e.item.key == key)
            .ok_or_else(|| zbus::Error::Failure(format!("no tray item {key}")))?;
        Ok((e.item.service.clone(), e.item.path.clone()))
    }

    async fn item_call(
        &self,
        key: &str,
        member: &str,
        body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> zbus::Result<()> {
        let (service, path) = self.target(key)?;
        match call::<()>(&self.shared.conn, &service, &path, ITEM_IFACE, member, body).await {
            // Items need not implement every verb (menu-only items have no Activate).
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod" =>
            {
                Ok(())
            }
            other => other,
        }
    }

    /// `Activate(0, 0)`.
    pub async fn activate(&self, key: &str) -> zbus::Result<()> {
        self.item_call(key, "Activate", &(0i32, 0i32)).await
    }

    pub async fn secondary_activate(&self, key: &str) -> zbus::Result<()> {
        self.item_call(key, "SecondaryActivate", &(0i32, 0i32))
            .await
    }

    pub async fn context_menu(&self, key: &str, x: i32, y: i32) -> zbus::Result<()> {
        self.item_call(key, "ContextMenu", &(x, y)).await
    }

    pub async fn scroll(&self, key: &str, delta: i32, horizontal: bool) -> zbus::Result<()> {
        let orientation = if horizontal { "horizontal" } else { "vertical" };
        self.item_call(key, "Scroll", &(delta, orientation)).await
    }

    fn menu_target(&self, key: &str) -> zbus::Result<(String, String)> {
        let entries = self.shared.entries.lock().unwrap();
        let e = entries
            .iter()
            .find(|e| e.item.key == key)
            .ok_or_else(|| zbus::Error::Failure(format!("no tray item {key}")))?;
        let path = e
            .item
            .menu_path
            .clone()
            .ok_or_else(|| zbus::Error::Failure(format!("tray item {key} has no menu")))?;
        Ok((e.item.service.clone(), path))
    }

    /// Loads the menu: `AboutToShow(0)`
    /// (an error there is not fatal), then the whole subtree with `GetLayout(0, -1)`. From here
    /// until [`Tray::close_menu`] the tray keeps the tree current and raises
    /// [`Event::MenuChanged`].
    pub async fn open_menu(&self, key: &str) -> zbus::Result<MenuItem> {
        let (service, path) = self.menu_target(key)?;
        self.refresh_menu(key, &service, &path, 0, true).await?;
        self.menu(key)
            .ok_or_else(|| zbus::Error::Failure("menu closed while loading".into()))
    }

    pub fn close_menu(&self, key: &str) {
        if let Some(e) = self
            .shared
            .entries
            .lock()
            .unwrap()
            .iter_mut()
            .find(|e| e.item.key == key)
        {
            e.menu = None;
        }
    }

    pub fn menu(&self, key: &str) -> Option<MenuItem> {
        self.shared
            .entries
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.item.key == key)
            .and_then(|e| e.menu.clone())
    }

    /// `AboutToShow(id)` then a refetch of that subtree, for apps that fill submenus lazily.
    pub async fn menu_about_to_show(&self, key: &str, id: i32) -> zbus::Result<()> {
        let (service, path) = self.menu_target(key)?;
        self.refresh_menu(key, &service, &path, id, true).await
    }

    pub async fn menu_event(&self, key: &str, id: i32, kind: MenuEventKind) -> zbus::Result<()> {
        let (service, path) = self.menu_target(key)?;
        call::<()>(
            &self.shared.conn,
            &service,
            &path,
            MENU_IFACE,
            "Event",
            &(id, kind.wire(), Value::I32(0), now_secs()),
        )
        .await
    }

    pub async fn menu_click(&self, key: &str, id: i32) -> zbus::Result<()> {
        self.menu_event(key, id, MenuEventKind::Clicked).await
    }

    async fn refresh_menu(
        &self,
        key: &str,
        service: &str,
        path: &str,
        parent: i32,
        about_to_show: bool,
    ) -> zbus::Result<()> {
        let conn = &self.shared.conn;
        if about_to_show {
            let _ = call::<bool>(conn, service, path, MENU_IFACE, "AboutToShow", &(parent,)).await;
        }
        let (_rev, layout): (u32, RawLayout) = call(
            conn,
            service,
            path,
            MENU_IFACE,
            "GetLayout",
            &(parent, -1i32, Vec::<&str>::new()),
        )
        .await?;
        let node = build_layout(layout);
        let mut entries = self.shared.entries.lock().unwrap();
        let Some(e) = entries.iter_mut().find(|e| e.item.key == key) else {
            return Ok(());
        };
        if parent == 0 {
            e.menu = Some(node);
        } else if let Some(menu) = e.menu.as_mut()
            && let Some(slot) = menu.find_mut(parent)
        {
            *slot = node;
        }
        Ok(())
    }
}

#[derive(Debug, serde::Deserialize, zbus::zvariant::Type)]
struct RawLayout {
    id: i32,
    props: HashMap<String, OwnedValue>,
    children: Vec<OwnedValue>,
}

fn build_layout(raw: RawLayout) -> MenuItem {
    let mut node = MenuItem::new(raw.id);
    node.set_properties(&raw.props);
    if raw.id == 0 {
        node.has_children = true;
    }
    node.children = raw
        .children
        .into_iter()
        .filter_map(|child| {
            let structure = zbus::zvariant::Structure::try_from(Value::from(child)).ok()?;
            let (id, props, children) =
                <(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>)>::try_from(structure).ok()?;
            Some(build_layout(RawLayout {
                id,
                props,
                children,
            }))
        })
        .collect();
    node
}

fn map_signals(
    stream: MessageStream,
    reg: AbortRegistration,
    f: impl Fn(Message) -> Option<Input> + Send + 'static,
) -> InputStream {
    Box::pin(Abortable::new(
        stream.filter_map(move |m| {
            let out = m.ok().and_then(&f);
            async move { out }
        }),
        reg,
    ))
}

impl TrayEvents {
    /// The next change, or `None` once the connection is gone.
    pub async fn next(&mut self) -> Option<Event> {
        let inputs = self.core.inputs.clone();
        loop {
            if let Some(e) = self.core.queue.pop_front() {
                return Some(e);
            }
            let input = match self.backlog.pop_front() {
                Some(input) => input,
                None => poll_fn(|cx| inputs.lock().unwrap().poll_next_unpin(cx)).await?,
            };
            drain_while(&inputs, &mut self.backlog, self.core.handle(input)).await;
        }
    }
}

impl Core {
    async fn try_own_watcher(&self) -> bool {
        let reply = self
            .shared
            .conn
            .request_name_with_flags(WATCHER_NAME, RequestNameFlags::DoNotQueue.into())
            .await;
        matches!(
            reply,
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner)
        )
    }

    async fn connect_to_watcher(&mut self) {
        let conn = self.shared.conn.clone();
        if self.owns_watcher {
            register_own_host(&conn, &self.watcher, &self.host_name).await;
        } else {
            let _ = call::<()>(
                &conn,
                WATCHER_NAME,
                WATCHER_PATH,
                WATCHER_IFACE,
                "RegisterStatusNotifierHost",
                &(self.host_name.as_str(),),
            )
            .await;
        }

        let keys: Vec<String> = if self.owns_watcher {
            self.watcher.lock().unwrap().items.clone()
        } else {
            let got: zbus::Result<OwnedValue> = call(
                &conn,
                WATCHER_NAME,
                WATCHER_PATH,
                PROPS_IFACE,
                "Get",
                &(WATCHER_IFACE, "RegisteredStatusNotifierItems"),
            )
            .await;
            got.ok()
                .and_then(|v| Vec::<String>::try_from(v).ok())
                .unwrap_or_default()
        };
        for key in keys {
            self.add_item(&key).await;
        }
    }

    fn known(&self, key: &str) -> bool {
        self.shared
            .entries
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.item.key == key)
    }

    async fn read_item(
        &self,
        service: &str,
        path: &str,
    ) -> zbus::Result<HashMap<String, OwnedValue>> {
        call(
            &self.shared.conn,
            service,
            path,
            PROPS_IFACE,
            "GetAll",
            &(ITEM_IFACE,),
        )
        .await
    }

    async fn add_item(&mut self, raw_key: &str) {
        let key = qualify(raw_key, None);
        if self.known(&key) {
            return;
        }
        let (service, path) = split_key(&key);
        let Ok(props) = self.read_item(service, &path).await else {
            return;
        };
        let conn = self.shared.conn.clone();
        let Ok(bus_name) = BusName::try_from(service) else {
            return;
        };
        let Ok(dbus) = DBusProxy::new(&conn).await else {
            return;
        };
        let Ok(owner) = dbus.get_name_owner(bus_name).await else {
            return;
        };
        let owner = owner.to_string();

        let mut item = Item {
            key: key.clone(),
            service: service.to_owned(),
            path: path.clone(),
            ..Item::default()
        };
        apply_properties(&mut item, &props);

        let (abort, reg) = AbortHandle::new_pair();
        let (abort_menu, reg_menu) = AbortHandle::new_pair();
        let item_rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(owner.as_str())
            .and_then(|b| b.path(path.as_str()));
        let Ok(item_rule) = item_rule else { return };
        let Ok(stream) = MessageStream::for_match_rule(item_rule.build(), &conn, None).await else {
            return;
        };
        let k = key.clone();
        self.inputs
            .lock()
            .unwrap()
            .push(map_signals(stream, reg, move |m| {
                let h = m.header();
                let iface = h.interface()?.as_str();
                let member = h.member()?.as_str();
                let relevant = (iface == ITEM_IFACE && member.starts_with("New"))
                    || (iface == PROPS_IFACE && member == "PropertiesChanged");
                relevant.then(|| Input::ItemSignal { key: k.clone() })
            }));

        if let Some(menu_path) = item.menu_path.clone() {
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(owner.as_str())
                .and_then(|b| b.path(menu_path.as_str()))
                .and_then(|b| b.interface(MENU_IFACE));
            if let Ok(rule) = rule
                && let Ok(stream) = MessageStream::for_match_rule(rule.build(), &conn, None).await
            {
                {
                    let k = key.clone();
                    self.inputs
                        .lock()
                        .unwrap()
                        .push(map_signals(stream, reg_menu, move |m| {
                            let member = m.header().member()?.as_str().to_owned();
                            match member.as_str() {
                                "LayoutUpdated" => {
                                    let (_rev, parent): (u32, i32) = m.body().deserialize().ok()?;
                                    Some(Input::LayoutUpdated {
                                        key: k.clone(),
                                        parent,
                                    })
                                }
                                "ItemsPropertiesUpdated" => Some(Input::PropertiesUpdated {
                                    key: k.clone(),
                                    body: m,
                                }),
                                _ => None,
                            }
                        }));
                }
            }
        }

        self.shared.entries.lock().unwrap().push(Entry {
            item: item.clone(),
            owner,
            menu: None,
            aborts: vec![abort, abort_menu],
        });
        self.queue.push_back(Event::Added(Box::new(item)));
    }

    fn remove_item(&mut self, key: &str) {
        let removed = {
            let mut entries = self.shared.entries.lock().unwrap();
            entries
                .iter()
                .position(|e| e.item.key == key)
                .map(|i| entries.remove(i))
        };
        if let Some(e) = removed {
            e.aborts.iter().for_each(AbortHandle::abort);
            self.queue.push_back(Event::Removed {
                key: key.to_owned(),
            });
        }
    }

    fn clear_items(&mut self) {
        let keys: Vec<String> = self
            .shared
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.item.key.clone())
            .collect();
        for key in keys {
            self.remove_item(&key);
        }
    }

    async fn handle(&mut self, input: Input) {
        match input {
            Input::WatcherItem { key, registered } => {
                if registered {
                    self.add_item(&key).await;
                } else {
                    self.remove_item(&qualify(&key, None));
                }
            }
            Input::NameOwner { name, new_owner } => self.name_owner(&name, &new_owner).await,
            Input::ItemSignal { key } => self.refresh_item(&key).await,
            Input::LayoutUpdated { key, parent } => self.layout_updated(&key, parent).await,
            Input::PropertiesUpdated { key, body } => self.properties_updated(&key, &body),
        }
    }

    async fn name_owner(&mut self, name: &str, new_owner: &str) {
        if name == WATCHER_NAME {
            if new_owner.is_empty() {
                self.owns_watcher = false;
                self.clear_items();
                if self.try_own_watcher().await {
                    self.owns_watcher = true;
                }
            } else {
                self.connect_to_watcher().await;
            }
            return;
        }
        if !new_owner.is_empty() {
            return;
        }
        service_gone(&self.shared.conn, &self.watcher, name).await;
        let gone: Vec<String> = self
            .shared
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.item.service == name || e.owner == name)
            .map(|e| e.item.key.clone())
            .collect();
        for key in gone {
            self.remove_item(&key);
        }
    }

    async fn refresh_item(&mut self, key: &str) {
        let Some((service, path)) = self
            .shared
            .entries
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.item.key == key)
            .map(|e| (e.item.service.clone(), e.item.path.clone()))
        else {
            return;
        };
        let Ok(props) = self.read_item(&service, &path).await else {
            return;
        };
        let mut entries = self.shared.entries.lock().unwrap();
        let Some(e) = entries.iter_mut().find(|e| e.item.key == key) else {
            return;
        };
        let mut next = e.item.clone();
        apply_properties(&mut next, &props);
        let fields = diff(&e.item, &next);
        if fields.is_empty() {
            return;
        }
        e.item = next;
        self.queue.push_back(Event::Changed {
            key: key.to_owned(),
            fields,
        });
    }

    async fn layout_updated(&mut self, key: &str, parent: i32) {
        let tray = Tray {
            shared: self.shared.clone(),
        };
        let Ok((service, path)) = tray.menu_target(key) else {
            return;
        };
        if tray.menu(key).is_none() {
            return;
        }
        if tray
            .refresh_menu(key, &service, &path, parent, false)
            .await
            .is_ok()
        {
            self.queue.push_back(Event::MenuChanged {
                key: key.to_owned(),
            });
        }
    }

    fn properties_updated(&mut self, key: &str, body: &Message) {
        let Ok((updated, removed)) = body.body().deserialize::<(
            Vec<(i32, HashMap<String, OwnedValue>)>,
            Vec<(i32, Vec<String>)>,
        )>() else {
            return;
        };
        let mut entries = self.shared.entries.lock().unwrap();
        let Some(menu) = entries
            .iter_mut()
            .find(|e| e.item.key == key)
            .and_then(|e| e.menu.as_mut())
        else {
            return;
        };
        for (id, props) in &updated {
            if let Some(node) = menu.find_mut(*id) {
                node.update_properties(props, &[]);
            }
        }
        for (id, names) in &removed {
            if let Some(node) = menu.find_mut(*id) {
                node.update_properties(&HashMap::new(), names);
            }
        }
        self.queue.push_back(Event::MenuChanged {
            key: key.to_owned(),
        });
    }
}
