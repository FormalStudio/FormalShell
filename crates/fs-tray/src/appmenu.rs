//! App menus: the registrar apps name their main menu to, and the one menu
//! being followed (the focused app's), kept current off its signals.
//!
//! [`AppMenus::start`] returns the handle and the [`AppMenuEvents`] pump the
//! caller polls on its own executor, as the tray's is.

use std::{
    collections::VecDeque,
    future::{Future, poll_fn},
    pin::Pin,
    sync::{Arc, Mutex},
};

use fs_dbus::{Owner, OwnerWatch};
use futures_util::{
    Stream, StreamExt,
    future::AbortHandle,
    stream::{Abortable, SelectAll},
};
use zbus::{
    Connection,
    fdo::{RequestNameFlags, RequestNameReply},
};

use crate::{
    dbusmenu::{Address, MenuEventKind},
    registrar::{REGISTRAR_NAME, REGISTRAR_PATH, Registrar, Registry},
    types::MenuItem,
};

/// The followed process's menu, whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub pid: u32,
    pub address: Address,
    pub tree: MenuItem,
}

#[derive(Debug)]
pub enum AppMenuEvent {
    /// [`AppMenus::held`] changed.
    Changed,
    /// A call on the followed menu failed; it is held as it was.
    Failed(zbus::Error),
}

enum Ask {
    Follow(Option<u32>),
    AboutToShow(i32),
    Event(i32, MenuEventKind),
}

enum Input {
    Ask(Ask),
    Registered,
    Gone(String),
    Signal,
}

type InputStream = Pin<Box<dyn Stream<Item = Input> + Send>>;
type Inputs = Arc<Mutex<SelectAll<InputStream>>>;

struct Shared {
    conn: Connection,
    registry: Registry,
    asks: async_channel::Sender<Ask>,
    held: Mutex<Option<Held>>,
}

/// Reads the followed menu and acts on it. Cheap to clone.
#[derive(Clone)]
pub struct AppMenus {
    shared: Arc<Shared>,
}

/// The pump: nothing here moves unless [`AppMenuEvents::next`] is polled.
pub struct AppMenuEvents {
    core: Core,
    backlog: VecDeque<Input>,
}

struct Core {
    shared: Arc<Shared>,
    inputs: Inputs,
    pid: Option<u32>,
    /// Ends the followed menu's signal stream.
    signals: Option<AbortHandle>,
}

/// A read of the layout answers every signal before it, and only the last
/// process asked for is followed.
fn push_input(backlog: &mut VecDeque<Input>, input: Input) {
    match input {
        Input::Signal if backlog.iter().any(|i| matches!(i, Input::Signal)) => return,
        Input::Ask(Ask::Follow(_)) => backlog.retain(|i| !matches!(i, Input::Ask(Ask::Follow(_)))),
        _ => {}
    }
    backlog.push_back(input);
}

async fn drain_while<T>(inputs: &Inputs, backlog: &mut VecDeque<Input>, fut: impl Future<Output = T>) -> T {
    fs_dbus::drain_while(|cx| inputs.lock().unwrap().poll_next_unpin(cx), |input| push_input(backlog, input), fut).await
}

impl AppMenus {
    /// Serves the registrar on `conn` and takes its name. Fails when
    /// another process holds the name.
    pub async fn start(conn: Connection) -> zbus::Result<(AppMenus, AppMenuEvents)> {
        let (registered_tx, registered_rx) = async_channel::unbounded::<()>();
        let (registrar, registry) = Registrar::new(move || {
            let _ = registered_tx.try_send(());
        });
        conn.object_server().at(REGISTRAR_PATH, registrar).await?;
        let (asks, asks_rx) = async_channel::unbounded();
        let shared = Arc::new(Shared { conn: conn.clone(), registry, asks, held: Mutex::new(None) });
        let inputs: Inputs = Arc::new(Mutex::new(SelectAll::new()));
        let mut backlog = VecDeque::new();

        let owners = OwnerWatch::any(&conn).await?;
        {
            let mut all = inputs.lock().unwrap();
            all.push(Box::pin(owners.filter_map(|c| async move { (c.owner == Owner::Gone).then_some(Input::Gone(c.name)) })));
            all.push(Box::pin(asks_rx.map(Input::Ask)));
            all.push(Box::pin(registered_rx.map(|()| Input::Registered)));
        }
        let reply = drain_while(&inputs, &mut backlog, conn.request_name_with_flags(REGISTRAR_NAME, RequestNameFlags::DoNotQueue.into())).await?;
        if !matches!(reply, RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) {
            return Err(zbus::Error::NameTaken);
        }
        let core = Core { shared: shared.clone(), inputs, pid: None, signals: None };
        Ok((AppMenus { shared }, AppMenuEvents { core, backlog }))
    }

