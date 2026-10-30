//! logind's sleep hook for the lock: a `delay` inhibitor held while the session is awake,
//! so PrepareForSleep(true) reaches the lock before the machine sleeps. The
//! lock lets it go once its surface is secure (or the wait gave up), and a
//! fresh one is taken on PrepareForSleep(false).

use std::cell::RefCell;

use calloop::channel::Sender;
use futures_lite::StreamExt;
use zbus::Proxy;
use zbus::zvariant::OwnedFd;

use crate::runtime::Ctx;
use crate::wayland::lock::LockMsg;

const WHY: (&str, &str, &str, &str) = ("sleep", "FormalShell", "Lock the session before sleep", "delay");

thread_local! {
    static HELD: RefCell<Option<OwnedFd>> = const { RefCell::new(None) };
    static TX: RefCell<Option<Sender<LockMsg>>> = const { RefCell::new(None) };
}

pub fn start(ctx: &Ctx, tx: Sender<LockMsg>) {
    TX.with(|t| *t.borrow_mut() = Some(tx));
    ctx.spawn(run());
}

fn send(msg: LockMsg) {
    TX.with(|t| {
        if let Some(tx) = t.borrow().as_ref() {
            let _ = tx.send(msg);
        }
    });
}

/// Lets the machine sleep. Closing the descriptor is the release.
pub fn release() {
    if HELD.with(|h| h.borrow_mut().take()).is_some() {
        send(LockMsg::Inhibiting(false));
    }
}

async fn run() {
    let Ok(conn) = crate::services::bus::system().await else { return };
    let proxy = match Proxy::new(&conn, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await {
        Ok(p) => p,
        Err(err) => return eprintln!("sleep: logind: {err}"),
    };
    let mut signals = match proxy.receive_signal("PrepareForSleep").await {
        Ok(s) => s,
        Err(err) => return eprintln!("sleep: PrepareForSleep: {err}"),
    };
    send(LockMsg::Monitoring(true));
    take(&proxy).await;
    while let Some(msg) = signals.next().await {
        let Ok(sleeping) = msg.body().deserialize::<bool>() else { continue };
        if !sleeping {
            take(&proxy).await;
        }
        send(LockMsg::Sleep(sleeping));
    }
    send(LockMsg::Monitoring(false));
}

async fn take(proxy: &Proxy<'_>) {
    if HELD.with(|h| h.borrow().is_some()) {
        return;
    }
    match proxy.call::<_, _, OwnedFd>("Inhibit", &WHY).await {
        Ok(fd) => {
            HELD.with(|h| *h.borrow_mut() = Some(fd));
            send(LockMsg::Inhibiting(true));
        }
        Err(err) => eprintln!("sleep: Inhibit: {err}"),
    }
}
