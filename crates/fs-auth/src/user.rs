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

/// The logind session this process acts for, the first of: logind's session
/// for this pid; the user's display session, which polkit hands a request
/// from a process in no session to (under uwsm the shell and every app it
/// starts run in the user manager, outside every session, and inherit
/// whatever id the launcher had, maybe stale or an ssh login's);
/// `$XDG_SESSION_ID` when logind knows it and it is this uid's. The error
/// names why each step gave nothing.
pub async fn session(connection: &Connection) -> zbus::Result<Session> {
    let manager = zbus::Proxy::new(connection, LOGIN1, "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await?;
    let bus = Bus { connection, manager };
    let env = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty());
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    resolve(&bus, std::process::id(), uid, env.as_deref()).await
}

/// What [`resolve`] asks logind.
trait Logind {
    async fn by_pid(&self, pid: u32) -> zbus::Result<OwnedObjectPath>;
    async fn by_id(&self, id: &str) -> zbus::Result<OwnedObjectPath>;
    /// A session's id and the uid it belongs to.
    async fn session(&self, path: &OwnedObjectPath) -> zbus::Result<(String, u32)>;
    /// The user's display session, its id empty when it has none.
    async fn display(&self, uid: u32) -> zbus::Result<(String, OwnedObjectPath)>;
}

struct Bus<'a> {
    connection: &'a Connection,
    manager: zbus::Proxy<'a>,
}

impl Logind for Bus<'_> {
    async fn by_pid(&self, pid: u32) -> zbus::Result<OwnedObjectPath> {
        self.manager.call("GetSessionByPID", &(pid,)).await
    }

    async fn by_id(&self, id: &str) -> zbus::Result<OwnedObjectPath> {
        self.manager.call("GetSession", &(id,)).await
    }

    async fn session(&self, path: &OwnedObjectPath) -> zbus::Result<(String, u32)> {
        let session = zbus::Proxy::new(self.connection, LOGIN1, path.clone(), "org.freedesktop.login1.Session").await?;
        let id: String = session.get_property("Id").await?;
        let (uid, _): (u32, OwnedObjectPath) = session.get_property("User").await?;
        Ok((id, uid))
    }

    async fn display(&self, uid: u32) -> zbus::Result<(String, OwnedObjectPath)> {
        let user: OwnedObjectPath = self.manager.call("GetUser", &(uid,)).await?;
        let user = zbus::Proxy::new(self.connection, LOGIN1, user, "org.freedesktop.login1.User").await?;
        user.get_property("Display").await
    }
}

async fn resolve(logind: &impl Logind, pid: u32, uid: u32, env: Option<&str>) -> zbus::Result<Session> {
    let pid_why = match logind.by_pid(pid).await {
        Ok(path) => match logind.session(&path).await {
            Ok((id, _)) => return Ok(Session { path, id }),
            Err(err) => format!("pid {pid}'s session: {err}"),
        },
        Err(err) => format!("pid {pid}: {err}"),
    };
    let display_why = match logind.display(uid).await {
        Ok((id, path)) if !id.is_empty() => return Ok(Session { path, id }),
        Ok(_) => format!("uid {uid} has no display session"),
        Err(err) => format!("uid {uid}'s display session: {err}"),
    };
    let env_why = match env {
        None => "XDG_SESSION_ID unset".to_owned(),
        Some(asked) => match logind.by_id(asked).await {
            Err(err) => format!("XDG_SESSION_ID {asked}: {err}"),
            Ok(path) => match logind.session(&path).await {
                Ok((id, owner)) if owner == uid => return Ok(Session { path, id }),
                Ok((_, owner)) => format!("XDG_SESSION_ID {asked} is uid {owner}'s"),
                Err(err) => format!("XDG_SESSION_ID {asked}: {err}"),
            },
        },
    };
    Err(zbus::Error::Failure(format!("no logind session ({pid_why}; {display_why}; {env_why})")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sessions by id with their uid, a pid's session, and the display
    /// session.
    struct Fake {
        sessions: Vec<(&'static str, u32)>,
        pid: Option<&'static str>,
        display: &'static str,
    }

    fn path(id: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(format!("/org/freedesktop/login1/session/_3{id}")).unwrap()
    }

    fn missing() -> zbus::Error {
        zbus::Error::Failure("no such session".into())
    }

    impl Logind for Fake {
        async fn by_pid(&self, _: u32) -> zbus::Result<OwnedObjectPath> {
            self.pid.map(path).ok_or_else(missing)
        }

        async fn by_id(&self, id: &str) -> zbus::Result<OwnedObjectPath> {
            self.sessions.iter().find(|(s, _)| *s == id).map(|(s, _)| path(s)).ok_or_else(missing)
        }

        async fn session(&self, p: &OwnedObjectPath) -> zbus::Result<(String, u32)> {
            self.sessions.iter().find(|(s, _)| path(s) == *p).map(|(s, uid)| ((*s).to_owned(), *uid)).ok_or_else(missing)
        }

        async fn display(&self, _: u32) -> zbus::Result<(String, OwnedObjectPath)> {
            Ok((self.display.to_owned(), if self.display.is_empty() { OwnedObjectPath::try_from("/").unwrap() } else { path(self.display) }))
        }
    }

    fn id(fake: &Fake, env: Option<&str>) -> Result<String, String> {
        futures_lite::future::block_on(resolve(fake, 42, 1000, env)).map(|s| s.id).map_err(|e| e.to_string())
    }

    #[test]
    fn the_pids_own_session_wins_over_the_environment() {
        let fake = Fake { sessions: vec![("2", 1000), ("5", 1000)], pid: Some("5"), display: "2" };
        assert_eq!(id(&fake, Some("2")), Ok("5".into()));
    }

    #[test]
    fn a_stale_environment_id_is_passed_over() {
        let fake = Fake { sessions: vec![("2", 1000)], pid: None, display: "2" };
        assert_eq!(id(&fake, Some("7")), Ok("2".into()));
        let alone = Fake { sessions: vec![], pid: None, display: "" };
        assert!(id(&alone, Some("7")).unwrap_err().contains("XDG_SESSION_ID 7: "));
    }

    #[test]
    fn an_environment_id_of_another_users_session_is_passed_over() {
        let fake = Fake { sessions: vec![("2", 1000), ("4", 1001)], pid: None, display: "2" };
        assert_eq!(id(&fake, Some("4")), Ok("2".into()));
        let no_display = Fake { sessions: vec![("4", 1001)], pid: None, display: "" };
        assert!(id(&no_display, Some("4")).is_err());
    }

    #[test]
    fn with_no_pid_session_the_display_session_wins_over_the_environment() {
        // An ssh login's id inherited by a shell its user manager runs.
        let fake = Fake { sessions: vec![("2", 1000), ("3", 1000)], pid: None, display: "2" };
        assert_eq!(id(&fake, Some("3")), Ok("2".into()));
        assert_eq!(id(&fake, None), Ok("2".into()));
    }

    #[test]
    fn with_no_display_session_the_environment_id_is_taken() {
        let fake = Fake { sessions: vec![("3", 1000)], pid: None, display: "" };
        assert_eq!(id(&fake, Some("3")), Ok("3".into()));
    }

    #[test]
    fn nothing_resolving_names_every_step() {
        let fake = Fake { sessions: vec![("4", 1001)], pid: None, display: "" };
        let err = id(&fake, Some("4")).unwrap_err();
        assert!(err.contains("pid 42") && err.contains("XDG_SESSION_ID 4 is uid 1001's") && err.contains("no display session"), "{err}");
    }
}
