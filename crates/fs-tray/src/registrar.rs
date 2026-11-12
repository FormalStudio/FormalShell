//! `com.canonical.AppMenu.Registrar`: an app names the dbusmenu that holds
//! its main menu.
//!
//! The protocol keys a menu by an X11 window id, which a Wayland window
//! does not have. A registration is therefore kept by the caller's process
//! id, the one a compositor reports for that process's windows, and the
//! window id only pairs `RegisterWindow` with `UnregisterWindow`.

use std::sync::{Arc, Mutex};

use zbus::{
    Connection, fdo, interface,
    message::Header,
    names::BusName,
    zvariant::{ObjectPath, OwnedObjectPath},
};

use crate::dbusmenu::Address;

/// Not the interface's own name: Qt and GTK apps hide their in-window menu
/// bars once `com.canonical.AppMenu.Registrar` has an owner.
pub const REGISTRAR_NAME: &str = "org.atoll.AppMenu.Registrar";
pub const REGISTRAR_PATH: &str = "/com/canonical/AppMenu/Registrar";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub pid: u32,
    pub window: u32,
    pub menu: Address,
}

/// The menus apps registered. Cheap to clone.
#[derive(Clone, Default)]
pub struct Registry {
    entries: Arc<Mutex<Vec<Registration>>>,
}

impl Registry {
    /// The menu of the process `pid`, the last it registered.
    pub fn for_pid(&self, pid: u32) -> Option<Address> {
        self.entries.lock().unwrap().iter().rev().find(|r| r.pid == pid).map(|r| r.menu.clone())
    }

    pub fn all(&self) -> Vec<Registration> {
        self.entries.lock().unwrap().clone()
    }

    /// The processes with a registration, ascending.
    pub fn pids(&self) -> Vec<u32> {
        let mut pids: Vec<u32> = self.entries.lock().unwrap().iter().map(|r| r.pid).collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// Drops what the connection `service` registered, once it left the
    /// bus. True when it had registered anything.
    pub fn forget(&self, service: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|r| r.menu.service != service);
        entries.len() != before
    }
}

/// The served object. `changed` runs after every registration and removal,
/// on whichever executor drives the connection.
pub struct Registrar {
    registry: Registry,
    changed: Box<dyn Fn() + Send + Sync>,
}

impl Registrar {
    pub fn new(changed: impl Fn() + Send + Sync + 'static) -> (Self, Registry) {
        let registry = Registry::default();
        (Self { registry: registry.clone(), changed: Box::new(changed) }, registry)
    }
}

fn sender(header: &Header<'_>) -> fdo::Result<String> {
    header
        .sender()
        .map(|s| s.to_string())
        .ok_or_else(|| fdo::Error::Failed("a registration needs a sender".into()))
}

/// What `org.atoll.MenuBar` tells a Mac app about the shell's menu bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BarState {
    /// How far down from the top edge the bar reaches, so a fullscreen
    /// window's title bar can sit clear of it; 0 while the bar sits on
    /// another edge.
    pub height: u32,
    /// The bar is up over a fullscreen window, brought there by the pointer
    /// at the top edge, which the window itself never sees.
    pub revealed: bool,
}

/// `org.atoll.MenuBar`, served beside the registrar.
#[derive(Default)]
pub struct MenuBar {
    state: BarState,
}

#[interface(name = "org.atoll.MenuBar")]
impl MenuBar {
    #[zbus(property)]
    fn height(&self) -> u32 {
        self.state.height
    }

    #[zbus(property)]
    fn revealed(&self) -> bool {
        self.state.revealed
    }
}

impl MenuBar {
    /// Sets the bar `conn` serves, announcing each property that moved.
    pub(crate) async fn set(conn: &Connection, state: BarState) -> zbus::Result<()> {
        let iface = conn.object_server().interface::<_, MenuBar>(REGISTRAR_PATH).await?;
        let mut bar = iface.get_mut().await;
        let before = std::mem::replace(&mut bar.state, state);
        if before.height != state.height {
            bar.height_changed(iface.signal_emitter()).await?;
        }
        if before.revealed != state.revealed {
            bar.revealed_changed(iface.signal_emitter()).await?;
        }
        Ok(())
    }
}

#[interface(name = "com.canonical.AppMenu.Registrar")]
impl Registrar {
    async fn register_window(
        &self,
        window_id: u32,
        menu_object_path: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        let service = sender(&header)?;
        let name = BusName::try_from(service.as_str()).map_err(|e| fdo::Error::Failed(e.to_string()))?;
        let pid = fdo::DBusProxy::new(conn).await?.get_connection_unix_process_id(name).await?;
        let menu = Address { service, path: menu_object_path.to_string() };
        {
            let mut entries = self.registry.entries.lock().unwrap();
            entries.retain(|r| !(r.menu.service == menu.service && r.window == window_id));
            entries.push(Registration { pid, window: window_id, menu });
        }
        (self.changed)();
        Ok(())
    }

    fn unregister_window(&self, window_id: u32, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        let service = sender(&header)?;
        self.registry.entries.lock().unwrap().retain(|r| !(r.menu.service == service && r.window == window_id));
        (self.changed)();
        Ok(())
    }

    fn get_menu_for_window(&self, window_id: u32) -> fdo::Result<(String, OwnedObjectPath)> {
        let entries = self.registry.entries.lock().unwrap();
        let r = entries
            .iter()
            .rev()
            .find(|r| r.window == window_id)
            .ok_or_else(|| fdo::Error::Failed(format!("no menu registered for window {window_id}")))?;
        let path = ObjectPath::try_from(r.menu.path.as_str()).map_err(|e| fdo::Error::Failed(e.to_string()))?;
        Ok((r.menu.service.clone(), path.into()))
    }
}
