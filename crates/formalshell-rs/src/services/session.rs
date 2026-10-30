//! logind's screen lock for this shell's own session: its Lock signal
//! raises the lock surface, and LockedHint follows the surface, so
//! `loginctl lock-session`, `lock-sessions`, idle daemons and lid scripts
//! all reach the same lock.
//!
//! The session is `$XDG_SESSION_ID` when the shell was started inside one,
//! else logind's `auto`: the caller's session, or the user's display
//! session for a caller in none. Under uwsm the shell runs in the user
//! manager, outside every session, and `auto` is what finds the desktop's.
//!
//! The Unlock signal never lifts the lock. Logind lets any process of the
//! session's own user call UnlockSession with no authentication, and the
//! signal does not say who asked, so honouring it would be an unlock without
//! the password.

use std::cell::RefCell;

use calloop::channel::Sender;
use futures_lite::StreamExt;
use zbus::Proxy;
use zbus::zvariant::OwnedObjectPath;

use crate::runtime::Ctx;
use crate::wayland::lock::LockMsg;

const LOGIN1: &str = "org.freedesktop.login1";
const SESSION: &str = "org.freedesktop.login1.Session";

thread_local! {
    static PROXY: RefCell<Option<Proxy<'static>>> = const { RefCell::new(None) };
    static HINT: RefCell<Option<bool>> = const { RefCell::new(None) };
}

pub fn start(ctx: &Ctx, tx: Sender<LockMsg>) {
    ctx.spawn(run(tx));
}

/// Asks logind to lock the session, which comes back as its Lock signal.
/// False with no session to ask.
pub fn request(ctx: &Ctx) -> bool {
    let Some(proxy) = PROXY.with(|p| p.borrow().clone()) else { return false };
    ctx.spawn(async move {
        if let Err(err) = proxy.call_method("Lock", &()).await {
            eprintln!("session: Lock: {err}");
        }
    });
    true
}

/// logind's TerminateSession on `id`, the lock's "Other user". The answer
/// comes back as [`LockMsg::Refused`] when logind says no.
pub fn terminate(ctx: &Ctx, id: String, tx: Sender<LockMsg>) {
    ctx.spawn(async move {
        let result = async {
            let conn = crate::services::bus::system().await?;
            let manager = Proxy::new(&conn, LOGIN1, "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await?;
            manager.call::<_, _, ()>("TerminateSession", &(id.as_str(),)).await
        }
        .await;
        if let Err(err) = result {
            eprintln!("session: TerminateSession {id}: {err}");
            let _ = tx.send(LockMsg::Refused("Could not end the session".into()));
        }
    });
}

/// SetLockedHint, sent once per change.
pub fn locked_hint(ctx: &Ctx, locked: bool) {
    let Some(proxy) = PROXY.with(|p| p.borrow().clone()) else { return };
    if HINT.with(|h| h.replace(Some(locked))) == Some(locked) {
        return;
    }
    ctx.spawn(async move {
        if let Err(err) = proxy.call_method("SetLockedHint", &(locked,)).await {
            eprintln!("session: SetLockedHint: {err}");
        }
    });
}

async fn run(tx: Sender<LockMsg>) {
    let Ok(conn) = crate::services::bus::system().await else { return };
    let manager = match Proxy::new(&conn, LOGIN1, "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await {
        Ok(p) => p,
        Err(err) => return eprintln!("session: logind: {err}"),
    };
    let id = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "auto".into());
    let path: OwnedObjectPath = match manager.call("GetSession", &(id.as_str(),)).await {
        Ok(p) => p,
        Err(err) => return eprintln!("session: GetSession {id}: {err}"),
    };
    let proxy = match Proxy::new(&conn, LOGIN1, path.clone(), SESSION).await {
        Ok(p) => p,
        Err(err) => return eprintln!("session: {path}: {err}"),
    };
    let (Ok(locks), Ok(unlocks)) = (proxy.receive_signal("Lock").await, proxy.receive_signal("Unlock").await) else {
        return eprintln!("session: no Lock/Unlock signals on {path}");
    };
    let name = proxy.get_property::<String>("Id").await.unwrap_or(id);
    eprintln!("session: watching logind session {name}");
    PROXY.with(|p| *p.borrow_mut() = Some(proxy));
    let _ = tx.send(LockMsg::Session(Some(name)));
    let mut signals = locks.map(|_| true).or(unlocks.map(|_| false));
    while let Some(lock) = signals.next().await {
        let _ = tx.send(LockMsg::Logind(lock));
    }
    PROXY.with(|p| *p.borrow_mut() = None);
    let _ = tx.send(LockMsg::Session(None));
}
