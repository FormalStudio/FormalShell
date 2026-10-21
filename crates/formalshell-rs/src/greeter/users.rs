//! The people who can log in: `/etc/passwd` accounts in the regular uid
//! range with a login shell, as GDM and cosmic-greeter list them. Their
//! pictures are the greeter's mirrored `avatars/<login>`, else
//! AccountsService's icon.

use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub login: String,
    /// The GECOS name, else the login.
    pub name: String,
}

pub fn scan() -> Vec<User> {
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
            let name = gecos.split(',').next().filter(|n| !n.trim().is_empty()).unwrap_or(login);
            Some(User { login: login.to_owned(), name: name.trim().to_owned() })
        })
        .collect();
    out.sort_by(|a, b| a.login.cmp(&b.login));
    out
}

/// The session user's own full name, for the lock screen, else their login.
pub fn own_name() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let login = std::env::var("USER").unwrap_or_default();
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        passwd
            .lines()
            .map(|l| l.split(':').collect::<Vec<_>>())
            .find(|f| f.first() == Some(&login.as_str()))
            .and_then(|f| f.get(4).and_then(|g| g.split(',').next()).map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned))
            .unwrap_or(login)
    })
}

pub fn avatar_path(state: &Path, login: &str) -> Option<String> {
    [state.join("avatars").join(login), Path::new("/var/lib/AccountsService/icons").join(login)]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.to_str().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_people_not_services() {
        let users = parse(
            "root:x:0:0:System administrator:/root:/run/current-system/sw/bin/fish\n\
             kyandesutter:x:1000:100:Kyan De Sutter,,,:/home/kyandesutter:/run/current-system/sw/bin/fish\n\
             greeter:x:999:999::/var/empty:/run/current-system/sw/bin/nologin\n\
             nobody:x:65534:65534:Unprivileged account:/var/empty:/run/current-system/sw/bin/nologin\n\
             alice:x:1001:100::/home/alice:/bin/bash\n",
        );
        assert_eq!(
            users,
            [User { login: "alice".into(), name: "alice".into() }, User { login: "kyandesutter".into(), name: "Kyan De Sutter".into() }]
        );
    }
}
