//! The `org.freedesktop.Notifications` server (Desktop Notifications 1.2).
//!
//! Behaviour: ids start at 1, `replaces_id` updates in place and only for
//! an id that is still live, the server never expires anything itself (the
//! owner calls [`Server::close`] with [`CloseReason::Expired`]), and a
//! non-resident notification closes as dismissed after an action is invoked.
//!
//! The bus name is requested without replacement but queued: a running
//! notification daemon is never displaced, and the bus hands the name over
//! once it exits (a D-Bus-activated daemon that took it while the shell was
//! down, say).

mod image;
mod iface;
mod model;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use async_channel::{Receiver, Sender};
use enumflags2::BitFlags;
use zbus::connection::Builder;

pub use image::{Image, ImageData};
pub use model::{Action, CloseReason, Notification, Percent, Urgency};

pub const BUS_NAME: &str = "org.freedesktop.Notifications";
pub const OBJECT_PATH: &str = "/org/freedesktop/Notifications";

#[derive(Debug)]
pub enum Error {
    NoSuchNotification(u32),
    NoSuchAction { id: u32, key: String },
    Bus(zbus::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoSuchNotification(id) => write!(f, "no live notification {id}"),
            Error::NoSuchAction { id, key } => write!(f, "notification {id} has no action {key:?}"),
            Error::Bus(e) => write!(f, "d-bus: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Error::Bus(e)
    }
}

/// What `GetServerInformation` reports: the identity `formalshell`, spec 1.2.
pub(crate) const NAME: &str = "formalshell";
pub(crate) const VENDOR: &str = "FormalShell";
pub(crate) const SPEC_VERSION: &str = "1.2";
/// No markup; actions, persistence and static icons are served.
pub(crate) const CAPABILITIES: [&str; 4] = ["persistence", "body", "actions", "icon-static"];

/// A change the sender made. Closes the owner causes itself ([`Server::close`],
/// [`Server::invoke_action`]) are not repeated here.
#[derive(Debug, Clone)]
pub enum Event {
    Notified(Arc<Notification>),
    /// Same id, new content: a `replaces_id` or synchronous-tag update.
    Replaced(Arc<Notification>),
    /// The sender called `CloseNotification`.
    Closed { id: u32 },
}

pub(crate) struct State {
    next_id: u32,
    live: BTreeMap<u32, Arc<Notification>>,
}

pub(crate) struct Shared {
    state: Mutex<State>,
    tx: Sender<Event>,
    pub(crate) version: String,
}

impl Shared {
    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn emit(&self, event: Event) {
        let _ = self.tx.try_send(event);
    }
}

impl State {
    /// Registers `args` under the id it replaces, or a fresh one.
    pub(crate) fn upsert(&mut self, replaces_id: u32, args: model::NotifyArgs) -> (Arc<Notification>, bool) {
        let mut n = Notification::from_args(0, args);
        let replaced = (replaces_id != 0 && self.live.contains_key(&replaces_id))
            .then_some(replaces_id)
            .or_else(|| {
                let tag = n.synchronous.as_deref()?;
                self.live
                    .values()
                    .find(|o| o.synchronous.as_deref() == Some(tag) && o.app_name == n.app_name)
                    .map(|o| o.id)
            });
        n.id = replaced.unwrap_or_else(|| self.fresh_id());
        let n = Arc::new(n);
        self.live.insert(n.id, n.clone());
        (n, replaced.is_some())
    }

    fn fresh_id(&mut self) -> u32 {
        loop {
            let id = self.next_id;
            self.next_id = self.next_id.checked_add(1).unwrap_or(1);
            if id != 0 && !self.live.contains_key(&id) {
                return id;
            }
        }
    }

    pub(crate) fn remove(&mut self, id: u32) -> Option<Arc<Notification>> {
        self.live.remove(&id)
    }
}

/// A running server. Dropping it (and every clone of its connection) releases
/// the bus name.
pub struct Server {
    conn: zbus::Connection,
    shared: Arc<Shared>,
}

impl Server {
    /// Serves on the session bus, reporting `version` as the server version.
    pub async fn start(version: &str) -> Result<(Server, Receiver<Event>), Error> {
        Self::start_on(Builder::session()?, version).await
    }

    /// Serves on the bus `builder` connects to. The builder must not have a
    /// name or interfaces of its own.
    pub async fn start_on(builder: Builder<'_>, version: &str) -> Result<(Server, Receiver<Event>), Error> {
        let (tx, rx) = async_channel::unbounded();
        let shared = Arc::new(Shared {
            state: Mutex::new(State { next_id: 1, live: BTreeMap::new() }),
            tx,
            version: version.to_owned(),
        });
        let conn = builder.serve_at(OBJECT_PATH, iface::Notifications { shared: shared.clone() })?.build().await?;
        // The builder always passes DoNotQueue, and request_name's defaults replace the owner.
        conn.request_name_with_flags(BUS_NAME, BitFlags::empty()).await?;
        Ok((Server { conn, shared }, rx))
    }

    pub fn connection(&self) -> &zbus::Connection {
        &self.conn
    }

    /// Live notifications in id order.
    pub fn notifications(&self) -> Vec<Arc<Notification>> {
        self.shared.state().live.values().cloned().collect()
    }

    pub fn get(&self, id: u32) -> Option<Arc<Notification>> {
        self.shared.state().live.get(&id).cloned()
    }

    /// Closes a notification and emits `NotificationClosed` with `reason`.
    /// Returns false when `id` is not live.
    pub async fn close(&self, id: u32, reason: CloseReason) -> Result<bool, Error> {
        if self.shared.state().remove(id).is_none() {
            return Ok(false);
        }
        iface::closed(&self.conn, id, reason).await?;
        Ok(true)
    }

    /// Emits `ActivationToken` (when given) then `ActionInvoked`, then closes
    /// the notification as dismissed unless it is resident. Returns whether it
    /// was closed.
    pub async fn invoke_action(&self, id: u32, key: &str, activation_token: Option<&str>) -> Result<bool, Error> {
        let n = self.get(id).ok_or(Error::NoSuchNotification(id))?;
        if !n.actions.iter().any(|a| a.key == key) {
            return Err(Error::NoSuchAction { id, key: key.to_owned() });
        }
        iface::action_invoked(&self.conn, id, key, activation_token).await?;
        if n.resident {
            return Ok(false);
        }
        self.close(id, CloseReason::Dismissed).await
    }
}
