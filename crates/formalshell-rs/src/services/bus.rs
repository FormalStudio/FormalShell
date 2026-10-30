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

/// A connection of the service's own, for one that awaits replies while its
/// signal streams sit undrained: past zbus's queue (64) the socket reader
/// blocks, which on a shared connection stalls every other service. Driven
/// on the service thread rather than a zbus thread of its own.
pub async fn private(ctx: &Ctx, builder: zbus::Result<Builder<'static>>) -> zbus::Result<Connection> {
    let conn = builder?.internal_executor(false).build().await?;
    let executor = conn.executor().clone();
    ctx.spawn(async move {
        loop {
            executor.tick().await;
        }
    });
    Ok(conn)
}
