//! Who the shell runs as and which logind session it acts for, the one
//! answer the lock, the polkit agent, the network service and the greeter
//! read, and the people who can log in, which the greeter lists.

use std::sync::OnceLock;

pub use fs_auth::user::Session;

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub login: String,
    /// The GECOS name, else the login.
    pub name: String,
    pub uid: u32,
}

/// `/etc/passwd` accounts in the regular uid range with a login shell, as
/// GDM and cosmic-greeter list them.
pub fn people() -> Vec<User> {
    std::fs::read_to_string("/etc/passwd").map(|t| parse(&t)).unwrap_or_default()
}

fn parse(passwd: &str) -> Vec<User> {
    let mut out: Vec<User> = passwd
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            let [login, _, uid, _, gecos, _, shell] = f[..] else { return None };
            let uid: u32 = uid.parse().ok()?;
            if !(1000..60000).contains(&uid) || shell.ends_with("nologin") || shell.ends_with("false") {
                return None;
            }
            Some(User { login: login.to_owned(), name: full_name(gecos, login), uid })
        })
        .collect();
    out.sort_by(|a, b| a.login.cmp(&b.login));
    out
}

/// Before anyone has logged in here: the lowest uid, the machine's first account, rather than
/// whichever login sorts first (a test account `fstest0` before its owner).
pub fn first_account(users: &[User]) -> usize {
    users.iter().enumerate().min_by_key(|(_, u)| u.uid).map_or(0, |(i, _)| i)
}

/// GECOS's first field, else the login.
fn full_name(gecos: &str, login: &str) -> String {
    gecos.split(',').next().filter(|n| !n.trim().is_empty()).unwrap_or(login).trim().to_owned()
}

/// The account this process runs as, read once.
fn own() -> Option<&'static User> {
    static OWN: OnceLock<Option<User>> = OnceLock::new();
    OWN.get_or_init(|| {
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let account = fs_auth::user::account(uid)?;
        Some(User { name: full_name(&account.gecos, &account.login), login: account.login, uid })
    })
    .as_ref()
}

/// The login this process runs as: who PAM authenticates for the lock.
pub fn login() -> Option<&'static str> {
    own().map(|u| u.login.as_str())
}

/// Its full name, else its login, for the lock screen.
pub fn name() -> &'static str {
    own().map_or("", |u| u.name.as_str())
}

/// The logind session the shell acts for: the lock watches it and the
/// polkit agent registers for it ([`fs_auth::user::session`]).
pub async fn session(connection: &zbus::Connection) -> zbus::Result<Session> {
    fs_auth::user::session(connection).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_people_not_services() {
        let users = parse(
            "root:x:0:0:System administrator:/root:/usr/bin/fish\n\
             kyandesutter:x:1000:100:Kyan De Sutter,,,:/home/kyandesutter:/usr/bin/fish\n\
             greeter:x:999:999::/var/empty:/usr/sbin/nologin\n\
             nobody:x:65534:65534:Unprivileged account:/var/empty:/usr/sbin/nologin\n\
             alice:x:1001:100::/home/alice:/bin/bash\n",
        );
        assert_eq!(
            users,
            [
                User { login: "alice".into(), name: "alice".into(), uid: 1001 },
                User { login: "kyandesutter".into(), name: "Kyan De Sutter".into(), uid: 1000 }
            ]
        );
        assert_eq!(first_account(&users), 1);
    }
}
