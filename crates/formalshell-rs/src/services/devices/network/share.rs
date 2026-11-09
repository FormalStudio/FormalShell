//! The network panel's password and QR share rows for the connected network.
//! Both read its saved passphrase through NetworkManager once (a request
//! while a read is in flight joins it), the QR handing the payload to
//! qrencode over stdin. The secret lives only in the state the rows hold and
//! is never logged.

use std::time::Duration;

use fs_devices::network::wifiqr::{self, Fields, PayloadError};
use fs_network::{NetworkManager, Security};
use futures_lite::AsyncWriteExt;

use super::{INNER, Inner, publish};
use crate::runtime::Ctx;
use crate::services::proc;

/// The connected network's saved passphrase, read only when asked for and
/// dropped on hide, on the panel closing and on roaming. No IPC verb reads
/// it, and its `Debug` never prints it.
#[derive(Clone, Default, PartialEq)]
pub enum Reveal {
    #[default]
    Idle,
    Reading,
    Shown(String),
    Failed(&'static str),
}

/// The share row's code. The matrix encodes the passphrase, so it lives
/// exactly as long as the expanded row.
#[derive(Clone, Default, PartialEq)]
pub enum Qr {
    #[default]
    Closed,
    Generating,
    Failed(&'static str),
    Shown(Vec<String>),
}

impl std::fmt::Debug for Reveal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reveal::Shown(_) => f.write_str("Shown(..)"),
            Reveal::Idle => f.write_str("Idle"),
            Reveal::Reading => f.write_str("Reading"),
            Reveal::Failed(e) => write!(f, "Failed({e})"),
        }
    }
}

impl std::fmt::Debug for Qr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Qr::Shown(_) => f.write_str("Shown(..)"),
            Qr::Closed => f.write_str("Closed"),
            Qr::Generating => f.write_str("Generating"),
            Qr::Failed(e) => write!(f, "Failed({e})"),
        }
    }
}

/// The connected network, as the rows need it.
#[derive(Clone)]
struct Target {
    ssid: String,
    security: Security,
    hidden: bool,
}

enum Read {
    NotConnected,
    Enterprise,
    Open(Target),
    /// `None` when the profile holds no passphrase.
    Psk(Target, Option<String>),
    Failed,
}

fn target(i: &Inner) -> Option<Target> {
    let n = i.snapshot.as_ref()?.networks.iter().find(|n| n.connected())?;
    Some(Target { ssid: n.ssid.clone(), security: n.security, hidden: n.hidden })
}

async fn read(nm: &NetworkManager, t: Target) -> Read {
    if t.security.is_enterprise() {
        return Read::Enterprise;
    }
    if !t.security.is_secured() {
        return Read::Open(t);
    }
    match nm.saved_psk(&t.ssid).await {
        Ok(psk) => Read::Psk(t, psk),
        Err(err) => {
            eprintln!("network: no passphrase read for {}: {err}", t.ssid);
            Read::Failed
        }
    }
}

/// SHOW or HIDE on the password row.
pub fn reveal_toggle(ctx: &Ctx) {
    let read = INNER.with_borrow_mut(|i| {
        if i.reveal != Reveal::Idle {
            i.reveal = Reveal::Idle;
            false
        } else {
            i.reveal = Reveal::Reading;
            true
        }
    });
    publish();
    if read {
        request(ctx);
    }
}

/// The share row expanding or collapsing.
pub fn qr_toggle(ctx: &Ctx) {
    let open = INNER.with_borrow_mut(|i| {
        if i.qr != Qr::Closed {
            i.qr = Qr::Closed;
            false
        } else {
            i.qr = Qr::Generating;
            true
        }
    });
    publish();
    if !open {
        return;
    }
    let c = ctx.clone();
    ctx.spawn(async move {
        if proc::capture(&proc::argv(&["qrencode", "--version"]), Duration::from_secs(5)).await.code == proc::MISSING {
            return qr_fail("NO QRENCODE");
        }
        if INNER.with_borrow(|i| i.qr == Qr::Generating) {
            request(&c);
        }
    });
}

fn qr_fail(message: &'static str) {
    INNER.with_borrow_mut(|i| {
        if i.qr == Qr::Generating {
            i.qr = Qr::Failed(message);
        }
    });
    publish();
}

/// A request while a read is in flight joins it.
fn request(ctx: &Ctx) {
    if INNER.with_borrow_mut(|i| std::mem::replace(&mut i.reading_fields, true)) {
        return;
    }
    let (nm, target) = INNER.with_borrow(|i| (i.nm.clone(), target(i)));
    let c = ctx.clone();
    ctx.spawn(async move {
        let read = match (nm, target) {
            (Some(nm), Some(t)) => read(&nm, t).await,
            _ => Read::NotConnected,
        };
        INNER.with_borrow_mut(|i| i.reading_fields = false);
        on_reveal(&read);
        on_qr(&c, read);
    });
}

fn on_reveal(read: &Read) {
    let next = match read {
        Read::Enterprise => Reveal::Failed("ENTERPRISE"),
        Read::Open(_) => Reveal::Failed("OPEN NETWORK"),
        Read::Psk(_, None) => Reveal::Failed("NO PASSWORD SAVED"),
        Read::Psk(_, Some(psk)) => Reveal::Shown(psk.clone()),
        Read::NotConnected | Read::Failed => Reveal::Failed("ERROR"),
    };
    INNER.with_borrow_mut(|i| {
        if i.reveal == Reveal::Reading {
            i.reveal = next;
        }
    });
    publish();
}

fn on_qr(ctx: &Ctx, read: Read) {
    if INNER.with_borrow(|i| i.qr != Qr::Generating) {
        return;
    }
    let (target, key_mgmt, password) = match read {
        Read::NotConnected => return qr_fail("NOT CONNECTED"),
        Read::Enterprise => return qr_fail("ENTERPRISE CANNOT SHARE"),
        Read::Failed | Read::Psk(_, None) => return qr_fail("ERROR"),
        Read::Open(t) => (t, "", String::new()),
        Read::Psk(t, Some(psk)) => (t, "wpa-psk", psk),
    };
    let fields = Fields {
        ssid: target.ssid,
        key_mgmt: key_mgmt.to_owned(),
        password,
        hidden: if target.hidden { "yes" } else { "no" }.to_owned(),
        wep_key: String::new(),
    };
    let payload = match wifiqr::build_payload(&fields) {
        Ok(p) => p,
        Err(PayloadError::NoSsid) => return qr_fail("NOT CONNECTED"),
        Err(_) => return qr_fail("ERROR"),
    };
    drop(fields);
    // The payload reaches qrencode over stdin, never argv.
    ctx.spawn(async move {
        let child = proc::command("qrencode")
            .args(["--type", "ASCII", "--margin", "4", "--output", "-"])
            .stdin(async_process::Stdio::piped())
            .stdout(async_process::Stdio::piped())
            .stderr(async_process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return qr_fail("ERROR") };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload.as_bytes()).await;
        }
        drop(payload);
        let Ok(out) = child.output().await else { return qr_fail("ERROR") };
        if !out.status.success() {
            return qr_fail("ERROR");
        }
        let matrix = wifiqr::parse_matrix(&String::from_utf8_lossy(&out.stdout));
        if matrix.is_empty() {
            return qr_fail("ERROR");
        }
        INNER.with_borrow_mut(|i| {
            if i.qr == Qr::Generating {
                i.qr = Qr::Shown(matrix);
            }
        });
        publish();
    });
}
