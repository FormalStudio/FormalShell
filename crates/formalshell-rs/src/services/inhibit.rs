//! `org.freedesktop.ScreenSaver` on the session bus, the inhibit API Steam
//! and SDL use under XWayland. Every live cookie holds caffeinate's idle
//! inhibitor: this service keeps the cookie table and publishes its holders,
//! the UI thread holds the inhibitor while the toggle is on or any holder is
//! left. A client leaving the bus takes its cookies with it.
//!
//! The name is requested without replacing an owner: on a desktop whose own
//! daemon already serves it, the shell runs without it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_lite::StreamExt;
use zbus::connection::Builder;
use zbus::message::Header;
use zbus::{fdo, interface};

use super::caffeinate::{Diff, Holder};
use crate::runtime::{Ctx, Publisher};
use crate::store;

const BUS_NAME: &str = "org.freedesktop.ScreenSaver";
const PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];

/// The screensaver's own state for `GetActive`, written by the UI thread.
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_active(active: bool) {
    ACTIVE.store(active, Ordering::Relaxed);
}

#[derive(Default)]
pub struct Cookies {
    last: u32,
    live: BTreeMap<u32, Holder>,
}

impl Cookies {
    pub fn inhibit(&mut self, app: &str, reason: &str, sender: &str) -> u32 {
        loop {
            self.last = self.last.wrapping_add(1);
            if self.last != 0 && !self.live.contains_key(&self.last) {
                break;
            }
        }
        let holder = Holder { cookie: self.last, app: app.into(), reason: reason.into(), sender: sender.into() };
        self.live.insert(self.last, holder);
        self.last
    }

    pub fn uninhibit(&mut self, cookie: u32) -> bool {
        self.live.remove(&cookie).is_some()
    }

    pub fn release_sender(&mut self, sender: &str) -> bool {
        let before = self.live.len();
        self.live.retain(|_, h| h.sender != sender);
        self.live.len() != before
    }

    pub fn holders(&self) -> Vec<Holder> {
        self.live.values().cloned().collect()
    }
}

struct Shared {
    cookies: Mutex<Cookies>,
    publisher: Publisher,
}

impl Shared {
    fn change(&self, f: impl FnOnce(&mut Cookies) -> bool) {
        let mut cookies = self.cookies.lock().unwrap();
        if f(&mut cookies) {
            self.publisher.publish(store::Diff::Caffeinate(Diff::Holders(cookies.holders())));
        }
    }
}

struct ScreenSaver {
    shared: Arc<Shared>,
}

#[interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    fn inhibit(&self, application_name: &str, reason_for_inhibit: &str, #[zbus(header)] header: Header<'_>) -> u32 {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let mut cookie = 0;
        self.shared.change(|c| {
            cookie = c.inhibit(application_name, reason_for_inhibit, &sender);
            true
        });
        eprintln!("inhibit: {sender} {application_name:?} ({reason_for_inhibit:?}) took cookie {cookie}");
        cookie
    }

    fn un_inhibit(&self, cookie: u32) {
        self.shared.change(|c| c.uninhibit(cookie));
    }

    fn get_active(&self) -> bool {
        ACTIVE.load(Ordering::Relaxed)
    }
}

pub async fn run(ctx: Ctx) {
    let shared = Arc::new(Shared { cookies: Mutex::new(Cookies::default()), publisher: ctx.publisher().clone() });
    let built = async {
        let mut builder = Builder::session()?;
        for path in PATHS {
            builder = builder.serve_at(path, ScreenSaver { shared: shared.clone() })?;
        }
        builder.name(BUS_NAME)?.allow_name_replacements(false).replace_existing_names(false).build().await
    };
    let conn = match built.await {
        Ok(conn) => conn,
        Err(zbus::Error::NameTaken) => {
            eprintln!("inhibit: {BUS_NAME} is owned elsewhere, running without it");
            return;
        }
        Err(err) => {
            eprintln!("inhibit: {err}");
            return;
        }
    };
    let mut owners = match fdo::DBusProxy::new(&conn).await {
        Ok(dbus) => match dbus.receive_name_owner_changed().await {
            Ok(s) => s,
            Err(err) => return eprintln!("inhibit: NameOwnerChanged: {err}"),
        },
        Err(err) => return eprintln!("inhibit: {err}"),
    };
    while let Some(signal) = owners.next().await {
        let Ok(args) = signal.args() else { continue };
        if args.new_owner().is_none() && args.name().starts_with(':') {
            let gone = args.name().to_string();
            shared.change(|c| c.release_sender(&gone));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_distinct_nonzero_cookies() {
        let mut c = Cookies::default();
        let a = c.inhibit("Steam", "game", ":1.5");
        let b = c.inhibit("SDL", "video", ":1.6");
        assert_ne!(a, 0);
        assert_ne!(a, b);
        assert_eq!(c.holders().len(), 2);
        assert_eq!(c.holders()[0].app, "Steam");
    }

    #[test]
    fn releases_one_cookie() {
        let mut c = Cookies::default();
        let a = c.inhibit("Steam", "game", ":1.5");
        let b = c.inhibit("Steam", "overlay", ":1.5");
        assert!(c.uninhibit(a));
        assert!(!c.uninhibit(a));
        assert_eq!(c.holders().iter().map(|h| h.cookie).collect::<Vec<_>>(), vec![b]);
    }

    #[test]
    fn releases_every_cookie_of_a_sender() {
        let mut c = Cookies::default();
        c.inhibit("Steam", "game", ":1.5");
        c.inhibit("Steam", "overlay", ":1.5");
        let kept = c.inhibit("mpv", "video", ":1.9");
        assert!(c.release_sender(":1.5"));
        assert!(!c.release_sender(":1.5"));
        assert_eq!(c.holders().iter().map(|h| h.cookie).collect::<Vec<_>>(), vec![kept]);
    }

    #[test]
    fn wraps_past_live_cookies_and_zero() {
        let mut c = Cookies { last: u32::MAX - 1, live: BTreeMap::new() };
        let a = c.inhibit("a", "", ":1.1");
        assert_eq!(a, u32::MAX);
        c.last = 0;
        c.live.insert(1, Holder { cookie: 1, app: "b".into(), reason: String::new(), sender: ":1.2".into() });
        assert_eq!(c.inhibit("c", "", ":1.3"), 2);
        c.last = u32::MAX - 1;
        assert_eq!(c.inhibit("d", "", ":1.4"), 3);
    }
}
