//! logind's shutdown hook for the departure veil: a `delay` inhibitor held
//! while the session runs, so PrepareForShutdown(true) reaches the shell
//! before the session is torn down. The veil lets it go once its black has
//! been shown (or the wait gave up), and a fresh one is taken when a
//! shutdown is cancelled.

use std::cell::RefCell;

use calloop::channel::Sender;
use futures_lite::StreamExt;
use zbus::Proxy;
use zbus::zvariant::OwnedFd;

use crate::runtime::Ctx;

const WHY: (&str, &str, &str, &str) = ("shutdown", "FormalShell", "Fade the session out before power-off", "delay");

thread_local! {
    static HELD: RefCell<Option<OwnedFd>> = const { RefCell::new(None) };
}

/// `tx` carries each PrepareForShutdown's argument.
pub fn start(ctx: &Ctx, tx: Sender<bool>) {
    ctx.spawn(run(tx));
}

/// Lets the shutdown go on. Closing the descriptor is the release.
pub fn release() {
    HELD.with(|h| h.borrow_mut().take());
}

async fn run(tx: Sender<bool>) {
    let Ok(conn) = zbus::Connection::system().await else { return };
    let proxy = match Proxy::new(&conn, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await {
        Ok(p) => p,
        Err(err) => return eprintln!("shutdown: logind: {err}"),
    };
    let mut signals = match proxy.receive_signal("PrepareForShutdown").await {
        Ok(s) => s,
        Err(err) => return eprintln!("shutdown: PrepareForShutdown: {err}"),
    };
    take(&proxy).await;
    while let Some(msg) = signals.next().await {
        let Ok(going) = msg.body().deserialize::<bool>() else { continue };
        if !going {
            take(&proxy).await;
        }
        let _ = tx.send(going);
    }
}

async fn take(proxy: &Proxy<'_>) {
    if HELD.with(|h| h.borrow().is_some()) {
        return;
    }
    match proxy.call::<_, _, OwnedFd>("Inhibit", &WHY).await {
        Ok(fd) => HELD.with(|h| *h.borrow_mut() = Some(fd)),
        Err(err) => eprintln!("shutdown: Inhibit: {err}"),
    }
}