    /// The process whose menu is held from here on, or none.
    pub fn follow(&self, pid: Option<u32>) {
        let _ = self.shared.asks.try_send(Ask::Follow(pid));
    }

    pub fn held(&self) -> Option<Held> {
        self.shared.held.lock().unwrap().clone()
    }

    /// Has the app bring the submenu `id` up to date; the tree is read
    /// again after it.
    pub fn about_to_show(&self, id: i32) {
        let _ = self.shared.asks.try_send(Ask::AboutToShow(id));
    }

    pub fn event(&self, id: i32, kind: MenuEventKind) {
        let _ = self.shared.asks.try_send(Ask::Event(id, kind));
    }

    /// Where the process `pid` serves `path`, when it registered a menu:
    /// its bus name is the registration's.
    pub fn sibling(&self, pid: u32, path: &str) -> Option<Address> {
        let main = self.shared.registry.for_pid(pid)?;
        Some(Address { service: main.service, path: path.to_owned() })
    }

    pub fn connection(&self) -> &Connection {
        &self.shared.conn
    }
}

impl AppMenuEvents {
    pub async fn next(&mut self) -> Option<AppMenuEvent> {
        let inputs = self.core.inputs.clone();
        loop {
            let input = match self.backlog.pop_front() {
                Some(input) => input,
                None => poll_fn(|cx| inputs.lock().unwrap().poll_next_unpin(cx)).await?,
            };
            match drain_while(&inputs, &mut self.backlog, self.core.handle(input)).await {
                Ok(true) => return Some(AppMenuEvent::Changed),
                Ok(false) => {}
                Err(err) => return Some(AppMenuEvent::Failed(err)),
            }
        }
    }
}

impl Core {
    fn address(&self) -> Option<Address> {
        self.shared.held.lock().unwrap().as_ref().map(|h| h.address.clone())
    }

    /// True when the held menu changed.
    async fn handle(&mut self, input: Input) -> zbus::Result<bool> {
        match input {
            Input::Ask(Ask::Follow(pid)) => {
                self.pid = pid;
                self.retarget().await
            }
            Input::Registered => self.retarget().await,
            Input::Gone(name) => {
                if self.shared.registry.forget(&name) {
                    self.retarget().await
                } else {
                    Ok(false)
                }
            }
            Input::Signal => self.reload().await,
            Input::Ask(Ask::AboutToShow(id)) => {
                let Some(address) = self.address() else { return Ok(false) };
                address.about_to_show(&self.shared.conn, id).await?;
                self.reload().await
            }
            Input::Ask(Ask::Event(id, kind)) => {
                if let Some(address) = self.address() {
                    address.event(&self.shared.conn, id, kind).await?;
                }
                Ok(false)
            }
        }
    }

    /// Holds the menu the followed process registered, when that is not the
    /// one held.
    async fn retarget(&mut self) -> zbus::Result<bool> {
        let want = self.pid.and_then(|pid| Some((pid, self.shared.registry.for_pid(pid)?)));
        if want.as_ref().map(|(_, a)| a) == self.address().as_ref() {
            return Ok(false);
        }
        if let Some(signals) = self.signals.take() {
            signals.abort();
        }
        let had = self.shared.held.lock().unwrap().take().is_some();
        let Some((pid, address)) = want else { return Ok(had) };
        let changes = address.changes(&self.shared.conn).await?;
        let (abort, registration) = AbortHandle::new_pair();
        self.inputs.lock().unwrap().push(Box::pin(Abortable::new(changes.map(|_| Input::Signal), registration)));
        self.signals = Some(abort);
        let tree = address.layout(&self.shared.conn, 0).await?;
        *self.shared.held.lock().unwrap() = Some(Held { pid, address, tree });
        Ok(true)
    }

    async fn reload(&mut self) -> zbus::Result<bool> {
        let Some(address) = self.address() else { return Ok(false) };
        let tree = address.layout(&self.shared.conn, 0).await?;
        let mut held = self.shared.held.lock().unwrap();
        match held.as_mut().filter(|h| h.address == address) {
            Some(h) if h.tree != tree => {
                h.tree = tree;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
