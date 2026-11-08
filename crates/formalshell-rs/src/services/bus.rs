//! One connection per bus shared by every service: zbus runs a thread for
//! each connection it opens. A service that owns a well-known name before
//! anything else may call on it builds its own (`fs-notifd`, `inhibit.rs`).

use async_lock::OnceCell;
use zbus::Connection;
use zbus::connection::Builder;

use crate::runtime::Ctx;

static SYSTEM: OnceCell<Connection> = OnceCell::new();
static SESSION: OnceCell<Connection> = OnceCell::new();

pub async fn system() -> zbus::Result<Connection> {
    SYSTEM.get_or_try_init(Connection::system).await.cloned()
}

pub async fn session() -> zbus::Result<Connection> {
    SESSION.get_or_try_init(Connection::session).await.cloned()
}

/// A connection of the service's own (`fs_dbus::private`, which says when
/// one is needed), driven on the service thread rather than a zbus thread
/// of its own.
pub async fn private(ctx: &Ctx, builder: zbus::Result<Builder<'static>>) -> zbus::Result<Connection> {
    let (conn, tick) = fs_dbus::private(builder?).await?;
    ctx.spawn(tick);
    Ok(conn)
}
