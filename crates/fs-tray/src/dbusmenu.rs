//! The dbusmenu client: one menu at a bus name and an object path, whoever
//! serves it (a tray item, an app's main menu, a dock quicklist).

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use zbus::{
    Connection, MatchRule, MessageStream,
    message::Type,
    zvariant::{OwnedValue, Value},
};

use crate::types::MenuItem;

pub(crate) const MENU_IFACE: &str = "com.canonical.dbusmenu";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEventKind {
    Clicked,
    Opened,
    Closed,
}

impl MenuEventKind {
    fn wire(self) -> &'static str {
        match self {
            Self::Clicked => "clicked",
            Self::Opened => "opened",
            Self::Closed => "closed",
        }
    }
}

pub(crate) async fn call<R>(
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

/// Where a menu is served.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Address {
    /// The owner's bus name.
    pub service: String,
    pub path: String,
}

impl Address {
    /// `AboutToShow(id)`: the app fills that menu in and brings its items'
    /// states up to date. Answers whether it changed anything.
    pub async fn about_to_show(&self, conn: &Connection, id: i32) -> zbus::Result<bool> {
        call(conn, &self.service, &self.path, MENU_IFACE, "AboutToShow", &(id,)).await
    }

    /// The whole subtree under `parent` (`GetLayout(parent, -1, [])`).
    pub async fn layout(&self, conn: &Connection, parent: i32) -> zbus::Result<MenuItem> {
        let (_rev, layout): (u32, RawLayout) = call(
            conn,
            &self.service,
            &self.path,
            MENU_IFACE,
            "GetLayout",
            &(parent, -1i32, Vec::<&str>::new()),
        )
        .await?;
        Ok(build_layout(layout))
    }

    pub async fn event(&self, conn: &Connection, id: i32, kind: MenuEventKind) -> zbus::Result<()> {
        call::<()>(
            conn,
            &self.service,
            &self.path,
            MENU_IFACE,
            "Event",
            &(id, kind.wire(), Value::I32(0), now_secs()),
        )
        .await
    }

    /// `LayoutUpdated` and `ItemsPropertiesUpdated` from this menu. The
    /// caller drains it while it awaits any reply on `conn`
    /// (`fs_dbus::drain_while`).
    pub async fn changes(&self, conn: &Connection) -> zbus::Result<MessageStream> {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(self.service.as_str())?
            .path(self.path.as_str())?
            .interface(MENU_IFACE)?
            .build();
        MessageStream::for_match_rule(rule, conn, None).await
    }
}
