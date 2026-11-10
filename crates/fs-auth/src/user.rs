//! Who a uid is, and which logind session this process acts for: what PAM
//! is asked to authenticate and what the polkit agent registers for.

use std::ffi::CStr;

use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

const LOGIN1: &str = "org.freedesktop.login1";

/// One passwd entry, through NSS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub login: String,
    pub gecos: String,
}

/// `uid`'s passwd entry, `None` when there is none.
pub fn account(uid: u32) -> Option<Account> {
    let mut buf = vec![0u8; 4096];
    loop {
        // SAFETY: an all-zero passwd is a valid value for getpwuid_r to overwrite.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: every pointer is valid for the call and buf outlives the
        // reads of pw_name and pw_gecos below.
        let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr().cast(), buf.len(), &mut result) };
        if rc == libc::ERANGE && buf.len() < 1 << 20 {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if rc != 0 || result.is_null() || pwd.pw_name.is_null() {
            return None;
        }
        // SAFETY: getpwuid_r succeeded, so both point into buf or are null.
        let login = unsafe { CStr::from_ptr(pwd.pw_name) }.to_str().ok()?.to_owned();
        let gecos = if pwd.pw_gecos.is_null() { String::new() } else { unsafe { CStr::from_ptr(pwd.pw_gecos) }.to_string_lossy().into_owned() };
        return Some(Account { login, gecos });
    }
}

/// A logind session: its object path and its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub path: OwnedObjectPath,
    pub id: String,
}

/// The logind session this process acts for: `$XDG_SESSION_ID` when it was
/// started inside one, else logind's `auto`, the caller's own session or,
/// for a caller in none (the shell under uwsm runs in the user manager), the
/// user's display session.
pub async fn session(connection: &Connection) -> zbus::Result<Session> {
    let manager = zbus::Proxy::new(connection, LOGIN1, "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await?;
    let asked = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "auto".into());
    let path: OwnedObjectPath = manager.call("GetSession", &(asked.as_str(),)).await?;
    let session = zbus::Proxy::new(connection, LOGIN1, path.clone(), "org.freedesktop.login1.Session").await?;
    let id: String = session.get_property("Id").await?;
    Ok(Session { path, id })
}
