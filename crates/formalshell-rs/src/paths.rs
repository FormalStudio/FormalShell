//! Where the shell keeps and finds files. Every XDG lookup goes through
//! here so the fallbacks agree: an unset or empty variable means the
//! default under `$HOME`, and a runtime dir that is missing falls back to a
//! per-user directory under `/tmp`.
//!
//! Also holds the names another program reads or writes: `install.rs`
//! writes them into the login script and the PAM file, the shell reads them.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The shell's own directory under each XDG base and the runtime dir.
pub const SHELL_DIR: &str = "formalshell";

/// Left by the login script before the compositor starts; the shell raises
/// the boot lock when it finds it.
pub const LOGIN_LOCK_MARKER: &str = "login-lock";

/// Left by the boot lock once the password has been checked, carrying
/// another session's environment and command; the login script starts that
/// one when the compositor has gone.
pub const LOGIN_SESSION_MARKER: &str = "login-session";

/// The XDG base directory spec treats a relative value as unset.
fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn base(env: &str, under_home: &str) -> PathBuf {
    var(env).unwrap_or_else(|| home().join(under_home))
}

/// `$XDG_STATE_HOME`, else `~/.local/state`.
pub fn state() -> PathBuf {
    base("XDG_STATE_HOME", ".local/state")
}

/// `$XDG_CONFIG_HOME`, else `~/.config`.
pub fn config() -> PathBuf {
    base("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_CACHE_HOME`, else `~/.cache`.
pub fn cache() -> PathBuf {
    base("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_DATA_HOME`, else `~/.local/share`.
pub fn data() -> PathBuf {
    base("XDG_DATA_HOME", ".local/share")
}

/// `data()` then `$XDG_DATA_DIRS` (else `/usr/local/share:/usr/share`),
/// relative entries dropped, each once.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![data()];
    match std::env::var_os("XDG_DATA_DIRS").filter(|v| !v.is_empty()) {
        Some(system) => dirs.extend(std::env::split_paths(&system)),
        None => dirs.extend(["/usr/local/share", "/usr/share"].map(PathBuf::from)),
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| d.is_absolute() && seen.insert(d.clone()));
    dirs
}

/// `$XDG_RUNTIME_DIR` when the session set one, for files that mean
/// nothing without it (another program's socket).
pub fn session_runtime() -> Option<PathBuf> {
    var("XDG_RUNTIME_DIR")
}

/// `<session runtime>/formalshell/login-session`, only while the session's
/// runtime dir is this user's alone. The login script runs what it finds
/// there, so a dir another user can reach, or no `$XDG_RUNTIME_DIR` at all
/// (the `/tmp` fallback), means no handover.
pub fn login_session_marker() -> Option<PathBuf> {
    let runtime = session_runtime()?;
    private_dir(&runtime).then(|| runtime.join(SHELL_DIR).join(LOGIN_SESSION_MARKER))
}

/// A directory, not a link to one, owned by this user with no group or
/// world access. Its ancestors are trusted, as logind's `/run/user/<uid>` is.
pub fn private_dir(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0)
}

/// A fresh 0600 file in a 0700 directory of this user's own, never through
/// a link. The directory is made if missing; one that is a link or another
/// user's is refused.
pub fn write_private(path: &Path, body: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    let dir = path.parent().unwrap_or(Path::new("."));
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        made => made?,
    }
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not a directory of this user's own"));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        removed => removed?,
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(path)?;
    file.write_all(body.as_ref())
}

/// `$XDG_RUNTIME_DIR`, else `/tmp/formalshell-<uid>`.
pub fn runtime() -> PathBuf {
    session_runtime().unwrap_or_else(|| PathBuf::from(format!("/tmp/formalshell-{}", unsafe { libc::getuid() })))
}

/// `<runtime>/formalshell`.
pub fn shell_runtime() -> PathBuf {
    runtime().join(SHELL_DIR)
}

/// `<config>/formalshell`: settings.json, menu.jsonc, plugins.
pub fn shell_config() -> PathBuf {
    config().join(SHELL_DIR)
}

/// `<state>/formalshell`: state.json and the theme engine's output.
pub fn shell_state() -> PathBuf {
    state().join(SHELL_DIR)
}

/// `<cache>/formalshell`.
pub fn shell_cache() -> PathBuf {
    cache().join(SHELL_DIR)
}

/// `$WAYLAND_DISPLAY` as a file name: one socket per display, so a nested
/// session's shell does not hand its calls to the outer one.
pub fn display_key() -> String {
    std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "default".into()).replace('/', "_")
}

/// One ShojiWM run: its control socket's name (`$SHOJIWM_SOCKET`) and when
/// that socket was made, since the name comes back with the next session
/// on the same display.
pub fn shoji_instance(socket: &str) -> Option<String> {
    let path = PathBuf::from(socket);
    let made = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some(format!("{}-{made}", path.file_name()?.to_string_lossy()))
}

/// Temp file beside the target, then a rename over it, so a reader (or a
/// crash) never sees half a file. The temp name carries the pid: two
/// shells overlap across an instance takeover, and a fixed name lets one
/// rename the other's half-written file.
pub fn write_atomic(path: &Path, body: impl AsRef<[u8]>) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(body.as_ref())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("formalshell").join("state.json");
        write_atomic(&path, "one\n").unwrap();
        write_atomic(&path, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["state.json"]);
    }

    #[test]
    fn a_private_dir_is_this_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("run");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(private_dir(&dir));
        std::os::unix::fs::symlink(&dir, root.path().join("link")).unwrap();
        assert!(!private_dir(&root.path().join("link")));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(!private_dir(&dir));
        assert!(!private_dir(&root.path().join("missing")));
    }

    #[test]
    fn a_private_write_never_follows_a_link() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("formalshell").join("login-session");
        write_private(&path, "one\n").unwrap();
        std::fs::set_permissions(path.parent().unwrap(), std::fs::Permissions::from_mode(0o755)).unwrap();
        write_private(&path, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
        assert_eq!((mode(path.parent().unwrap()), mode(&path)), (0o700, 0o600));

        let elsewhere = root.path().join("elsewhere");
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        write_private(&path, "three\n").unwrap();
        assert!(!elsewhere.exists() && std::fs::read_to_string(&path).unwrap() == "three\n");

        let linked = root.path().join("linked");
        std::os::unix::fs::symlink(path.parent().unwrap(), &linked).unwrap();
        assert!(write_private(&linked.join("login-session"), "four\n").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "three\n");
    }
}
