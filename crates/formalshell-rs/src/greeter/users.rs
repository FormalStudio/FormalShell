//! The pictures of the people who can log in: the greeter's mirrored
//! `avatars/<login>`, else AccountsService's icon.

use std::path::Path;

pub fn avatar_path(state: &Path, login: &str) -> Option<String> {
    [state.join("avatars").join(login), Path::new("/var/lib/AccountsService/icons").join(login)]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.to_str().map(str::to_owned))
}
