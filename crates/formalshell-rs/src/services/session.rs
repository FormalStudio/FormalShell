//! logind's screen lock for this shell's own session: its Lock signal
//! raises the lock surface, and LockedHint follows the surface, so
//! `loginctl lock-session`, `lock-sessions`, idle daemons and lid scripts
//! all reach the same lock.
//!
//! The session is the one `services::users` names, the same one the polkit
//! agent registers for.
//!
//! The Unlock signal lifts the lock, as GNOME's and KDE's do: polkit
//! already gates `loginctl unlock-session`, and a process running as the
//! user can defeat a screen lock without it anyway.

use std::cell::RefCell;

use futures_lite::StreamExt;
use zbus::Proxy;

use crate::runtime::{Ctx, Msg, Publisher};

const LOGIN1: &str = "org.freedesktop.login1";
const SESSION: &str = "org.freedesktop.login1.Session";

thread_local! {
    static PROXY: RefCell<Option<Proxy<'static>>> = const { RefCell::new(None) };
    static HINT: RefCell<Option<bool>> = const { RefCell::new(None) };
}

/// What logind tells the lock.
pub enum Event {
    /// PrepareForSleep.
    Sleep(bool),
    /// Whether the sleep delay inhibitor is held.
    Inhibiting(bool),
    Monitoring(bool),
    /// The logind session whose Lock signal is watched, once found.
    Session(Option<String>),
    /// logind's Lock (true) or Unlock (false) for that session.
    Lock(bool),
    /// logind refused one of the lock's buttons: said under the field.
    Refused(String),
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.publisher().clone()));
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
/// comes back as [`Event::Refused`] when logind says no.
pub fn terminate(ctx: &Ctx, id: String) {
    let ui = ctx.publisher().clone();
    ctx.spawn(async move {
        let result = async {
            let conn = crate::services::bus::system().await?;
            let manager = Proxy::new(&conn, LOGIN1, "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await?;
            manager.call::<_, _, ()>("TerminateSession", &(id.as_str(),)).await
        }
        .await;
        if let Err(err) = result {
            eprintln!("session: TerminateSession {id}: {err}");
            ui.send(Msg::Lock(Event::Refused("Could not end the session".into())));
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

async fn run(ui: Publisher) {
    let Ok(conn) = crate::services::bus::system().await else { return };
    let crate::services::users::Session { path, id: name } = match crate::services::users::session(&conn).await {
        Ok(s) => s,
        Err(err) => return eprintln!("session: no logind session: {err}"),
    };
    let proxy = match Proxy::new(&conn, LOGIN1, path.clone(), SESSION).await {
        Ok(p) => p,
        Err(err) => return eprintln!("session: {path}: {err}"),
    };
    let (Ok(locks), Ok(unlocks)) = (proxy.receive_signal("Lock").await, proxy.receive_signal("Unlock").await) else {
        return eprintln!("session: no Lock/Unlock signals on {path}");
    };
    eprintln!("session: watching logind session {name}");
    PROXY.with(|p| *p.borrow_mut() = Some(proxy));
    ui.send(Msg::Lock(Event::Session(Some(name))));
    let mut signals = locks.map(|_| true).or(unlocks.map(|_| false));
    while let Some(lock) = signals.next().await {
        ui.send(Msg::Lock(Event::Lock(lock)));
    }
    PROXY.with(|p| *p.borrow_mut() = None);
    ui.send(Msg::Lock(Event::Session(None)));
}
